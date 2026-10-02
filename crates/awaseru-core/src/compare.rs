//! Comparing snapshots — §5.1, §5.2, §2.2, §2.3.
//!
//! # Why a comparison takes three states and not two
//!
//! Because §5.2 requires every comparison to report `moved` — how much of the
//! compared data the **reference itself** changed — and that number cannot be
//! computed from the two results alone. It needs the state they both began
//! from.
//!
//! This is not a refinement to be added later. §2.2 calls a comparison where
//! the reference changed nothing *vacuous*, and says it is the most common way
//! a verification tool lies, because it lies by passing. A comparison that
//! could not tell a real agreement from an agreement over untouched bytes would
//! be that tool. So the seed is a parameter, and there is no form of this
//! function that goes without it.
//!
//! # What it will not do
//!
//! It will not produce `Agrees` from anything it is unsure of. Every path that
//! cannot establish both "these are the same bytes of the same region" and
//! "the reference moved some of them" ends in `NotDetermined`, with a cause
//! saying which thing was missing (§2.3).

use crate::snapshot::{NotComparable, Snapshot};
use crate::verdict::{Difference, Undetermined, Verdict, fold};

/// The three states a comparison is over.
#[derive(Debug, Clone, Copy)]
pub struct Comparison<'a> {
    /// What both sides began from. Required, because §5.2's `moved` is
    /// measured against it.
    pub seed: &'a Snapshot,
    /// What the reference produced. This is the ground.
    pub reference: &'a Snapshot,
    /// What the reimplementation produced.
    pub candidate: &'a Snapshot,
}

impl<'a> Comparison<'a> {
    /// Refuses three states that are not states of the same thing — §16.5,
    /// §6.6.
    ///
    /// Checked before anything is compared, because a comparison across an
    /// upgrade produces a difference that will be read as the developer's
    /// error.
    pub fn check_comparable(&self) -> Result<(), NotComparable> {
        self.reference
            .provenance()
            .comparable_with(self.candidate.provenance())?;
        self.reference
            .provenance()
            .comparable_with(self.seed.provenance())?;
        Ok(())
    }
}

/// Compares every region either result carries, and folds the verdicts.
///
/// The **union** of the two sides' regions, not the intersection: a region one
/// side captured and the other did not is an absence to be reported (§3.5), and
/// intersecting would quietly drop it.
///
/// Refuses rather than compares when the states are not comparable (§16.5). A
/// caller who insists converts the refusal with `Undetermined::from`, which is
/// the "reported as not determined when the caller insists" half of §16.5 —
/// written in one place so that insisting is a visible act.
///
/// The processor state is **not** part of this, and `compare_processor` says
/// why.
pub fn compare(c: Comparison<'_>) -> Result<Verdict, NotComparable> {
    c.check_comparable()?;

    let mut names: Vec<&str> = c.reference.names().collect();
    for name in c.candidate.names() {
        if !names.contains(&name) {
            names.push(name);
        }
    }

    let verdicts: Vec<Verdict> = names.iter().map(|name| region(c, name)).collect();
    Ok(fold(&verdicts))
}

/// Compares the named regions and folds the verdicts, whether or not either
/// side carries them — a name nobody captured is an absence (§3.5), which is
/// the point of being able to ask for one.
pub fn compare_regions(c: Comparison<'_>, names: &[&str]) -> Result<Verdict, NotComparable> {
    c.check_comparable()?;
    let verdicts: Vec<Verdict> = names.iter().map(|name| region(c, name)).collect();
    Ok(fold(&verdicts))
}

/// One region.
///
/// The order of the checks is the substance of this function, and each one
/// exists because skipping it would produce a confident answer about something
/// else:
///
/// 1. both results must carry the region, or it is absent;
/// 2. they must cover the same bytes of it, or equal contents mean nothing;
/// 3. the seed must carry those same bytes, or `moved` is unknowable;
/// 4. the reference must have moved at least one of them, or the comparison is
///    vacuous;
/// 5. and only then are the two results compared.
pub fn region(c: Comparison<'_>, name: &str) -> Verdict {
    let absent = || {
        Verdict::NotDetermined(Undetermined::RegionAbsent {
            region: name.to_string(),
        })
    };
    let spans_differ = || {
        Verdict::NotDetermined(Undetermined::SpansDiffer {
            region: name.to_string(),
        })
    };

    let (Some(reference), Some(candidate)) = (c.reference.get(name), c.candidate.get(name)) else {
        return absent();
    };
    if !reference.covers_the_same_as(candidate) {
        return spans_differ();
    }

    let Some(seed) = c.seed.get(name) else {
        return Verdict::NotDetermined(Undetermined::MovementUnknown {
            region: name.to_string(),
        });
    };
    if !seed.covers_the_same_as(reference) {
        return spans_differ();
    }

    let compared = reference.len();
    let moved = differing(seed.bytes(), reference.bytes());
    if moved == 0 {
        // §2.2. The two sides agree about data neither of them wrote, and
        // saying "agrees" here is the lie this project is built to not tell.
        return Verdict::NotDetermined(Undetermined::Vacuous { compared });
    }

    match first_difference(reference.bytes(), candidate.bytes()) {
        None => Verdict::Agrees { compared, moved },
        Some(at) => Verdict::Differs(Difference {
            // The region's offset, not the span's: a developer reads this
            // against the region, and a number relative to wherever the
            // capture happened to start would send them to the wrong place.
            first: reference.offset + at,
            expected: reference.bytes()[at],
            found: candidate.bytes()[at],
            differing: differing(reference.bytes(), candidate.bytes()),
            compared,
        }),
    }
}

/// The processor state (§3.3), compared opaquely.
///
/// Separate from `compare`, and not folded into it, for a reason that is about
/// honesty rather than tidiness. The record this version carries is opaque
/// (§7.6 is still open), and on at least one backend it **begins with a cycle
/// count**. So two runs that did the same work in different numbers of cycles
/// differ here, and the difference is reported as a byte offset in an opaque
/// blob — which is true, useless, and would outrank every region's verdict in
/// the fold (§2.3 ranks a difference above everything).
///
/// Folding it in would therefore make `compare` answer "differs" for almost
/// every real comparison, about something nobody asked about. §5.4's
/// localisation is what makes a processor-state comparison worth having, and it
/// needs the layout transcribed, which is M3's.
///
/// It is here, and callable, because §3.3 is emphatic that the processor state
/// is not optional — and because an absent one must be reportable as absent.
pub fn compare_processor(c: Comparison<'_>) -> Verdict {
    let (Some(reference), Some(candidate)) = (c.reference.processor(), c.candidate.processor())
    else {
        return Verdict::NotDetermined(Undetermined::CapabilityAbsent {
            capability: "the processor state, which one of these snapshots does not carry".into(),
        });
    };
    if reference.len() != candidate.len() {
        return Verdict::NotDetermined(Undetermined::SpansDiffer {
            region: "the processor state".into(),
        });
    }
    let Some(seed) = c.seed.processor() else {
        return Verdict::NotDetermined(Undetermined::MovementUnknown {
            region: "the processor state".into(),
        });
    };

    let compared = reference.len();
    let moved = differing(seed.bytes(), reference.bytes());
    if moved == 0 {
        return Verdict::NotDetermined(Undetermined::Vacuous { compared });
    }
    match first_difference(reference.bytes(), candidate.bytes()) {
        None => Verdict::Agrees { compared, moved },
        Some(at) => Verdict::Differs(Difference {
            first: at,
            expected: reference.bytes()[at],
            found: candidate.bytes()[at],
            differing: differing(reference.bytes(), candidate.bytes()),
            compared,
        }),
    }
}

/// §16.5's other half: the refusal, turned into a verdict, for a caller who
/// insists on one.
///
/// A conversion rather than a second comparison function, so that insisting is
/// something written at the call site and visible in a review.
impl From<NotComparable> for Undetermined {
    fn from(e: NotComparable) -> Self {
        Undetermined::StatesIncomparable {
            why: e.to_string(),
        }
    }
}

fn differing(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

fn first_difference(a: &[u8], b: &[u8]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| x != y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::{Access, Region};
    use crate::run::Position;
    use crate::snapshot::{Processor, Provenance};

    fn provenance() -> Provenance {
        Provenance {
            reference: "ref-a".into(),
            backend: "a-backend".into(),
            version: "1.0.0".into(),
            software: "abcdef0123456789".into(),
        }
    }

    fn work() -> Region {
        Region::bytes("work", 16, Access::ReadWrite)
    }

    /// A snapshot of `work` holding `bytes`, whole.
    fn snap(bytes: &[u8]) -> Snapshot {
        Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0x8000 })
            .whole(work(), bytes.to_vec())
            .expect("it fits")
            .build()
    }

    fn empty() -> Snapshot {
        Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0x8000 }).build()
    }

    const SEED: [u8; 16] = [0; 16];
    const MOVED: [u8; 16] = [1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    fn comparison<'a>(
        seed: &'a Snapshot,
        reference: &'a Snapshot,
        candidate: &'a Snapshot,
    ) -> Comparison<'a> {
        Comparison {
            seed,
            reference,
            candidate,
        }
    }

    // ---- the four it must get right ------------------------------------

    /// Equal results, over bytes the reference moved, is agreement — and it
    /// says how much moved.
    #[test]
    fn equal_results_over_moved_bytes_agree() {
        let (seed, reference, candidate) = (snap(&SEED), snap(&MOVED), snap(&MOVED));
        let verdict = compare(comparison(&seed, &reference, &candidate)).expect("comparable");
        assert_eq!(
            verdict,
            Verdict::Agrees {
                compared: 16,
                moved: 4
            }
        );
    }

    /// **The test §2.2 exists for.** Equal results over bytes the reference did
    /// *not* move is not agreement. Written without the seed, this is the most
    /// confident wrong answer the tool could give.
    #[test]
    fn equal_results_over_bytes_nothing_moved_are_vacuous_and_not_agreement() {
        let (seed, reference, candidate) = (snap(&SEED), snap(&SEED), snap(&SEED));
        let verdict = compare(comparison(&seed, &reference, &candidate)).expect("comparable");
        assert_eq!(
            verdict,
            Verdict::NotDetermined(Undetermined::Vacuous { compared: 16 })
        );
        assert_ne!(
            verdict,
            Verdict::Agrees {
                compared: 16,
                moved: 0
            },
            "there is no such thing as agreement over nothing moved"
        );
        assert!(verdict.to_string().contains("neither side wrote"));
    }

    /// A difference names where, what, and how many — against the region's
    /// offsets, which is what a person reads it against.
    #[test]
    fn a_difference_says_where_and_what_and_how_many() {
        let mut wrong = MOVED;
        wrong[2] = 0xFF;
        let (seed, reference, candidate) = (snap(&SEED), snap(&MOVED), snap(&wrong));
        let verdict = compare(comparison(&seed, &reference, &candidate)).expect("comparable");
        match verdict {
            Verdict::Differs(d) => {
                assert_eq!(d.first, 2);
                assert_eq!((d.expected, d.found), (3, 0xFF));
                assert_eq!(d.differing, 1);
                assert_eq!(d.compared, 16);
            }
            other => panic!("expected a difference, got {other}"),
        }
    }

    /// §3.5. A region one side did not capture is absent, not equal — whichever
    /// side is missing it.
    #[test]
    fn a_region_one_side_did_not_capture_is_absent_on_either_side() {
        let moved = snap(&MOVED);
        let nothing = empty();
        let seed = snap(&SEED);

        for (reference, candidate) in [(&moved, &nothing), (&nothing, &moved)] {
            let verdict = compare(comparison(&seed, reference, candidate)).expect("comparable");
            assert!(
                matches!(
                    verdict,
                    Verdict::NotDetermined(Undetermined::RegionAbsent { .. })
                ),
                "got {verdict}"
            );
        }
    }

    /// The union, not the intersection. A region only the candidate carries
    /// must be reported absent rather than skipped — skipping it is how a
    /// comparison comes to be over less than it was asked for.
    #[test]
    fn a_region_only_one_side_carries_is_not_quietly_dropped() {
        let seed = snap(&SEED);
        let reference = empty();
        let candidate = snap(&MOVED);
        let verdict = compare(comparison(&seed, &reference, &candidate)).expect("comparable");
        assert!(
            matches!(
                verdict,
                Verdict::NotDetermined(Undetermined::RegionAbsent { .. })
            ),
            "an empty reference against a candidate that captured something must not come out \
             as agreement over nothing, got {verdict}"
        );
    }

    // ---- the ways a comparison can be meaningless -----------------------

    /// Without the region in the seed, §2.2's number cannot be computed, so
    /// nothing is claimed.
    #[test]
    fn without_the_seeds_region_the_movement_is_unknown_and_nothing_is_claimed() {
        let seed = empty();
        let (reference, candidate) = (snap(&MOVED), snap(&MOVED));
        let verdict = compare(comparison(&seed, &reference, &candidate)).expect("comparable");
        assert_eq!(
            verdict,
            Verdict::NotDetermined(Undetermined::MovementUnknown {
                region: "work".into()
            })
        );
        assert!(verdict.to_string().contains("not evidence"));
    }

    /// Two captures of different bytes of one region are not comparable, even
    /// when their contents are identical — which is the case that would
    /// otherwise pass.
    #[test]
    fn captures_of_different_bytes_are_not_compared_however_equal() {
        let seed = snap(&SEED);
        let reference = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .span(work(), 0, vec![7u8; 4])
            .unwrap()
            .build();
        let candidate = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .span(work(), 8, vec![7u8; 4])
            .unwrap()
            .build();
        let verdict = compare(comparison(&seed, &reference, &candidate)).expect("comparable");
        assert!(
            matches!(
                verdict,
                Verdict::NotDetermined(Undetermined::SpansDiffer { .. })
            ),
            "identical contents at different offsets are not agreement, got {verdict}"
        );
    }

    /// §16.5. A comparison across versions is refused, before anything is
    /// compared — so that a difference produced by an upgrade is never
    /// reported as a difference in somebody's work.
    #[test]
    fn a_comparison_across_versions_is_refused_rather_than_answered() {
        let seed = snap(&SEED);
        let reference = snap(&MOVED);
        let mut other = provenance();
        other.version = "1.0.1".into();
        let candidate = Snapshot::builder(other, Position::InstructionBoundary { pc: 0 })
            .whole(work(), MOVED.to_vec())
            .unwrap()
            .build();

        let err = compare(comparison(&seed, &reference, &candidate)).expect_err("not comparable");
        assert!(matches!(err, NotComparable::DifferentReference { .. }));

        // And the other half of §16.5: a caller who insists gets a verdict,
        // and it is `not determined` — never the agreement the bytes would
        // have given.
        let insisted = Verdict::NotDetermined(Undetermined::from(err));
        assert!(matches!(
            insisted,
            Verdict::NotDetermined(Undetermined::StatesIncomparable { .. })
        ));
        assert!(insisted.to_string().contains("new reference"));
    }

    // ---- the processor state --------------------------------------------

    /// It is not part of `compare`, and this is the test that keeps it out.
    ///
    /// Folded in, the opaque record's leading cycle count would differ for
    /// almost every real comparison and outrank every region in the fold
    /// (§2.3), so `compare` would answer "differs" about something nobody
    /// asked about.
    #[test]
    fn the_processor_state_is_not_folded_into_the_region_comparison() {
        // The seed carries one too, or `compare_processor` would answer
        // `MovementUnknown` and this test would be checking that instead —
        // which is what its first version did.
        let seed = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .processor(Processor::opaque(vec![0, 0, 0, 0]))
            .whole(work(), SEED.to_vec())
            .unwrap()
            .build();
        let reference = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .processor(Processor::opaque(vec![1, 2, 3, 4]))
            .whole(work(), MOVED.to_vec())
            .unwrap()
            .build();
        let candidate = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .processor(Processor::opaque(vec![9, 9, 9, 9]))
            .whole(work(), MOVED.to_vec())
            .unwrap()
            .build();

        assert_eq!(
            compare(comparison(&seed, &reference, &candidate)).expect("comparable"),
            Verdict::Agrees {
                compared: 16,
                moved: 4
            },
            "the regions agree, and the processor states differing must not change that"
        );

        // Asked for on its own, it differs — and reports it against the
        // record's own offsets.
        let verdict = compare_processor(comparison(&seed, &reference, &candidate));
        assert!(
            matches!(verdict, Verdict::Differs(_)),
            "asked directly, it must answer, got {verdict}"
        );
    }

    /// A processor state one side does not carry is absent, not equal. §3.3 is
    /// emphatic that a comparison seeded without it runs some other routine's
    /// registers.
    #[test]
    fn an_absent_processor_state_is_reported_absent() {
        let seed = snap(&SEED);
        let with = Snapshot::builder(provenance(), Position::InstructionBoundary { pc: 0 })
            .processor(Processor::opaque(vec![1, 2, 3, 4]))
            .build();
        let without = empty();
        let verdict = compare_processor(comparison(&seed, &with, &without));
        assert!(
            matches!(
                verdict,
                Verdict::NotDetermined(Undetermined::CapabilityAbsent { .. })
            ),
            "got {verdict}"
        );
    }

    // ---- asking for a particular set ------------------------------------

    #[test]
    fn a_name_nobody_captured_can_be_asked_for_and_comes_back_absent() {
        let (seed, reference, candidate) = (snap(&SEED), snap(&MOVED), snap(&MOVED));
        let verdict = compare_regions(comparison(&seed, &reference, &candidate), &["nowhere"])
            .expect("comparable");
        assert!(
            matches!(
                verdict,
                Verdict::NotDetermined(Undetermined::RegionAbsent { .. })
            ),
            "got {verdict}"
        );
        assert_eq!(
            compare_regions(comparison(&seed, &reference, &candidate), &["work"])
                .expect("comparable"),
            Verdict::Agrees {
                compared: 16,
                moved: 4
            }
        );
    }

    /// Asking for nothing is not agreement. `fold` already refuses an empty
    /// set; this is the path that reaches it.
    #[test]
    fn asking_for_no_regions_is_not_agreement() {
        let (seed, reference, candidate) = (snap(&SEED), snap(&MOVED), snap(&MOVED));
        let verdict =
            compare_regions(comparison(&seed, &reference, &candidate), &[]).expect("comparable");
        assert!(
            matches!(
                verdict,
                Verdict::NotDetermined(Undetermined::Vacuous { compared: 0 })
            ),
            "got {verdict}"
        );
    }
}
