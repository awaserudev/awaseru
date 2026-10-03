//! The platform boundary — §7.
//!
//! A backend implements this. Nothing here names a console, a memory or a
//! manufacturer: the host asks what regions exist and addresses them by the
//! names it is given (§2.7), which is what makes a second platform an addition
//! rather than a rewrite.

use crate::blob::{Blob, StateError};
use crate::capability::Capabilities;
use crate::region::{Region, Regions, SpanError};
use crate::run::{Bound, Stop};
use crate::snapshot::Processor;

/// A backend's own version, as the library reports it — §16.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendVersion {
    /// What the backend says, in the form it says it.
    pub reported: String,
    /// When it was built, where the backend can say.
    pub built: Option<String>,
}

/// Why a read did not produce bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// No region of that name. Distinct from every other failure, because
    /// §3.5 says absent is not equal and a comparison must be able to tell
    /// this apart from a read that went wrong.
    Absent { region: String },
    /// The region exists but may not be read (§3.1).
    NotReadable { region: String },
    /// The span is not a span of that region.
    Span(SpanError),
    /// The backend failed for a reason of its own.
    Backend { why: String },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Absent { region } => {
                write!(f, "this backend exposes no region named `{region}`")
            }
            ReadError::NotReadable { region } => {
                write!(f, "the region `{region}` is write-only")
            }
            ReadError::Span(e) => write!(f, "{e}"),
            ReadError::Backend { why } => write!(f, "the backend could not read it: {why}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl From<SpanError> for ReadError {
    fn from(e: SpanError) -> Self {
        ReadError::Span(e)
    }
}

/// Why a write did not happen.
///
/// Separate from `ReadError` rather than shared with it, because the cases do
/// not line up: a region can be readable and not writable, and the thing a
/// caller does about "this region is read-only" is nothing like what it does
/// about "this region does not exist".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// No region of that name (§3.5).
    Absent { region: String },
    /// The region exists and may not be written (§3.1). A cartridge's program
    /// data is the ordinary case: the console cannot write it, and a comparison
    /// that wrote to it would be changing its subject rather than measuring it.
    NotWritable { region: String },
    /// The span is not a span of that region.
    Span(SpanError),
    /// The reference is running, so a write would land in memory its own thread
    /// is also writing.
    ///
    /// Measured rather than assumed: the first backend's reads are torn the
    /// same way, and a blob loaded into a running machine is overtaken by
    /// execution before anything can be read back (`doc/backend.md`).
    NotStopped,
    Backend { why: String },
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Absent { region } => {
                write!(f, "this backend exposes no region named `{region}`")
            }
            WriteError::NotWritable { region } => {
                write!(f, "the region `{region}` cannot be written")
            }
            WriteError::Span(e) => write!(f, "{e}"),
            WriteError::NotStopped => write!(
                f,
                "the reference is running, so a write would land in memory it is writing too"
            ),
            WriteError::Backend { why } => write!(f, "the backend could not write it: {why}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<SpanError> for WriteError {
    fn from(e: SpanError) -> Self {
        WriteError::Span(e)
    }
}

/// Why a run could not be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// Nothing is loaded to run.
    NothingLoaded,
    Backend { why: String },
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::NothingLoaded => write!(f, "no software is loaded in the backend"),
            RunError::Backend { why } => write!(f, "the backend could not run: {why}"),
        }
    }
}

impl std::error::Error for RunError {}

/// How a reference came up — §4.12.
///
/// Carried on the trait rather than left to each backend's own type, because
/// §4.12 requires every run to say this and a requirement that depends on the
/// host knowing which backend it has is a requirement that will be skipped.
///
/// Nothing here names a platform (§2.7): the settled memories are named by the
/// backend and the host only passes the names along.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beginning {
    /// Whether it began at a position it can be returned to.
    ///
    /// `false` means `return_to_origin` will refuse and a definition cannot be
    /// replayed — so an anchor cannot be demonstrated, and §2.5 is not
    /// available.
    pub reproducible: bool,
    /// Memories settled on the way up, by the backend's names for them. Empty
    /// when none were, which is what the hardware does and is **not**
    /// reproducible between processes on at least one backend.
    pub settled: Vec<String>,
}

impl std::fmt::Display for Beginning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.reproducible, self.settled.is_empty()) {
            (true, false) => write!(
                f,
                "began at a position it can return to, with {} memories settled — reproducible, \
                 and not what the hardware does",
                self.settled.len()
            ),
            (true, true) => write!(
                f,
                "began at a position it can return to, with memory left as the backend filled \
                 it — which does NOT repeat between processes"
            ),
            (false, false) => write!(
                f,
                "began where the backend happened to be, with {} memories settled — the position \
                 does NOT repeat",
                self.settled.len()
            ),
            (false, true) => write!(
                f,
                "began where the backend happened to be, with memory left as it was — NOTHING \
                 about this run repeats"
            ),
        }
    }
}

impl Beginning {
    /// Whether a run from here can be compared with a run from anywhere else
    /// (§2.5). A report that did not say this would be a report somebody
    /// trusts.
    pub fn repeats(&self) -> bool {
        self.reproducible && !self.settled.is_empty()
    }
}

/// When a byte was last written — §5.4's cheap filter.
///
/// Three answers rather than an `Option<u64>`, because the two kinds of
/// nothing are different: a backend that keeps no such record and a byte that
/// has never been written would otherwise read the same, and the first means
/// *ask something else* while the second is an answer.
///
/// The stamp is in whatever clock the backend keeps. It is comparable with
/// other stamps from the same backend and with nothing else — not with a cycle
/// count, not between processes. So it answers "was this written, and was it
/// written after that one", and never "when" in any absolute sense.
/// §10's execution coverage for a span: how many times each byte ran.
///
/// Not to be confused with `anchor::Coverage`, which is §4.8's cheap check — a
/// digest of the regions an anchor declares. Two different questions wearing
/// one word; this one is about instructions and that one is about bytes at
/// rest.
///
/// **The shape is what M6's gate measured and not what would be convenient**
/// (§2.4). The backend counts per byte, incrementing every byte of an
/// instruction each time that instruction runs — so the loop's opcode and its
/// operand report the same number, and that number is the number of passes.
/// Keeping the count rather than a flag keeps what was measured: a byte run
/// once and a byte run ten thousand times are different facts, and a boolean
/// throws the second away.
///
/// What a caller usually wants is the opposite question. §10 says coverage is
/// "the structural answer to *there is always a routine I did not know
/// about*: it says what has **not** been seen yet", so `never_ran` is the
/// method this type exists for and `ran` is its complement.
///
/// # What a count does NOT mean
///
/// It is a count of executions since the last `forget_coverage`, and nothing
/// else. It is not a measure of time, it does not say in what order the bytes
/// ran, and comparing one span's counts against another's says only which ran
/// more often. Order is `call-and-return-events` (§10), which no backend here
/// declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionCoverage {
    /// The region the span was read from — a name, never an enum (§8.5).
    pub region: String,
    /// Where the span begins in that region.
    pub offset: usize,
    /// One entry per byte of the span, in order.
    pub executions: Vec<u32>,
}

impl ExecutionCoverage {
    /// The offsets, in the region's own numbering, of bytes that never ran.
    ///
    /// Returned as ranges because the interesting answer is "this stretch was
    /// never reached", and a list of individual offsets over a cartridge would
    /// be longer than the cartridge.
    pub fn never_ran(&self) -> Vec<std::ops::Range<usize>> {
        let mut gaps = Vec::new();
        let mut start = None;
        for (i, count) in self.executions.iter().enumerate() {
            match (count, start) {
                (0, None) => start = Some(i),
                (0, Some(_)) => {}
                (_, Some(from)) => {
                    gaps.push(self.offset + from..self.offset + i);
                    start = None;
                }
                (_, None) => {}
            }
        }
        if let Some(from) = start {
            gaps.push(self.offset + from..self.offset + self.executions.len());
        }
        gaps
    }

    /// How many bytes of the span ran at least once.
    pub fn ran(&self) -> usize {
        self.executions.iter().filter(|c| **c > 0).count()
    }

    /// How many did not.
    pub fn untouched(&self) -> usize {
        self.executions.len() - self.ran()
    }

    /// Whether one byte ran, by its offset in the region.
    ///
    /// `None` when the offset is outside the span this covers, which is a
    /// different answer from "it did not run" and is kept apart on purpose
    /// (§2.3's habit, applied to a span).
    pub fn ran_at(&self, offset: usize) -> Option<bool> {
        offset
            .checked_sub(self.offset)
            .and_then(|i| self.executions.get(i))
            .map(|c| *c > 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recency {
    /// The byte's last write, in the backend's clock.
    Stamp(u64),
    /// The backend keeps the record and the byte has never been written.
    NeverWritten,
    /// This backend does not keep one (§7.3's `write-recency`).
    NotSupplied,
}

impl std::fmt::Display for Recency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Recency::Stamp(at) => write!(f, "last written at stamp {at} of the backend's clock"),
            Recency::NeverWritten => write!(f, "never written"),
            Recency::NotSupplied => {
                write!(f, "this backend keeps no record of when a byte was written")
            }
        }
    }
}

/// What a backend must do.
///
/// # What is not here yet, and why
///
/// §7.2's mandatory list also has writing a region, reading and writing the
/// processor state, and saving and loading an opaque state blob. They are
/// absent here because nothing yet calls them, and because their shapes are
/// genuine unknowns — what a processor state *is*, across platforms that do not
/// share a register file, is the question §7.6 leaves open until a second
/// backend exists to answer it with. Declaring them now would be guessing at a
/// signature, which is the one thing §2.4 says not to do.
///
/// What is here is what M0 exercises, and the three decisions that keep the
/// rest open are already taken (§7.5): regions are named and enumerated, the
/// trait lives in this crate rather than the binary, and no platform name
/// crosses this line.
pub trait Platform {
    /// The version the loaded backend reports about itself.
    fn version(&self) -> BackendVersion;

    /// How this reference came up — §4.12. A report says it next to the result.
    fn beginning(&self) -> Beginning;

    /// What this backend can do beyond §7.2's mandatory verbs — §7.3.
    ///
    /// Required rather than defaulted, and the choice is deliberate. A default
    /// of "nothing" would be safe in the sense that nothing is relied on, and
    /// unsafe in the sense that matters: a backend that gained a capability and
    /// forgot to declare it would go on answering *not determined* to
    /// comparisons it could in fact settle, and nothing would say why. Making
    /// every backend write the list means the list is a statement rather than
    /// an oversight.
    ///
    /// What belongs in it is what this crate has **exercised**, not what the
    /// emulator behind it exports. A route that exists and has not been taken
    /// is recorded in the backend's documentation, never declared here.
    fn capabilities(&self) -> Capabilities;

    /// Everything this backend exposes. The host asks; it does not assume.
    fn regions(&self) -> Regions;

    /// A whole region.
    fn read(&self, region: &str) -> Result<Vec<u8>, ReadError>;

    /// A span of one.
    fn read_span(&self, region: &str, offset: usize, len: usize) -> Result<Vec<u8>, ReadError>;

    /// Advances, bounded, and says where it stopped and why (§4.2, §4.3).
    ///
    /// A bound this backend cannot honour is `Reason::Refused`, never an
    /// approximation of it.
    fn run(&mut self, bound: Bound) -> Result<Stop, RunError>;

    /// A whole region, written.
    ///
    /// **This does not seed the machine**, and callers reaching for it to do so
    /// are the reason the warning is here rather than in a guide. A console is
    /// not its memories: its video and audio hardware, its transfer units, its
    /// timers and its master clock are not in any region, so a machine with
    /// memory written into it is a machine in a state it could not have reached
    /// by running. §4.7 is why arriving somewhere uses a blob instead, and
    /// §5.3's perturbation is what this verb is for.
    fn write(&mut self, region: &str, bytes: &[u8]) -> Result<(), WriteError>;

    /// A span of one, written.
    fn write_span(&mut self, region: &str, offset: usize, bytes: &[u8])
    -> Result<(), WriteError>;

    /// The processor state (§3.3).
    ///
    /// Opaque in this version — §7.6's question about what a processor state is
    /// across platforms that do not share a register file is still open, and
    /// §2.4 says not to answer it by guessing a shape.
    fn read_processor(&self) -> Result<Processor, ReadError>;

    /// The processor state, written. Carries the same warning as `write`.
    fn write_processor(&mut self, processor: &Processor) -> Result<(), WriteError>;

    /// Takes the machine's whole state, opaquely (§7.2, §4.7).
    ///
    /// Unlike `write`, this **is** the way to put a machine somewhere: a blob
    /// holds everything, including the parts §3's model does not name.
    ///
    /// Implementations must give the blob the position the machine is at
    /// **after** the save, because on at least one backend saving advances it
    /// (`doc/backend.md`).
    fn save_state(&mut self) -> Result<Blob, StateError>;

    /// Returns the reference to the reproducible position it came up at, so
    /// that a definition can be replayed from the beginning.
    ///
    /// This is what makes §4.8's demonstration possible: replaying a definition
    /// three times and comparing the results needs a beginning to replay from,
    /// and it must be the *same* beginning every time or the comparison is of
    /// three different things.
    ///
    /// A backend whose starting position is not reproducible must refuse
    /// rather than return somewhere near it. §2.5 is the whole reason this
    /// verb exists, and a near miss would defeat it silently.
    fn return_to_origin(&mut self) -> Result<(), RunError>;

    /// When the byte at `offset` of `region` was last written — §5.4's cheap
    /// filter, and the only part of localisation that costs nothing.
    ///
    /// Defaulted, which is the opposite choice from `capabilities` and
    /// deliberately so: the default is the conservative answer — `NotSupplied`
    /// — so a backend that has not implemented this makes every caller ask
    /// something else rather than believe a wrong stamp. `capabilities` has no
    /// default because its conservative answer is the one that *silently
    /// removes* function.
    ///
    /// A backend declaring `write-recency` must not answer `NotSupplied`, and
    /// one that does not declare it must not answer anything else.
    fn write_recency(&self, region: &str, offset: usize) -> Result<Recency, ReadError> {
        let _ = (region, offset);
        Ok(Recency::NotSupplied)
    }

    /// §10's execution coverage over a span — which of those bytes ran.
    ///
    /// Defaulted to a refusal rather than to an empty answer, and the reason is
    /// §2.3: a backend that keeps no record and returned "nothing ran" would be
    /// telling a caller that its routine was never reached, which is a wrong
    /// answer where "ask something else" is the right one. `write_recency`
    /// defaults to `NotSupplied` for the same reason; this one has no such
    /// value in its type, so the refusal carries it.
    ///
    /// A backend declaring `execution-coverage` must implement this, and one
    /// that does not declare it must not.
    ///
    /// **The span is the caller's, because reading is not free.** Measured on
    /// the first backend at 34 µs per kilobyte, a whole cartridge is about as
    /// expensive as the measurement it describes, so a verb that only answered
    /// for an entire region would make the cheap question impossible to ask.
    fn coverage(&self, region: &str, offset: usize, length: usize) -> Result<ExecutionCoverage, ReadError> {
        let _ = (region, offset, length);
        Err(ReadError::Backend {
            why: "this backend does not keep an execution record (§7.3's `execution-coverage`)"
                .to_string(),
        })
    }

    /// Throws away what has been counted, so that the next reading of
    /// `coverage` is about what happens after this call.
    ///
    /// Here because M6's gate measured that it is how the question is asked on
    /// the first backend: the counts reset and the backend's clock does not, so
    /// "what ran in **this** run" is a forget, a run and a reading — not a
    /// subtraction of two readings. A backend whose counts could not be cleared
    /// would have to answer that question differently, and would say so here.
    fn forget_coverage(&mut self) -> Result<(), ReadError> {
        Err(ReadError::Backend {
            why: "this backend does not keep an execution record (§7.3's `execution-coverage`)"
                .to_string(),
        })
    }

    /// Puts one back, and checks that it arrived.
    ///
    /// The check is not optional politeness. A backend may report nothing at
    /// all about a load that failed (§13's Q12), so an implementation that did
    /// not compare where it landed against what the blob recorded would leave
    /// its caller unable to tell a resumed machine from an untouched one.
    fn load_state(&mut self, blob: &Blob) -> Result<(), StateError>;
}

/// Checks a read against the region set, so that every backend does not repeat
/// the same four refusals — and so that they refuse in the same words.
///
/// Returns the region when the read is allowed.
pub fn check_read<'a>(
    regions: &'a Regions,
    name: &str,
    span: Option<(usize, usize)>,
) -> Result<&'a Region, ReadError> {
    let region = regions.get(name).ok_or_else(|| ReadError::Absent {
        region: name.to_string(),
    })?;
    if !region.access.readable() {
        return Err(ReadError::NotReadable {
            region: name.to_string(),
        });
    }
    if let Some((offset, len)) = span {
        region.span(offset, len)?;
    }
    Ok(region)
}

/// Checks a write against the region set, the mirror of `check_read` — and
/// separate from it, because `Access` answers two questions and a function that
/// answered one of them for both would have to pick which.
///
/// Returns the region when the write is allowed.
pub fn check_write<'a>(
    regions: &'a Regions,
    name: &str,
    span: Option<(usize, usize)>,
) -> Result<&'a Region, WriteError> {
    let region = regions.get(name).ok_or_else(|| WriteError::Absent {
        region: name.to_string(),
    })?;
    if !region.access.writable() {
        return Err(WriteError::NotWritable {
            region: name.to_string(),
        });
    }
    if let Some((offset, len)) = span {
        region.span(offset, len)?;
    }
    Ok(region)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covering(offset: usize, executions: &[u32]) -> ExecutionCoverage {
        ExecutionCoverage {
            region: "program-rom".into(),
            offset,
            executions: executions.to_vec(),
        }
    }

    /// §10's question is "what has NOT been seen", so the gaps are the answer
    /// this type exists to give — and the ends are where an off-by-one hides.
    #[test]
    fn the_gaps_are_found_at_both_ends_and_in_the_middle() {
        // A gap opening the span, one closing it, and one between two runs.
        let c = covering(0x100, &[0, 0, 3, 1, 0, 0, 0, 7, 0]);
        assert_eq!(
            c.never_ran(),
            vec![0x100..0x102, 0x104..0x107, 0x108..0x109],
            "offsets are the REGION's, not the span's"
        );
        assert_eq!(c.ran(), 3);
        assert_eq!(c.untouched(), 6);
        assert_eq!(c.ran() + c.untouched(), c.executions.len());

        // The two degenerate spans, which are the ones a loop gets wrong.
        assert_eq!(covering(0, &[1, 1, 1]).never_ran(), vec![]);
        assert_eq!(covering(9, &[0, 0]).never_ran(), vec![9..11]);
        assert_eq!(covering(4, &[]).never_ran(), vec![]);
        assert_eq!(covering(4, &[]).ran(), 0);
        assert_eq!(covering(4, &[]).untouched(), 0);
    }

    /// Outside the span is not the same answer as "did not run" — §2.3's habit,
    /// applied to a span. A caller that asked about a byte this reading does not
    /// cover must not be told it was never reached.
    #[test]
    fn a_byte_outside_the_span_is_not_an_answer_about_that_byte() {
        let c = covering(0x200, &[5, 0, 2]);
        assert_eq!(c.ran_at(0x200), Some(true));
        assert_eq!(c.ran_at(0x201), Some(false));
        assert_eq!(c.ran_at(0x202), Some(true));
        assert_eq!(c.ran_at(0x203), None, "one past the end");
        assert_eq!(c.ran_at(0x1FF), None, "one before the start");
        assert_eq!(c.ran_at(0), None, "and an offset below the span does not wrap");
    }

    /// A backend that has not implemented this refuses rather than answering
    /// "nothing ran", which would tell a caller its routine was never reached.
    #[test]
    fn a_backend_without_the_record_refuses_rather_than_saying_nothing_ran() {
        struct Bare;
        impl Platform for Bare {
            fn version(&self) -> BackendVersion { unimplemented!() }
            fn beginning(&self) -> Beginning { unimplemented!() }
            fn capabilities(&self) -> crate::Capabilities { crate::Capabilities::of([]) }
            fn regions(&self) -> crate::Regions { unimplemented!() }
            fn read(&self, _: &str) -> Result<Vec<u8>, ReadError> { unimplemented!() }
            fn read_span(&self, _: &str, _: usize, _: usize) -> Result<Vec<u8>, ReadError> { unimplemented!() }
            fn run(&mut self, _: crate::Bound) -> Result<crate::Stop, RunError> { unimplemented!() }
            fn write(&mut self, _: &str, _: &[u8]) -> Result<(), WriteError> { unimplemented!() }
            fn write_span(&mut self, _: &str, _: usize, _: &[u8]) -> Result<(), WriteError> { unimplemented!() }
            fn read_processor(&self) -> Result<crate::Processor, ReadError> { unimplemented!() }
            fn write_processor(&mut self, _: &crate::Processor) -> Result<(), WriteError> { unimplemented!() }
            fn save_state(&mut self) -> Result<crate::Blob, crate::StateError> { unimplemented!() }
            fn return_to_origin(&mut self) -> Result<(), RunError> { unimplemented!() }
            fn load_state(&mut self, _: &crate::Blob) -> Result<(), crate::StateError> { unimplemented!() }
        }

        let mut bare = Bare;
        let refused = bare.coverage("program-rom", 0, 16).expect_err("no record, no answer");
        assert!(
            refused.to_string().contains("execution-coverage"),
            "the refusal names the capability a caller would have to look for: {refused}"
        );
        assert!(
            bare.forget_coverage().is_err(),
            "and forgetting a record that does not exist is not a success"
        );
        // The conservative answer is a refusal and not an empty reading: a
        // caller told `ran() == 0` would conclude its routine was never reached.
        assert!(
            Platform::write_recency(&bare, "program-rom", 0).is_ok(),
            "the other defaulted verb answers NotSupplied, which its type can carry"
        );
    }
    use crate::region::Access;

    fn set() -> Regions {
        Regions::new(vec![
            Region::bytes("readable", 16, Access::ReadOnly),
            Region::bytes("writeonly", 16, Access::WriteOnly),
        ])
    }

    /// §4.12. The four ways a reference can begin read differently, and the
    /// three that do not repeat say so in capitals — because the line a reader
    /// skims is the one that has to be unskimmable when it is bad.
    #[test]
    fn a_beginning_says_whether_the_run_repeats_and_the_bad_ones_shout() {
        let beginning = |reproducible, settled: &[&str]| Beginning {
            reproducible,
            settled: settled.iter().map(|s| (*s).to_string()).collect(),
        };

        let good = beginning(true, &["work-ram"]);
        assert!(good.repeats());
        assert!(good.to_string().contains("reproducible"), "{good}");
        assert!(
            good.to_string().contains("not what the hardware does"),
            "even the good case admits the divergence: {good}"
        );

        for bad in [
            beginning(true, &[]),
            beginning(false, &["work-ram"]),
            beginning(false, &[]),
        ] {
            assert!(!bad.repeats(), "{bad}");
            let said = bad.to_string();
            assert!(
                said.contains("NOT") || said.contains("NOTHING"),
                "a beginning that does not repeat must say so plainly: {said}"
            );
        }

        let all: Vec<String> = [
            beginning(true, &["x"]),
            beginning(true, &[]),
            beginning(false, &["x"]),
            beginning(false, &[]),
        ]
        .iter()
        .map(|b| b.to_string())
        .collect();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b, "the four must not read alike");
            }
        }
    }

    #[test]
    fn a_region_that_is_not_there_is_absent_and_says_its_name() {
        let err = check_read(&set(), "nowhere", None).expect_err("not there");
        assert_eq!(
            err,
            ReadError::Absent {
                region: "nowhere".into()
            }
        );
        assert!(err.to_string().contains("nowhere"));
    }

    /// Absent and unreadable are different answers. Folded into one, a
    /// comparison could not tell "the backend has no such thing" from "it has
    /// it and will not show you", and §3.5 turns only the first into
    /// `NotDetermined(RegionAbsent)`.
    #[test]
    fn write_only_is_refused_as_itself_and_not_as_absence() {
        let err = check_read(&set(), "writeonly", None).expect_err("not readable");
        assert_eq!(
            err,
            ReadError::NotReadable {
                region: "writeonly".into()
            }
        );
        assert_ne!(
            err,
            ReadError::Absent {
                region: "writeonly".into()
            },
            "a write-only region exists; saying it is absent would be a different claim"
        );
    }

    #[test]
    fn a_span_past_the_end_is_refused_through_the_same_door() {
        let err = check_read(&set(), "readable", Some((8, 16))).expect_err("past the end");
        assert!(matches!(err, ReadError::Span(_)), "got {err:?}");
        assert!(err.to_string().contains("readable"), "and names the region");
    }

    /// Readable and writable are different questions, and the two checks give
    /// different answers to them. A single check reused for both would have to
    /// decide which of `Access`'s two halves it meant, and would then be wrong
    /// about every region that is one and not the other.
    #[test]
    fn a_read_only_region_can_be_read_and_not_written() {
        let set = Regions::new(vec![
            Region::bytes("rom", 16, Access::ReadOnly),
            Region::bytes("ram", 16, Access::ReadWrite),
        ]);

        assert!(check_read(&set, "rom", None).is_ok());
        let err = check_write(&set, "rom", None).expect_err("read-only");
        assert_eq!(
            err,
            WriteError::NotWritable {
                region: "rom".into()
            }
        );
        assert_ne!(
            err,
            WriteError::Absent {
                region: "rom".into()
            },
            "a read-only region exists; saying it is absent would be a different claim"
        );

        assert!(check_write(&set, "ram", None).is_ok());
        assert!(check_write(&set, "ram", Some((8, 8))).is_ok());
    }

    #[test]
    fn a_write_is_refused_through_the_same_three_doors_as_a_read() {
        let set = Regions::new(vec![Region::bytes("ram", 16, Access::ReadWrite)]);
        assert!(matches!(
            check_write(&set, "nowhere", None).expect_err("absent"),
            WriteError::Absent { .. }
        ));
        let err = check_write(&set, "ram", Some((8, 16))).expect_err("past the end");
        assert!(matches!(err, WriteError::Span(_)), "got {err:?}");
        assert!(err.to_string().contains("ram"), "and names the region");
    }

    /// A write-only region is writable. §3.1 has the variant because hardware
    /// registers that latch a value and read back as something else are
    /// ordinary, and the write check must not refuse them for being unreadable.
    #[test]
    fn a_write_only_region_can_be_written_and_not_read() {
        let set = Regions::new(vec![Region::bytes("latch", 16, Access::WriteOnly)]);
        assert!(check_write(&set, "latch", None).is_ok());
        assert!(check_read(&set, "latch", None).is_err());
    }

    #[test]
    fn a_readable_region_and_a_span_inside_it_are_allowed() {
        let set = set();
        assert_eq!(check_read(&set, "readable", None).unwrap().size, 16);
        assert!(check_read(&set, "readable", Some((0, 16))).is_ok());
        assert!(check_read(&set, "readable", Some((16, 0))).is_ok());
    }
}
