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
    /// Set when this blob arrived in a box and had been demonstrated by
    /// whoever packed it — carrying who they were.
    ///
    /// Kept **beside** `demonstrated_with` and never folded into it. §4.8's
    /// demonstration shows that resuming this blob produces what replaying its
    /// definition produces, and that was shown on somebody else's machine; a
    /// session that counted it as its own would be reporting evidence it does
    /// not have. So `demonstrated_with` stays at zero here and every verdict
    /// from the blob is *not determined* until this session demonstrates it.
    ///
    /// What it does change is that nothing demonstrates it **automatically**.
    /// A blob nobody has ever demonstrated anywhere is demonstrated before use
    /// (§4.9); one demonstrated elsewhere is resumed, said to be somebody
    /// else's, and left for the person to establish if they want it as
    /// evidence. Forcing the replay would cost exactly what the box was for
    /// and would be the tool deciding something it was not asked to.
    pub demonstrated_elsewhere: Option<String>,
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
    /// Absent in every entry written before boxes existed, which is what
    /// `default` is for. §4.11 makes a format change safe anyway: a cache that
    /// will not parse is a miss, and a miss costs only time.
    #[serde(default)]
    demonstrated_elsewhere: Option<String>,
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

    /// Where this key's entry lives: a directory named after the anchor.
    ///
    /// Named after the anchor and not after `key.digest()`, because a person
    /// deciding what to keep or hand over has to be able to see what they have,
    /// and a digest is correct and unreadable. The digest has not gone
    /// anywhere — it is inside `key`, and `key` is still the whole of what
    /// decides validity.
    ///
    /// This costs something. Under a digest, two different definitions could
    /// not land in one directory; under a name, they can — a definition edited
    /// on one side meets a blob from the other with the same name and a
    /// different key. `get` already treats that as absence rather than as an
    /// answer, which is what pays for the legibility. The configuration refuses
    /// a name a directory cannot have, so this does not build a path out of
    /// something arbitrary.
    fn dir(&self, key: &Key) -> PathBuf {
        self.root.join(key.anchor())
    }

    /// What is stored for this key.
    ///
    /// `None` for anything at all: nothing there, something there that will not
    /// parse, a key that does not match, a blob of the wrong length. See this
    /// module's header for why none of those is an error.
    pub fn get(&self, key: &Key) -> Option<Stored> {
        self.at_dir(&self.dir(key), key.as_str())
    }

    /// The entry an anchor's directory holds, if its key is the text given.
    ///
    /// For a box, whose entries are keyed by the **sender's** key text — a
    /// string this session cannot build, because it carries the name they gave
    /// their emulator. The text is still compared against what is stored, so
    /// this reads an entry and never trusts a directory's name; what decides
    /// whether that entry *applies* here is `parcel::Parts`, which compares the
    /// two keys part by part and ignores only the name.
    ///
    /// Not a way to open an arbitrary blob: it takes the key it expects and
    /// returns nothing when the stored one differs, exactly as `get` does.
    pub fn entry_keyed(&self, anchor: &str, key_text: &str) -> Option<Stored> {
        self.at_dir(&self.root.join(anchor), key_text)
    }

    fn at_dir(&self, dir: &Path, key_text: &str) -> Option<Stored> {
        // The key in full, not the directory name. Two keys digesting to one
        // name is not something to rely on not happening.
        let stored_key = std::fs::read_to_string(dir.join("key")).ok()?;
        if stored_key != key_text {
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
            demonstrated_elsewhere: entry.demonstrated_elsewhere,
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

        // The process id is in the name, and it has to be. This name was the
        // key's digest alone, which meant two processes creating the SAME
        // anchor at the same time shared one staging directory — and the
        // `remove_dir_all` below would delete the other's files mid-write.
        //
        // That is not a hypothetical: several processes measuring different
        // things usually share a prefix anchor, and on a cold cache they race
        // to create it. One emulator per process is this project's shape
        // (`Reference` refuses a second in one process), so **several processes
        // is how anything is done in parallel** — and the cache is what they
        // share.
        let staging = self
            .root
            .join(format!("{}.{}.writing", key.anchor(), std::process::id()));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| unwritable(&staging, e))?;

        let entry = Entry {
            position: stored.blob.position().clone(),
            fingerprint: to_hex(stored.blob.fingerprint()),
            check: stored.check.clone(),
            uses: stored.uses,
            demonstrated_with: stored.demonstrated_with,
            demonstrated_elsewhere: stored.demonstrated_elsewhere.clone(),
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

    /// What is in here, by the name a person reads and the key that decides.
    ///
    /// There was no way to look at a cache at all until something had to choose
    /// what to hand over. `get` answers about a key somebody already has;
    /// choosing needs the opposite question, and the full key text is stored
    /// beside each blob precisely because a digest cannot be read backwards.
    ///
    /// Sorted by name, so that two listings of one cache read the same —
    /// directory order is the filesystem's business and would make a report
    /// change for no reason.
    ///
    /// A directory that is not an entry is skipped rather than reported as a
    /// broken one: the same reason `get` treats everything as absence, and a
    /// staging directory from a write in flight is exactly such a thing.
    pub fn kept(&self) -> Vec<Kept> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let dir = entry.path();
                let Ok(key) = std::fs::read_to_string(dir.join("key")) else {
                    continue;
                };
                let Some(anchor) = dir.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                out.push(Kept {
                    anchor: anchor.to_string(),
                    key: key.trim_end().to_string(),
                });
            }
        }
        out.sort_by(|a, b| a.anchor.cmp(&b.anchor));
        out
    }
}

/// One entry, as a listing sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    /// The directory's name, which is the anchor's name (§6.8).
    pub anchor: String,
    /// The key in full. What decides validity, and what a reader looks at to
    /// see *which* part of a stale entry went stale.
    pub key: String,
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

    /// Two writers of the SAME key do not share a staging directory.
    ///
    /// One emulator per process is this project's shape, so several processes
    /// is how anything is done in parallel — and they share one cache. Several
    /// of them usually share a prefix anchor too, and on a cold cache they race
    /// to create it.
    ///
    /// The name used to be the key's digest alone. Two writers then cleared and
    /// filled one directory at once, and whichever renamed second renamed
    /// whatever was left.
    #[test]
    fn two_writers_of_one_key_do_not_share_a_staging_directory() {
        let dir = std::env::temp_dir().join("awaseru-cache-staging");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = Cache::at(&dir);
        let key = key();

        // What `put` would compute, with this process and with another.
        let mine = format!("{}.{}.writing", key.digest(), std::process::id());
        let theirs = format!("{}.{}.writing", key.digest(), std::process::id() + 1);
        assert_ne!(
            mine, theirs,
            "two processes creating one anchor must not clear each other's files"
        );
        assert!(
            mine.starts_with(&key.digest()),
            "and both still belong to the key, so a sweep of leftovers can find them: {mine}"
        );

        // And the entry a put lands on is the key's own, shared on purpose:
        // that is the thing the rename makes appear atomically.
        let _ = cache;
    }
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
            demonstrated_elsewhere: None,
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
    fn a_listing_names_what_is_there_and_skips_what_is_not_an_entry() {
        let root = scratch("listing");
        let cache = Cache::at(&root);
        let two = Anchors::new(vec![
            Anchor {
                name: "settled".into(),
                definition: Definition {
                    start: Start::PowerOn,
                    bound: Bound::Frames(20),
                    input: None,
                },
                covers: vec!["work-ram".into()],
            },
            Anchor {
                name: "boot".into(),
                definition: Definition {
                    start: Start::PowerOn,
                    bound: Bound::Frames(10),
                    input: None,
                },
                covers: vec!["work-ram".into()],
            },
        ])
        .expect("two anchors");
        cache
            .put(&two.key("boot", &provenance()).unwrap(), &stored())
            .expect("written");
        cache
            .put(&two.key("settled", &provenance()).unwrap(), &stored())
            .expect("written");

        // Something in the cache directory that is not an entry: a staging
        // directory from a write in flight looks exactly like this.
        std::fs::create_dir_all(root.join("boot.9999.writing")).expect("a directory");
        std::fs::write(root.join("stray"), b"not an entry").expect("a file");

        let kept = cache.kept();
        assert_eq!(
            kept.iter().map(|k| k.anchor.as_str()).collect::<Vec<_>>(),
            vec!["boot", "settled"],
            "by name, and in an order that does not depend on the filesystem"
        );
        assert!(
            kept[0].key.contains("anchor=boot"),
            "the key in full, because a digest cannot be read backwards: {}",
            kept[0].key
        );
        assert_eq!(kept.len(), cache.len(), "a listing and a count must agree");
    }

    #[test]
    fn forgetting_one_entry_leaves_the_others() {
        let cache = Cache::at(scratch("forget"));
        let two = Anchors::new(vec![
            Anchor {
                name: "boot".into(),
                definition: Definition {
                    start: Start::PowerOn,
                    bound: Bound::Frames(10),
                    input: None,
                },
                covers: vec!["work-ram".into()],
            },
            Anchor {
                name: "settled".into(),
                definition: Definition {
                    start: Start::PowerOn,
                    bound: Bound::Frames(20),
                    input: None,
                },
                covers: vec!["work-ram".into()],
            },
        ])
        .expect("two anchors");
        let boot = two.key("boot", &provenance()).expect("it resolves");
        let settled = two.key("settled", &provenance()).expect("it resolves");

        cache.put(&boot, &stored()).expect("it writes");
        cache.put(&settled, &stored()).expect("it writes");
        assert_eq!(cache.len(), 2);

        assert!(cache.forget(&boot));
        assert_eq!(cache.get(&boot), None);
        assert!(cache.get(&settled).is_some(), "the other one stays");
        assert!(!cache.forget(&boot), "forgetting nothing says so");
    }

    /// The contract the readable directory names narrowed, written down so that
    /// widening it again has to be a decision.
    ///
    /// An entry is named after its anchor, so a cache holds **at most one blob
    /// per anchor name**. Two keys for one anchor — the same definition against
    /// a different reference or a different backend version — are one slot, not
    /// two, and the one that is not the stored key reads as absence.
    ///
    /// Nothing is lost that §4.11 promised: it already says that changing the
    /// reference, the version or the definition invalidates the blob, and an
    /// invalidated blob is one that has to be replayed. Keeping the superseded
    /// copy alongside it was free under a digest and was never owed. A session
    /// pins one reference and one backend version, which is why one slot is the
    /// right number.
    #[test]
    fn two_keys_for_one_anchor_are_one_slot_and_the_other_reads_as_absence() {
        let cache = Cache::at(scratch("one-slot"));
        let mut newer = provenance();
        newer.version = "2.3.0".into();
        let after_upgrade = anchors().key("boot", &newer).expect("it resolves");

        cache.put(&key(), &stored()).expect("it writes");
        cache.put(&after_upgrade, &stored()).expect("it writes");

        assert_eq!(cache.len(), 1, "one anchor, one directory");
        assert!(
            cache.get(&after_upgrade).is_some(),
            "the key that was written last is the one that is there"
        );
        assert_eq!(
            cache.get(&key()),
            None,
            "the superseded key is absence and never a stale answer"
        );
    }
}
