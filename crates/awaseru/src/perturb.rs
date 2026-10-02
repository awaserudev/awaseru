//! §5.3's controls.
//!
//! > A measurement without a control that varies is incomplete. The tool cannot
//! > force the client to run one, but it can record that none was run, and it
//! > does.
//!
//! # What a control is here, and what it is not
//!
//! Run the reference again with a named input changed, and compare it against
//! **the same candidate as before**. The candidate is what a reimplementation
//! produced from the original inputs, and it is deliberately left alone: the
//! question is not whether the reimplementation tracks the reference, it is
//! whether *this comparison* can tell anything apart at all.
//!
//! So a control that changes the verdict has shown the comparison to be
//! sensitive to that input. A control that leaves the verdict exactly where it
//! was has shown the opposite — the comparison cannot discriminate it — and
//! that is a result about the comparison, reported rather than hidden.
//!
//! # Why an unnoticed control is not a failure
//!
//! Because there are two reasons for one, and the tool cannot tell them apart:
//! the comparison may be blind, or the perturbed input may be one the routine
//! genuinely does not read. Both are worth knowing and neither is an error. The
//! honest report is "this comparison does not discriminate this perturbation",
//! and what that means is the caller's to decide — which is exactly what §5.3
//! asks for, since a perturbation nobody would notice proves nothing either
//! way.
//!
//! # Why both verdicts are measured here
//!
//! The plain comparison is run again inside this function rather than taken
//! from the caller. Two verdicts are only comparable if the runs behind them
//! were made the same way, and a verdict carried in from an earlier
//! measurement — different seeding, a different beginning, a cache that was
//! warm the second time — would be compared against a run it has nothing to do
//! with. It costs a second measurement, and §2.5 is worth more than the
//! seconds.

use awaseru_core::snapshot::{NotComparable, Provenance, Snapshot};
use awaseru_core::{Comparison, Verdict, compare};

use crate::arrive::Arriver;
use crate::routine::{self, Given, Routine, Span};

/// A named change to one input — §5.3's "a named input changed".
///
/// Named because a report that said "the verdict did not move when something
/// was changed" would be unactionable. The name is what a reader looks up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Perturbation {
    pub name: String,
    /// The input to change, and what to put there instead.
    pub given: Given,
}

impl Perturbation {
    pub fn new(name: impl Into<String>, span: Span, bytes: Vec<u8>) -> Self {
        Perturbation {
            name: name.into(),
            given: Given { span, bytes },
        }
    }

    /// The givens of a measurement, with this perturbation in place of the one
    /// it names.
    ///
    /// A perturbation of a span nobody seeded is **added** rather than
    /// refused: the inputs a routine reads are not all seeded by a comparison,
    /// and changing one the caller left at whatever the machine held is a
    /// legitimate control.
    pub fn applied_to(&self, given: &[Given]) -> Vec<Given> {
        let mut applied: Vec<Given> = given
            .iter()
            .filter(|g| g.span != self.given.span)
            .cloned()
            .collect();
        applied.push(self.given.clone());
        applied
    }
}

/// What a control showed — or that none was run (§5.3).
///
/// An enum and not an `Option`, so that the absence has a `Display` of its own.
/// A report whose control field read "None" would be read as "no problem".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Control {
    /// Nobody ran one. **The tool cannot force a control and records its
    /// absence** — §5.3's last sentence, as a value.
    #[default]
    NotRun,
    Ran {
        /// Which perturbation, by name.
        perturbation: String,
        /// The verdict with the inputs as given.
        plain: Verdict,
        /// And with the named input changed.
        perturbed: Verdict,
    },
}

impl Control {
    /// Whether the comparison noticed — `None` when no control was run, which
    /// is a different answer from "no".
    pub fn noticed(&self) -> Option<bool> {
        match self {
            Control::NotRun => None,
            Control::Ran {
                plain, perturbed, ..
            } => Some(plain != perturbed),
        }
    }

    /// Whether this comparison has been shown to discriminate anything.
    ///
    /// False for a control that was not run as well as for one that was not
    /// noticed, and that is the point: a measurement with no control and a
    /// measurement whose control went unnoticed are both measurements nothing
    /// has shown to be sensitive to its inputs.
    pub fn discriminates(&self) -> bool {
        self.noticed() == Some(true)
    }
}

impl std::fmt::Display for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Control::NotRun => write!(
                f,
                "no control was run, so nothing here has been shown to be sensitive to its \
                 inputs — a measurement without a control that varies is incomplete (§5.3)"
            ),
            Control::Ran {
                perturbation,
                plain,
                perturbed,
            } if plain == perturbed => write!(
                f,
                "the control `{perturbation}` was NOT noticed: changing it left the verdict at \
                 `{plain}`, so this comparison cannot discriminate it. Either the comparison is \
                 blind or the routine does not read what was changed"
            ),
            Control::Ran {
                perturbation,
                plain,
                perturbed,
            } => write!(
                f,
                "the control `{perturbation}` was noticed: `{plain}` became `{perturbed}`"
            ),
        }
    }
}

/// Why a control could not be run.
#[derive(Debug)]
pub enum Error {
    Routine(routine::Error),
    /// The three states are not states of the same thing (§16.5). Refused
    /// rather than compared, as everywhere else.
    NotComparable(NotComparable),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Routine(e) => write!(f, "{e}"),
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

impl From<NotComparable> for Error {
    fn from(e: NotComparable) -> Self {
        Error::NotComparable(e)
    }
}

/// Runs §5.3's control: the same comparison twice, once with a named input
/// changed.
///
/// `candidate` is what a reimplementation produced from `given`, and it is used
/// for **both** comparisons unchanged. The reference moves; the candidate does
/// not; whether the verdict moves is the answer.
///
/// Leaves the reference where the perturbed measurement ended, which is the
/// routine's return.
pub fn control(
    arriver: &mut Arriver<'_>,
    provenance: &Provenance,
    routine: &Routine,
    given: &[Given],
    candidate: &Snapshot,
    perturbation: &Perturbation,
) -> Result<Control, Error> {
    let verdict_of = |measured: &routine::Measured| -> Result<Verdict, Error> {
        Ok(compare(Comparison {
            seed: &measured.seed,
            reference: &measured.result,
            candidate,
        })?)
    };

    routine::rewind_if_unanchored(arriver, routine)?;
    let plain = routine::measure(arriver, provenance, routine, given)?;
    let plain = verdict_of(&plain)?;

    routine::rewind_if_unanchored(arriver, routine)?;
    let perturbed = routine::measure(
        arriver,
        provenance,
        routine,
        &perturbation.applied_to(given),
    )?;
    let perturbed = verdict_of(&perturbed)?;

    Ok(Control::Ran {
        perturbation: perturbation.name.clone(),
        plain,
        perturbed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use awaseru_core::verdict::{Difference, Undetermined};

    fn agrees() -> Verdict {
        Verdict::Agrees {
            compared: 64,
            moved: 64,
        }
    }

    fn differs() -> Verdict {
        Verdict::Differs(Difference::new(0, 0x01, 0x02, 64, 64))
    }

    /// §5.3's three answers, and the one that matters most is the middle one:
    /// a control that was run and went unnoticed must not read like one that
    /// was run and worked.
    #[test]
    fn the_three_states_of_a_control_all_say_something_different() {
        let not_run = Control::NotRun;
        let unnoticed = Control::Ran {
            perturbation: "a byte the routine never reads".into(),
            plain: agrees(),
            perturbed: agrees(),
        };
        let noticed = Control::Ran {
            perturbation: "the first input byte".into(),
            plain: agrees(),
            perturbed: differs(),
        };

        assert_eq!(not_run.noticed(), None, "not run is not the same as no");
        assert_eq!(unnoticed.noticed(), Some(false));
        assert_eq!(noticed.noticed(), Some(true));

        assert!(!not_run.discriminates());
        assert!(!unnoticed.discriminates());
        assert!(noticed.discriminates());

        let said: Vec<String> = [&not_run, &unnoticed, &noticed]
            .iter()
            .map(|c| c.to_string())
            .collect();
        for (i, a) in said.iter().enumerate() {
            for b in &said[i + 1..] {
                assert_ne!(a, b, "two of the three read alike");
            }
        }
        assert!(said[0].contains("no control was run"), "{}", said[0]);
        assert!(
            said[1].contains("NOT noticed") && said[1].contains("cannot discriminate"),
            "{}",
            said[1]
        );
        assert!(
            said[1].contains("a byte the routine never reads"),
            "an unnoticed control must name itself, or nobody can look it up: {}",
            said[1]
        );
        assert!(said[2].contains("noticed"), "{}", said[2]);
    }

    /// The default must be the absence. A `Control` that defaulted to anything
    /// else would let a report claim a control nobody ran.
    #[test]
    fn the_default_control_is_the_one_nobody_ran() {
        assert_eq!(Control::default(), Control::NotRun);
    }

    /// A verdict that changes *kind* is noticed; so is one that changes only
    /// its numbers. §5.2's `moved` is part of the verdict, and a perturbation
    /// that changed how much the reference moved has been noticed even if both
    /// runs agreed with the candidate.
    #[test]
    fn a_verdict_that_moved_at_all_counts_as_noticed() {
        let ran = |plain, perturbed| Control::Ran {
            perturbation: "x".into(),
            plain,
            perturbed,
        };
        assert_eq!(
            ran(
                agrees(),
                Verdict::Agrees {
                    compared: 64,
                    moved: 63
                }
            )
            .noticed(),
            Some(true)
        );
        assert_eq!(
            ran(
                agrees(),
                Verdict::NotDetermined(Undetermined::Vacuous { compared: 64 })
            )
            .noticed(),
            Some(true),
            "a perturbation that made the comparison vacuous has been noticed — and the \
             vacuousness is what the reader needs to see"
        );
    }

    /// Applying a perturbation replaces the given it names and keeps the rest.
    /// A perturbation that replaced everything would be a different
    /// measurement rather than a control.
    #[test]
    fn a_perturbation_replaces_one_input_and_leaves_the_others() {
        let first = Given {
            span: Span::new("work-ram", 0x300, 2),
            bytes: vec![1, 2],
        };
        let second = Given {
            span: Span::new("work-ram", 0x380, 1),
            bytes: vec![3],
        };
        let p = Perturbation::new(
            "the first input",
            Span::new("work-ram", 0x300, 2),
            vec![9, 9],
        );

        let applied = p.applied_to(&[first.clone(), second.clone()]);
        assert_eq!(applied.len(), 2, "one replaced, one kept");
        assert!(applied.contains(&second), "the untouched input must survive");
        assert!(
            applied.contains(&p.given),
            "and the perturbed one must be the perturbation's"
        );
        assert!(!applied.contains(&first), "the original must be gone");

        // A span nobody seeded is added, because an input a comparison left
        // alone is still an input the routine may read.
        let fresh = Perturbation::new("elsewhere", Span::new("work-ram", 0x3C0, 1), vec![7]);
        let applied = fresh.applied_to(std::slice::from_ref(&first));
        assert_eq!(applied.len(), 2);
        assert!(applied.contains(&first));
    }
}
