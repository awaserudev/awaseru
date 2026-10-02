//! Positions, bounds and stop reasons — §3.4, §4.2, §4.3.

/// Where execution stands, and what kind of place that is — §3.4.
///
/// The kind is carried because **a frame boundary is not necessarily an
/// instruction boundary**. A reference sampled at the end of a frame is
/// frequently part way through an instruction, and a state taken there cannot
/// be seeded into a reimplementation, because there is no instruction to begin
/// at. Losing the distinction means discovering it much later as a comparison
/// that cannot be explained.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Until the program counter reaches this address.
    Address(u64),
    /// For this many instructions.
    Instructions(u64),
}

impl std::fmt::Display for Bound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bound::Frames(n) => write!(f, "{n} frame(s)"),
            Bound::Address(a) => write!(f, "until {a:#X}"),
            Bound::Instructions(n) => write!(f, "{n} instruction(s)"),
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
            Reason::BoundReached | Reason::AddressHit { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
