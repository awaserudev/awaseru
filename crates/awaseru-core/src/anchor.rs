//! Anchors — §4.7 to §4.11.
//!
//! A named position worth returning to: a name, a definition of how to reach
//! it, and the regions it declares. The cached blob itself is not here — that
//! is the cache's, and the cache is machine-local because paths are (§6.1,
//! §6.7).
//!
//! # What this module is really for
//!
//! §4.8. Resuming from a cached blob instead of replaying trades time for the
//! worst-shaped risk the tool has: if the cached state is not the state a
//! replay would have produced, every comparison below it measures the wrong
//! machine **and passes**. So two things live here and neither is optional.
//!
//! The **key** (§4.11) is everything a blob's validity depends on, written out
//! as a string a person can read. A blob whose key does not match the anchor
//! asking for it is thrown away. The key is transitive: an anchor defined on
//! top of another includes that one's key, so a change anywhere upstream
//! invalidates everything below it.
//!
//! The **cheap check** (§4.8) is what runs on every load, as opposed to the
//! demonstration that runs once: the position is the one recorded, and a digest
//! over the regions the anchor declares is the one recorded. On a backend whose
//! load reports nothing (§13's Q12) this is not merely an audit of the cache —
//! it is the only way to know a load did anything.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use crate::capability::{Capabilities, Capability};
use crate::platform::{Platform, ReadError};
use crate::run::{Bound, Position};
use crate::snapshot::Provenance;

/// Where a definition begins — §4.7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// The reproducible power-on. The root of every chain.
    PowerOn,
    /// Another anchor, by name. **Anchors compose**, and this is how: the
    /// expensive prefix is paid once and everything below it is cheap to add.
    Anchor(String),
}

impl std::fmt::Display for Start {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Start::PowerOn => write!(f, "power-on"),
            Start::Anchor(name) => write!(f, "anchor `{name}`"),
        }
    }
}

/// A recorded input log — §4.7.
///
/// Declared because a definition has one wherever the software needs input
/// before it will proceed, and leaving the field out would make such an anchor
/// impossible to write down rather than impossible to reach. What is **not**
/// decided here is the encoding of `recorded`: that is settled by whatever can
/// actually drive the backend's inputs, and §2.4 says not to guess a shape.
///
/// An anchor carrying one of these needs `Capability::InputReplay` to be
/// reached, and is **refused** rather than reached without it — see
/// `Anchors::chain_for`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputLog {
    pub name: String,
    /// Opaque until something can replay it.
    pub recorded: Vec<u8>,
}

/// How to reach an anchor — §4.7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub start: Start,
    /// How far to run from there. Bounded, like every run (§4.2).
    pub bound: Bound,
    /// Where the software needs input first.
    pub input: Option<InputLog>,
}

impl Definition {
    /// What a backend must declare before this definition can be replayed —
    /// §7.3.
    ///
    /// Here rather than at the call site because the question is about the
    /// definition: an anchor that needs input needs something to press the
    /// buttons, whoever is asking.
    pub fn requires(&self) -> Vec<Capability> {
        let mut needed = Vec::new();
        if self.input.is_some() {
            needed.push(Capability::InputReplay);
        }
        needed
    }
}

/// A named position worth returning to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// What configuration and reports call it (§4.7).
    pub name: String,
    pub definition: Definition,
    /// The regions §4.8's cheap check digests on every load.
    ///
    /// An anchor that declares nothing is checked for nothing. §4.10's guidance
    /// is to declare the regions the comparisons actually read, so that a stale
    /// blob is caught on the load rather than by a wrong verdict later.
    pub covers: Vec<String>,
}

/// §4.11's key: everything a blob's validity depends on.
///
/// A readable string rather than a digest, because a blob thrown away should be
/// explicable — somebody looking at a cache that keeps missing wants to see
/// *which* part changed. Whoever stores a blob may hash this for a file name;
/// the string is what decides validity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key(String);

impl Key {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A short, file-name-safe form of the same key, for a cache that needs one.
    ///
    /// The full string stays the thing that decides validity; this is only a
    /// name. A cache that compared these instead would be trusting a digest
    /// where it could have compared the thing itself.
    pub fn digest(&self) -> String {
        let out = Sha256::digest(self.0.as_bytes());
        out.iter().fold(String::with_capacity(64), |mut s, byte| {
            use std::fmt::Write;
            let _ = write!(s, "{byte:02x}");
            s
        })
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Why an anchor cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorError {
    /// No anchor of that name is declared.
    Unknown { name: String, declared: Vec<String> },
    /// A chain that comes back to itself. Refused rather than followed, which
    /// would be a replay that never finishes.
    Cycle { through: Vec<String> },
    /// The definition needs a capability this reference does not declare
    /// (§7.3). Refused rather than attempted without it: an anchor reached
    /// without the input it says it needs is not that anchor (§2.4).
    ///
    /// Which capabilities are absent is the *backend's* answer and not this
    /// module's. An earlier version of this file asserted that no backend
    /// could replay an input log, which was true of the only backend there was
    /// and is not something the platform-independent half can know.
    NeedsCapability {
        anchor: String,
        capability: Capability,
        needed_for: String,
    },
    /// Two anchors share a name, so naming one is ambiguous.
    Duplicate { name: String },
}

impl std::fmt::Display for AnchorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnchorError::Unknown { name, declared } => write!(
                f,
                "no anchor is declared as `{name}`. Declared: {}",
                if declared.is_empty() {
                    "nothing".to_string()
                } else {
                    declared.join(", ")
                }
            ),
            AnchorError::Cycle { through } => write!(
                f,
                "these anchors are defined in a circle: {}. Following it would be a replay that \
                 never finishes",
                through.join(" -> ")
            ),
            AnchorError::NeedsCapability {
                anchor,
                capability,
                needed_for,
            } => write!(
                f,
                "the anchor `{anchor}` needs {needed_for} to be reached, which takes the \
                 capability `{}` — {} — and this reference does not declare it (§7.3). Reaching \
                 the anchor without it would arrive somewhere else and call it this anchor, so \
                 it is refused instead (§2.4)",
                capability.name(),
                capability.means()
            ),
            AnchorError::Duplicate { name } => write!(
                f,
                "two anchors are declared as `{name}`, so naming one of them is ambiguous"
            ),
        }
    }
}

impl std::error::Error for AnchorError {}

/// The declared anchors, as the configuration gives them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Anchors {
    anchors: Vec<Anchor>,
}

impl Anchors {
    /// Refuses two anchors of one name at construction, so that nothing
    /// downstream has to decide which it meant.
    pub fn new(anchors: Vec<Anchor>) -> Result<Self, AnchorError> {
        for (i, a) in anchors.iter().enumerate() {
            if anchors[i + 1..].iter().any(|b| b.name == a.name) {
                return Err(AnchorError::Duplicate {
                    name: a.name.clone(),
                });
            }
        }
        Ok(Anchors { anchors })
    }

    pub fn get(&self, name: &str) -> Option<&Anchor> {
        self.anchors.iter().find(|a| a.name == name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.anchors.iter().map(|a| a.name.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Anchor> {
        self.anchors.iter()
    }

    pub fn len(&self) -> usize {
        self.anchors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    /// The chain from the origin to `name`, origin first.
    ///
    /// Refuses a cycle and a name nobody declares — both before anything is
    /// run, because each is a reason the anchor cannot be reached at all, and
    /// both are decidable from the configuration alone.
    ///
    /// What this does **not** check is whether a backend can replay the
    /// definitions it finds: that is `chain_for`, and it needs a backend's
    /// declaration (§7.3). Configuration loading uses this one, so that a
    /// configuration is not refused over a reference it has not opened.
    pub fn chain(&self, name: &str) -> Result<Vec<&Anchor>, AnchorError> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut at = name.to_string();

        loop {
            if !seen.insert(at.clone()) {
                let mut through: Vec<String> = chain.iter().map(|a: &&Anchor| a.name.clone()).collect();
                through.reverse();
                through.push(at);
                return Err(AnchorError::Cycle { through });
            }
            let anchor = self.get(&at).ok_or_else(|| AnchorError::Unknown {
                name: at.clone(),
                declared: self.names().map(str::to_string).collect(),
            })?;
            chain.push(anchor);
            match &anchor.definition.start {
                Start::PowerOn => break,
                Start::Anchor(parent) => at = parent.clone(),
            }
        }

        chain.reverse();
        Ok(chain)
    }

    /// The same chain, against what a reference declares it can do — §7.3.
    ///
    /// Every anchor in the chain is checked, not only the one asked for: a
    /// definition built on top of one that needs input cannot be replayed
    /// either, and a refusal naming the anchor that actually needs the
    /// capability is the one a reader can act on.
    pub fn chain_for(
        &self,
        name: &str,
        declared: &Capabilities,
    ) -> Result<Vec<&Anchor>, AnchorError> {
        let chain = self.chain(name)?;
        for anchor in &chain {
            let needed = anchor.definition.requires();
            // `require_all` answers whether anything is missing; which one it
            // was is what the refusal has to name, so it is found here rather
            // than parsed back out of a sentence.
            if let Some(capability) = needed.into_iter().find(|c| !declared.has(*c)) {
                return Err(AnchorError::NeedsCapability {
                    anchor: anchor.name.clone(),
                    capability,
                    needed_for: match (capability, &anchor.definition.input) {
                        (Capability::InputReplay, Some(log)) => {
                            format!("the input log `{}`", log.name)
                        }
                        _ => "its definition".to_string(),
                    },
                });
            }
        }
        Ok(chain)
    }

    /// §4.11's key for `name`, against the reference and software it is for.
    ///
    /// Transitive: the key of an anchor defined on top of another contains
    /// that one's key, so a change anywhere upstream invalidates everything
    /// below. A key that only described the anchor itself would leave a blob
    /// valid after its own prefix had been redefined, which is the stale-blob
    /// failure §4.11 is about.
    pub fn key(&self, name: &str, provenance: &Provenance) -> Result<Key, AnchorError> {
        let chain = self.chain(name)?;
        let mut key = format!(
            "reference={} backend={} version={} software={}",
            provenance.reference, provenance.backend, provenance.version, provenance.software
        );
        for anchor in chain {
            // Covered regions are sorted, so that reordering a list in a
            // configuration does not throw away a cache that is still valid.
            let mut covers = anchor.covers.clone();
            covers.sort();
            key = format!(
                "{key} | anchor={} start={} bound={} covers=[{}]",
                anchor.name,
                anchor.definition.start,
                anchor.definition.bound,
                covers.join(",")
            );
        }
        Ok(Key(key))
    }
}

/// A digest of the regions an anchor declares — §4.8's cheap check, one half.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Coverage {
    entries: Vec<(String, String)>,
}

impl Coverage {
    /// Reads the declared regions and digests each.
    ///
    /// A region the backend does not expose is an error rather than a skipped
    /// entry: an anchor declaring a region that is not there is a configuration
    /// mistake, and silently covering less than it says is how a cheap check
    /// comes to check nothing.
    pub fn of(platform: &dyn Platform, covers: &[String]) -> Result<Self, ReadError> {
        let mut entries = Vec::new();
        for name in covers {
            let bytes = platform.read(name)?;
            let out = Sha256::digest(&bytes);
            let hex = out.iter().fold(String::with_capacity(64), |mut s, byte| {
                use std::fmt::Write;
                let _ = write!(s, "{byte:02x}");
                s
            });
            entries.push((name.clone(), hex));
        }
        Ok(Coverage { entries })
    }

    /// A coverage from digests somebody else computed — a cache reading one
    /// back, or a test.
    ///
    /// The digests must have come from `of`, because a comparison between a
    /// digest of these bytes and a digest of something else computed another
    /// way says nothing. There is no way to check that here, which is why this
    /// says so rather than pretending to.
    pub fn from_digests(entries: Vec<(String, String)>) -> Self {
        Coverage { entries }
    }

    /// The regions and their digests, in the order they were read.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(n, d)| (n.as_str(), d.as_str()))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Which regions differ from `other`, by name. Empty when they agree.
    pub fn differing(&self, other: &Coverage) -> Vec<String> {
        let mut out = Vec::new();
        for (name, digest) in &self.entries {
            match other.entries.iter().find(|(n, _)| n == name) {
                Some((_, theirs)) if theirs == digest => {}
                _ => out.push(name.clone()),
            }
        }
        for (name, _) in &other.entries {
            if !self.entries.iter().any(|(n, _)| n == name) {
                out.push(name.clone());
            }
        }
        out
    }
}

/// What a load must reproduce for a blob to be believed — §4.8.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CheapCheck {
    pub position: Position,
    pub coverage: Coverage,
}

/// Why a cheap check failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckFailed {
    Position {
        expected: Position,
        found: Position,
    },
    Regions {
        differing: Vec<String>,
    },
}

impl std::fmt::Display for CheckFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckFailed::Position { expected, found } => write!(
                f,
                "the blob was taken at {expected} and the load arrived at {found}"
            ),
            CheckFailed::Regions { differing } => write!(
                f,
                "the load arrived in the right place and {} do not hold what the blob recorded: \
                 {}. The cache is stale, and it is about to be thrown away",
                if differing.len() == 1 {
                    "one region does"
                } else {
                    "these regions"
                },
                differing.join(", ")
            ),
        }
    }
}

impl std::error::Error for CheckFailed {}

impl CheapCheck {
    /// Compares what a load produced against what the blob recorded.
    ///
    /// **An anchor that declares no regions passes this on the position alone**,
    /// and that is worth knowing rather than hiding: §4.10's guidance is to
    /// declare what the comparisons read, and a check over nothing is the state
    /// an anchor is in until somebody does.
    pub fn verify(&self, position: &Position, coverage: &Coverage) -> Result<(), CheckFailed> {
        if position != &self.position {
            return Err(CheckFailed::Position {
                expected: self.position.clone(),
                found: position.clone(),
            });
        }
        let differing = self.coverage.differing(coverage);
        if !differing.is_empty() {
            return Err(CheckFailed::Regions { differing });
        }
        Ok(())
    }

    /// Whether this check is capable of catching anything beyond the position.
    pub fn covers_anything(&self) -> bool {
        !self.coverage.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provenance() -> Provenance {
        Provenance {
            reference: "ref-a".into(),
            backend: "a-backend".into(),
            version: "1.0.0".into(),
            software: "abcdef0123456789".into(),
        }
    }

    fn anchor(name: &str, start: Start, covers: &[&str]) -> Anchor {
        Anchor {
            name: name.into(),
            definition: Definition {
                start,
                bound: Bound::Frames(10),
                input: None,
            },
            covers: covers.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn chain_of_three() -> Anchors {
        Anchors::new(vec![
            anchor("boot", Start::PowerOn, &["work-ram"]),
            anchor("ready", Start::Anchor("boot".into()), &["work-ram"]),
            anchor("later", Start::Anchor("ready".into()), &["work-ram"]),
        ])
        .expect("distinct names")
    }

    // ---- the chain ------------------------------------------------------

    #[test]
    fn a_chain_runs_from_the_origin_to_the_anchor_asked_for() {
        let anchors = chain_of_three();
        let names: Vec<&str> = anchors
            .chain("later")
            .expect("it resolves")
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, ["boot", "ready", "later"], "origin first");
    }

    /// **A circle is refused rather than followed.** Followed, it is a replay
    /// that never finishes — and the loop would be inside the tool, where
    /// nothing times it out.
    #[test]
    fn a_circle_of_anchors_is_refused_and_named() {
        let anchors = Anchors::new(vec![
            anchor("a", Start::Anchor("b".into()), &[]),
            anchor("b", Start::Anchor("a".into()), &[]),
        ])
        .expect("distinct names");
        let err = anchors.chain("a").expect_err("a circle");
        match &err {
            AnchorError::Cycle { through } => {
                assert!(through.len() >= 2, "got {through:?}");
                assert!(through.contains(&"a".to_string()));
            }
            other => panic!("got {other}"),
        }
        assert!(err.to_string().contains("never finishes"), "said: {err}");

        // And an anchor defined on itself is the same mistake.
        let itself = Anchors::new(vec![anchor("a", Start::Anchor("a".into()), &[])]).unwrap();
        assert!(matches!(
            itself.chain("a").expect_err("itself"),
            AnchorError::Cycle { .. }
        ));
    }

    #[test]
    fn an_anchor_built_on_one_nobody_declared_is_refused_by_name() {
        let anchors = Anchors::new(vec![anchor("a", Start::Anchor("missing".into()), &[])]).unwrap();
        let err = anchors.chain("a").expect_err("no such parent");
        match &err {
            AnchorError::Unknown { name, declared } => {
                assert_eq!(name, "missing");
                assert_eq!(declared, &["a".to_string()]);
            }
            other => panic!("got {other}"),
        }
    }

    #[test]
    fn two_anchors_with_one_name_are_refused_at_construction() {
        let err = Anchors::new(vec![
            anchor("a", Start::PowerOn, &[]),
            anchor("a", Start::PowerOn, &[]),
        ])
        .expect_err("ambiguous");
        assert!(matches!(err, AnchorError::Duplicate { .. }), "got {err}");
    }

    /// §4.7's input log, against §7.3's declaration. The refusal is now the
    /// reference's answer rather than this module's assumption — but it is
    /// still a refusal: ignored, an anchor would be reached without the input
    /// it says it needs and the result would be called that anchor.
    #[test]
    fn an_anchor_needing_input_is_refused_by_a_reference_that_cannot_replay_one() {
        let mut needs = anchor("needs-input", Start::PowerOn, &[]);
        needs.definition.input = Some(InputLog {
            name: "press-start".into(),
            recorded: vec![1, 2, 3],
        });
        assert_eq!(
            needs.definition.requires(),
            vec![Capability::InputReplay],
            "the definition is what needs it, whoever is asking"
        );
        let anchors = Anchors::new(vec![needs]).unwrap();

        // Structurally it is fine, and that is the point of the split: the
        // configuration declaring it is not wrong.
        assert!(
            anchors.chain("needs-input").is_ok(),
            "nothing is wrong with the chain itself"
        );

        let err = anchors
            .chain_for("needs-input", &Capabilities::none())
            .expect_err("not declared");
        assert!(
            matches!(
                err,
                AnchorError::NeedsCapability {
                    capability: Capability::InputReplay,
                    ..
                }
            ),
            "got {err}"
        );
        assert!(err.to_string().contains("press-start"), "said: {err}");
        assert!(err.to_string().contains("input-replay"), "said: {err}");
        assert!(
            err.to_string().contains("arrive somewhere else"),
            "the refusal must say why ignoring it would be worse, said: {err}"
        );

        // And a reference that declares it gets the chain. This half is what
        // keeps the refusal from being a hard-coded `false`.
        assert!(
            anchors
                .chain_for("needs-input", &Capabilities::of([Capability::InputReplay]))
                .is_ok(),
            "a reference that can replay one must not be refused"
        );
    }

    /// The refusal names the anchor that needs the capability, not the one
    /// asked for. A chain is only as replayable as its weakest link, and a
    /// message naming the wrong link sends a reader to the wrong definition.
    #[test]
    fn a_definition_built_on_one_needing_input_is_refused_naming_the_one_that_needs_it() {
        let mut first = anchor("first", Start::PowerOn, &[]);
        first.definition.input = Some(InputLog {
            name: "press-start".into(),
            recorded: vec![1],
        });
        let second = anchor("second", Start::Anchor("first".into()), &[]);
        let anchors = Anchors::new(vec![first, second]).unwrap();

        let err = anchors
            .chain_for("second", &Capabilities::none())
            .expect_err("its prefix cannot be replayed");
        match err {
            AnchorError::NeedsCapability { ref anchor, .. } => {
                assert_eq!(anchor, "first", "said: {err}");
            }
            other => panic!("got {other}"),
        }
    }

    /// A definition with no input needs nothing, so a backend declaring
    /// nothing can still replay it. Without this, the gate would refuse every
    /// anchor the project actually uses.
    #[test]
    fn a_definition_needing_nothing_is_replayable_by_a_backend_declaring_nothing() {
        let anchors = Anchors::new(vec![anchor("plain", Start::PowerOn, &[])]).unwrap();
        assert!(anchors.get("plain").unwrap().definition.requires().is_empty());
        assert!(anchors.chain_for("plain", &Capabilities::none()).is_ok());
    }

    // ---- the key, §4.11 --------------------------------------------------

    /// **The test §4.11 exists for.** A key is transitive, so redefining a
    /// prefix invalidates everything below it. Without this, a blob stays valid
    /// after the anchor it was derived through has changed — and resuming it
    /// puts the machine somewhere nothing describes.
    #[test]
    fn redefining_a_prefix_changes_the_key_of_everything_below_it() {
        let before = chain_of_three()
            .key("later", &provenance())
            .expect("it resolves");

        let mut altered = vec![
            anchor("boot", Start::PowerOn, &["work-ram"]),
            anchor("ready", Start::Anchor("boot".into()), &["work-ram"]),
            anchor("later", Start::Anchor("ready".into()), &["work-ram"]),
        ];
        // Only the *first* anchor's bound changes.
        altered[0].definition.bound = Bound::Frames(11);
        let after = Anchors::new(altered)
            .unwrap()
            .key("later", &provenance())
            .expect("it resolves");

        assert_ne!(
            before, after,
            "a change to the root must invalidate the key of an anchor three links down"
        );
        assert_ne!(before.digest(), after.digest());
    }

    /// The reference and the software are in the key, because §16.5 says a
    /// behaviour change in a reference is a new reference and a blob from one
    /// is not a blob for another.
    #[test]
    fn the_reference_and_the_software_are_part_of_the_key() {
        let anchors = chain_of_three();
        let base = anchors.key("boot", &provenance()).unwrap();

        let mut other_version = provenance();
        other_version.version = "1.0.1".into();
        assert_ne!(base, anchors.key("boot", &other_version).unwrap());

        let mut other_software = provenance();
        other_software.software = "0000000000000000".into();
        assert_ne!(base, anchors.key("boot", &other_software).unwrap());
    }

    /// Reordering a covers list does not invalidate a cache that is still
    /// valid. The set is what matters, not the order somebody typed it in.
    #[test]
    fn reordering_the_covered_regions_does_not_change_the_key() {
        let one = Anchors::new(vec![anchor("a", Start::PowerOn, &["x", "y"])]).unwrap();
        let other = Anchors::new(vec![anchor("a", Start::PowerOn, &["y", "x"])]).unwrap();
        assert_eq!(
            one.key("a", &provenance()).unwrap(),
            other.key("a", &provenance()).unwrap()
        );

        // Adding one, though, does.
        let more = Anchors::new(vec![anchor("a", Start::PowerOn, &["x", "y", "z"])]).unwrap();
        assert_ne!(
            one.key("a", &provenance()).unwrap(),
            more.key("a", &provenance()).unwrap()
        );
    }

    /// A key is readable, which is the point of it being a string: somebody
    /// looking at a cache that keeps missing can see which part changed.
    #[test]
    fn a_key_says_what_it_is_made_of() {
        let key = chain_of_three().key("ready", &provenance()).unwrap();
        let said = key.as_str();
        for part in ["ref-a", "1.0.0", "abcdef0123456789", "boot", "ready"] {
            assert!(said.contains(part), "the key must name {part}: {said}");
        }
        assert!(
            !said.contains("later"),
            "and must not contain an anchor below it: {said}"
        );
        assert_eq!(key.digest().len(), 64);
    }

    // ---- the cheap check, §4.8 -------------------------------------------

    fn coverage(entries: &[(&str, &str)]) -> Coverage {
        Coverage::from_digests(
            entries
                .iter()
                .map(|(n, d)| ((*n).to_string(), (*d).to_string()))
                .collect(),
        )
    }

    #[test]
    fn a_check_passes_when_the_position_and_the_regions_both_match() {
        let check = CheapCheck {
            position: Position::FrameBoundary { frame: 7 },
            coverage: coverage(&[("work-ram", "aa"), ("palette-ram", "bb")]),
        };
        assert_eq!(
            check.verify(
                &Position::FrameBoundary { frame: 7 },
                &coverage(&[("work-ram", "aa"), ("palette-ram", "bb")])
            ),
            Ok(())
        );
        assert!(check.covers_anything());
    }

    /// A load that landed elsewhere, and a load that landed in the right place
    /// with the wrong contents, are different failures. The second is the one
    /// M1 found the position check cannot catch, and it is why the regions are
    /// digested at all.
    #[test]
    fn the_two_ways_a_check_fails_are_told_apart() {
        let check = CheapCheck {
            position: Position::FrameBoundary { frame: 7 },
            coverage: coverage(&[("work-ram", "aa")]),
        };

        let wrong_place = check
            .verify(
                &Position::FrameBoundary { frame: 8 },
                &coverage(&[("work-ram", "aa")]),
            )
            .expect_err("elsewhere");
        assert!(matches!(wrong_place, CheckFailed::Position { .. }));

        let wrong_contents = check
            .verify(
                &Position::FrameBoundary { frame: 7 },
                &coverage(&[("work-ram", "zz")]),
            )
            .expect_err("stale");
        match &wrong_contents {
            CheckFailed::Regions { differing } => assert_eq!(differing, &["work-ram".to_string()]),
            other => panic!("got {other}"),
        }
        assert!(wrong_contents.to_string().contains("stale"), "said: {wrong_contents}");
        assert_ne!(wrong_place.to_string(), wrong_contents.to_string());
    }

    /// A region missing from one side counts as differing. Treated as
    /// agreement, an anchor whose coverage shrank would keep passing a check
    /// that no longer looks at what it used to.
    #[test]
    fn a_region_present_on_one_side_only_is_a_difference() {
        let a = coverage(&[("work-ram", "aa"), ("palette-ram", "bb")]);
        let b = coverage(&[("work-ram", "aa")]);
        assert_eq!(a.differing(&b), ["palette-ram".to_string()]);
        assert_eq!(b.differing(&a), ["palette-ram".to_string()]);
    }

    /// An anchor that declares nothing is checked on its position alone, and
    /// says so. §4.10 tells a user to declare what their comparisons read; this
    /// is what it costs not to.
    #[test]
    fn an_anchor_that_declares_nothing_admits_it_checks_nothing() {
        let check = CheapCheck {
            position: Position::FrameBoundary { frame: 1 },
            coverage: coverage(&[]),
        };
        assert!(!check.covers_anything());
        assert_eq!(
            check.verify(&Position::FrameBoundary { frame: 1 }, &coverage(&[])),
            Ok(()),
            "it passes, and `covers_anything` is how a caller finds out how little that means"
        );
    }
}
