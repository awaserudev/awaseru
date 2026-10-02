//! Capabilities — §7.3.
//!
//! §7.2 has a short mandatory list every backend must do, and a second list of
//! things a backend *may* do. This module is the vocabulary for the second
//! one, and the gate in front of it: **a backend states what it can do, the
//! host asks before relying on it, and a comparison that needed something
//! absent is not determined (§2.3), never silently weaker.**
//!
//! Three rules shape what is here.
//!
//! **A capability is named, not a platform's feature.** No entry says
//! "breakpoint", "trace logger" or anything else one emulator calls its own
//! (§2.7). Each is phrased as the question a comparison asks — *can you stop
//! when this address is written?* — so a second backend answering it with
//! entirely different machinery answers the same question.
//!
//! **A route is not a declaration.** A backend's library may export something
//! that looks like a capability; until the backend crate has exercised it and
//! can answer with it, it is not declared. The first backend has three such
//! routes recorded in `doc/backend.md` and declares none of them, which is the
//! difference between §7.3 and a guess.
//!
//! **An absent capability is a verdict, not a shrug.** `needing` is how a
//! comparison that rests on one says so, and what it produces is
//! `Undetermined::CapabilityAbsent` — the third value, with the capability
//! named in it.

use crate::verdict::{Undetermined, Verdict};

/// One thing a backend may be able to do beyond §7.2's mandatory verbs.
///
/// Deliberately a closed enum rather than strings: a capability a comparison
/// asks for by a name the backend spells differently is a capability silently
/// absent, which is the failure §7.3 exists to prevent. A backend with
/// something genuinely new adds a variant here, and the compiler then shows
/// every comparison that could use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// Stop a run when a given address is executed — §4.3's bound by address.
    StopOnExecution,
    /// Stop a run when a given address is written.
    StopOnWrite,
    /// Stop a run when a given address is read.
    StopOnRead,
    /// Say *when* a byte was last written, in some clock of the backend's own.
    ///
    /// Useful only against itself: one byte's recency against another's. §5.4's
    /// cheap filter, and no answer at all to "where from".
    WriteRecency,
    /// Say *which position* last wrote a byte — §5.4's third item, the one the
    /// specification says changes the developer's day.
    WritingPosition,
    /// Say which instructions a run executed — §7.2's execution coverage.
    ExecutionCoverage,
    /// Report calls and returns as they happen — §7.2.
    CallAndReturnEvents,
    /// Report writes to the machine's registers with their position within a
    /// frame — §7.2.
    RegisterWrites,
    /// Replay a recorded input log, so that a definition needing input can be
    /// reached at all (§4.7).
    InputReplay,
}

impl Capability {
    /// Every capability this vocabulary has, so that a backend's declaration
    /// can be read against the whole list rather than against a reader's
    /// memory of it.
    pub const ALL: [Capability; 9] = [
        Capability::StopOnExecution,
        Capability::StopOnWrite,
        Capability::StopOnRead,
        Capability::WriteRecency,
        Capability::WritingPosition,
        Capability::ExecutionCoverage,
        Capability::CallAndReturnEvents,
        Capability::RegisterWrites,
        Capability::InputReplay,
    ];

    /// The name a report prints and a configuration would spell.
    pub fn name(self) -> &'static str {
        match self {
            Capability::StopOnExecution => "stop-on-execution",
            Capability::StopOnWrite => "stop-on-write",
            Capability::StopOnRead => "stop-on-read",
            Capability::WriteRecency => "write-recency",
            Capability::WritingPosition => "writing-position",
            Capability::ExecutionCoverage => "execution-coverage",
            Capability::CallAndReturnEvents => "call-and-return-events",
            Capability::RegisterWrites => "register-writes",
            Capability::InputReplay => "input-replay",
        }
    }

    /// What a caller gets if it is there — one line, because a capability
    /// absent from a report is read by somebody deciding what to do next.
    pub fn means(self) -> &'static str {
        match self {
            Capability::StopOnExecution => "stop a run when an address is executed",
            Capability::StopOnWrite => "stop a run when an address is written",
            Capability::StopOnRead => "stop a run when an address is read",
            Capability::WriteRecency => "say when a byte was last written, in the backend's own clock",
            Capability::WritingPosition => "say which position last wrote a byte",
            Capability::ExecutionCoverage => "say which instructions a run executed",
            Capability::CallAndReturnEvents => "report calls and returns as they happen",
            Capability::RegisterWrites => "report register writes with their position in a frame",
            Capability::InputReplay => "replay a recorded input log",
        }
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// What one backend declares — §7.3.
///
/// Held sorted and without repeats so that two backends declaring the same
/// things read the same way, and so that a report's list is stable between
/// runs (§2.5 is about the whole report, not only the bytes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    declared: Vec<Capability>,
}

impl Capabilities {
    /// A backend that declares nothing. Valid, and the honest answer for a
    /// backend driven through an interface that offers only §7.2's verbs.
    pub fn none() -> Self {
        Capabilities::default()
    }

    /// A declaration, from whatever order it is written in.
    pub fn of(declared: impl IntoIterator<Item = Capability>) -> Self {
        let mut declared: Vec<Capability> = declared.into_iter().collect();
        declared.sort();
        declared.dedup();
        Capabilities { declared }
    }

    /// Whether this backend declares it.
    pub fn has(&self, capability: Capability) -> bool {
        self.declared.contains(&capability)
    }

    /// What is declared, sorted.
    pub fn declared(&self) -> &[Capability] {
        &self.declared
    }

    /// Everything in the vocabulary this backend does *not* declare.
    ///
    /// Here because a report that lists only what a backend can do reads like
    /// a complete answer. A reader deciding whether to trust a result wants
    /// the other list.
    pub fn absent(&self) -> Vec<Capability> {
        Capability::ALL
            .into_iter()
            .filter(|c| !self.has(*c))
            .collect()
    }

    /// The ask. `Ok` when it is declared; the third verdict's cause when not.
    pub fn require(&self, capability: Capability) -> Result<(), Undetermined> {
        if self.has(capability) {
            Ok(())
        } else {
            Err(Undetermined::CapabilityAbsent {
                capability: format!("{} ({})", capability.name(), capability.means()),
            })
        }
    }

    /// The same ask for several, reporting the first absent one in the order
    /// given — so that a caller naming its needs in order of importance gets
    /// the answer it would have wanted.
    pub fn require_all(&self, capabilities: &[Capability]) -> Result<(), Undetermined> {
        for capability in capabilities {
            self.require(*capability)?;
        }
        Ok(())
    }
}

impl std::fmt::Display for Capabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.declared.is_empty() {
            return write!(f, "nothing beyond the mandatory verbs");
        }
        let names: Vec<&str> = self.declared.iter().map(|c| c.name()).collect();
        write!(f, "{}", names.join(", "))
    }
}

/// A verdict a caller is only entitled to while the backend declares what the
/// comparison rested on — §7.3.
///
/// # When this is the right tool, and when it is not
///
/// Use it where the *verdict* depends on the capability: a comparison of
/// something only that capability can read, or one whose bytes were chosen by
/// asking the backend a question it cannot answer. Agreement reached that way
/// is not agreement; it is a comparison of whatever came back instead.
///
/// Do **not** use it where the capability only adds detail. §5.4's writing
/// position is the example: a difference is a difference whether or not
/// anything can name the instruction that wrote it, and turning it into *not
/// determined* would be hiding a real result behind a missing one. That belongs
/// in the report as an absent field with the capability named.
///
/// An already-not-determined verdict keeps its own cause. It cannot become more
/// not determined, and the cause it already carries — vacuous, absent, did not
/// arrive — is the one a reader can act on.
pub fn needing(declared: &Capabilities, needed: &[Capability], verdict: Verdict) -> Verdict {
    if matches!(verdict, Verdict::NotDetermined(_)) {
        return verdict;
    }
    match declared.require_all(needed) {
        Ok(()) => verdict,
        Err(cause) => Verdict::NotDetermined(cause),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verdict::Difference;

    #[test]
    fn a_declaration_is_sorted_deduped_and_reads_the_same_either_way_round() {
        let one = Capabilities::of([
            Capability::WritingPosition,
            Capability::StopOnExecution,
            Capability::WritingPosition,
        ]);
        let other = Capabilities::of([Capability::StopOnExecution, Capability::WritingPosition]);
        assert_eq!(one, other, "order and repeats must not make two declarations");
        assert_eq!(one.declared().len(), 2);
        assert_eq!(one.to_string(), "stop-on-execution, writing-position");
    }

    /// A backend declaring nothing says so in words. An empty list printed as
    /// an empty string reads like a backend that answered no question.
    #[test]
    fn declaring_nothing_is_said_rather_than_printed_blank() {
        let none = Capabilities::none();
        assert!(none.declared().is_empty());
        assert_eq!(none.absent().len(), Capability::ALL.len());
        assert!(
            none.to_string().contains("nothing"),
            "got `{none}`"
        );
    }

    /// The two lists must partition the vocabulary. A capability missing from
    /// both would be one nobody can ask about.
    #[test]
    fn what_is_declared_and_what_is_absent_cover_the_whole_vocabulary() {
        let some = Capabilities::of([Capability::StopOnWrite, Capability::WriteRecency]);
        let mut all: Vec<Capability> = some.declared().to_vec();
        all.extend(some.absent());
        all.sort();
        assert_eq!(all, Capability::ALL.to_vec());
    }

    /// Every capability must print a distinct name and a distinct meaning: a
    /// report naming one of them is useless if two share a name.
    #[test]
    fn the_names_and_the_meanings_are_all_distinct() {
        let mut names: Vec<&str> = Capability::ALL.iter().map(|c| c.name()).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), before, "two capabilities share a name");

        let mut means: Vec<&str> = Capability::ALL.iter().map(|c| c.means()).collect();
        means.sort_unstable();
        means.dedup();
        assert_eq!(means.len(), before, "two capabilities share a meaning");
    }

    #[test]
    fn the_ask_names_the_capability_and_what_it_would_have_given() {
        let declared = Capabilities::of([Capability::StopOnExecution]);
        assert!(declared.require(Capability::StopOnExecution).is_ok());

        let cause = declared
            .require(Capability::WritingPosition)
            .expect_err("not declared");
        let said = cause.to_string();
        assert!(said.contains("writing-position"), "{said}");
        assert!(
            said.contains("which position last wrote a byte"),
            "a reader who does not know the name needs the meaning: {said}"
        );
    }

    /// The order the caller writes its needs in is the order it hears about
    /// them, so the first thing it named is the thing it is told about.
    #[test]
    fn several_needs_report_the_first_absent_one_in_the_order_asked() {
        let declared = Capabilities::of([Capability::StopOnWrite]);
        let cause = declared
            .require_all(&[
                Capability::StopOnWrite,
                Capability::WritingPosition,
                Capability::InputReplay,
            ])
            .expect_err("two are absent");
        assert!(
            cause.to_string().contains("writing-position"),
            "got {cause}"
        );
        assert!(
            declared
                .require_all(&[Capability::StopOnWrite])
                .is_ok()
        );
        assert!(
            Capabilities::none().require_all(&[]).is_ok(),
            "needing nothing is satisfied by a backend that declares nothing"
        );
    }

    /// §7.3's sentence, as a test: agreement that rested on an absent
    /// capability is **not** agreement. This is the assertion the whole module
    /// exists for — if `needing` returned its argument unchanged, this fails.
    #[test]
    fn agreement_resting_on_an_absent_capability_is_not_determined() {
        let agrees = Verdict::Agrees {
            compared: 64,
            moved: 64,
        };
        let verdict = needing(
            &Capabilities::none(),
            &[Capability::ExecutionCoverage],
            agrees.clone(),
        );
        match &verdict {
            Verdict::NotDetermined(Undetermined::CapabilityAbsent { capability }) => {
                assert!(capability.contains("execution-coverage"), "{capability}");
            }
            other => panic!("agreement must not survive the capability it rested on: {other}"),
        }
        assert_ne!(verdict, agrees, "§2.3: never collapsed into agreement");

        let declared = Capabilities::of([Capability::ExecutionCoverage]);
        assert_eq!(
            needing(&declared, &[Capability::ExecutionCoverage], agrees.clone()),
            agrees,
            "and a declared capability leaves the verdict alone"
        );
    }

    /// A difference goes the same way when the verdict itself rested on the
    /// capability. The doc comment is emphatic that §5.4's localisation is
    /// *not* such a case, and that distinction is the caller's to make.
    #[test]
    fn a_difference_resting_on_an_absent_capability_is_also_not_determined() {
        let differs = Verdict::Differs(Difference::new(3, 1, 2, 1, 8));
        let verdict = needing(
            &Capabilities::none(),
            &[Capability::RegisterWrites],
            differs.clone(),
        );
        assert!(
            matches!(
                verdict,
                Verdict::NotDetermined(Undetermined::CapabilityAbsent { .. })
            ),
            "got {verdict}"
        );
    }

    /// An absent capability must not overwrite a cause a reader can act on.
    /// "The reference changed none of these bytes" is a thing to go and fix;
    /// "a capability is missing" on top of it is noise that hides it.
    #[test]
    fn a_verdict_already_not_determined_keeps_its_own_cause() {
        let vacuous = Verdict::NotDetermined(Undetermined::Vacuous { compared: 64 });
        assert_eq!(
            needing(
                &Capabilities::none(),
                &[Capability::WriteRecency],
                vacuous.clone()
            ),
            vacuous
        );
    }
}
