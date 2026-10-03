//! Measuring one routine — §5.6, and §4.5's rule.
//!
//! > **Routine-level comparison is the primary unit.** Seed a state, run one
//! > routine, compare the data that routine touches.
//!
//! This is that sentence as a function. What it adds over doing the five steps
//! by hand is one thing, and it is the thing §4.5 exists for: **the bound is
//! not the caller's to forget.** A measurement of a routine runs to that
//! routine's return address and no further, because the next thing to run
//! writes over the data being compared and the comparison then becomes a
//! reading of something else — silently, and passing.
//!
//! The generated fixture of §11.3 makes that concrete: the instruction after
//! its routine's return writes over the output's first byte. Bounded to the
//! return, a measurement sees the routine's answer; one instruction further and
//! it sees `0xFF`.
//!
//! # What comes back, and why three things rather than one
//!
//! §5.2 requires every comparison to report how much the reference itself
//! moved, which needs the state the routine began from as well as the one it
//! produced. So a measurement carries the **seed** and the **result**, over the
//! same spans, and the differ gets both.

use std::time::{Duration, Instant};

use awaseru_core::anchor::AnchorError;
use awaseru_core::run::{Bound, Reason, Stop};
use awaseru_core::snapshot::Provenance;
use awaseru_core::{CaptureError, ReadError, RunError, Snapshot, Undetermined, WriteError};

use crate::arrive::{ArriveError, Arrived, Arriver};

/// A named span of a region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub region: String,
    pub offset: usize,
    pub length: usize,
}

impl Span {
    pub fn new(region: impl Into<String>, offset: usize, length: usize) -> Self {
        Span {
            region: region.into(),
            offset,
            length,
        }
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` {:#x}..{:#x}",
            self.region,
            self.offset,
            self.offset + self.length
        )
    }
}

/// Bytes to place in a span before the routine runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Given {
    pub span: Span,
    pub bytes: Vec<u8>,
}

/// §5.6's unit of work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routine {
    /// What reports call it.
    pub name: String,
    /// Its first instruction.
    pub entry: u64,
    /// Where it returns to.
    ///
    /// **This is what keeps §4.5**, and it is a field rather than something a
    /// caller passes per measurement so that it cannot be forgotten once and
    /// remembered thereafter.
    pub returns_to: u64,
    /// How many instructions the measurement may spend — from the entry to
    /// the return, and §4.5's rule that it must not run past its subject.
    ///
    /// This is the bound that matters. A routine is as long as a routine, so
    /// this number is small and knowable: a person can count the instructions
    /// in the listing and show the arithmetic, which is what keeps it from
    /// being a guess wearing a number.
    pub within: u64,
    /// How many instructions **reaching** the entry may spend, when that is a
    /// different number.
    ///
    /// `None` means `within`, which is what this field did before it existed
    /// and is right whenever the routine runs soon after the anchor.
    ///
    /// It exists because the two runs have nothing in common
    /// (`doc/findings.md`'s thirty-sixth entry). Reaching a routine depends on
    /// how far the anchor is from it and can be a whole frame of software;
    /// running one is as long as the routine. A budget large enough to reach a
    /// distant routine no longer bounds the measurement, and one tight enough
    /// to bound it cannot reach — **one field cannot be both**, and the first
    /// use of this tool got away with it only because its subject happened to
    /// be reached within a tight bound on itself.
    ///
    /// Additive on purpose: `within` keeps its name and its meaning, and a
    /// routine written before this field existed behaves exactly as it did.
    pub reaching: Option<u64>,
    /// The anchor to begin from, by name (§4.7). `None` means the reference
    /// wherever it already is, which is reproducible only if somebody else
    /// made it so.
    pub from: Option<String>,
    /// The spans the routine writes, captured before and after.
    ///
    /// Not what it *reads*: a comparison is about what a routine produced, and
    /// capturing its inputs as well would report agreement about bytes the
    /// caller wrote itself.
    pub writes: Vec<Span>,
}

/// One measurement of one routine.
#[derive(Debug, Clone)]
pub struct Measured {
    pub routine: String,
    /// How the reference reached the anchor, when one was named. Carries
    /// §4.8's caveat, which every verdict from this measurement must too.
    pub arrival: Option<Arrived>,
    /// The written spans as they were when the routine began — §5.2's seed.
    pub seed: Snapshot,
    /// And as the routine left them. The reference's own result.
    pub result: Snapshot,
    /// Where it stopped, which §4.5 requires to be the return.
    pub stop: Stop,
    pub took: Duration,
}

impl Measured {
    /// A candidate shaped exactly like this result — the same regions, the
    /// same spans, the same provenance and position — with bytes a
    /// reimplementation produced.
    ///
    /// Here rather than at each call site because §5.1's comparison refuses
    /// two states that do not cover the same bytes (`SpansDiffer`), and a
    /// candidate assembled by hand is the easiest way to produce that refusal
    /// and read it as a result. The bytes are given in the order the routine's
    /// `writes` named them, which is the order the result carries them in.
    pub fn candidate(&self, produced: &[Vec<u8>]) -> Result<Snapshot, CandidateError> {
        let captured: Vec<_> = self.result.captures().collect();
        if captured.len() != produced.len() {
            return Err(CandidateError::CountDiffers {
                captured: captured.len(),
                given: produced.len(),
            });
        }

        let mut builder =
            Snapshot::builder(self.result.provenance().clone(), self.result.position().clone());
        for (capture, bytes) in captured.iter().zip(produced) {
            if capture.len() != bytes.len() {
                return Err(CandidateError::LengthDiffers {
                    region: capture.region.name.clone(),
                    offset: capture.offset,
                    captured: capture.len(),
                    given: bytes.len(),
                });
            }
            builder = builder
                .span(capture.region.clone(), capture.offset, bytes.clone())
                .expect("a span the reference itself captured is a span of its region");
        }
        Ok(builder.build())
    }

    /// What makes a verdict from this measurement not evidence, if anything.
    ///
    /// Only the arrival's caveat for now: a routine measured from an anchor
    /// nobody demonstrated is a routine measured from a state nobody has shown
    /// to be the right one (§4.8).
    pub fn caveat(&self) -> Option<&Undetermined> {
        self.arrival.as_ref().and_then(|a| a.caveat.as_ref())
    }
}

/// Why a candidate could not be shaped like a measurement's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateError {
    /// A different number of spans than the measurement captured.
    CountDiffers { captured: usize, given: usize },
    /// One span's bytes are not as many as the reference produced there.
    LengthDiffers {
        region: String,
        offset: usize,
        captured: usize,
        given: usize,
    },
}

impl std::fmt::Display for CandidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CandidateError::CountDiffers { captured, given } => write!(
                f,
                "the measurement captured {captured} span(s) and {given} were given for the \
                 candidate. A candidate shaped differently from the result would be compared \
                 span by span against the wrong spans"
            ),
            CandidateError::LengthDiffers {
                region,
                offset,
                captured,
                given,
            } => write!(
                f,
                "the reference produced {captured} bytes at `{region}`+{offset} and {given} were \
                 given for the candidate. Padded or truncated, the comparison would be over bytes \
                 nobody produced"
            ),
        }
    }
}

impl std::error::Error for CandidateError {}

/// Why a routine could not be measured.
#[derive(Debug)]
pub enum Error {
    Arrive(ArriveError),
    Anchors(AnchorError),
    Read(ReadError),
    Write(WriteError),
    Run(RunError),
    Capture(CaptureError),
    /// The reference never reached the routine, so nothing was measured of it.
    NeverEntered { routine: String, stop: Stop },
    /// It entered and did not return within its budget.
    ///
    /// **Not** reported as a measurement with a caveat: a routine that did not
    /// return has written an unknown amount of what it was going to write, and
    /// a comparison over that is a comparison of a routine half way through
    /// (§4.5).
    NeverReturned { routine: String, stop: Stop },
    /// A given's bytes do not fill the span it names.
    GivenDoesNotFit { span: Span, bytes: usize },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Arrive(e) => write!(f, "{e}"),
            Error::Anchors(e) => write!(f, "{e}"),
            Error::Read(e) => write!(f, "{e}"),
            Error::Write(e) => write!(f, "{e}"),
            Error::Run(e) => write!(f, "{e}"),
            Error::Capture(e) => write!(f, "{e}"),
            Error::NeverEntered { routine, stop } => write!(
                f,
                "the reference never reached `{routine}`: {stop}. Nothing was measured of it, \
                 which is a different thing from measuring it and finding nothing"
            ),
            Error::NeverReturned { routine, stop } => write!(
                f,
                "`{routine}` did not return within its budget: {stop}. It has written some \
                 unknown part of what it was going to write, and comparing that would be \
                 comparing a routine half way through (§4.5)"
            ),
            Error::GivenDoesNotFit { span, bytes } => write!(
                f,
                "{bytes} bytes were given for {span}, which holds a different number. A given \
                 that was padded or truncated would seed a routine with inputs nobody chose"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<ArriveError> for Error {
    fn from(e: ArriveError) -> Self {
        Error::Arrive(e)
    }
}
impl From<AnchorError> for Error {
    fn from(e: AnchorError) -> Self {
        Error::Anchors(e)
    }
}
impl From<ReadError> for Error {
    fn from(e: ReadError) -> Self {
        Error::Read(e)
    }
}
impl From<WriteError> for Error {
    fn from(e: WriteError) -> Self {
        Error::Write(e)
    }
}
impl From<RunError> for Error {
    fn from(e: RunError) -> Self {
        Error::Run(e)
    }
}
impl From<CaptureError> for Error {
    fn from(e: CaptureError) -> Self {
        Error::Capture(e)
    }
}

/// Puts the reference back where a replay of this routine must start from.
///
/// An anchor gets itself there (`Arriver::arrive` resumes or replays), so this
/// is about the other case: a routine named without one is reached from the
/// reference's own origin, and after one measurement the machine is standing at
/// that routine's **return** — past the entry. Running to the entry from there
/// arrives nowhere and spends the whole budget proving it, which is what the
/// first localisation did before this existed.
///
/// Here rather than in `measure`, which is documented to run from wherever the
/// reference already is: §5.4's localisation and §5.3's control are the two
/// things that measure the *same* routine more than once, and they are the ones
/// that must rewind.
pub(crate) fn rewind_if_unanchored(
    arriver: &mut Arriver<'_>,
    routine: &Routine,
) -> Result<(), Error> {
    if routine.from.is_none() {
        arriver.return_to_origin()?;
    }
    Ok(())
}

/// Arrive, reach the entry, seed — the first three of §5.6's steps.
///
/// Shared because §5.4's localisation is the same three steps followed by a
/// different run, and two copies of them would be two chances for the seeding
/// to drift apart. A localisation seeded differently from the measurement it
/// explains would name the writer of a byte in some other run.
pub(crate) fn enter_and_seed(
    arriver: &mut Arriver<'_>,
    routine: &Routine,
    given: &[Given],
) -> Result<Option<Arrived>, Error> {
    for g in given {
        if g.bytes.len() != g.span.length {
            return Err(Error::GivenDoesNotFit {
                span: g.span.clone(),
                bytes: g.bytes.len(),
            });
        }
    }

    let arrival = match &routine.from {
        None => None,
        Some(anchor) => Some(arriver.arrive(anchor)?),
    };

    let stop = arriver.run(Bound::Address {
        address: routine.entry,
        within: reaching_budget(routine),
    })?;
    if stop.reason
        != (Reason::AddressHit {
            address: routine.entry,
        })
    {
        return Err(Error::NeverEntered {
            routine: routine.name.clone(),
            stop,
        });
    }

    // After the entry is reached, so that the routine has not read them yet,
    // and before anything is captured, so that a seed is the state the routine
    // actually began from.
    for g in given {
        arriver.write_span(&g.span.region, g.span.offset, &g.bytes)?;
    }

    Ok(arrival)
}

/// Which budget bounds the run that **reaches** the entry.
///
/// A named decision rather than an `unwrap_or` inline, because it is the whole
/// of finding 36's fix and a decision inline cannot be mutated against. §4.5
/// bounds the measurement; this bounds the search for its start, and the two
/// are different numbers. Absent means they are the same, which is right
/// whenever the routine runs soon after its anchor.
fn reaching_budget(routine: &Routine) -> u64 {
    routine.reaching.unwrap_or(routine.within)
}

/// Runs one routine and brings back what it did — §5.6's five steps.
///
/// Arrive, reach the entry, seed, capture, run to the return, capture again.
/// The two bounds are the routine's own, which is how §4.5 is kept by the tool
/// rather than by whoever calls it.
pub fn measure(
    arriver: &mut Arriver<'_>,
    provenance: &Provenance,
    routine: &Routine,
    given: &[Given],
) -> Result<Measured, Error> {
    let began = Instant::now();

    // ---- 1 to 3: where it starts from, its entry, its inputs -------------
    let arrival = enter_and_seed(arriver, routine, given)?;

    // ---- 4. the state it begins from -------------------------------------
    let spans: Vec<(&str, usize, usize)> = routine
        .writes
        .iter()
        .map(|s| (s.region.as_str(), s.offset, s.length))
        .collect();
    let seed = arriver.capture(provenance.clone(), &spans)?;

    // ---- 5. until it returns, and not one instruction further ------------
    let stop = arriver.run(Bound::Address {
        address: routine.returns_to,
        within: routine.within,
    })?;
    if stop.reason != (Reason::AddressHit {
        address: routine.returns_to,
    }) {
        return Err(Error::NeverReturned {
            routine: routine.name.clone(),
            stop,
        });
    }

    let result = arriver.capture(provenance.clone(), &spans)?;

    Ok(Measured {
        routine: routine.name.clone(),
        arrival,
        seed,
        result,
        stop,
        took: began.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finding 36's fix, both answers.
    ///
    /// One field cannot bound two runs that have nothing in common: reaching a
    /// routine can be a whole frame of software, running one is as long as the
    /// routine, and §4.5 needs the second bound tight. The mutation this
    /// catches is using `within` for both, which is what the code did — and
    /// which the first use of this tool got away with only because its subject
    /// happened to be reached within a tight bound on itself.
    #[test]
    fn reaching_has_its_own_budget_and_falls_back_to_the_measurements() {
        let mut routine = Routine {
            name: "r".into(),
            entry: 0x8000,
            returns_to: 0x8010,
            within: 2_000,
            reaching: None,
            from: None,
            writes: Vec::new(),
        };

        // Absent: the two are the same number, which is what a routine written
        // before this field existed meant.
        assert_eq!(reaching_budget(&routine), 2_000);

        // Present: reaching is allowed far more than the measurement, and the
        // measurement's own bound does not move.
        routine.reaching = Some(5_000_000);
        assert_eq!(reaching_budget(&routine), 5_000_000);
        assert_eq!(
            routine.within, 2_000,
            "§4.5's bound on the measurement must not move when reaching is widened"
        );

        // And the other way round: reached immediately, measured loosely.
        routine.reaching = Some(1);
        assert_eq!(reaching_budget(&routine), 1);
    }
}
