//! §5.4's third item: which position last wrote the byte.
//!
//! > "Your byte is one too low" sends them reading; "the write at this
//! > position did not happen" is the answer.
//!
//! The first two items of §5.4 — the offset and the two values — are what a
//! comparison knows by looking at bytes. This one is not knowable from bytes at
//! all: it is a fact about the run that produced them, and the run is over by
//! the time anybody is reading the difference. So localising is **a replay**,
//! which is what M2's anchors were built for, and it is a second request rather
//! than something every comparison pays for (§4.2 has no hidden runs).
//!
//! # The two depths, and why both
//!
//! The **cheap filter** is the access counters' write record, read where the
//! machine already stands. It can say *never written*, which is an answer and
//! costs nothing. It cannot say where from — the stamp it carries is the
//! backend's own clock, comparable with other stamps and with nothing else.
//!
//! The **exact answer** is a write breakpoint and a replay. The write is caught
//! during the instruction performing it, before it commits, and the
//! instruction's own program counter is the store. That position is
//! mid-instruction, which is both the exact answer and a place §3.4 forbids
//! seeding from — so it is reported and never reused as a starting point.
//!
//! # What bounds the replay
//!
//! §4.5. The bound carries the routine's return address as well as the byte, so
//! the replay stops at the end of its subject whatever the byte does. Without
//! that, a byte written once before the return and never again would send the
//! replay on through the return and into the next routine, and the position it
//! finally reported would name an instruction with nothing to do with the
//! question.

use awaseru_core::run::{Bound, Reason};
use awaseru_core::{Capability, Position, Recency, Wrote};

use crate::arrive::Arriver;
use crate::routine::{self, Given, Routine};

/// What was asked, and how much of it the answer cost.
///
/// Carried alongside the answer because §5.4's third item is the expensive one:
/// a report that did not say whether a replay happened would make the cheap
/// path and the exact path look alike, and somebody tuning a slow run needs to
/// know which they got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Localised {
    pub region: String,
    pub offset: usize,
    pub wrote: Wrote,
    /// Whether the answer cost a replay, or the filter settled it.
    pub replayed: bool,
    /// What the replay began from: the anchor's name, or `None` for the
    /// reference's own origin (§2.5).
    ///
    /// Carried because a localisation is only about the measurement it
    /// explains if both began in the same place. A measurement run from
    /// wherever the machine happened to be and a localisation run from the
    /// origin are two runs, and the position named belongs to the second.
    pub from: Option<String>,
}

impl Localised {
    /// Whether the position named is one §3.4 would allow a state to be seeded
    /// at.
    ///
    /// It should not be: a write is caught during the instruction performing
    /// it, so the exact answer is always mid-instruction. The question is here
    /// because the two are easy to confuse, and because a report whose
    /// position looked seedable would invite somebody to seed from it.
    pub fn wrote_at_an_instruction_boundary(&self) -> bool {
        matches!(&self.wrote, Wrote::At { position, .. } if position.is_instruction_boundary())
    }
}

impl std::fmt::Display for Localised {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}`+{}: {}{}",
            self.region,
            self.offset,
            self.wrote,
            match (self.replayed, &self.from) {
                (false, _) => String::new(),
                (true, None) => " (found by replaying the routine from the origin)".to_string(),
                (true, Some(anchor)) => {
                    format!(" (found by replaying the routine from `{anchor}`)")
                }
            }
        )
    }
}

/// What localisation needs, and what it answers when a backend has not
/// declared it — §7.3.
///
/// Two capabilities, in the order they matter: stopping on the write is what
/// finds the moment, and naming the instruction is what makes the moment an
/// answer. A backend with the first and not the second could stop in exactly
/// the right place and say nothing useful about it, so the answer names
/// whichever is missing first rather than reporting "localisation" as one
/// indivisible thing.
///
/// Separate from `localise` so that this decision can be tested without a
/// reference: it is the one branch of localisation that must work on a backend
/// that cannot localise, which is precisely the backend hardest to arrange for
/// a test.
pub fn missing_capability(declared: &awaseru_core::Capabilities) -> Option<Wrote> {
    [Capability::StopOnWrite, Capability::WritingPosition]
        .into_iter()
        .find(|c| !declared.has(*c))
        .map(|absent| Wrote::NotAvailable {
            capability: absent.name().to_string(),
        })
}

/// Names the position that last wrote one byte, during one routine — §5.4.
///
/// Asked **after** a measurement of the same routine with the same givens, and
/// while the reference still stands where that measurement left it: the cheap
/// filter reads the machine as it is, and the replay re-derives the measurement
/// from its own beginning. Called at any other moment, the filter is answering
/// about some other run.
///
/// The reference is left wherever the replay ended, which is the routine's
/// return — the same place a measurement leaves it, and not the place it was
/// when this was called.
pub fn localise(
    arriver: &mut Arriver<'_>,
    routine: &Routine,
    given: &[Given],
    region: &str,
    offset: usize,
) -> Result<Localised, routine::Error> {
    let answer = |wrote, replayed| {
        Ok(Localised {
            region: region.to_string(),
            offset,
            wrote,
            replayed,
            from: routine.from.clone(),
        })
    };

    // ---- what it would take, asked before it is relied on (§7.3) ---------
    if let Some(absent) = missing_capability(&arriver.capabilities()) {
        return answer(absent, false);
    }

    // ---- the cheap filter -------------------------------------------------
    // A byte with no write record at all cannot have been written by the
    // routine, so there is nothing to replay for. Everything else falls
    // through: a stamp says *something* wrote it at some point, which includes
    // the seeding and everything before the routine, so it is a reason to go
    // and look rather than an answer.
    match arriver.write_recency(region, offset)? {
        Recency::NeverWritten => return answer(Wrote::NothingWrote, false),
        Recency::Stamp(_) => {}
        // Declared and then not supplied. The declaration is wrong, and
        // reporting the capability as absent is the honest reading of the two
        // contradicting each other — §2.4 prefers a refusal to a guess.
        Recency::NotSupplied => {
            return answer(
                Wrote::NotAvailable {
                    capability: Capability::WriteRecency.name().to_string(),
                },
                false,
            );
        }
    }

    // ---- the replay -------------------------------------------------------
    // A replay needs a beginning. An anchor is one and `arrive` gets there
    // itself; without one, the beginning is the origin, and rewinding to it is
    // not optional — the machine is standing at the routine's *return* when
    // this is called, which is past the entry, so running to the entry from
    // there arrives nowhere and spends the whole budget doing it.
    routine::rewind_if_unanchored(arriver, routine)?;
    routine::enter_and_seed(arriver, routine, given)?;

    // Every write hit costs the routine at least one instruction, so a routine
    // that must return within `within` instructions cannot write one byte more
    // often than that. The cap is derived from the bound rather than picked,
    // and it is here because §4.2's "no unbounded run" is about the loop as
    // well as about each run in it.
    let cap = routine.within;
    let mut writes: u64 = 0;
    let mut last: Option<Position> = None;

    loop {
        let stop = arriver.run(Bound::Write {
            region: region.to_string(),
            offset,
            until: routine.returns_to,
            within: routine.within,
        })?;
        match &stop.reason {
            Reason::WriteHit { .. } => {
                writes += 1;
                last = Some(stop.position.clone());
                if writes > cap {
                    // Not reachable by a routine that returns: it would have
                    // had to write the byte more times than it has
                    // instructions. Reported rather than looped on.
                    return Err(routine::Error::NeverReturned {
                        routine: routine.name.clone(),
                        stop,
                    });
                }
            }
            Reason::AddressHit { address } if *address == routine.returns_to => break,
            // Anything else is the replay not being the run it was supposed to
            // re-derive: the budget ran out, the backend refused the bound, or
            // it stopped somewhere nobody asked about. A position named from
            // such a run would be a position from a different run.
            _ => {
                return Err(routine::Error::NeverReturned {
                    routine: routine.name.clone(),
                    stop,
                });
            }
        }
    }

    match last {
        // The filter let it through on a stamp and the routine turns out not to
        // write it: the stamp was from before the routine — the seeding, or the
        // power-on. The replay is the authority and this is its answer.
        None => answer(Wrote::NothingWrote, true),
        Some(position) => answer(Wrote::At { position, writes }, true),
    }
}

/// The same, for a difference a comparison has just produced.
///
/// `Difference::first` is an offset **into its region** (§5.4's first item is
/// read against the region, not against whatever span was captured), so the
/// region's name is all this needs beyond the difference itself — and the
/// difference does not carry it, because a comparison over several regions
/// folds into one verdict and the caller is the one that knows which name it
/// asked about.
pub fn localise_difference(
    arriver: &mut Arriver<'_>,
    routine: &Routine,
    given: &[Given],
    region: &str,
    difference: &awaseru_core::Difference,
) -> Result<Localised, routine::Error> {
    localise(arriver, routine, given, region, difference.first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use awaseru_core::Capabilities;

    /// §7.3 at the one place it decides something in this module. The order
    /// matters: a backend that can stop on a write but not name the writer is
    /// told about the naming, because that is the one it would have to go and
    /// get.
    #[test]
    fn a_backend_that_cannot_localise_is_told_which_half_it_lacks() {
        let nothing = missing_capability(&Capabilities::none());
        assert_eq!(
            nothing,
            Some(Wrote::NotAvailable {
                capability: "stop-on-write".into()
            }),
            "with neither, the first thing it needs is the one reported"
        );

        let halfway = missing_capability(&Capabilities::of([Capability::StopOnWrite]));
        assert_eq!(
            halfway,
            Some(Wrote::NotAvailable {
                capability: "writing-position".into()
            }),
            "stopping in the right place is not an answer about what wrote it"
        );

        assert_eq!(
            missing_capability(&Capabilities::of([
                Capability::StopOnWrite,
                Capability::WritingPosition,
            ])),
            None,
            "and a backend with both is not refused"
        );
    }

    /// The four states must not read alike. A reader who cannot tell "nobody
    /// asked" from "nothing wrote it" will take the one that suits them.
    #[test]
    fn the_four_answers_all_say_something_different() {
        let said: Vec<String> = [
            Wrote::NotLooked,
            Wrote::NotAvailable {
                capability: "writing-position".into(),
            },
            Wrote::NothingWrote,
            Wrote::At {
                position: Position::MidInstruction { pc: 0x80_2C },
                writes: 1,
            },
        ]
        .iter()
        .map(|w| w.to_string())
        .collect();

        for (i, a) in said.iter().enumerate() {
            for b in &said[i + 1..] {
                assert_ne!(a, b, "two of the four answers read the same");
            }
        }
        assert!(said[1].contains("writing-position"), "{}", said[1]);
        assert!(said[3].contains("802C"), "{}", said[3]);
        assert!(
            !said[3].contains("not the only one"),
            "a byte written once must not be hedged: {}",
            said[3]
        );

        // Written twice, the report says the first writer is not named. A
        // developer told "the write at this position" about a byte written
        // three times would go and read the wrong instruction.
        let twice = Wrote::At {
            position: Position::MidInstruction { pc: 0x80_2C },
            writes: 3,
        }
        .to_string();
        assert!(twice.contains("3 times"), "{twice}");
        assert!(twice.contains("not the only one"), "{twice}");
    }

    #[test]
    fn a_localisation_says_whether_it_cost_a_replay() {
        let cheap = Localised {
            region: "work-ram".into(),
            offset: 0x400,
            wrote: Wrote::NothingWrote,
            replayed: false,
            from: None,
        };
        let dear = Localised {
            replayed: true,
            ..cheap.clone()
        };
        assert!(!cheap.to_string().contains("replay"), "{cheap}");
        assert!(dear.to_string().contains("replaying"), "{dear}");
        assert!(dear.to_string().contains("work-ram"), "{dear}");
        assert!(
            dear.to_string().contains("origin"),
            "a replay must say where it began: {dear}"
        );
        let anchored = Localised {
            from: Some("after-the-opening".into()),
            ..dear.clone()
        };
        assert!(
            anchored.to_string().contains("after-the-opening"),
            "{anchored}"
        );
    }
}
