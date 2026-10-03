//! A box: one anchor and its ancestry, taken out of a session to be handed over.
//!
//! The word in the specification and on the command line is **box**. The type
//! here is `Parcel` because `Box` belongs to the language, and a name that
//! shadowed it would make every signature in this module read wrongly.
//!
//! ## A directory, not an archive
//!
//! A box is a directory. Compressing it or committing it is the person's to do,
//! with the tools they already have — an archive format would be a sixth
//! dependency (§17.2) for something `zip` and `git` already do, and a directory
//! is the form that can be looked at before it is sent.
//!
//! ## What travels: the ancestry, never the siblings
//!
//! Two pieces of work that branch from a common trunk share the trunk. Handing
//! one of them over therefore hands over the trunk, and that is not the same as
//! handing over the other branch:
//!
//! ```text
//! origin
//! └── opening
//!     └── settled
//!         ├── branch-a     asking for branch-b packs none of this
//!         └── branch-b     <- asked for
//! ```
//!
//! This is `Anchors::chain`, which walks from the leaf to the origin and
//! nothing sideways, so siblings do not travel by construction rather than by
//! being filtered out.
//!
//! The box carries the whole chain and not only the leaf's blob. A blob on its
//! own resumes perfectly well — it is a complete machine state — but §4.11 says
//! a blob is a cache and never an input, and a cache nobody can re-derive is a
//! cache that can only be believed. With the definitions and the input logs
//! inside, a receiver can resume at any level of it, or replay it.
//!
//! ## What it does not carry
//!
//! The software. A box holds the software's identity and a receiver whose copy
//! differs is refused — not as a rule anybody had to be told, but because the
//! key would not match.

use std::fmt;
use std::path::{Path, PathBuf};

use awaseru_core::anchor::{Anchor, AnchorError, Anchors, Start};
use awaseru_core::snapshot::Provenance;
use awaseru_core::Bound;

use crate::cache::Cache;

/// What a box says it is, at its root.
pub const DESCRIPTION: &str = "box.toml";
/// The chain's definitions, as configuration a receiver can read.
pub const DEFINITIONS: &str = "definitions.toml";

#[derive(Debug)]
pub enum ParcelError {
    /// The anchors do not resolve: an unknown name, or a circle.
    Anchors(AnchorError),
    /// Nothing in this session is kept for any anchor in the chain, so there is
    /// nothing a box would carry that configuration does not already.
    NothingKept { leaf: String, chain: Vec<String> },
    /// The place asked for already has something in it.
    Occupied { at: PathBuf },
    Io { at: PathBuf, why: std::io::Error },
}

impl fmt::Display for ParcelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParcelError::Anchors(e) => write!(f, "{e}"),
            ParcelError::NothingKept { leaf, chain } => write!(
                f,
                "this session keeps no blob for `{leaf}` or for anything it is built on ({}), so \
                 a box would carry only definitions — which the configuration already carries. \
                 Arrive at it once and the blob is there to put in one",
                chain.join(" -> ")
            ),
            ParcelError::Occupied { at } => write!(
                f,
                "`{}` already has something in it. A box written over another box would be two \
                 boxes mixed, and which anchor it was of would be anybody's guess",
                at.display()
            ),
            ParcelError::Io { at, why } => {
                write!(f, "`{}` could not be used: {why}", at.display())
            }
        }
    }
}

impl std::error::Error for ParcelError {}

impl From<AnchorError> for ParcelError {
    fn from(e: AnchorError) -> Self {
        ParcelError::Anchors(e)
    }
}

/// What went into a box, so that packing can report rather than claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    pub at: PathBuf,
    /// The anchor asked for.
    pub of: String,
    /// The chain, origin first.
    pub chain: Vec<String>,
    /// Those that went in with a blob.
    pub with_blob: Vec<String>,
    /// Those that went in as a definition only, because this session keeps no
    /// blob for them. Reported rather than hidden: a receiver replays those
    /// legs, and knowing which is the difference between a surprise and a plan.
    pub definition_only: Vec<String>,
    /// Input logs carried, by the name they have inside the box.
    pub logs: Vec<String>,
}

impl fmt::Display for Packed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` packed into {}: {} anchor(s) of its ancestry, {} with a blob",
            self.of,
            self.at.display(),
            self.chain.len(),
            self.with_blob.len()
        )?;
        if !self.definition_only.is_empty() {
            write!(
                f,
                ". A receiver replays these, because this session keeps no blob for them: {}",
                self.definition_only.join(", ")
            )?;
        }
        if !self.logs.is_empty() {
            write!(f, ". Input log(s) carried: {}", self.logs.join(", "))?;
        }
        Ok(())
    }
}

/// Packs `leaf` and its ancestry out of `cache` into `at`.
pub fn pack(
    anchors: &Anchors,
    leaf: &str,
    provenance: &Provenance,
    cache: &Cache,
    at: &Path,
) -> Result<Packed, ParcelError> {
    let chain = anchors.chain(leaf)?;
    let names: Vec<String> = chain.iter().map(|a| a.name.clone()).collect();

    // Refused rather than emptied: what is in there may be somebody's.
    if let Ok(mut entries) = std::fs::read_dir(at)
        && entries.next().is_some()
    {
        return Err(ParcelError::Occupied {
            at: at.to_path_buf(),
        });
    }
    make(at)?;

    let mut with_blob = Vec::new();
    let mut definition_only = Vec::new();
    for anchor in &chain {
        let key = anchors.key(&anchor.name, provenance)?;
        match cache.get(&key) {
            None => definition_only.push(anchor.name.clone()),
            Some(_) => {
                // Copied from the cache's own directory rather than written
                // back out of the parsed entry: what travels is then byte for
                // byte what this session has, and a format this build does not
                // fully understand still arrives intact.
                let from = cache.root().join(&anchor.name);
                let to = at.join("anchors").join(&anchor.name);
                make(&to)?;
                for part in ["key", "entry.toml", "blob"] {
                    std::fs::copy(from.join(part), to.join(part)).map_err(|why| {
                        ParcelError::Io {
                            at: from.join(part),
                            why,
                        }
                    })?;
                }
                with_blob.push(anchor.name.clone());
            }
        }
    }

    if with_blob.is_empty() {
        let _ = std::fs::remove_dir_all(at);
        return Err(ParcelError::NothingKept {
            leaf: leaf.to_string(),
            chain: names,
        });
    }

    let mut logs = Vec::new();
    for anchor in &chain {
        if let Some(log) = &anchor.definition.input {
            let name = log_name(&anchor.name, &log.name);
            make(&at.join("input"))?;
            let to = at.join("input").join(&name);
            std::fs::write(&to, &log.recorded)
                .map_err(|why| ParcelError::Io { at: to, why })?;
            logs.push(name);
        }
    }

    let definitions = at.join(DEFINITIONS);
    std::fs::write(&definitions, render(&chain)).map_err(|why| ParcelError::Io {
        at: definitions,
        why,
    })?;

    let description = at.join(DESCRIPTION);
    std::fs::write(
        &description,
        describe(leaf, provenance, &names, &with_blob, &definition_only),
    )
    .map_err(|why| ParcelError::Io {
        at: description,
        why,
    })?;

    Ok(Packed {
        at: at.to_path_buf(),
        of: leaf.to_string(),
        chain: names,
        with_blob,
        definition_only,
        logs,
    })
}

fn make(at: &Path) -> Result<(), ParcelError> {
    std::fs::create_dir_all(at).map_err(|why| ParcelError::Io {
        at: at.to_path_buf(),
        why,
    })
}

/// What an input log is called inside a box.
///
/// Named after the anchor that uses it, so two logs cannot collide on a base
/// name they happened to share in two directories. Safe to rename, and this is
/// the reason it is safe: §4.11's key hashes a log's **contents** and never its
/// path, exactly so that where a recording sits is not part of what a blob is
/// keyed by.
fn log_name(anchor: &str, original: &str) -> String {
    let base = original
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(original)
        .trim();
    if base.is_empty() {
        format!("{anchor}.log")
    } else {
        format!("{anchor}-{base}")
    }
}

/// The chain as `[[anchor]]` blocks a receiver's configuration can read.
///
/// Rendered rather than copied out of the sender's file, because the sender's
/// file holds anchors that are not in this chain and paths that are theirs.
/// Relative, so §6.3 resolves it against this file.
fn render(chain: &[&Anchor]) -> String {
    let mut out = String::new();
    out.push_str("# The anchors `box.toml` is of, and everything they are built on.\n");
    out.push_str("# Read these beside your own configuration (§6.3 resolves the paths\n");
    out.push_str("# below against this file).\n");
    for anchor in chain {
        out.push_str("\n[[anchor]]\n");
        out.push_str(&format!("name = {:?}\n", anchor.name));
        if let Start::Anchor(parent) = &anchor.definition.start {
            out.push_str(&format!("after = {parent:?}\n"));
        }
        match &anchor.definition.bound {
            Bound::Frames(n) => out.push_str(&format!("frames = {n}\n")),
            Bound::Instructions(n) => out.push_str(&format!("instructions = {n}\n")),
            Bound::Address { address, within } => {
                out.push_str(&format!("address = \"{address:X}\"\nwithin = {within}\n"));
            }
            // §5.4's localisation is not something an anchor is declared with,
            // so there is no configuration for it. Said in a comment rather
            // than rendered as a key that would not read back.
            other => out.push_str(&format!("# this bound has no configuration: {other}\n")),
        }
        if let Some(log) = &anchor.definition.input {
            out.push_str(&format!(
                "input = {:?}\n",
                format!("input/{}", log_name(&anchor.name, &log.name))
            ));
        }
        if !anchor.covers.is_empty() {
            let covers: Vec<String> = anchor.covers.iter().map(|c| format!("{c:?}")).collect();
            out.push_str(&format!("covers = [{}]\n", covers.join(", ")));
        }
    }
    out
}

fn describe(
    leaf: &str,
    provenance: &Provenance,
    chain: &[String],
    with_blob: &[String],
    definition_only: &[String],
) -> String {
    let list = |items: &[String]| {
        items
            .iter()
            .map(|i| format!("{i:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "# What this box is of.\n\
         #\n\
         # It carries the software's IDENTITY and not the software. A copy whose\n\
         # digest differs produces a different key, so the blobs in here simply\n\
         # do not apply to it (§4.11).\n\
         of = {leaf:?}\n\
         software = {:?}\n\
         reference = {:?}\n\
         backend = {:?}\n\
         version = {:?}\n\
         \n\
         # Origin first. `with_blob` resumes; the rest a receiver replays.\n\
         chain = [{}]\n\
         with_blob = [{}]\n\
         definition_only = [{}]\n",
        provenance.software,
        provenance.reference,
        provenance.backend,
        provenance.version,
        list(chain),
        list(with_blob),
        list(definition_only),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use awaseru_core::anchor::{Definition, InputLog};
    use awaseru_core::anchor::{CheapCheck, Coverage};
    use awaseru_core::{Blob, Position};

    use crate::cache::Stored;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("awaseru-parcel-test-{name}"));
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

    fn plain(name: &str, after: Option<&str>, frames: u64) -> Anchor {
        Anchor {
            name: name.into(),
            definition: Definition {
                start: match after {
                    None => Start::PowerOn,
                    Some(parent) => Start::Anchor(parent.into()),
                },
                bound: Bound::Frames(frames),
                input: None,
            },
            covers: vec!["work-ram".into()],
        }
    }

    /// origin -> opening -> settled -> { branch-a, branch-b }
    ///
    /// The shape the whole unit is about: two pieces of work that share a
    /// trunk.
    fn branching() -> Anchors {
        Anchors::new(vec![
            plain("opening", None, 10),
            plain("settled", Some("opening"), 20),
            plain("branch-a", Some("settled"), 30),
            plain("branch-b", Some("settled"), 40),
        ])
        .expect("four anchors")
    }

    fn fill(cache: &Cache, anchors: &Anchors, names: &[&str]) {
        for name in names {
            let key = anchors.key(name, &provenance()).expect("it resolves");
            cache.put(&key, &stored()).expect("written");
        }
    }

    /// U4's done-condition. A box of a leaf carries its trunk and **not** its
    /// sibling — the thing the user asked for in so many words.
    #[test]
    fn a_box_of_one_branch_carries_the_trunk_and_not_the_other_branch() {
        let root = scratch("branches");
        let cache = Cache::at(root.join("anchors"));
        let anchors = branching();
        fill(&cache, &anchors, &["opening", "settled", "branch-a", "branch-b"]);

        let at = root.join("box");
        let packed = pack(&anchors, "branch-b", &provenance(), &cache, &at).expect("it packs");

        assert_eq!(packed.chain, vec!["opening", "settled", "branch-b"]);
        assert_eq!(packed.with_blob, vec!["opening", "settled", "branch-b"]);
        assert!(packed.definition_only.is_empty());

        for carried in ["opening", "settled", "branch-b"] {
            assert!(
                at.join("anchors").join(carried).join("blob").is_file(),
                "{carried} should have travelled"
            );
        }
        assert!(
            !at.join("anchors").join("branch-a").exists(),
            "the SIBLING must not travel, even though this session has its blob"
        );

        let definitions = std::fs::read_to_string(at.join(DEFINITIONS)).expect("there");
        assert!(definitions.contains("name = \"branch-b\""), "{definitions}");
        assert!(definitions.contains("after = \"settled\""), "{definitions}");
        assert!(
            !definitions.contains("branch-a"),
            "nor does the sibling's definition: {definitions}"
        );

        let described = std::fs::read_to_string(at.join(DESCRIPTION)).expect("there");
        assert!(described.contains("of = \"branch-b\""), "{described}");
        assert!(
            described.contains("software = \"abcdef0123456789\""),
            "it carries the software's identity, {described}"
        );
    }

    /// The blob is copied byte for byte out of the cache's own directory, so a
    /// box is what the session has rather than what this build understood of
    /// it.
    #[test]
    fn what_travels_is_byte_for_byte_what_the_session_kept() {
        let root = scratch("bytes");
        let cache = Cache::at(root.join("anchors"));
        let anchors = branching();
        fill(&cache, &anchors, &["opening"]);

        let at = root.join("box");
        pack(&anchors, "opening", &provenance(), &cache, &at).expect("it packs");

        for part in ["key", "entry.toml", "blob"] {
            let here = std::fs::read(root.join("anchors").join("opening").join(part)).unwrap();
            let there = std::fs::read(at.join("anchors").join("opening").join(part)).unwrap();
            assert_eq!(here, there, "{part} must arrive unchanged");
        }
    }

    /// An ancestor this session never arrived at travels as a definition, and
    /// the report says so rather than letting the receiver find out by waiting.
    #[test]
    fn an_ancestor_with_no_blob_travels_as_a_definition_and_is_reported() {
        let root = scratch("partial");
        let cache = Cache::at(root.join("anchors"));
        let anchors = branching();
        fill(&cache, &anchors, &["branch-b"]);

        let at = root.join("box");
        let packed = pack(&anchors, "branch-b", &provenance(), &cache, &at).expect("it packs");

        assert_eq!(packed.with_blob, vec!["branch-b"]);
        assert_eq!(packed.definition_only, vec!["opening", "settled"]);
        assert!(
            !at.join("anchors").join("opening").exists(),
            "there is no blob to carry for it"
        );

        let definitions = std::fs::read_to_string(at.join(DEFINITIONS)).expect("there");
        assert!(
            definitions.contains("name = \"opening\""),
            "but its definition still travels, or the chain cannot be replayed: {definitions}"
        );
        let said = packed.to_string();
        assert!(said.contains("replays these"), "{said}");
        assert!(said.contains("opening"), "{said}");
    }

    /// Refused, and made to happen: a box with no blob at all carries nothing
    /// the configuration does not already carry.
    #[test]
    fn a_box_with_no_blob_anywhere_in_the_chain_is_refused() {
        let root = scratch("empty");
        let cache = Cache::at(root.join("anchors"));
        let anchors = branching();

        let at = root.join("box");
        let err = pack(&anchors, "branch-b", &provenance(), &cache, &at)
            .expect_err("nothing to put in it");
        match &err {
            ParcelError::NothingKept { leaf, chain } => {
                assert_eq!(leaf, "branch-b");
                assert_eq!(chain, &["opening", "settled", "branch-b"]);
            }
            other => panic!("got {other}"),
        }
        assert!(
            err.to_string().contains("Arrive at it once"),
            "the refusal says what to do, said: {err}"
        );
        assert!(
            !at.exists(),
            "and it leaves nothing half-made behind: {}",
            at.display()
        );
    }

    /// Refused rather than emptied. What is in there may be somebody's.
    #[test]
    fn a_place_that_already_has_something_in_it_is_refused() {
        let root = scratch("occupied");
        let cache = Cache::at(root.join("anchors"));
        let anchors = branching();
        fill(&cache, &anchors, &["opening"]);

        let at = root.join("box");
        std::fs::create_dir_all(&at).expect("a directory");
        std::fs::write(at.join("something-of-mine"), b"keep me").expect("written");

        let err =
            pack(&anchors, "opening", &provenance(), &cache, &at).expect_err("it is occupied");
        assert!(matches!(err, ParcelError::Occupied { .. }), "got {err}");
        assert!(
            at.join("something-of-mine").is_file(),
            "and what was there is still there"
        );
    }

    /// An input log travels by its contents, renamed after the anchor that uses
    /// it, and the carried definition points at where it landed.
    #[test]
    fn an_input_log_travels_and_the_definition_points_at_it_inside_the_box() {
        let root = scratch("log");
        let cache = Cache::at(root.join("anchors"));
        let anchors = Anchors::new(vec![
            Anchor {
                name: "opening".into(),
                definition: Definition {
                    start: Start::PowerOn,
                    bound: Bound::Frames(17_767),
                    input: Some(InputLog {
                        name: "../movie/opening.mmo".into(),
                        path: PathBuf::from("/somewhere/else/opening.mmo"),
                        recorded: b"the recording".to_vec(),
                    }),
                },
                covers: vec!["work-ram".into()],
            },
            plain("settled", Some("opening"), 20),
        ])
        .expect("two anchors");
        fill(&cache, &anchors, &["settled"]);

        let at = root.join("box");
        let packed = pack(&anchors, "settled", &provenance(), &cache, &at).expect("it packs");

        assert_eq!(packed.logs, vec!["opening-opening.mmo"]);
        assert_eq!(
            std::fs::read(at.join("input").join("opening-opening.mmo")).unwrap(),
            b"the recording",
            "by its contents, which is what §4.11 keys on"
        );

        let definitions = std::fs::read_to_string(at.join(DEFINITIONS)).expect("there");
        assert!(
            definitions.contains("input = \"input/opening-opening.mmo\""),
            "relative, so §6.3 resolves it against this file: {definitions}"
        );
        assert!(
            !definitions.contains("/somewhere/else"),
            "and the sender's machine does not travel: {definitions}"
        );
    }

    #[test]
    fn a_log_is_named_after_its_anchor_so_two_cannot_collide_on_a_base_name() {
        assert_eq!(log_name("a", "../movie/opening.mmo"), "a-opening.mmo");
        assert_eq!(log_name("b", "../other/opening.mmo"), "b-opening.mmo");
        assert_eq!(log_name("a", "opening.mmo"), "a-opening.mmo");
        assert_eq!(log_name("a", ""), "a.log");
    }

    #[test]
    fn an_unknown_anchor_is_the_anchors_own_refusal_and_not_a_new_one() {
        let root = scratch("unknown");
        let cache = Cache::at(root.join("anchors"));
        let err = pack(&branching(), "nowhere", &provenance(), &cache, &root.join("box"))
            .expect_err("refused");
        assert!(matches!(err, ParcelError::Anchors(_)), "got {err}");
        assert!(err.to_string().contains("nowhere"), "said: {err}");
    }
}
