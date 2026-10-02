//! Snapshots — §3.2.
//!
//! A snapshot is the contents of **some** set of regions, the processor state,
//! the position, and the identity of the backend and software it came from.
//!
//! # The word that shapes the whole module: *some*
//!
//! A snapshot need not be complete, and **a snapshot that omits a region is not
//! a snapshot saying the region is empty** (§3.5). So there is no accessor here
//! that turns an absent region into bytes — no `bytes_or_empty`, no `Default`
//! for a capture, nothing that returns a zero-filled buffer for a name nobody
//! captured. `get` returns an `Option`, and a comparison has to say what it
//! does with `None`.
//!
//! That is §3.5's "optional in the type" taken literally: the type is where the
//! rule is kept, because a rule kept in a convention is a rule until somebody
//! is in a hurry.
//!
//! # Why `Snapshot` is not `PartialEq`
//!
//! Deliberately. `a == b` would be a two-valued answer to the question §2.3
//! says has three, and it would be the shortest thing to write. Comparing
//! snapshots goes through a comparison that produces a `Verdict`, which has to
//! account for what was not captured on either side.
//!
//! # Seeding a snapshot is not resuming a blob
//!
//! §3.4 says a snapshot cannot be seeded at a position that is not an
//! instruction boundary — there is no instruction to begin at. That rule is
//! about *this*: writing memory and registers into a machine and letting it go.
//! It does not apply to a backend-opaque blob (§4.7), which restores whatever
//! the reference was in the middle of; measured, and recorded in
//! `doc/backend.md`. The two are different mechanisms and `can_be_seeded` is
//! about this one.

use crate::region::{Region, SpanError};
use crate::run::Position;

/// Where a snapshot came from — §3.2, §6.6.
///
/// Carried because a comparison between snapshots taken against different
/// references, or different software, is not a comparison (§16.5). A behaviour
/// change in the reference is a new reference, so a snapshot is only
/// interpretable against the thing that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// The reference's configured name (§6.4) — what reports call it.
    pub reference: String,
    /// The backend implementation behind that name.
    pub backend: String,
    /// The version the loaded library reported (§16.1).
    pub version: String,
    /// The software's identity (§6.6).
    pub software: String,
}

impl std::fmt::Display for Provenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({} {}) on software {}",
            self.reference,
            self.backend,
            self.version,
            &self.software[..self.software.len().min(12)]
        )
    }
}

/// Why two snapshots cannot be compared at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotComparable {
    /// §16.5. The references are not the same reference.
    DifferentReference { first: String, second: String },
    /// The software is not the same software (§6.6).
    DifferentSoftware { first: String, second: String },
}

impl std::fmt::Display for NotComparable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotComparable::DifferentReference { first, second } => write!(
                f,
                "these snapshots came from different references — {first} and {second}. A \
                 behaviour change in a reference is a new reference (§16.5), so what one says \
                 is not evidence about the other"
            ),
            NotComparable::DifferentSoftware { first, second } => write!(
                f,
                "these snapshots came from different software — {} and {}. Comparing them \
                 would be comparing two programs (§6.6)",
                &first[..first.len().min(12)],
                &second[..second.len().min(12)]
            ),
        }
    }
}

impl std::error::Error for NotComparable {}

impl Provenance {
    /// Whether a snapshot from here may be compared against one from there.
    ///
    /// The reference's *name* is what is checked rather than the backend and
    /// version separately, because §6.4 gives two builds of one backend
    /// different names for exactly this reason — and then the version is
    /// checked too, because the same name can be pointed at a new build.
    pub fn comparable_with(&self, other: &Provenance) -> Result<(), NotComparable> {
        if self.software != other.software {
            return Err(NotComparable::DifferentSoftware {
                first: self.software.clone(),
                second: other.software.clone(),
            });
        }
        if self.reference != other.reference
            || self.backend != other.backend
            || self.version != other.version
        {
            return Err(NotComparable::DifferentReference {
                first: format!("{} {} {}", self.reference, self.backend, self.version),
                second: format!("{} {} {}", other.reference, other.backend, other.version),
            });
        }
        Ok(())
    }
}

/// The processor state, as this version carries it — §3.3.
///
/// Opaque, and that is a decision rather than a shortcut. The backend's own
/// write of this state is a copy of the same bytes its read produced
/// (`doc/backend.md`), so handing them back reproduces the state exactly
/// without this project transcribing a register layout it would have to guess
/// at. §7.6's question about what a processor state *is* across platforms that
/// do not share a register file stays open, and §2.4 says not to answer it by
/// guessing a shape.
///
/// What this cannot do is say **which register** differs, which is what §5.4's
/// localisation needs. That is where the layout has to be transcribed, and it
/// is not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Processor {
    bytes: Vec<u8>,
}

impl Processor {
    pub fn opaque(bytes: Vec<u8>) -> Self {
        Processor { bytes }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// One region's contents, or a span of one, as captured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    /// The region as the backend declared it at the moment of capture. Kept
    /// because a region's size is a property of the software loaded (§3.1), so
    /// the declaration is part of what was observed.
    pub region: Region,
    /// Where in the region the bytes start. Carried because a span captured at
    /// an offset and compared as though it began at zero is a comparison of
    /// the wrong bytes that looks exactly like a comparison of the right ones.
    pub offset: usize,
    bytes: Vec<u8>,
}

impl Captured {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Whether this capture is the whole region.
    pub fn is_whole_region(&self) -> bool {
        self.offset == 0 && self.bytes.len() == self.region.size
    }

    /// Whether two captures cover the same bytes of the same region, which they
    /// must before their contents mean anything compared.
    pub fn covers_the_same_as(&self, other: &Captured) -> bool {
        self.region.name == other.region.name
            && self.offset == other.offset
            && self.bytes.len() == other.bytes.len()
    }
}

/// A snapshot — §3.2.
///
/// Built through `Builder`, so that a capture which does not fit the region it
/// claims cannot be put in one.
#[derive(Debug, Clone)]
pub struct Snapshot {
    provenance: Provenance,
    position: Position,
    processor: Option<Processor>,
    captures: Vec<Captured>,
}

impl Snapshot {
    pub fn builder(provenance: Provenance, position: Position) -> Builder {
        Builder {
            provenance,
            position,
            processor: None,
            captures: Vec::new(),
        }
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn position(&self) -> &Position {
        &self.position
    }

    /// The processor state, or `None` when it was not captured.
    ///
    /// `None` is not "all registers were zero". §3.3 says a comparison seeded
    /// without the processor state runs somebody else's routine with these
    /// registers, so the difference between absent and zero is the difference
    /// between a comparison and a wrong answer.
    pub fn processor(&self) -> Option<&Processor> {
        self.processor.as_ref()
    }

    /// What was captured, in the order it was captured.
    pub fn captures(&self) -> impl Iterator<Item = &Captured> {
        self.captures.iter()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.captures.iter().map(|c| c.region.name.as_str())
    }

    /// The capture for that region name, or `None` — §3.5.
    ///
    /// There is no variant of this that returns bytes for a name nobody
    /// captured. A caller holding `None` has to decide what that means, and
    /// §2.3 has already decided: *not determined*.
    pub fn get(&self, region: &str) -> Option<&Captured> {
        self.captures.iter().find(|c| c.region.name == region)
    }

    pub fn len(&self) -> usize {
        self.captures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.captures.is_empty()
    }

    /// How many bytes this snapshot holds across every capture, for a report
    /// that has to say what a comparison was over.
    pub fn byte_count(&self) -> usize {
        self.captures.iter().map(|c| c.bytes.len()).sum()
    }

    /// Whether this snapshot can be written back into a machine — §3.4.
    ///
    /// Only at an instruction boundary. Anywhere else there is no instruction
    /// to begin at, and a reference started there is running from the middle of
    /// something. This is about structured seeding and not about resuming a
    /// backend-opaque blob, which has no such restriction (§4.7).
    pub fn can_be_seeded(&self) -> bool {
        self.position.is_instruction_boundary()
    }
}

/// Collects a snapshot, refusing a capture that does not fit its region.
#[derive(Debug, Clone)]
pub struct Builder {
    provenance: Provenance,
    position: Position,
    processor: Option<Processor>,
    captures: Vec<Captured>,
}

/// Why a snapshot could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// The bytes do not fit the region they claim to come from.
    Span(SpanError),
    /// The bytes are not as many as the span says.
    LengthDisagrees {
        region: String,
        claimed: usize,
        given: usize,
    },
    /// Two captures for one region name, which would make `get` depend on
    /// insertion order.
    AlreadyCaptured { region: String },
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Span(e) => write!(f, "{e}"),
            BuildError::LengthDisagrees {
                region,
                claimed,
                given,
            } => write!(
                f,
                "the capture of `{region}` claims {claimed} bytes and carries {given}"
            ),
            BuildError::AlreadyCaptured { region } => write!(
                f,
                "`{region}` is already captured in this snapshot; two captures of one name would \
                 make a lookup depend on which was added first"
            ),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<SpanError> for BuildError {
    fn from(e: SpanError) -> Self {
        BuildError::Span(e)
    }
}

impl Builder {
    pub fn processor(mut self, processor: Processor) -> Self {
        self.processor = Some(processor);
        self
    }

    /// Captures a whole region.
    pub fn whole(self, region: Region, bytes: Vec<u8>) -> Result<Self, BuildError> {
        let size = region.size;
        self.span(region, 0, bytes).and_then(|b| {
            let last = b.captures.last().expect("just pushed");
            if last.bytes.len() != size {
                return Err(BuildError::LengthDisagrees {
                    region: last.region.name.clone(),
                    claimed: size,
                    given: last.bytes.len(),
                });
            }
            Ok(b)
        })
    }

    /// Captures a span of one.
    ///
    /// The span is checked against the region, so a snapshot cannot hold a
    /// capture that claims to be bytes the region does not have.
    pub fn span(
        mut self,
        region: Region,
        offset: usize,
        bytes: Vec<u8>,
    ) -> Result<Self, BuildError> {
        if self.captures.iter().any(|c| c.region.name == region.name) {
            return Err(BuildError::AlreadyCaptured {
                region: region.name,
            });
        }
        region.span(offset, bytes.len())?;
        self.captures.push(Captured {
            region,
            offset,
            bytes,
        });
        Ok(self)
    }

    pub fn build(self) -> Snapshot {
        Snapshot {
            provenance: self.provenance,
            position: self.position,
            processor: self.processor,
            captures: self.captures,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::Access;

    fn provenance() -> Provenance {
        Provenance {
            reference: "ref-a".into(),
            backend: "a-backend".into(),
            version: "1.0.0".into(),
            software: "abcdef0123456789".into(),
        }
    }

    fn work() -> Region {
        Region::bytes("work", 256, Access::ReadWrite)
    }

    fn snapshot() -> Snapshot {
        Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0x8000 })
            .processor(Processor::opaque(vec![1, 2, 3, 4]))
            .whole(work(), vec![7u8; 256])
            .expect("it fits")
            .build()
    }

    // ---- §3.5, the rule this module exists for -------------------------

    /// **The test this module exists for.** A region nobody captured comes back
    /// as `None`, and there is no accessor that would turn it into bytes.
    ///
    /// Written with a zero-filled fallback, a comparison over an uncaptured
    /// region would agree with anything that happened to be zero there, and
    /// report it as agreement.
    #[test]
    fn a_region_that_was_not_captured_is_none_and_not_empty_bytes() {
        let s = snapshot();
        assert!(s.get("work").is_some());
        assert_eq!(s.get("elsewhere"), None);
        assert_eq!(s.names().collect::<Vec<_>>(), ["work"]);
        assert_eq!(
            s.len(),
            1,
            "a snapshot's length is what it captured, not what the backend has"
        );
    }

    /// A capture that is there and empty is a different thing from one that is
    /// not there, and both are expressible.
    #[test]
    fn an_empty_capture_and_an_absent_one_are_distinguishable() {
        let s = Snapshot::builder(provenance(), Position::FrameBoundary { frame: 1 })
            .span(work(), 0, vec![])
            .expect("an empty span is a valid span")
            .build();

        let captured = s.get("work").expect("it is there, and it is empty");
        assert!(captured.is_empty());
        assert!(s.get("elsewhere").is_none(), "this one is not there at all");
    }

    // ---- captures describe what they cover -----------------------------

    /// A span records where it starts. Two spans of the same length at
    /// different offsets are not the same bytes, and comparing them as though
    /// they were looks exactly like comparing the right ones.
    #[test]
    fn two_spans_at_different_offsets_do_not_cover_the_same_bytes() {
        let a = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .span(work(), 0x10, vec![0u8; 16])
            .unwrap()
            .build();
        let b = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .span(work(), 0x20, vec![0u8; 16])
            .unwrap()
            .build();

        let (a, b) = (a.get("work").unwrap(), b.get("work").unwrap());
        assert_eq!(a.bytes(), b.bytes(), "the contents happen to be equal");
        assert!(
            !a.covers_the_same_as(b),
            "but they are different bytes of the region, and a comparison must not call this \
             agreement"
        );
        assert!(!a.is_whole_region());
    }

    #[test]
    fn a_whole_region_capture_says_so() {
        let s = snapshot();
        let c = s.get("work").unwrap();
        assert!(c.is_whole_region());
        assert_eq!(c.offset, 0);
        assert_eq!(c.len(), 256);
    }

    // ---- the builder refuses what cannot be true -----------------------

    /// Bytes that do not fit the region are refused. Without the check a
    /// snapshot could hold 512 bytes of a 256-byte region and every comparison
    /// over it would be over something that does not exist.
    #[test]
    fn a_capture_that_does_not_fit_its_region_is_refused() {
        let err = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .whole(work(), vec![0u8; 512])
            .expect_err("twice the region");
        assert!(matches!(err, BuildError::Span(_)), "got {err:?}");

        let err = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .span(work(), 250, vec![0u8; 10])
            .expect_err("runs off the end");
        assert!(matches!(err, BuildError::Span(_)), "got {err:?}");
        assert!(err.to_string().contains("work"));
    }

    /// `whole` means whole. A short read presented as a whole region would
    /// compare equal over the bytes nobody looked at.
    #[test]
    fn a_short_capture_is_not_a_whole_region() {
        let err = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .whole(work(), vec![0u8; 100])
            .expect_err("not the whole region");
        assert_eq!(
            err,
            BuildError::LengthDisagrees {
                region: "work".into(),
                claimed: 256,
                given: 100
            }
        );
    }

    #[test]
    fn one_region_cannot_be_captured_twice() {
        let err = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .span(work(), 0, vec![1u8; 4])
            .unwrap()
            .span(work(), 8, vec![2u8; 4])
            .expect_err("already captured");
        assert!(matches!(err, BuildError::AlreadyCaptured { .. }), "got {err:?}");
    }

    // ---- provenance, §16.5 ---------------------------------------------

    /// §16.5: a snapshot from one version is not evidence about another. This
    /// is the check that stops a comparison being run across an upgrade and
    /// reported as the developer's error.
    #[test]
    fn snapshots_from_different_versions_are_not_comparable() {
        let a = provenance();
        let mut b = provenance();
        b.version = "1.0.1".into();
        let err = a.comparable_with(&b).expect_err("different versions");
        assert!(
            matches!(err, NotComparable::DifferentReference { .. }),
            "got {err:?}"
        );
        assert!(err.to_string().contains("new reference"));
        assert_eq!(a.comparable_with(&a), Ok(()));
    }

    /// And different software is a different refusal, because it is a different
    /// problem: one is an upgrade, the other is the wrong file.
    #[test]
    fn snapshots_from_different_software_are_refused_as_that_and_not_as_a_version() {
        let a = provenance();
        let mut b = provenance();
        b.software = "0000000000000000".into();
        let err = a.comparable_with(&b).expect_err("different software");
        assert!(
            matches!(err, NotComparable::DifferentSoftware { .. }),
            "got {err:?}"
        );
        assert_ne!(
            err,
            NotComparable::DifferentReference {
                first: String::new(),
                second: String::new()
            }
        );
    }

    // ---- §3.3 and §3.4 --------------------------------------------------

    /// An uncaptured processor state is `None`, not a zeroed one. §3.3 says a
    /// comparison seeded without it runs some other routine's registers.
    #[test]
    fn an_uncaptured_processor_state_is_absent_rather_than_zero() {
        let s = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 }).build();
        assert!(s.processor().is_none());
        assert_eq!(snapshot().processor().map(|p| p.len()), Some(4));
    }

    /// §3.4, which M0 measured on a real reference: a frame boundary is not an
    /// instruction boundary, so a snapshot taken there cannot be seeded.
    #[test]
    fn only_a_snapshot_at_an_instruction_boundary_can_be_seeded() {
        let at = |position| Snapshot::builder(provenance(), position).build().can_be_seeded();
        assert!(at(Position::InstructionBoundary { pc: 0x8000 }));
        assert!(!at(Position::FrameBoundary { frame: 7 }));
        assert!(!at(Position::MidInstruction { pc: 0x8000 }));
        assert!(!at(Position::Unclassified { pc: 0x8000 }));
    }

    #[test]
    fn a_snapshot_says_what_it_holds_and_where_it_came_from() {
        let s = snapshot();
        assert_eq!(s.byte_count(), 256);
        assert_eq!(s.provenance().reference, "ref-a");
        assert!(s.provenance().to_string().contains("ref-a"));
        assert!(
            s.provenance().to_string().contains("abcdef012345"),
            "the software's identity is in the report, abbreviated"
        );
        assert_eq!(s.position(), &Position::InstructionBoundary { pc: 0x8000 });
    }
}
