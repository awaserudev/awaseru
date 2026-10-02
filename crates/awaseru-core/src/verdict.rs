//! Verdicts — §2.3.
//!
//! Three values, never two. A tool that cannot tell "they match" from "I did
//! not look" is worse than no tool, so the type makes the two impossible to
//! write the same way, and anything that folds several verdicts into one has to
//! say what it does with the third.

use crate::run::Position;

/// Why a comparison did not happen, or happened without meaning.
///
/// The causes are §2.3's, and each is a different thing to do about it, which
/// is why they are not one string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undetermined {
    /// The backend does not expose that region (§3.5). Absent is not equal.
    RegionAbsent { region: String },
    /// Two references were asked and they disagreed with each other (§5.5).
    /// The ground is not solid here, and that is the answer.
    ReferencesDisagree { first: String, second: String },
    /// The run stopped before reaching the point being compared (§4.3).
    DidNotArrive { stopped: String },
    /// Nothing the reference did touched the compared bytes (§2.2), so the two
    /// sides agree about data neither of them wrote.
    Vacuous { compared: usize },
    /// A capability the comparison needed is not one this backend declares
    /// (§7.3).
    CapabilityAbsent { capability: String },
    /// §2.2's `moved` could not be computed, because the state both sides
    /// began from does not carry this region. Agreement might be real and
    /// might be over bytes nothing touched, and there is no way to tell which
    /// — so it is not reported as agreement.
    MovementUnknown { region: String },
    /// The two sides captured different bytes of the region, so their contents
    /// are not comparable however equal they happen to be.
    SpansDiffer { region: String },
    /// The states are not states of the same thing — a different version of
    /// the reference (§16.5), or different software (§6.6). Reported this way
    /// only when the caller insisted; refused otherwise.
    StatesIncomparable { why: String },
    /// A state was written into the machine at a position no state should be
    /// written at (§3.4), because the caller asked for that and undertook to
    /// carry this.
    SeededFromNoBoundary { position: String },
    /// The run began from an anchor nobody has shown to be equivalent to
    /// replaying its definition (§4.8). The blob may well be right; nothing
    /// has established that it is.
    AnchorNotDemonstrated { anchor: String },
}

impl std::fmt::Display for Undetermined {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Undetermined::RegionAbsent { region } => {
                write!(f, "the backend does not expose a region named `{region}`")
            }
            Undetermined::ReferencesDisagree { first, second } => write!(
                f,
                "the references `{first}` and `{second}` disagree with each other, \
                 so there is no ground to compare against here"
            ),
            Undetermined::DidNotArrive { stopped } => {
                write!(f, "the run stopped at {stopped}, before the point compared")
            }
            Undetermined::Vacuous { compared } => write!(
                f,
                "the reference changed none of the {compared} bytes compared, so agreement \
                 here is agreement about data neither side wrote"
            ),
            Undetermined::CapabilityAbsent { capability } => write!(
                f,
                "this backend does not declare the capability `{capability}`, which the \
                 comparison needed"
            ),
            Undetermined::MovementUnknown { region } => write!(
                f,
                "the state both sides began from does not carry `{region}`, so there is no \
                 telling how much of it the reference moved — and agreement over bytes nothing \
                 touched is not evidence (§2.2)"
            ),
            Undetermined::SpansDiffer { region } => write!(
                f,
                "the two sides captured different bytes of `{region}`, so their contents are not \
                 comparable however equal they happen to be"
            ),
            Undetermined::StatesIncomparable { why } => write!(f, "{why}"),
            Undetermined::AnchorNotDemonstrated { anchor } => write!(
                f,
                "this run began from the anchor `{anchor}`, which has never been shown to be \
                 equivalent to replaying its definition (§4.8). Its blob may well be right, and \
                 nothing has established that — so a comparison from it is not evidence"
            ),
            Undetermined::SeededFromNoBoundary { position } => write!(
                f,
                "this run began from a state written in at {position}, which is not an \
                 instruction boundary — so the machine started part way through something and \
                 what it did afterwards is not what the software would have done (§3.4)"
            ),
        }
    }
}

/// §5.4's third item: what is known about the write that produced the
/// reference's value at the first differing offset.
///
/// Four states, and the first two are the ones that keep this honest. "Nobody
/// asked" and "nothing wrote it" are different answers, and neither of them is
/// "the backend cannot say" — a report that printed one blank for all three
/// would let a reader draw whichever conclusion they already believed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Wrote {
    /// Nobody asked. Every comparison produces this, because localising costs
    /// a replay and §4.2 has no hidden runs: it is a second request.
    #[default]
    NotLooked,
    /// Asked, and this backend does not declare what it would take (§7.3).
    /// The capability is named, because which one is missing is what a reader
    /// would go and change.
    NotAvailable { capability: String },
    /// Asked, and nothing wrote that byte between the seed and the stop. The
    /// reference's value there is the seed's, which usually means the
    /// difference is in the reimplementation's extra write rather than in the
    /// reference's missing one.
    NothingWrote,
    /// The position that last wrote it (§5.4).
    ///
    /// Expect a `MidInstruction` position: a write is caught *during* the
    /// instruction that performs it, which is both where the answer is and a
    /// place §3.4 says nothing may be seeded from.
    At {
        position: Position,
        /// How many times it was written between the seed and the stop, so
        /// that "the last write" is not read as "the only write". A byte
        /// written twice has a first writer this does not name.
        writes: u64,
    },
}

impl std::fmt::Display for Wrote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Wrote::NotLooked => write!(f, "nobody asked what wrote it"),
            Wrote::NotAvailable { capability } => write!(
                f,
                "nothing here can name what wrote it: this backend does not declare `{capability}`"
            ),
            Wrote::NothingWrote => write!(
                f,
                "nothing wrote it — the reference's value there is the one it was seeded with"
            ),
            Wrote::At { position, writes } => {
                if *writes > 1 {
                    write!(
                        f,
                        "last written {position}, and written {writes} times in all — so this is \
                         the last writer and not the only one"
                    )
                } else {
                    write!(f, "written {position}")
                }
            }
        }
    }
}

/// Where two sides parted, and by how much.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// The first offset at which they differ.
    pub first: usize,
    /// What each side had there.
    pub expected: u8,
    pub found: u8,
    /// How many of the compared bytes differ in all.
    pub differing: usize,
    /// How many were compared, so that the count above has a denominator.
    pub compared: usize,
    /// §5.4's third item, which a comparison cannot fill on its own: naming
    /// the writer takes a replay, and a replay takes a reference. `NotLooked`
    /// until something asks.
    pub wrote: Wrote,
}

impl Difference {
    /// The four things a comparison knows by itself. §5.4's third item is
    /// `NotLooked` until something goes and looks.
    pub fn new(first: usize, expected: u8, found: u8, differing: usize, compared: usize) -> Self {
        Difference {
            first,
            expected,
            found,
            differing,
            compared,
            wrote: Wrote::NotLooked,
        }
    }

    /// The same difference, with §5.4's third item filled in.
    pub fn localised(mut self, wrote: Wrote) -> Self {
        self.wrote = wrote;
        self
    }
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} of {} bytes differ, first at {}: expected {:#04X}, found {:#04X}",
            self.differing, self.compared, self.first, self.expected, self.found
        )?;
        match &self.wrote {
            // The ordinary case says nothing extra: a report that ended every
            // difference with "nobody asked what wrote it" would train its
            // reader to stop reading the end of the line.
            Wrote::NotLooked => Ok(()),
            wrote => write!(f, " ({wrote})"),
        }
    }
}

/// The answer to one comparison.
///
/// Deliberately without `is_ok`, without a conversion to `bool` and without a
/// `Default`: every one of those would give somebody a way to treat
/// "not determined" as "agrees" without writing it down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Agrees {
        /// How many bytes were compared, and how many of them the reference
        /// itself moved (§2.2). Agreement over nothing is reported as
        /// `NotDetermined(Vacuous)` instead, so a non-zero `moved` here is an
        /// invariant rather than a hope.
        compared: usize,
        moved: usize,
    },
    Differs(Difference),
    NotDetermined(Undetermined),
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Verdict::Agrees { compared, moved } => write!(
                f,
                "agrees over {compared} bytes, of which the reference moved {moved}"
            ),
            Verdict::Differs(d) => write!(f, "differs: {d}"),
            Verdict::NotDetermined(why) => write!(f, "not determined: {why}"),
        }
    }
}

/// Folds several verdicts into one.
///
/// The order matters and is the point of the function existing at all:
/// **a difference outranks an absence, and an absence outranks agreement.**
/// Written the obvious way — "all of them agree" — a run containing one
/// `NotDetermined` and nothing else would come out as agreement, which is the
/// mistake §2.3 exists to prevent.
///
/// An empty set is not agreement either: nothing was compared.
pub fn fold(verdicts: &[Verdict]) -> Verdict {
    if verdicts.is_empty() {
        return Verdict::NotDetermined(Undetermined::Vacuous { compared: 0 });
    }

    if let Some(Verdict::Differs(d)) = verdicts.iter().find(|v| matches!(v, Verdict::Differs(_))) {
        return Verdict::Differs(d.clone());
    }

    if let Some(Verdict::NotDetermined(why)) = verdicts
        .iter()
        .find(|v| matches!(v, Verdict::NotDetermined(_)))
    {
        return Verdict::NotDetermined(why.clone());
    }

    let compared = verdicts
        .iter()
        .map(|v| match v {
            Verdict::Agrees { compared, .. } => *compared,
            _ => 0,
        })
        .sum();
    let moved = verdicts
        .iter()
        .map(|v| match v {
            Verdict::Agrees { moved, .. } => *moved,
            _ => 0,
        })
        .sum();
    Verdict::Agrees { compared, moved }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agrees() -> Verdict {
        Verdict::Agrees {
            compared: 10,
            moved: 4,
        }
    }

    fn undetermined() -> Verdict {
        Verdict::NotDetermined(Undetermined::RegionAbsent {
            region: "somewhere".into(),
        })
    }

    fn differs() -> Verdict {
        Verdict::Differs(Difference::new(3, 0x4F, 0x00, 1, 10))
    }

    /// **The test this module exists for.** A set containing one undetermined
    /// verdict is not agreement. Written as "none of them differ", this comes
    /// out as agreement and the tool starts lying.
    #[test]
    fn one_undetermined_verdict_makes_the_whole_fold_undetermined() {
        let folded = fold(&[agrees(), undetermined(), agrees()]);
        assert!(
            matches!(folded, Verdict::NotDetermined(_)),
            "a fold containing an undetermined verdict must not be agreement, got {folded}"
        );
        assert_ne!(folded, agrees());
    }

    /// And a difference outranks an absence: knowing they differ is a stronger
    /// statement than not knowing, so it is the one reported.
    #[test]
    fn a_difference_outranks_an_absence() {
        let folded = fold(&[undetermined(), differs(), agrees()]);
        match folded {
            Verdict::Differs(d) => assert_eq!(d.first, 3),
            other => panic!("expected the difference to win, got {other}"),
        }
    }

    /// Nothing compared is not agreement.
    #[test]
    fn an_empty_fold_is_not_agreement() {
        let folded = fold(&[]);
        assert!(
            matches!(
                folded,
                Verdict::NotDetermined(Undetermined::Vacuous { compared: 0 })
            ),
            "got {folded}"
        );
    }

    /// Agreement sums both counts, so the denominator survives the fold. A
    /// fold that kept only the first would report agreement over ten bytes
    /// when twenty were compared.
    #[test]
    fn agreement_sums_what_was_compared_and_what_moved() {
        let folded = fold(&[agrees(), agrees()]);
        assert_eq!(
            folded,
            Verdict::Agrees {
                compared: 20,
                moved: 8
            }
        );
    }

    /// Every variant says something a person can act on, and the undetermined
    /// ones say *which* kind of not-knowing it is.
    #[test]
    fn every_verdict_explains_itself() {
        assert!(agrees().to_string().contains("moved 4"));
        assert!(differs().to_string().contains("first at 3"));
        assert!(undetermined().to_string().contains("somewhere"));

        let vacuous = Verdict::NotDetermined(Undetermined::Vacuous { compared: 512 });
        let said = vacuous.to_string();
        assert!(
            said.contains("512") && said.contains("neither side wrote"),
            "a vacuous verdict must say why agreement there would be empty, said: {said}"
        );
    }
}
