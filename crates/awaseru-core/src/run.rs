//! Positions, bounds and stop reasons — §3.4, §4.2, §4.3.

/// Where execution stands, and what kind of place that is — §3.4.
///
/// The kind is carried because **a frame boundary is not necessarily an
/// instruction boundary**. A reference sampled at the end of a frame is
/// frequently part way through an instruction, and a state taken there cannot
/// be seeded into a reimplementation, because there is no instruction to begin
/// at. Losing the distinction means discovering it much later as a comparison
/// that cannot be explained.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Position {
    /// Between frames. Says nothing about instructions.
    FrameBoundary { frame: u64 },
    /// Between instructions, at this program counter.
    InstructionBoundary { pc: u64 },
    /// Part way through an instruction, at this program counter.
    ///
    /// Not an error: it is where a reference often is, and the honest thing is
    /// to say so rather than to round it to the nearest boundary.
    MidInstruction { pc: u64 },
    /// Somewhere the backend can name but not classify.
    Unclassified { pc: u64 },
}

impl Position {
    /// Whether a state taken here can be seeded into a reimplementation.
    pub fn is_instruction_boundary(&self) -> bool {
        matches!(self, Position::InstructionBoundary { .. })
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Position::FrameBoundary { frame } => write!(f, "frame boundary {frame}"),
            Position::InstructionBoundary { pc } => write!(f, "instruction boundary at {pc:#X}"),
            Position::MidInstruction { pc } => write!(f, "part way through the instruction at {pc:#X}"),
            Position::Unclassified { pc } => write!(f, "{pc:#X}, of a kind the backend does not say"),
        }
    }
}

/// How far a run goes — §4.2.
///
/// Every run is bounded and the bound is part of the request. There is no
/// unbounded run: one that does not stop is indistinguishable from one that has
/// not finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    /// To the end of this many frames.
    Frames(u64),
    /// Until the program counter reaches `address`, or `within` instructions
    /// have run — whichever comes first.
    ///
    /// **The budget is not optional**, which is §4.4: every run carries one,
    /// and exhausting it is a stop reason rather than a failure. An address is
    /// the first bound this project has that can fail to arrive — frames and
    /// instructions always do — and a run that does not stop is
    /// indistinguishable from one that has not finished (§4.2).
    ///
    /// A count and not a clock, so the same run exhausts it at the same place
    /// every time (§2.5). A wall-clock limit would make a slow machine report
    /// a different result from a fast one.
    Address { address: u64, within: u64 },
    /// For this many instructions.
    Instructions(u64),
    /// Until the byte at `offset` of `region` is written, or the program
    /// counter reaches `until`, or `within` instructions have run — whichever
    /// comes first. §5.4's localisation is built on it.
    ///
    /// # Why `until` is not optional
    ///
    /// §4.5: a measurement must not run past its subject. Localising a
    /// difference in what a routine wrote means finding the write *inside that
    /// routine*, and a bound that waited only for a write would keep running
    /// when the write does not come again — through the return, into whatever
    /// the software does next, and the position it eventually reported would
    /// name an instruction that has nothing to do with the question. So the end
    /// of the subject is part of the bound, and a run that reaches it first
    /// says so (`Reason::AddressHit`).
    ///
    /// A byte is addressed by region and offset rather than by an address
    /// (§3.1): the same byte of memory is reachable through more than one
    /// address on some machines, and a comparison is about the byte.
    Write {
        region: String,
        offset: usize,
        until: u64,
        within: u64,
    },
}

impl std::fmt::Display for Bound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bound::Frames(n) => write!(f, "{n} frame(s)"),
            Bound::Address { address, within } => {
                write!(f, "until {address:#X}, within {within} instruction(s)")
            }
            Bound::Instructions(n) => write!(f, "{n} instruction(s)"),
            Bound::Write {
                region,
                offset,
                until,
                within,
            } => write!(
                f,
                "until `{region}`+{offset} is written or {until:#X} is reached, within {within} \
                 instruction(s)"
            ),
        }
    }
}

/// Why a run ended, and where — §4.3.
///
/// A reason is a result, not an error. A backend that cannot honour a bound
/// says so here rather than approximating it, which is §2.4 at the point where
/// approximating would be easiest and least visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop {
    pub reason: Reason,
    pub position: Position,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// The bound that was asked for was reached.
    BoundReached,
    /// An address stopped it before the bound did.
    AddressHit { address: u64 },
    /// The step budget ran out first.
    BudgetExhausted,
    /// A byte stopped it: something wrote the byte the bound named, and the
    /// stop's position is **the instruction doing the writing** where the
    /// backend declares `writing-position` (§5.4, §7.3).
    ///
    /// Expect a mid-instruction position: the write is caught before it
    /// commits, which is the only moment at which the writer can be named.
    WriteHit { region: String, offset: usize },
    /// The backend declines this bound. Not a failure of the run — a statement
    /// about the backend, which §7.3 says must be declared rather than guessed.
    Refused { why: String },
    /// The backend reached a state it cannot continue from.
    CannotContinue { why: String },
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.reason {
            Reason::BoundReached => write!(f, "the bound was reached, at {}", self.position),
            Reason::AddressHit { address } => {
                write!(f, "stopped at {address:#X}, which is {}", self.position)
            }
            Reason::BudgetExhausted => write!(f, "the budget ran out at {}", self.position),
            Reason::WriteHit { region, offset } => write!(
                f,
                "`{region}`+{offset} was written {}",
                self.position
            ),
            Reason::Refused { why } => write!(f, "the backend refused: {why}"),
            Reason::CannotContinue { why } => {
                write!(f, "the backend cannot continue from {}: {why}", self.position)
            }
        }
    }
}

impl Stop {
    /// Whether the run got where it was asked to go.
    ///
    /// Every other reason means a comparison made at this point is comparing
    /// something other than what was asked for, which §2.3 reports as not
    /// determined rather than as a difference.
    pub fn arrived(&self) -> bool {
        matches!(
            self.reason,
            Reason::BoundReached | Reason::AddressHit { .. } | Reason::WriteHit { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §4.4. An address bound says both things, and a reader of the bound can
    /// see the budget — which is the point of it being in the bound rather
    /// than hidden in an implementation.
    #[test]
    fn an_address_bound_says_how_long_it_will_wait() {
        let said = Bound::Address {
            address: 0xC4_0000,
            within: 50_000,
        }
        .to_string();
        assert!(said.contains("C40000"), "said: {said}");
        assert!(
            said.contains("50000") && said.contains("instruction"),
            "the budget must be visible in the bound, not only in the code that honours it: \
             {said}"
        );

        // And two bounds to the same address with different budgets are
        // different bounds, because they can stop in different places.
        assert_ne!(
            Bound::Address {
                address: 0x8000,
                within: 1
            },
            Bound::Address {
                address: 0x8000,
                within: 2
            }
        );
    }

    #[test]
    fn only_a_frame_boundary_is_not_an_instruction_boundary() {
        assert!(!Position::FrameBoundary { frame: 7 }.is_instruction_boundary());
        assert!(Position::InstructionBoundary { pc: 0x8000 }.is_instruction_boundary());
        assert!(!Position::MidInstruction { pc: 0x8000 }.is_instruction_boundary());
        assert!(!Position::Unclassified { pc: 0x8000 }.is_instruction_boundary());
    }

    /// The distinction a comparison depends on: arriving and not arriving.
    /// A `Refused` or `BudgetExhausted` treated as arrival would have a
    /// comparison read whatever state happened to be there.
    #[test]
    fn only_reaching_the_bound_or_an_address_counts_as_arriving() {
        let at = Position::FrameBoundary { frame: 1 };
        let stop = |reason| Stop {
            reason,
            position: at.clone(),
        };

        assert!(stop(Reason::BoundReached).arrived());
        assert!(stop(Reason::AddressHit { address: 0x8000 }).arrived());
        assert!(!stop(Reason::BudgetExhausted).arrived());
        assert!(!stop(Reason::Refused { why: "no".into() }).arrived());
        assert!(!stop(Reason::CannotContinue { why: "no".into() }).arrived());
    }

    #[test]
    fn a_refusal_says_what_it_refused_and_does_not_pretend_to_a_position() {
        let stop = Stop {
            reason: Reason::Refused {
                why: "this backend cannot stop on an address".into(),
            },
            position: Position::FrameBoundary { frame: 0 },
        };
        let said = stop.to_string();
        assert!(said.contains("cannot stop on an address"), "said: {said}");
        assert!(
            !said.contains("frame boundary"),
            "a refusal is about the backend, not about where it happens to be: {said}"
        );
    }

    #[test]
    fn a_mid_instruction_position_says_so_rather_than_rounding() {
        let said = Position::MidInstruction { pc: 0x4E45 }.to_string();
        assert!(said.contains("part way through"), "said: {said}");
        assert!(said.contains("4E45"), "and names where: {said}");
    }
}
