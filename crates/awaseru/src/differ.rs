//! The differ, as one thing — §5 entire.
//!
//! §5 is four requirements and they are not separable:
//!
//! - §5.1's verdict, which is three-valued;
//! - §5.2's movement, which is what tells a measurement from a decoration;
//! - §5.4's localisation, where the backend supplies it;
//! - §5.3's control, or the record that none was run.
//!
//! A caller handed those as four returns would use the first and drop the
//! rest, and the tool would be back to printing "agrees". So they come back as
//! one `Report` with no public fields, and the one thing a caller cannot do is
//! read the verdict without everything that bears on it:
//! `Report::verdict` **applies the caveats**. An anchor nobody has
//! demonstrated (§4.8) or a beginning that does not repeat (§2.5) turns
//! agreement into *not determined*, in the accessor, every time. What the
//! comparison said before that is still readable — through a differently named
//! method, so that reading it is a choice somebody made.
//!
//! # Why the measurement happens twice when a control is run
//!
//! §5.3's control measures the plain comparison itself, for comparability
//! (`perturb`'s header says why). That gives this module a second verdict for
//! the same measurement, made the same way, and comparing the two is free:
//! **a measurement that does not repeat is not evidence** (§2.5), and until now
//! nothing in this project checked that at the level of a verdict. When they
//! disagree the report is `NotRepeatable` and says both readings.

use std::time::Duration;

use awaseru_core::platform::Beginning;
use awaseru_core::snapshot::Provenance;
use awaseru_core::{Comparison, Undetermined, Verdict};

use crate::arrive::Arriver;
use crate::localise::{self, Localised};
use crate::perturb::{self, Control, Perturbation};
use crate::routine::{self, Given, Measured, Routine};

/// What to examine, and how much of §5 to pay for.
///
/// A struct rather than six arguments, and the two optional members are
/// optional for opposite reasons: a control is something only the caller can
/// offer (§5.3), and localisation is something that costs a replay (§5.4).
#[derive(Debug, Clone)]
pub struct Request<'a> {
    pub routine: &'a Routine,
    /// The inputs to seed, in the spans the routine reads them from.
    pub given: &'a [Given],
    /// What the reimplementation produced, one `Vec` per span the routine
    /// declares it writes, in that order.
    pub produced: &'a [Vec<u8>],
    /// §5.3's control. `None` is **recorded** in the report, not skipped.
    pub control: Option<&'a Perturbation>,
    /// Whether to go and find what wrote the first differing byte (§5.4). It
    /// is a replay, so it is asked for rather than assumed — and it is only
    /// ever done when the verdict differs, since there is nothing to localise
    /// otherwise.
    pub localise: bool,
}

/// One routine, one candidate, all of §5 in one value.
///
/// No public fields and no `is_ok`, for the same reason `Verdict` has neither:
/// every shortcut out of this type is a way to report agreement that was never
/// established.
#[derive(Debug, Clone)]
pub struct Report {
    routine: String,
    /// What comparing the three states said, before anything about the run
    /// itself is applied.
    compared: Verdict,
    /// The same comparison the second time, when §5.3's control measured it
    /// again (§2.5).
    repeated: Option<Verdict>,
    /// §4.8's caveat, from the arrival.
    caveat: Option<Undetermined>,
    /// §4.12: how the reference came up.
    beginning: Beginning,
    /// Which region the difference is in — §13's Q16, answered.
    ///
    /// The comparison knows this and `Difference` does not carry it, so it was
    /// recoverable only while the comparison was in hand. On a wire it was not
    /// recoverable at all: a client got an offset and no name. It is carried
    /// here, from the region-by-region comparison that produced the verdict,
    /// and **not** taken from the localisation — a difference has a region
    /// whether or not anybody paid for a replay.
    differing_region: Option<String>,
    /// §5.4, when it was asked for and there was something to localise.
    located: Option<Localised>,
    /// §5.3.
    control: Control,
    took: Duration,
}

impl Report {
    /// §5.1's verdict, with everything that bears on it already applied.
    ///
    /// Three things can take a verdict away from what the bytes said, and all
    /// three are applied here rather than left for a caller to remember:
    ///
    /// 1. the same measurement twice giving two answers (§2.5);
    /// 2. an anchor nobody has demonstrated (§4.8);
    /// 3. a beginning that does not repeat (§2.5, §4.12).
    ///
    /// The order is by how much each undermines the rest: a measurement that
    /// disagrees with itself makes the other two moot.
    pub fn verdict(&self) -> Verdict {
        if let Some(repeated) = &self.repeated
            && repeated != &self.compared
        {
            return Verdict::NotDetermined(Undetermined::NotRepeatable {
                first: self.compared.to_string(),
                second: repeated.to_string(),
            });
        }
        if let Some(caveat) = &self.caveat {
            return Verdict::NotDetermined(caveat.clone());
        }
        if !self.beginning.repeats() {
            return Verdict::NotDetermined(Undetermined::StatesIncomparable {
                why: format!(
                    "the reference {} — so this comparison cannot be repeated and is not \
                     evidence (§2.5)",
                    self.beginning
                ),
            });
        }

        // §5.4 is part of a difference, not a field beside it: "on *differs*,
        // the report carries ... the position that last wrote that byte". The
        // comparison cannot fill it — naming a writer takes a replay — so it is
        // folded in here, where both are in hand.
        //
        // Found by a test rather than by reading: the wire form carried the
        // answer in its localisation and `not-looked` inside the difference,
        // which is the same fact in two places with one of them stale.
        if let (Verdict::Differs(difference), Some(located)) = (&self.compared, &self.located) {
            return Verdict::Differs(difference.clone().localised(located.wrote.clone()));
        }

        self.compared.clone()
    }

    /// What the comparison alone said.
    ///
    /// Here so that a reader can see what a caveat changed — "it agreed, and
    /// the anchor it agreed from has never been demonstrated" is a different
    /// thing to do about than "it differed". Named so that reaching for it
    /// instead of `verdict` is visible in a review.
    pub fn as_compared(&self) -> &Verdict {
        &self.compared
    }

    /// §5.2's movement: how many of the compared bytes the reference itself
    /// changed. `None` when the verdict is not determined, because then there
    /// is no count to report rather than a count of zero.
    pub fn moved(&self) -> Option<usize> {
        match &self.compared {
            Verdict::Agrees { moved, .. } => Some(*moved),
            Verdict::Differs(_) | Verdict::NotDetermined(_) => None,
        }
    }

    /// Which region a difference is in (§5.4's offset is read against it).
    ///
    /// `None` when the verdict is not a difference, and never `None` when it
    /// is: the comparison that produced it knew the name.
    pub fn differing_region(&self) -> Option<&str> {
        self.differing_region.as_deref()
    }

    /// §5.4, when it was asked for.
    pub fn localisation(&self) -> Option<&Localised> {
        self.located.as_ref()
    }

    /// §5.3, including the record that no control was run.
    pub fn control(&self) -> &Control {
        &self.control
    }

    /// §4.12.
    pub fn beginning(&self) -> &Beginning {
        &self.beginning
    }

    pub fn routine(&self) -> &str {
        &self.routine
    }

    pub fn took(&self) -> Duration {
        self.took
    }

    /// Whether this is a complete measurement — §5.3's first sentence.
    ///
    /// A verdict with no control that varies is incomplete however green it
    /// looks, and this is the question a caller asks about that. It is
    /// deliberately **not** folded into `verdict`: an unnoticed control is a
    /// statement about the comparison's sensitivity and not a reason to
    /// withdraw a difference that was actually found.
    pub fn complete(&self) -> bool {
        self.control.discriminates()
    }
}

impl std::fmt::Display for Report {
    /// All four parts of §5, in the order a reader needs them, with the ones
    /// that undermine the verdict last — because the end of the line is what
    /// gets skipped, so what is there must be what matters when it is bad.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}`: {} ({:.3}s)",
            self.routine,
            self.verdict(),
            self.took.as_secs_f64()
        )?;
        if self.verdict() != self.compared {
            write!(f, ". The comparison itself said: {}", self.compared)?;
        }
        if let Some(located) = &self.located {
            write!(f, ". {located}")?;
        }
        write!(f, ". {}", self.control)?;
        // Only when a control ran and went unnoticed: the record of one that
        // was never run already says it is incomplete, and a report that said
        // so twice in one line would read as two findings.
        if matches!(self.control, Control::Ran { .. }) && !self.complete() {
            write!(f, " — so this measurement is incomplete (§5.3)")?;
        }
        write!(f, ". The reference {}", self.beginning)
    }
}

/// Why a report could not be produced.
#[derive(Debug)]
pub enum Error {
    Routine(routine::Error),
    Candidate(routine::CandidateError),
    Control(perturb::Error),
    NotComparable(awaseru_core::snapshot::NotComparable),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Routine(e) => write!(f, "{e}"),
            Error::Candidate(e) => write!(f, "{e}"),
            Error::Control(e) => write!(f, "{e}"),
            Error::NotComparable(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<routine::Error> for Error {
    fn from(e: routine::Error) -> Self {
        Error::Routine(e)
    }
}
impl From<routine::CandidateError> for Error {
    fn from(e: routine::CandidateError) -> Self {
        Error::Candidate(e)
    }
}
impl From<perturb::Error> for Error {
    fn from(e: perturb::Error) -> Self {
        Error::Control(e)
    }
}
impl From<awaseru_core::snapshot::NotComparable> for Error {
    fn from(e: awaseru_core::snapshot::NotComparable) -> Self {
        Error::NotComparable(e)
    }
}

/// Measures a routine, compares a reimplementation against it, and answers
/// with all of §5.
pub fn examine(
    arriver: &mut Arriver<'_>,
    provenance: &Provenance,
    request: &Request<'_>,
) -> Result<Report, Error> {
    let began = std::time::Instant::now();
    let routine = request.routine;

    routine::rewind_if_unanchored(arriver, routine)?;
    let measured = routine::measure(arriver, provenance, routine, request.given)?;
    let candidate = measured.candidate(request.produced)?;

    // Compared region by region rather than through `compare`, which folds:
    // the fold keeps the difference and loses which region it came from, and
    // §5.4's localisation needs that name to know which byte to go and watch.
    let (compared, differing_region) = by_region(&measured, &candidate, routine)?;

    // ---- §5.4, only when there is something to localise ------------------
    let located = match (&compared, request.localise, &differing_region) {
        (Verdict::Differs(difference), true, Some(region)) => Some(localise::localise_difference(
            arriver,
            routine,
            request.given,
            region,
            difference,
        )?),
        _ => None,
    };

    // ---- §5.3, and §2.5 for free -----------------------------------------
    let (control, repeated) = match request.control {
        None => (Control::NotRun, None),
        Some(perturbation) => {
            let control = perturb::control(
                arriver,
                provenance,
                routine,
                request.given,
                &candidate,
                perturbation,
            )?;
            let repeated = match &control {
                Control::Ran { plain, .. } => Some(plain.clone()),
                Control::NotRun => None,
            };
            (control, repeated)
        }
    };

    Ok(Report {
        routine: routine.name.clone(),
        compared,
        repeated,
        caveat: measured.caveat().cloned(),
        beginning: arriver.beginning(),
        differing_region,
        located,
        control,
        took: began.elapsed(),
    })
}

/// The verdict, and which region a difference came from.
///
/// Returns the folded verdict (§2.3's ranking is `fold`'s, not this
/// function's) and the name of the region whose verdict it is, when that
/// verdict is a difference.
fn by_region(
    measured: &Measured,
    candidate: &awaseru_core::Snapshot,
    routine: &Routine,
) -> Result<(Verdict, Option<String>), Error> {
    let comparison = Comparison {
        seed: &measured.seed,
        reference: &measured.result,
        candidate,
    };
    comparison.check_comparable()?;

    let mut names: Vec<&str> = Vec::new();
    for span in &routine.writes {
        if !names.contains(&span.region.as_str()) {
            names.push(&span.region);
        }
    }

    let verdicts: Vec<Verdict> = names
        .iter()
        .map(|name| awaseru_core::compare::region(comparison, name))
        .collect();
    let folded = awaseru_core::verdict::fold(&verdicts);

    // Which region the folded verdict came from: the first whose own verdict
    // is the one that won. Compared by value rather than by index, so that a
    // change to `fold`'s ranking cannot make this name the wrong region.
    let of = names
        .iter()
        .zip(&verdicts)
        .find(|(_, verdict)| *verdict == &folded)
        .map(|(name, _)| (*name).to_string());

    Ok(match folded {
        Verdict::Differs(_) => (folded, of),
        _ => (folded, None),
    })
}

/// Everything a report is made of, for a caller that has already done the work.
///
/// A struct rather than nine arguments, and public because §5's requirements are
/// what a report must carry rather than how it was produced. Every field is
/// named, so nothing can be left out by forgetting it or put in the wrong
/// position — which a nine-argument function invites.
#[derive(Debug, Clone)]
pub struct Parts {
    pub routine: String,
    pub compared: Verdict,
    pub repeated: Option<Verdict>,
    pub caveat: Option<Undetermined>,
    pub beginning: Beginning,
    pub differing_region: Option<String>,
    pub located: Option<Localised>,
    pub control: Control,
    pub took: Duration,
}

/// A report built from parts.
pub fn report_of(parts: Parts) -> Report {
    Report {
        routine: parts.routine,
        compared: parts.compared,
        repeated: parts.repeated,
        caveat: parts.caveat,
        beginning: parts.beginning,
        differing_region: parts.differing_region,
        located: parts.located,
        control: parts.control,
        took: parts.took,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use awaseru_core::verdict::Difference;
    use awaseru_core::{Position, Wrote};

    fn repeats() -> Beginning {
        Beginning {
            reproducible: true,
            settled: vec!["work-ram".into()],
        }
    }

    fn agrees() -> Verdict {
        Verdict::Agrees {
            compared: 64,
            moved: 64,
        }
    }

    fn differs() -> Verdict {
        Verdict::Differs(Difference::new(0x400, 0x5E, 0x59, 64, 64))
    }

    fn noticed() -> Control {
        Control::Ran {
            perturbation: "the first input byte".into(),
            plain: agrees(),
            perturbed: differs(),
        }
    }

    /// The parts of a report, with everything that is not under test set to
    /// what a working measurement would have given.
    fn parts(compared: Verdict) -> Parts {
        Parts {
            routine: "running-total".into(),
            compared,
            repeated: None,
            caveat: None,
            beginning: repeats(),
            differing_region: Some("work-ram".into()),
            located: None,
            control: noticed(),
            took: Duration::from_millis(12),
        }
    }

    fn report(compared: Verdict, caveat: Option<Undetermined>, control: Control) -> Report {
        report_of(Parts {
            caveat,
            control,
            ..parts(compared)
        })
    }

    /// **The assertion this module exists for.** A verdict read out of a
    /// report carries what bears on it. An anchor nobody demonstrated turns
    /// agreement into not determined — in the accessor, so that no caller can
    /// take the agreement without the caveat.
    #[test]
    fn an_undemonstrated_anchor_takes_the_agreement_away() {
        let caveat = Undetermined::AnchorNotDemonstrated {
            anchor: "after-the-opening".into(),
        };
        let r = report(agrees(), Some(caveat.clone()), noticed());

        assert_eq!(r.verdict(), Verdict::NotDetermined(caveat));
        assert_eq!(
            r.as_compared(),
            &agrees(),
            "and what the bytes said is still readable, by a differently named method"
        );
        let said = r.to_string();
        assert!(said.contains("not determined"), "said: {said}");
        assert!(
            said.contains("The comparison itself said"),
            "a verdict the caveat changed must show both readings: {said}"
        );
    }

    /// §2.5, which nothing checked at this level before. Two readings of the
    /// same measurement that disagree are not evidence, whichever is right.
    #[test]
    fn a_measurement_that_does_not_repeat_is_not_determined() {
        let r = report_of(Parts {
            repeated: Some(differs()),
            ..parts(agrees())
        });
        match r.verdict() {
            Verdict::NotDetermined(Undetermined::NotRepeatable { first, second }) => {
                assert!(first.contains("agrees"), "{first}");
                assert!(second.contains("differ"), "{second}");
            }
            other => panic!("got {other}"),
        }

        // And the same reading twice is left alone — without this half, the
        // check above would be a check that no verdict ever stands.
        let r = report_of(Parts {
            repeated: Some(agrees()),
            ..parts(agrees())
        });
        assert_eq!(r.verdict(), agrees());
    }

    /// §2.5 and §4.12. A reference that did not come up somewhere that repeats
    /// cannot produce evidence, and the reason says what was wrong with the
    /// beginning rather than naming it in the abstract.
    #[test]
    fn a_beginning_that_does_not_repeat_takes_the_verdict_away() {
        let r = report_of(Parts {
            beginning: Beginning {
                reproducible: true,
                settled: vec![],
            },
            ..parts(agrees())
        });
        match r.verdict() {
            Verdict::NotDetermined(Undetermined::StatesIncomparable { why }) => {
                assert!(why.contains("does NOT repeat") || why.contains("NOT"), "{why}");
            }
            other => panic!("got {other}"),
        }
    }

    /// §5.3's first sentence as a question a caller can ask. Both the control
    /// nobody ran and the control nobody noticed leave the measurement
    /// incomplete, and neither withdraws a difference that was found.
    #[test]
    fn a_report_without_a_discriminating_control_is_incomplete_and_still_says_what_it_found() {
        let none = report(differs(), None, Control::NotRun);
        assert!(!none.complete());
        assert_eq!(none.verdict(), differs(), "§5.3 does not withdraw a finding");
        assert!(none.to_string().contains("incomplete"), "{none}");
        assert!(none.to_string().contains("no control was run"), "{none}");

        let unnoticed = report(
            differs(),
            None,
            Control::Ran {
                perturbation: "a byte nothing reads".into(),
                plain: differs(),
                perturbed: differs(),
            },
        );
        assert!(!unnoticed.complete());
        assert!(unnoticed.to_string().contains("incomplete"), "{unnoticed}");

        let good = report(differs(), None, noticed());
        assert!(good.complete());
        assert!(!good.to_string().contains("incomplete"), "{good}");
    }

    /// §5.4's third item belongs **inside** the difference, because that is
    /// what §5.4 says a report carries on *differs*. A report with the answer
    /// beside the difference and `not-looked` within it would be the same fact
    /// twice with one copy wrong — which is what the wire form showed before
    /// this was fixed.
    #[test]
    fn a_localisation_is_folded_into_the_difference_the_verdict_carries() {
        let located = Localised {
            region: "work-ram".into(),
            offset: 0x400,
            wrote: Wrote::At {
                position: Position::MidInstruction { pc: 0x80_2C },
                writes: 1,
            },
            replayed: true,
            from: None,
        };
        let r = report_of(Parts {
            located: Some(located.clone()),
            ..parts(differs())
        });

        match r.verdict() {
            Verdict::Differs(d) => assert_eq!(
                d.wrote,
                located.wrote,
                "the verdict's difference must carry what was localised"
            ),
            other => panic!("got {other}"),
        }

        // What the comparison alone said is still the comparison alone: it
        // never looked, and saying it did would be the stale copy in reverse.
        match r.as_compared() {
            Verdict::Differs(d) => assert_eq!(d.wrote, Wrote::NotLooked),
            other => panic!("got {other}"),
        }

        // And a report with no localisation is left exactly as compared.
        let unlocalised = report_of(Parts { ..parts(differs()) });
        assert_eq!(unlocalised.verdict(), differs());
    }

    /// §13's Q16, answered. A difference's offset is read against a region, and
    /// the region's name is carried by the report **whether or not anyone paid
    /// for a localisation**. Taken from the localisation instead, as it was
    /// first, a client that did not ask for §5.4 got an offset it could not
    /// place.
    #[test]
    fn a_difference_names_its_region_without_a_localisation() {
        let r = report_of(Parts { ..parts(differs()) });
        assert!(r.localisation().is_none(), "nobody asked for one");
        assert_eq!(
            r.differing_region(),
            Some("work-ram"),
            "and the region is still known, because the comparison knew it"
        );

        // And it is not claimed where there is no difference to place.
        let agreed = report_of(Parts {
            differing_region: None,
            ..parts(agrees())
        });
        assert_eq!(agreed.differing_region(), None);
    }

    /// §5.2. The movement is reported where there is one, and absent rather
    /// than zero where there is not — a report that said "moved 0" for a
    /// difference would be stating something it does not know.
    #[test]
    fn movement_is_reported_for_agreement_and_absent_elsewhere() {
        assert_eq!(report(agrees(), None, noticed()).moved(), Some(64));
        assert_eq!(report(differs(), None, noticed()).moved(), None);
        assert_eq!(
            report(
                Verdict::NotDetermined(Undetermined::Vacuous { compared: 64 }),
                None,
                noticed()
            )
            .moved(),
            None
        );
    }

    /// All four parts of §5 in one line, and the test is that none of them can
    /// go missing: a report printing three of the four would let a reader
    /// conclude from an incomplete picture.
    #[test]
    fn the_report_prints_every_part_of_section_five() {
        let r = report_of(Parts {
            located: Some(Localised {
                region: "work-ram".into(),
                offset: 0x400,
                wrote: Wrote::At {
                    position: Position::MidInstruction { pc: 0x80_2C },
                    writes: 1,
                },
                replayed: true,
                from: None,
            }),
            took: Duration::from_millis(500),
            ..parts(differs())
        });
        let said = r.to_string();

        assert!(said.contains("running-total"), "{said}");
        // §5.1 and §5.4's first two items.
        assert!(said.contains("differ") && said.contains("first at 1024"), "{said}");
        // §5.4's third.
        assert!(said.contains("802C"), "{said}");
        // §5.3.
        assert!(said.contains("the first input byte"), "{said}");
        // §4.12.
        assert!(said.contains("The reference began"), "{said}");
    }
}
