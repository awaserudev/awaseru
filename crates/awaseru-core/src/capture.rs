//! Taking a snapshot off a reference, and putting one back — §3.2, §3.3, §3.4.
//!
//! Two functions, and most of what is interesting about them is what they
//! refuse.
//!
//! `seed` refuses a snapshot that is not at an instruction boundary, because
//! there is no instruction to begin at (§3.4). It refuses one with no processor
//! state, because §3.3 says a comparison seeded without the registers runs some
//! other routine's registers and then reports the difference as that routine's
//! error. Neither refusal is a convenience that could be relaxed: both are the
//! difference between a comparison and a wrong answer.
//!
//! And `seed` is **not** how a reference is brought to a position. It writes
//! memories and registers, and a console is more than those (§4.7); what
//! arrives somewhere is a blob. `seed` is for putting a routine's inputs in
//! place before running that routine, which is §5.6's unit of work.

use crate::platform::{Platform, ReadError, WriteError};
use crate::run::Position;
use crate::snapshot::{BuildError, Provenance, Snapshot};
use crate::verdict::Undetermined;

/// Why a snapshot could not be taken.
#[derive(Debug)]
pub enum CaptureError {
    Read(ReadError),
    /// The bytes that came back do not fit the region they came from, which
    /// means the backend's declaration and its read disagree.
    Malformed(BuildError),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::Read(e) => write!(f, "{e}"),
            CaptureError::Malformed(e) => write!(
                f,
                "the backend's read and its own declaration disagree: {e}"
            ),
        }
    }
}

impl std::error::Error for CaptureError {}

impl From<ReadError> for CaptureError {
    fn from(e: ReadError) -> Self {
        CaptureError::Read(e)
    }
}

impl From<BuildError> for CaptureError {
    fn from(e: BuildError) -> Self {
        CaptureError::Malformed(e)
    }
}

/// Why a snapshot could not be put back.
#[derive(Debug)]
pub enum SeedError {
    /// §3.4. There is no instruction to begin at.
    NotAnInstructionBoundary { position: Position },
    /// §3.3. The registers are not optional.
    NoProcessorState,
    Write(WriteError),
}

impl std::fmt::Display for SeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SeedError::NotAnInstructionBoundary { position } => write!(
                f,
                "this snapshot was taken at {position}, and a state can only be written into a \
                 machine at an instruction boundary — anywhere else there is no instruction to \
                 begin at (§3.4). To arrive at a position like this one, resume a blob instead \
                 (§4.7)"
            ),
            SeedError::NoProcessorState => write!(
                f,
                "this snapshot carries no processor state, and seeding without the registers \
                 runs the routine with some other routine's registers — which then gets reported \
                 as that routine's error (§3.3)"
            ),
            SeedError::Write(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SeedError {}

impl From<WriteError> for SeedError {
    fn from(e: WriteError) -> Self {
        SeedError::Write(e)
    }
}

/// Reads the named regions and the processor state into a snapshot.
///
/// `position` is the caller's, because the trait has no "where are you" and
/// should not: a position is something a run arrived at, and the run that
/// arrived there is what knows it (§4.3).
///
/// `which` is the caller's too. §3.2 says a snapshot need not be complete, and
/// a function that captured everything would make the common case — a routine's
/// few regions — cost a console's worth of bytes (§3.6 is still open).
pub fn capture(
    platform: &dyn Platform,
    provenance: Provenance,
    position: Position,
    which: &[&str],
) -> Result<Snapshot, CaptureError> {
    let regions = platform.regions();
    let mut builder = Snapshot::builder(provenance, position);

    for name in which {
        // Absent is reported as absent rather than skipped: a snapshot missing
        // a region the caller asked for would be indistinguishable from one
        // where the caller did not ask (§3.5).
        let declared = regions.get(name).ok_or_else(|| ReadError::Absent {
            region: (*name).to_string(),
        })?;
        let bytes = platform.read(name)?;
        builder = builder.whole(declared.clone(), bytes)?;
    }

    // Not conditional. A snapshot without it cannot be seeded (§3.3), and a
    // caller who wanted one without would be asking for something that cannot
    // be put back.
    let processor = platform.read_processor()?;
    Ok(builder.processor(processor).build())
}

/// Reads spans rather than whole regions — §5.1's "a span within one".
///
/// What a routine touches is a few spans, not a console's worth of memory, and
/// §5.6 makes routine-level the primary unit. Capturing whole regions for it
/// would make the common case cost the uncommon one's bytes.
///
/// The processor state is captured too, for the same reason `capture` does it:
/// a snapshot without it cannot be seeded back (§3.3).
pub fn capture_spans(
    platform: &dyn Platform,
    provenance: Provenance,
    position: Position,
    spans: &[(&str, usize, usize)],
) -> Result<Snapshot, CaptureError> {
    let regions = platform.regions();
    let mut builder = Snapshot::builder(provenance, position);

    for (name, offset, length) in spans {
        let declared = regions.get(name).ok_or_else(|| ReadError::Absent {
            region: (*name).to_string(),
        })?;
        let bytes = platform.read_span(name, *offset, *length)?;
        builder = builder.span(declared.clone(), *offset, bytes)?;
    }

    let processor = platform.read_processor()?;
    Ok(builder.processor(processor).build())
}

/// Writes a snapshot's regions and processor state back into a machine.
///
/// Refuses, in this order: a position no state can be written at (§3.4), and a
/// snapshot with no processor state (§3.3). Both before anything is written, so
/// a refusal leaves the machine as it was.
pub fn seed(platform: &mut dyn Platform, snapshot: &Snapshot) -> Result<(), SeedError> {
    if !snapshot.can_be_seeded() {
        return Err(SeedError::NotAnInstructionBoundary {
            position: snapshot.position().clone(),
        });
    }
    let Some(processor) = snapshot.processor() else {
        return Err(SeedError::NoProcessorState);
    };

    for captured in snapshot.captures() {
        if captured.is_whole_region() {
            platform.write(&captured.region.name, captured.bytes())?;
        } else {
            platform.write_span(&captured.region.name, captured.offset, captured.bytes())?;
        }
    }
    // Last, because it is what makes the machine start where the snapshot says.
    platform.write_processor(processor)?;
    Ok(())
}

/// Seeds from **any** position, and hands back what the caller has undertaken
/// to carry — §3.4's other half.
///
/// §3.4 says the tool refuses to seed from a non-instruction boundary *unless
/// the caller asks for that explicitly and accepts the result as not
/// determined*. This is that path, and it is shaped so that accepting is not
/// optional: what comes back on the loose path is the cause itself, and a
/// caller that ignores it has visibly dropped a value.
///
/// `Ok(None)` means the position was fine and there is nothing to carry.
/// `Ok(Some(cause))` means the state went in anyway, and every verdict from the
/// run that follows is *not determined* — fold the cause in.
///
/// The processor state is still required (§3.3). That refusal is not a
/// position's to excuse.
pub fn seed_from_any_position(
    platform: &mut dyn Platform,
    snapshot: &Snapshot,
) -> Result<Option<Undetermined>, SeedError> {
    if snapshot.can_be_seeded() {
        seed(platform, snapshot)?;
        return Ok(None);
    }

    let Some(processor) = snapshot.processor() else {
        return Err(SeedError::NoProcessorState);
    };
    for captured in snapshot.captures() {
        if captured.is_whole_region() {
            platform.write(&captured.region.name, captured.bytes())?;
        } else {
            platform.write_span(&captured.region.name, captured.offset, captured.bytes())?;
        }
    }
    platform.write_processor(processor)?;

    Ok(Some(Undetermined::SeededFromNoBoundary {
        position: snapshot.position().to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::{Blob, StateError};
    use crate::region::{Access, Region, Regions};
    use crate::run::{Bound, Reason, Stop};
    use crate::snapshot::Processor;
    use crate::{BackendVersion, RunError, check_read, check_write};

    /// A platform with two regions and nothing behind them but a vector, so
    /// that `capture` and `seed` can be tested without a backend. It is
    /// deliberately simple: what is under test is the two functions' refusals,
    /// not an emulator.
    struct Fake {
        regions: Regions,
        work: Vec<u8>,
        rom: Vec<u8>,
        processor: Option<Processor>,
        writes: Vec<String>,
    }

    impl Fake {
        fn new() -> Self {
            Fake {
                regions: Regions::new(vec![
                    Region::bytes("work", 8, Access::ReadWrite),
                    Region::bytes("rom", 8, Access::ReadOnly),
                ]),
                work: vec![0; 8],
                rom: vec![9; 8],
                processor: Some(Processor::opaque(vec![1, 2, 3, 4])),
                writes: Vec::new(),
            }
        }
    }

    impl Platform for Fake {
        fn version(&self) -> BackendVersion {
            BackendVersion {
                reported: "1.0.0".into(),
                built: None,
            }
        }
        fn beginning(&self) -> crate::platform::Beginning {
            crate::platform::Beginning {
                reproducible: true,
                settled: vec!["work".into()],
            }
        }
        fn regions(&self) -> Regions {
            self.regions.clone()
        }
        fn read(&self, region: &str) -> Result<Vec<u8>, ReadError> {
            check_read(&self.regions, region, None)?;
            Ok(match region {
                "work" => self.work.clone(),
                _ => self.rom.clone(),
            })
        }
        fn read_span(&self, region: &str, offset: usize, len: usize) -> Result<Vec<u8>, ReadError> {
            check_read(&self.regions, region, Some((offset, len)))?;
            Ok(self.read(region)?[offset..offset + len].to_vec())
        }
        fn run(&mut self, _bound: Bound) -> Result<Stop, RunError> {
            Ok(Stop {
                reason: Reason::BoundReached,
                position: Position::InstructionBoundary { pc: 0 },
            })
        }
        fn write(&mut self, region: &str, bytes: &[u8]) -> Result<(), WriteError> {
            check_write(&self.regions, region, None)?;
            self.writes.push(region.to_string());
            self.work = bytes.to_vec();
            Ok(())
        }
        fn write_span(
            &mut self,
            region: &str,
            offset: usize,
            bytes: &[u8],
        ) -> Result<(), WriteError> {
            check_write(&self.regions, region, Some((offset, bytes.len())))?;
            self.writes.push(format!("{region}+{offset}"));
            self.work[offset..offset + bytes.len()].copy_from_slice(bytes);
            Ok(())
        }
        fn read_processor(&self) -> Result<Processor, ReadError> {
            self.processor.clone().ok_or(ReadError::Backend {
                why: "this one has none".into(),
            })
        }
        fn write_processor(&mut self, processor: &Processor) -> Result<(), WriteError> {
            self.writes.push("processor".into());
            self.processor = Some(processor.clone());
            Ok(())
        }
        fn return_to_origin(&mut self) -> Result<(), RunError> {
            self.work = vec![0; 8];
            self.writes.push("origin".into());
            Ok(())
        }
        fn save_state(&mut self) -> Result<Blob, StateError> {
            Err(StateError::Backend {
                why: "not what this fake is for".into(),
            })
        }
        fn load_state(&mut self, _blob: &Blob) -> Result<(), StateError> {
            Err(StateError::Backend {
                why: "not what this fake is for".into(),
            })
        }
    }

    fn provenance() -> Provenance {
        Provenance {
            reference: "ref-a".into(),
            backend: "fake".into(),
            version: "1.0.0".into(),
            software: "abcdef0123456789".into(),
        }
    }

    fn at_instruction() -> Position {
        Position::InstructionBoundary { pc: 0x8000 }
    }

    #[test]
    fn capture_takes_what_it_was_asked_for_and_the_processor_state() {
        let fake = Fake::new();
        let snapshot =
            capture(&fake, provenance(), at_instruction(), &["work"]).expect("it captures");
        assert_eq!(snapshot.names().collect::<Vec<_>>(), ["work"]);
        assert!(
            snapshot.get("rom").is_none(),
            "a region not asked for is not in the snapshot"
        );
        assert!(
            snapshot.processor().is_some(),
            "§3.3: the processor state is not optional, so capture does not make it so"
        );
        assert!(snapshot.can_be_seeded());
    }

    #[test]
    fn capturing_a_region_the_backend_does_not_have_is_refused_by_name() {
        let fake = Fake::new();
        let err = capture(&fake, provenance(), at_instruction(), &["nowhere"])
            .expect_err("no such region");
        assert!(err.to_string().contains("nowhere"), "said: {err}");
    }

    /// **§3.4, in the type.** A snapshot taken at a frame boundary cannot be
    /// written back, and the refusal says what to do instead.
    #[test]
    fn a_snapshot_from_a_frame_boundary_is_refused_and_says_to_resume_a_blob() {
        let mut fake = Fake::new();
        let snapshot = capture(
            &fake,
            provenance(),
            Position::FrameBoundary { frame: 7 },
            &["work"],
        )
        .expect("it captures");

        let err = seed(&mut fake, &snapshot).expect_err("not seedable");
        assert!(
            matches!(err, SeedError::NotAnInstructionBoundary { .. }),
            "got {err:?}"
        );
        let said = err.to_string();
        assert!(said.contains("no instruction to begin at"), "said: {said}");
        assert!(
            said.contains("resume a blob"),
            "and it must say what to do instead, said: {said}"
        );
        assert!(
            fake.writes.is_empty(),
            "a refusal must leave the machine untouched; it wrote {:?}",
            fake.writes
        );
    }

    /// §3.3. And nothing is written before the refusal.
    #[test]
    fn a_snapshot_with_no_processor_state_is_refused_before_anything_is_written() {
        let mut fake = Fake::new();
        let snapshot = Snapshot::builder(provenance(), at_instruction())
            .whole(Region::bytes("work", 8, Access::ReadWrite), vec![5; 8])
            .unwrap()
            .build();

        let err = seed(&mut fake, &snapshot).expect_err("no registers");
        assert!(matches!(err, SeedError::NoProcessorState), "got {err:?}");
        assert!(
            err.to_string().contains("some other routine's registers"),
            "said: {err}"
        );
        assert!(fake.writes.is_empty(), "wrote {:?}", fake.writes);
    }

    /// The round trip, on a fake: read, change the machine, seed, read again.
    ///
    /// The change in the middle is what makes it a test. Without it, a `seed`
    /// that did nothing would pass.
    #[test]
    fn a_snapshot_seeded_back_restores_what_was_changed() {
        let mut fake = Fake::new();
        fake.work = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let snapshot =
            capture(&fake, provenance(), at_instruction(), &["work"]).expect("it captures");

        fake.work = vec![0xFF; 8];
        fake.processor = Some(Processor::opaque(vec![0xFF; 4]));
        assert_ne!(
            fake.work,
            snapshot.get("work").unwrap().bytes(),
            "the machine really is somewhere else now"
        );

        seed(&mut fake, &snapshot).expect("it seeds");
        assert_eq!(fake.work, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            fake.processor.as_ref().unwrap().bytes(),
            &[1, 2, 3, 4],
            "the registers go back too"
        );
        assert!(
            fake.writes.last().is_some_and(|w| w == "processor"),
            "the processor state is written last, so that it is what the machine starts from; \
             wrote {:?}",
            fake.writes
        );
    }

    /// §3.4's escape hatch, and the shape that makes it honest: what comes
    /// back is the cause, so a caller who ignores it has dropped a value where
    /// a reviewer can see it.
    #[test]
    fn seeding_from_no_boundary_is_possible_and_hands_back_what_it_costs() {
        let mut fake = Fake::new();
        fake.work = vec![3; 8];
        let snapshot = capture(
            &fake,
            provenance(),
            Position::FrameBoundary { frame: 7 },
            &["work"],
        )
        .expect("it captures");

        fake.work = vec![0xFF; 8];
        let carried = seed_from_any_position(&mut fake, &snapshot)
            .expect("the loose path does not refuse a position")
            .expect("and it is not free");

        assert_eq!(fake.work, vec![3; 8], "the state really went in");
        assert!(
            matches!(carried, Undetermined::SeededFromNoBoundary { .. }),
            "got {carried:?}"
        );
        assert!(
            carried.to_string().contains("part way through"),
            "the cause must say what is wrong with the run that follows, said: {carried}"
        );

        // And it is not agreement. Folded into a run's verdicts it outranks
        // them, which is the whole point of handing it back.
        let folded = crate::verdict::fold(&[
            crate::verdict::Verdict::Agrees {
                compared: 8,
                moved: 4,
            },
            crate::verdict::Verdict::NotDetermined(carried),
        ]);
        assert!(
            matches!(folded, crate::verdict::Verdict::NotDetermined(_)),
            "got {folded}"
        );
    }

    /// The loose path is loose about the position and nothing else. §3.3's
    /// refusal is not a position's to excuse.
    #[test]
    fn the_loose_path_still_requires_the_processor_state() {
        let mut fake = Fake::new();
        let snapshot = Snapshot::builder(provenance(), Position::FrameBoundary { frame: 1 })
            .whole(Region::bytes("work", 8, Access::ReadWrite), vec![5; 8])
            .unwrap()
            .build();
        let err = seed_from_any_position(&mut fake, &snapshot).expect_err("no registers");
        assert!(matches!(err, SeedError::NoProcessorState), "got {err:?}");
        assert!(fake.writes.is_empty(), "wrote {:?}", fake.writes);
    }

    /// From a good position it carries nothing, so the ordinary case is not
    /// made to look costly.
    #[test]
    fn from_an_instruction_boundary_the_loose_path_carries_nothing() {
        let mut fake = Fake::new();
        let snapshot =
            capture(&fake, provenance(), at_instruction(), &["work"]).expect("it captures");
        assert_eq!(
            seed_from_any_position(&mut fake, &snapshot).expect("it seeds"),
            None
        );
    }

    /// A span capture holds the span and says where it starts, which is what
    /// lets a comparison refuse two captures of different bytes (§5.1).
    #[test]
    fn a_span_capture_holds_the_span_and_remembers_its_offset() {
        let mut fake = Fake::new();
        fake.work = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let snapshot = capture_spans(
            &fake,
            provenance(),
            at_instruction(),
            &[("work", 2, 4)],
        )
        .expect("it captures");

        let captured = snapshot.get("work").expect("captured");
        assert_eq!(captured.bytes(), &[3, 4, 5, 6]);
        assert_eq!(captured.offset, 2);
        assert!(
            !captured.is_whole_region(),
            "a span is not the region, and a comparison has to be able to tell"
        );
        assert!(
            snapshot.processor().is_some(),
            "§3.3 again: a snapshot that cannot be seeded back is not much of one"
        );
    }

    #[test]
    fn a_span_past_the_end_of_its_region_is_refused_at_capture() {
        let fake = Fake::new();
        let err = capture_spans(&fake, provenance(), at_instruction(), &[("work", 6, 4)])
            .expect_err("past the end");
        assert!(err.to_string().contains("work"), "said: {err}");
    }

    /// A read-only region in a snapshot cannot be seeded, and the refusal is
    /// about that rather than about the region being missing.
    #[test]
    fn seeding_a_read_only_region_is_refused_as_not_writable() {
        let mut fake = Fake::new();
        let snapshot = capture(&fake, provenance(), at_instruction(), &["rom"]).expect("captures");
        let err = seed(&mut fake, &snapshot).expect_err("read-only");
        assert!(
            matches!(err, SeedError::Write(WriteError::NotWritable { .. })),
            "got {err:?}"
        );
    }

    /// A span is seeded as a span. Writing it as a whole region would overwrite
    /// everything the snapshot does not carry.
    #[test]
    fn a_span_is_seeded_as_a_span_and_not_as_a_whole_region() {
        let mut fake = Fake::new();
        fake.work = vec![0xAA; 8];
        let snapshot = Snapshot::builder(provenance(), at_instruction())
            .span(Region::bytes("work", 8, Access::ReadWrite), 2, vec![1, 2])
            .unwrap()
            .processor(Processor::opaque(vec![0]))
            .build();

        seed(&mut fake, &snapshot).expect("it seeds");
        assert_eq!(
            fake.work,
            vec![0xAA, 0xAA, 1, 2, 0xAA, 0xAA, 0xAA, 0xAA],
            "only the span moved"
        );
        assert_eq!(fake.writes.first().map(String::as_str), Some("work+2"));
    }
}
