//! Backend-opaque state blobs — §7.2, and the mechanism §4.7 arrives with.
//!
//! # A blob is not a snapshot, and the difference is load-bearing
//!
//! A [`Snapshot`](crate::snapshot::Snapshot) is for **comparing**: it holds the
//! bytes of named regions, and anything above the platform boundary can read
//! them. A blob is for **arriving**: nothing here can read what is inside it,
//! and that is the point — it is whatever the backend needs to put its machine
//! back exactly as it was, including the parts §3.2's model does not name and
//! the parts nobody has thought of.
//!
//! The asymmetry that matters: **a snapshot cannot be seeded at a position that
//! is not an instruction boundary** (§3.4), because there is no instruction to
//! begin at. **A blob has no such restriction** — it restores whatever the
//! reference was in the middle of. That was measured, not assumed: a blob taken
//! at a frame boundary, which is usually mid-instruction, resumed exactly, and
//! running onward from the resume agreed with running onward from a replay
//! (`doc/backend.md`).
//!
//! # Why a blob carries its position, and why that position is the one *after*
//!
//! Also measured. On the first backend, saving a blob **advances the machine**
//! to the next place its debugger can break, which completes whatever
//! instruction was in progress. So the position a blob is of is the position
//! read after the save, never before; read the other way it appears to be off
//! by one.
//!
//! Carrying it is not bookkeeping. That backend's load reports nothing — given
//! nonsense it leaves the machine alone and says so to nobody — so comparing
//! the position after a load against the position the blob recorded is the
//! **only** way to find out whether the load did anything (§13's Q12). A blob
//! that did not carry its position would make that check impossible to write.

use crate::run::Position;

/// Opaque state, as the backend produced it.
///
/// `PartialEq` on purpose, and only on purpose: two blobs being byte-identical
/// is a fact about two files, not a verdict about two machines. Nothing here
/// invites it to be read as agreement, because nothing above the backend can
/// interpret the bytes at all.
#[derive(Clone, PartialEq, Eq)]
pub struct Blob {
    bytes: Vec<u8>,
    position: Position,
    fingerprint: Vec<u8>,
}

impl std::fmt::Debug for Blob {
    /// Says what it is and not what it holds. Several hundred kilobytes of
    /// opaque bytes in a panic message helps nobody.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Blob({} bytes, at {}, fingerprint of {})",
            self.bytes.len(),
            self.position,
            self.fingerprint.len()
        )
    }
}

impl Blob {
    /// `position` must be the position **after** the save — see this module's
    /// header for why that is not a detail.
    pub fn new(bytes: Vec<u8>, position: Position, fingerprint: Vec<u8>) -> Self {
        Blob {
            bytes,
            position,
            fingerprint,
        }
    }

    /// The bytes, for a backend to hand back to itself, or for a cache to hold.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Where the machine was when this was taken, which a load must arrive at.
    pub fn position(&self) -> &Position {
        &self.position
    }

    /// Something cheap the backend can read back after a load to see whether
    /// the load did anything. Opaque to everything above the backend, which is
    /// the only layer that knows what it put here.
    ///
    /// It exists because the position alone is not enough: this backend leaves
    /// the machine untouched when a load fails, so a load attempted while the
    /// machine is still *at* the blob's position would pass a position check
    /// without having done anything. A fingerprint over something monotonic —
    /// a cycle count — tells those apart as soon as the machine has moved at
    /// all.
    ///
    /// It is **not** §4.11's cheap check, which digests the regions an anchor
    /// declares. This is the narrower thing available without an anchor.
    pub fn fingerprint(&self) -> &[u8] {
        &self.fingerprint
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// Why a blob could not be taken, or could not be put back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// The reference is running. A blob taken from a running machine is of no
    /// particular moment, and one loaded into a running machine is overtaken by
    /// execution before anything can observe it (`doc/backend.md`).
    NotStopped,
    /// The load did not land where the blob says it was taken.
    ///
    /// On a backend whose load reports nothing, this is the **only** way a
    /// failed load is detected, which is why it is a distinct variant and not a
    /// `Backend` string: a caller may want to tell "it refused" from "it
    /// claimed to work and did not".
    LandedElsewhere {
        expected: Position,
        found: Position,
    },
    /// The load arrived at the right position and the machine is not the
    /// machine the blob was taken from.
    FingerprintDiffers,
    Backend { why: String },
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::NotStopped => write!(
                f,
                "the reference is running; stop it first, or what comes back is of no particular \
                 moment"
            ),
            StateError::LandedElsewhere { expected, found } => write!(
                f,
                "the blob was taken at {expected} and loading it arrived at {found}. The backend \
                 reports nothing about a load that fails, so this is what a failed load looks \
                 like — the machine is most likely untouched"
            ),
            StateError::FingerprintDiffers => write!(
                f,
                "the load arrived where the blob was taken and the machine is not the one the \
                 blob holds — most likely the blob is for other software, which this backend \
                 does not refuse for itself"
            ),
            StateError::Backend { why } => write!(f, "the backend could not do it: {why}"),
        }
    }
}

impl std::error::Error for StateError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob() -> Blob {
        Blob::new(
            vec![0xAB; 1024],
            Position::FrameBoundary { frame: 7 },
            vec![1, 2, 3, 4, 5, 6, 7, 8],
        )
    }

    /// A blob keeps the position it was taken at, which is what makes a failed
    /// load detectable on a backend that reports nothing about one.
    #[test]
    fn a_blob_carries_where_it_was_taken() {
        let b = blob();
        assert_eq!(b.position(), &Position::FrameBoundary { frame: 7 });
        assert_eq!(b.len(), 1024);
        assert_eq!(b.fingerprint().len(), 8);
    }

    /// **The variant that exists because of a measurement.** The first
    /// backend's load returns nothing at all, so a load that did nothing and a
    /// load that worked are the same call. The position check is the detector,
    /// and its message has to say that much or whoever reads it will go looking
    /// for a bug in their own code.
    #[test]
    fn landing_elsewhere_names_both_positions_and_says_what_it_means() {
        let err = StateError::LandedElsewhere {
            expected: Position::FrameBoundary { frame: 7 },
            found: Position::InstructionBoundary { pc: 0xC400CF },
        };
        let said = err.to_string();
        assert!(said.contains("frame boundary 7"), "said: {said}");
        assert!(said.contains("C400CF"), "said: {said}");
        assert!(
            said.contains("failed load"),
            "the message must say what this actually is, said: {said}"
        );
    }

    /// A blob taken at a frame boundary is the ordinary case, and §4.7 depends
    /// on it being usable. A snapshot taken there is not seedable (§3.4); this
    /// is the asymmetry, and nothing here refuses the blob for it.
    #[test]
    fn a_blob_from_a_position_no_snapshot_could_seed_is_still_a_blob() {
        for position in [
            Position::FrameBoundary { frame: 1 },
            Position::MidInstruction { pc: 0x8000 },
            Position::Unclassified { pc: 0x8000 },
        ] {
            let b = Blob::new(vec![1, 2, 3], position.clone(), vec![]);
            assert_eq!(b.position(), &position, "no position is refused here");
        }
    }

    /// The debug form says what it is without printing what it holds.
    #[test]
    fn a_blob_does_not_print_its_contents() {
        let said = format!("{:?}", blob());
        assert!(said.contains("1024 bytes"), "said: {said}");
        assert!(said.contains("frame boundary 7"), "said: {said}");
        assert!(
            said.len() < 200,
            "a blob's debug form must not be its contents, got {} characters",
            said.len()
        );
    }

    /// The errors are distinguishable, because what to do about them differs:
    /// one is a mistake in the call, one is a stale cache, one is the wrong file.
    /// The three failures are different things to be told, and a caller may
    /// well act differently on each: one is a mistake in the call, one is a
    /// load that did not take, one is a blob that is not of this machine.
    #[test]
    fn the_state_errors_read_differently_from_each_other() {
        let said: Vec<String> = [
            StateError::NotStopped,
            StateError::LandedElsewhere {
                expected: Position::FrameBoundary { frame: 1 },
                found: Position::FrameBoundary { frame: 2 },
            },
            StateError::FingerprintDiffers,
        ]
        .iter()
        .map(|e| e.to_string())
        .collect();

        assert!(said[0].contains("stop it first"), "said: {}", said[0]);
        assert!(said[1].contains("failed load"), "said: {}", said[1]);
        assert!(said[2].contains("other software"), "said: {}", said[2]);
        for (i, a) in said.iter().enumerate() {
            for b in &said[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
