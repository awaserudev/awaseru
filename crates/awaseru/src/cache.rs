//! The anchor cache — §4.11.
//!
//! Machine-local, because it holds paths and because §6.7 keeps it out of
//! configuration entirely: a shared cache of blobs would be sharing the one
//! artefact where a stale copy is invisible.
//!
//! # The property that matters more than any care taken here
//!
//! **A blob is a cache and never an input** (§4.11): deleting the entire cache
//! must change nothing except how long a run takes. Everything else in this
//! module follows from that, including the shape of its errors.
//!
//! So **every problem with the cache is a miss, not a failure.** An entry that
//! will not parse, a blob that is the wrong length, a key that does not match,
//! a directory somebody edited by hand: all of them come back as "nothing
//! cached", and the caller replays. A cache that could fail a run would be a
//! cache that is load-bearing, and then deleting it would stop being free.
//!
//! The one thing that *is* an error is being unable to **write**, because a
//! cache that silently never stores anything is a tool that is mysteriously
//! slow forever.
//!
//! # What is on disk
//!
//! One directory per key, named by the key's digest. Inside it:
//!
//! - `key` — the key in full. **This is what decides validity**, not the
//!   directory's name. Comparing digests would be trusting a hash where the
//!   thing itself is right there.
//! - `blob` — the backend's opaque bytes.
//! - `entry.toml` — the position the blob was taken at, its fingerprint, the
//!   coverage digests of §4.8, how many times it has been used, and how many
//!   replays its demonstration ran.

use std::path::{Path, PathBuf};

use awaseru_core::anchor::{CheapCheck, Key};
use awaseru_core::run::Position;
use awaseru_core::Blob;
use serde::{Deserialize, Serialize};

/// A blob and everything known about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub blob: Blob,
    pub check: CheapCheck,
    /// How many times it has been resumed since it was last demonstrated, for
    /// §4.9's `reverify_after`.
    pub uses: u64,
    /// How many replays the demonstration ran. **Zero means never
    /// demonstrated**, and §4.8 says every comparison from such an anchor is
    /// *not determined* rather than trusted.
    pub demonstrated_with: u32,
}

/// The only thing that can go wrong loudly.
#[derive(Debug)]
pub enum CacheError {
    /// The cache could not be written to. Loud on purpose: a cache that
    /// silently never stores is a tool that is slow forever for no visible
    /// reason.
    Unwritable { path: PathBuf, why: std::io::Error },
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Unwritable { path, why } => write!(
                f,
                "the anchor cache at {} could not be written: {why}. Nothing will be cached, so \
                 every run will replay from the origin — which is correct and slow, and worth \
                 saying out loud rather than discovering by stopwatch",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CacheError {}

/// What `entry.toml` holds.
#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    position: Position,
    /// Hexadecimal, because a blob's fingerprint is opaque bytes and a
    /// configuration file is text.
    fingerprint: String,
    check: CheapCheck,
    uses: u64,
    demonstrated_with: u32,
}

/// A machine-local store of anchor blobs.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Cache { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn dir(&self, key: &Key) -> PathBuf {
        self.root.join(key.digest())
    }

    /// What is stored for this key.
    ///
    /// `None` for anything at all: nothing there, something there that will not
    /// parse, a key that does not match, a blob of the wrong length. See this
    /// module's header for why none of those is an error.
    pub fn get(&self, key: &Key) -> Option<Stored> {
        let dir = self.dir(key);

        // The key in full, not the directory name. Two keys digesting to one
        // name is not something to rely on not happening.
        let stored_key = std::fs::read_to_string(dir.join("key")).ok()?;
        if stored_key != key.as_str() {
            return None;
        }

        let entry: Entry = toml::from_str(&std::fs::read_to_string(dir.join("entry.toml")).ok()?)
            .ok()?;
        let bytes = std::fs::read(dir.join("blob")).ok()?;
        if bytes.is_empty() {
            return None;
        }
        let fingerprint = from_hex(&entry.fingerprint)?;

        Some(Stored {
            blob: Blob::new(bytes, entry.position, fingerprint),
            check: entry.check,
            uses: entry.uses,
            demonstrated_with: entry.demonstrated_with,
        })
    }

    /// Stores a blob, replacing whatever was there.
    ///
    /// Written to a temporary name and renamed, so an interrupted write leaves
    /// the previous entry rather than half of a new one. A half-written blob
    /// would be caught by the cheap check, which is a worse place to catch it:
    /// by then a run has been spent.
    pub fn put(&self, key: &Key, stored: &Stored) -> Result<(), CacheError> {
        let dir = self.dir(key);
        let unwritable = |path: &Path, why: std::io::Error| CacheError::Unwritable {
            path: path.to_path_buf(),
            why,
        };

        let staging = self.root.join(format!("{}.writing", key.digest()));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| unwritable(&staging, e))?;

        let entry = Entry {
            position: stored.blob.position().clone(),
            fingerprint: to_hex(stored.blob.fingerprint()),
            check: stored.check.clone(),
            uses: stored.uses,
            demonstrated_with: stored.demonstrated_with,
        };
        let text = toml::to_string(&entry).expect("an entry is always serialisable");

        std::fs::write(staging.join("key"), key.as_str()).map_err(|e| unwritable(&staging, e))?;
        std::fs::write(staging.join("entry.toml"), text).map_err(|e| unwritable(&staging, e))?;
        std::fs::write(staging.join("blob"), stored.blob.bytes())
            .map_err(|e| unwritable(&staging, e))?;

        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&staging, &dir).map_err(|e| unwritable(&dir, e))?;
        Ok(())
    }

    /// Records one more use, for §4.9's `reverify_after`.
    ///
    /// A failure to write this is **not** an error: the count going stale makes
    /// the tool re-verify sooner or later than asked, which is a worse schedule
    /// and not a wrong answer.
    pub fn note_use(&self, key: &Key) {
        if let Some(mut stored) = self.get(key) {
            stored.uses += 1;
            let _ = self.put(key, &stored);
        }
    }

    /// Throws one entry away. `true` when there was one.
    pub fn forget(&self, key: &Key) -> bool {
        std::fs::remove_dir_all(self.dir(key)).is_ok()
    }

    /// Throws **everything** away, which §4.11 says must cost only time.
    pub fn clear(&self) -> usize {
        let mut gone = 0;
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                if std::fs::remove_dir_all(entry.path()).is_ok() {
                    gone += 1;
                }
            }
        }
        gone
    }

    /// How many entries are there, for a report. Directories that are not
    /// entries are not counted.
    pub fn len(&self) -> usize {
        std::fs::read_dir(&self.root)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().join("key").is_file())
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, byte| {
        let _ = write!(s, "{byte:02x}");
        s
    })
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use awaseru_core::anchor::{Anchor, Anchors, Coverage, Definition, Start};
    use awaseru_core::run::Bound;
    use awaseru_core::snapshot::Provenance;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("awaseru-cache-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        dir
    }

    fn provenance() -> Provenance {
        Provenance {
            reference: "ref-a".into(),
            backend: "a-backend".into(),
            version: "1.0.0".into(),
            software: "abcdef0123456789".into(),
        }
    }

    fn anchors() -> Anchors {
        Anchors::new(vec![Anchor {
            name: "boot".into(),
            definition: Definition {
                start: Start::PowerOn,
                bound: Bound::Frames(10),
                input: None,
            },
            covers: vec!["work-ram".into()],
        }])
        .expect("one anchor")
    }

    fn key() -> Key {
        anchors().key("boot", &provenance()).expect("it resolves")
    }

    fn stored() -> Stored {
        Stored {
            blob: Blob::new(
                vec![0xAB; 4096],
                Position::FrameBoundary { frame: 10 },
                vec![1, 2, 3, 4],
            ),
            check: CheapCheck {
                position: Position::FrameBoundary { frame: 10 },
                coverage: Coverage::from_digests(vec![("work-ram".into(), "aa".into())]),
            },
            uses: 0,
            demonstrated_with: 3,
        }
    }

    #[test]
    fn a_blob_comes_back_exactly_as_it_went_in() {
        let cache = Cache::at(scratch("round-trip"));
        assert!(cache.is_empty());
        cache.put(&key(), &stored()).expect("it writes");
        assert_eq!(cache.len(), 1);

        let back = cache.get(&key()).expect("it is there");
        assert_eq!(back, stored(), "a cache that changed a blob would be worse than none");
        assert_eq!(back.blob.bytes().len(), 4096);
    }

    /// **The property §4.11 is about.** Everything gone, and nothing is an
    /// error — a caller sees a miss and replays.
    #[test]
    fn clearing_the_cache_leaves_misses_and_not_failures() {
        let cache = Cache::at(scratch("clear"));
        cache.put(&key(), &stored()).expect("it writes");
        assert_eq!(cache.clear(), 1);
        assert_eq!(cache.get(&key()), None, "a miss");
        assert!(cache.is_empty());
        // And clearing an already-empty cache is not a failure either.
        assert_eq!(cache.clear(), 0);
    }

    /// Every way an entry can be broken is a **miss**. Each of these, treated
    /// as an error, would make a corrupted cache able to stop a run — and then
    /// deleting the cache would stop being free, which is the one property
    /// §4.11 insists on.
    #[test]
    fn every_broken_entry_is_a_miss_rather_than_a_failure() {
        let key = key();
        for (name, break_it) in [
            (
                "no key file",
                Box::new(|dir: &Path| {
                    let _ = std::fs::remove_file(dir.join("key"));
                }) as Box<dyn Fn(&Path)>,
            ),
            (
                "a key that does not match",
                Box::new(|dir: &Path| {
                    std::fs::write(dir.join("key"), "something else").unwrap();
                }),
            ),
            (
                "an entry that will not parse",
                Box::new(|dir: &Path| {
                    std::fs::write(dir.join("entry.toml"), "this is not toml {{{").unwrap();
                }),
            ),
            (
                "no blob",
                Box::new(|dir: &Path| {
                    let _ = std::fs::remove_file(dir.join("blob"));
                }),
            ),
            (
                "an empty blob",
                Box::new(|dir: &Path| {
                    std::fs::write(dir.join("blob"), b"").unwrap();
                }),
            ),
            (
                "a fingerprint that is not hexadecimal",
                Box::new(|dir: &Path| {
                    let text = std::fs::read_to_string(dir.join("entry.toml")).unwrap();
                    let broken = text.replace("fingerprint = \"01020304\"", "fingerprint = \"zz\"");
                    assert_ne!(broken, text, "the test's own substitution must apply");
                    std::fs::write(dir.join("entry.toml"), broken).unwrap();
                }),
            ),
        ] {
            let cache = Cache::at(scratch("broken"));
            cache.put(&key, &stored()).expect("it writes");
            assert!(cache.get(&key).is_some(), "it was there before {name}");

            break_it(&cache.dir(&key));
            assert_eq!(
                cache.get(&key),
                None,
                "with {name}, the cache must report a miss and not fail"
            );
        }
    }

    /// The key in full is what decides validity. Two anchors whose keys happen
    /// to digest alike would otherwise hand each other's blobs over, and the
    /// digest is only a file name.
    #[test]
    fn a_key_that_does_not_match_is_a_miss_even_from_the_right_directory() {
        let cache = Cache::at(scratch("key-mismatch"));
        cache.put(&key(), &stored()).expect("it writes");

        // The same anchor against different software is a different key.
        let mut other = provenance();
        other.software = "0000000000000000".into();
        let other_key = anchors().key("boot", &other).expect("it resolves");
        assert_ne!(other_key, key());
        assert_eq!(cache.get(&other_key), None);

        // Forge the directory name so only the key file can tell them apart.
        let forged = cache.root().join(other_key.digest());
        std::fs::create_dir_all(&forged).unwrap();
        for file in ["key", "entry.toml", "blob"] {
            std::fs::copy(cache.dir(&key()).join(file), forged.join(file)).unwrap();
        }
        assert_eq!(
            cache.get(&other_key),
            None,
            "the directory is right and the key inside is not, so this is a miss"
        );
    }

    #[test]
    fn a_use_is_counted_and_a_missing_entry_is_not_an_error_to_count() {
        let cache = Cache::at(scratch("uses"));
        cache.put(&key(), &stored()).expect("it writes");
        cache.note_use(&key());
        cache.note_use(&key());
        assert_eq!(cache.get(&key()).expect("there").uses, 2);

        cache.clear();
        cache.note_use(&key()); // nothing there; must not panic or fail
        assert_eq!(cache.get(&key()), None);
    }

    /// Zero replays means never demonstrated, and that has to survive the
    /// round trip — §4.8 turns it into a verdict.
    #[test]
    fn whether_an_anchor_was_ever_demonstrated_survives_the_round_trip() {
        let cache = Cache::at(scratch("demonstrated"));
        let mut never = stored();
        never.demonstrated_with = 0;
        cache.put(&key(), &never).expect("it writes");
        assert_eq!(cache.get(&key()).expect("there").demonstrated_with, 0);

        cache.put(&key(), &stored()).expect("it writes");
        assert_eq!(cache.get(&key()).expect("there").demonstrated_with, 3);
    }

    #[test]
    fn forgetting_one_entry_leaves_the_others() {
        let cache = Cache::at(scratch("forget"));
        let mut other = provenance();
        other.reference = "ref-b".into();
        let other_key = anchors().key("boot", &other).expect("it resolves");

        cache.put(&key(), &stored()).expect("it writes");
        cache.put(&other_key, &stored()).expect("it writes");
        assert_eq!(cache.len(), 2);

        assert!(cache.forget(&key()));
        assert_eq!(cache.get(&key()), None);
        assert!(cache.get(&other_key).is_some(), "the other one stays");
        assert!(!cache.forget(&key()), "forgetting nothing says so");
    }
}
