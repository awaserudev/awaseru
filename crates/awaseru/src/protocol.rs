//! The vocabulary — §8.3's data model, and the one both bindings of §8.4 use.
//!
//! > The data model is the contract; the encoding of a payload is an
//! > implementation detail that may change behind a version (§8.6).
//!
//! # Why these types and not the core's own
//!
//! `awaseru-core`'s types already derive serialization in one or two places,
//! and it would be shorter to put the whole model on the wire that way. This
//! module does not, and the reason is the sentence above: **the data model is
//! the contract.** Deriving the wire form from internal types makes every field
//! name and every variant name a promise nobody wrote down, so renaming a field
//! for clarity breaks a client. Here the shapes are written out, the JSON names
//! are chosen, and the conversions from the core are explicit — which is also
//! how §13's Q16 can be answered on the wire without changing the comparison
//! that produced the value.
//!
//! Each conversion is written with an **exhaustive match**, so a variant added
//! to the core stops this file compiling rather than silently serializing as
//! something else.
//!
//! # One pair of enums
//!
//! `Command` and `Reply`. Both bindings of §8.4 speak them: the subprocess
//! binding encodes them into §8.3's frames, and the in-process binding passes
//! them as values. That is what makes "same semantics, two bindings"
//! structural — there is one vocabulary and two ways of carrying it, rather
//! than two implementations that are meant to agree.
//!
//! # Where the bytes are
//!
//! A command or reply that carries state does **not** put it in the JSON.
//! §8.3's binary payload does, and the envelope says how to cut it up: each
//! span in the command has a length, and the payload is those spans'
//! bytes **concatenated in the order the spans are listed**. A client that can
//! count can cut it; nothing is base64 and nothing is doubled in size.
//!
//! # Strict one way, tolerant the other
//!
//! **Commands refuse a field nobody declared. Replies ignore one.** The
//! asymmetry is deliberate and it is what makes this vocabulary extensible at
//! all.
//!
//! A client that misspells a field is making a mistake, and §2.4 says the tool
//! does not guess what was meant: `deny_unknown_fields` on `Command` and
//! everything only a command carries turns a typo into a refusal that names the
//! field, rather than a measurement quietly made with a default nobody chose.
//!
//! A client reading a reply is in the opposite position. The tool it is talking
//! to may be **newer than the client**, and a newer tool says more. Until M6
//! the replies here were strict too — including where this host's own parent
//! process reads its child — so a field added to a report broke every older
//! reader and no addition was additive. Every new field would have been a
//! version bump (§8.6), which in practice means none get added.
//!
//! So the rule for growing this vocabulary is: **a new field is additive, a new
//! variant is not.** Adding `coverage` to a report is free, because a client
//! that does not know it skips it. Adding a *reply kind*, or a new variant to a
//! tagged enum like `Verdict` or `Cause`, is not free: an old client fails to
//! parse the tag, and rightly, because it has no idea what it is being told.
//! That is where §8.6's version lives, and it is a much rarer event than adding
//! a field.
//!
//! # The names are the configuration's
//!
//! §8.5. Every region is a `String` the backend and the mapping supplied, never
//! an enum this file knows. A vocabulary with a variant per region would be a
//! second naming scheme and a platform name on the wire (§2.7).

use serde::{Deserialize, Serialize};

/// The protocol's version — §8.6, which is open.
///
/// Carried in the handshake, refused on a mismatch, and deliberately not
/// negotiated: §13's Q3 says the first client written by someone who did not
/// write the tool is what settles how negotiation should work, and inventing it
/// before then is inventing a guess.
pub const PROTOCOL: u32 = 1;

// ---------------------------------------------------------------- commands --

/// Every command, by the name it travels under — §8.6's declared vocabulary.
///
/// Hand-written, and kept honest by a test rather than by care: the test maps
/// each `Command` variant to its name through an exhaustive `match`, so adding
/// a variant stops the build until this list is told about it. A list derived at
/// run time would need a macro or a crate; a list nobody checks would drift the
/// first time somebody was in a hurry.
pub const COMMANDS: &[&str] = &[
    "hello",
    "capabilities",
    "regions",
    "read",
    "write",
    "run",
    "arrive",
    "demonstrate",
    "reverify",
    "examine",
];

/// What a client asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Command {
    /// First, always. Says which protocol the client speaks.
    Hello { protocol: u32, client: String },
    /// What the backend declares it can do (§7.3).
    Capabilities,
    /// Every region the backend exposes, by name (§3.1).
    Regions,
    /// A span of one region. The bytes come back in the payload.
    Read {
        region: String,
        offset: usize,
        length: usize,
    },
    /// A span of one region, written from the payload.
    ///
    /// **Not a way to seed a machine** — `Platform::write`'s warning applies on
    /// the wire too: a console is not its memories. §5.3's perturbation is what
    /// this is for.
    Write { region: String, offset: usize },
    /// Advance, bounded (§4.2). There is no unbounded run to ask for.
    Run { bound: Bound },
    /// Put the reference at an anchor, and nothing else — §4.7, and Q26.
    ///
    /// **It does not demonstrate.** §4.9 demonstrates an anchor nobody has
    /// established before it is *used*, and that costs several replays of the
    /// definition; a client asking to arrive has not asked to spend them.
    /// Arriving is looking, examining is measuring, and the reply says which
    /// of the two this position is fit for rather than leaving silence to be
    /// read as "fine".
    ///
    /// This is the half of the tool §10 is about: looking at what the
    /// reference holds somewhere, before there is a reimplementation to
    /// compare it against. It had no verb, so the whole anchor cache was
    /// reachable only as a side effect of a measurement nobody wanted
    /// (`doc/findings.md`'s thirty-fourth entry).
    Arrive { anchor: String },
    /// Establish an anchor here — §4.8's five steps, asked for.
    ///
    /// The **opening** half of §4.9's bracket, and a command because nothing
    /// could ask for it. §4.9 runs it before the first *use* of an anchor
    /// nobody has established, which leaves two cases with no way out: an
    /// anchor reached by `Arrive`, which deliberately does not demonstrate, and
    /// a blob that came in a box carrying its packer's demonstration, which is
    /// theirs and counts for nothing here (§4.11, and `doc/findings.md`'s
    /// twenty-ninth entry). Both leave a position that resumes in milliseconds
    /// and from which no comparison is evidence, and `Reverify` does not help:
    /// it **checks** and does not establish.
    ///
    /// Not an extension of `Reverify` for that reason. §4.9 makes reverify the
    /// closing half, read after a session; conflating the two would blur a
    /// distinction the specification drew on purpose.
    ///
    /// It answers with what it cost, which is the number Q23 asked for and
    /// nobody had.
    Demonstrate { anchor: String },
    /// §4.9's closing check: replay the anchor's definition once and see that
    /// the cached blob still produces what replaying produces.
    ///
    /// The other half of a session's bracket — the opening half is the
    /// demonstration, which the tool runs itself before using an anchor nobody
    /// has demonstrated.
    Reverify { anchor: String },
    /// §5.6's cycle and all of §5's answers: measure a routine, compare a
    /// reimplementation's output against it, localise a difference, run a
    /// control.
    ///
    /// The payload is the `given` spans' bytes, then the `produced` spans'
    /// bytes, then the control's span if there is one — each as long as its
    /// span says.
    Examine {
        routine: Routine,
        given: Vec<Span>,
        /// One `Vec` of bytes per span the routine writes, taken from the
        /// payload after the given spans.
        produced: Vec<Span>,
        /// §5.3. Absent means no control was run, which the report records
        /// rather than hides.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        control: Option<Perturbation>,
        /// §10's execution coverage, over the span named. Absent asks for
        /// none, which is the default `doc/report-options.md` gives it.
        ///
        /// A span rather than a flag, because asking costs time proportional
        /// to its size — 34 µs per kilobyte on the first backend — so the
        /// caller says how much it is willing to pay for rather than being
        /// handed a cartridge. **Asking is naming one.**
        #[serde(default, skip_serializing_if = "Option::is_none")]
        coverage: Option<Span>,
        /// §5.4. A replay, so it is asked for.
        #[serde(default)]
        localise: bool,
    },
}

/// How far a run goes — §4.2's bound, on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "bound", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Bound {
    Frames {
        count: u64,
    },
    Instructions {
        count: u64,
    },
    /// §4.4's budget is part of the bound and not optional.
    Address {
        address: u64,
        within: u64,
    },
    /// §5.4's localisation bound: a byte, and the end of the subject (§4.5).
    Write {
        region: String,
        offset: usize,
        until: u64,
        within: u64,
    },
}

/// A named span of a region (§3.1, §8.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub region: String,
    pub offset: usize,
    pub length: usize,
}

/// §5.6's unit of work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Routine {
    pub name: String,
    pub entry: u64,
    /// What bounds the measurement (§4.5).
    pub returns_to: u64,
    /// §4.4's budget for the measurement, in instructions — from the entry to
    /// the return, and §4.5's rule that it must not run past its subject.
    pub within: u64,
    /// The budget for **reaching** the entry, when that is a different number.
    ///
    /// Absent means `within`, which is what a routine written before this
    /// field existed meant and is right whenever the routine runs soon after
    /// its anchor. The two runs have nothing in common — reaching can be a
    /// whole frame of software, running a routine is as long as the routine —
    /// and one number cannot bound both without being wrong for one of them
    /// (`doc/findings.md`'s thirty-sixth entry).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaching: Option<u64>,
    /// The anchor to begin from (§4.7), by name. Absent means wherever the
    /// reference already is, which repeats only if somebody made it so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// §5.3's named change to one input. Its bytes are at the end of the payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Perturbation {
    pub name: String,
    pub span: Span,
}

// ----------------------------------------------------------------- replies --

/// What the tool answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "kebab-case")]
pub enum Reply {
    /// The handshake's other half. Both versions, always, so a mismatch is
    /// legible from either side (§8.6).
    Hello {
        protocol: u32,
        /// The tool's own version (§16.1).
        tool: String,
        /// Every command this server has, by the name it is sent under — §8.6.
        ///
        /// §7.3 one level up. A backend's capabilities are declared rather than
        /// assumed because a client that guessed would be refused at the worst
        /// moment; the protocol's own vocabulary is the same thing, and it was
        /// missing. The first client written by somebody who did not write this
        /// tool had to **read the source** to learn that one of the commands it
        /// wanted did not exist (`doc/findings.md`'s thirty-fourth entry), which
        /// is the discovery §8.6 was waiting for.
        ///
        /// A field on a reply, which the rule already allows. What it buys is
        /// larger than itself: a reply **variant** is breaking only for a client
        /// that can receive it without having asked, and every reply answers a
        /// command the client sent. So a client that does not know a command
        /// never sends it and never sees its reply — and adding one stops being
        /// a version question.
        commands: Vec<String>,
    },
    Capabilities {
        declared: Vec<String>,
        /// What the backend does **not** declare. Present because a list of
        /// what a tool can do reads like a complete answer, and a reader
        /// deciding whether to trust a result wants the other list (§7.3).
        absent: Vec<String>,
    },
    Regions {
        regions: Vec<Region>,
    },
    /// The bytes are in the payload.
    Bytes {
        region: String,
        offset: usize,
        length: usize,
    },
    Written {
        region: String,
        offset: usize,
        length: usize,
    },
    Stopped {
        stop: Stop,
    },
    /// Boxed because a report carries all of §5 and the other replies carry a
    /// field or two: without it, every reply on the wire would be the size of
    /// the largest one.
    /// §4.9's closing check passed: everything that rested on this blob stands.
    /// Where the reference now stands, and what a comparison from here would
    /// be worth — the answer to `Arrive`.
    Arrived {
        anchor: String,
        /// Where it is, and what kind of place that is (§3.4).
        position: Position,
        /// Replayed or resumed, and how long — §4.12 next to the result.
        how: String,
        took_ms: u64,
        /// How the reference came up (§4.12).
        beginning: Beginning,
        /// §4.8: whether anything has shown this anchor produces what replaying
        /// its definition produces.
        established: Established,
    },
    /// §4.8 done, here, and what it cost — the answer to `Demonstrate`.
    Demonstrated {
        anchor: String,
        /// How many replays stood behind it (§4.9's `verify_from_origin`).
        replays: u32,
        took_ms: u64,
        /// §2.5: whether the same state was reached in another process as well.
        across_processes: bool,
    },
    Reverified {
        anchor: String,
        /// How many times the blob was resumed since it was demonstrated.
        uses: u64,
        took_ms: u64,
    },
    Report {
        report: Box<Report>,
    },
    /// A refusal — §14.2, and the one shape every failure takes.
    ///
    /// Both halves are required: what was looked for and what was found. A
    /// refusal that said only "invalid request" would make a client's author
    /// guess, and guessing is what §2.4 refuses on the tool's side of the line
    /// as well.
    Refused {
        looking_for: String,
        found: String,
    },
}

/// One region, as §3.1 models it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    pub name: String,
    pub size: usize,
    pub readable: bool,
    pub writable: bool,
}

/// Where execution stands, and what kind of place that is — §3.4.
///
/// The kind is on the wire because a frame boundary is not an instruction
/// boundary, and a client that cannot tell them apart will try to seed from one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "position", rename_all = "kebab-case")]
pub enum Position {
    FrameBoundary { frame: u64 },
    InstructionBoundary { pc: u64 },
    MidInstruction { pc: u64 },
    Unclassified { pc: u64 },
}

/// Why a run ended, and where — §4.3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stop {
    pub reason: Reason,
    pub position: Position,
    /// Whether the run got where it was asked to go. Derived, and on the wire
    /// anyway: every client would otherwise write this match itself, and the
    /// one that gets it wrong compares a state from the wrong place.
    pub arrived: bool,
    /// The sentence the tool would print.
    pub says: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum Reason {
    BoundReached,
    AddressHit { address: u64 },
    BudgetExhausted,
    WriteHit { region: String, offset: usize },
    Refused { why: String },
    CannotContinue { why: String },
}

/// §5.1's verdict. **Three values on the wire, never two** (§2.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "kebab-case")]
pub enum Verdict {
    Agrees {
        compared: usize,
        /// §5.2, reported always rather than on request.
        moved: usize,
    },
    Differs {
        difference: Difference,
    },
    /// The third value, with a machine-readable cause and the sentence.
    NotDetermined {
        cause: Cause,
        says: String,
    },
}

/// Why a comparison did not happen, or happened without meaning — §2.3.
///
/// A tag rather than a string, so that a client can branch on it; the sentence
/// travels beside it for a client that only prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    RegionAbsent,
    ReferencesDisagree,
    DidNotArrive,
    Vacuous,
    CapabilityAbsent,
    MovementUnknown,
    SpansDiffer,
    StatesIncomparable,
    NotRepeatable,
    AnchorNotDemonstrated,
    SeededFromNoBoundary,
}

/// §5.4's localisation, as far as it is known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Difference {
    /// Which region the offset below is read against (§5.4).
    ///
    /// Always present on a difference this tool produced — §13's Q16, answered
    /// in M4: the comparison knows the region and the report carries it,
    /// whether or not §5.4's localisation was asked for. Optional in the shape
    /// because a client assembling a difference of its own may not have one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    pub first: usize,
    pub expected: u8,
    pub found: u8,
    pub differing: usize,
    pub compared: usize,
    /// §5.4's third item.
    pub wrote: Wrote,
    /// What the mapping calls the differing byte — §M7. Absent when no mapping
    /// is loaded, and absent when one is and covers nothing here: a name is
    /// never invented to fill the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Named>,
}

/// What the mapping calls a place — §M7's third clause.
///
/// **In addition to the number, never instead of it.** A mapping is written by
/// hand and can be wrong; the offset and the address are what check it. A report
/// that replaced them with a name would make a mistyped mapping unfalsifiable,
/// which is the opposite of what this tool is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    pub name: String,
    /// How far into the symbol the place is, so that `tile-buffer` and
    /// `tile-buffer+40` are different answers.
    pub into: usize,
    /// §9.2: this symbol was not established by measurement, so the mapping
    /// says what it is rather than knows. A client that printed the name
    /// without this would present a guess as a fact.
    pub hypothesis: bool,
}

/// What is known about the write that produced the reference's value — §5.4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "wrote", rename_all = "kebab-case")]
pub enum Wrote {
    /// Nobody asked. Localising is a replay, so it is a second request.
    NotLooked,
    /// Asked, and the backend does not declare what it would take (§7.3).
    NotAvailable { capability: String },
    /// Asked, and nothing wrote it between the seed and the stop.
    NothingWrote,
    /// The position that last wrote it, and how many times it was written — so
    /// that "the last write" is not read as "the only write".
    At {
        position: Position,
        writes: u64,
        /// What the mapping calls the instruction that wrote it — §M7. Absent
        /// when nothing covers that address, never invented.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        symbol: Option<Named>,
    },
}

/// §5.3's control, or the record that none was run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "control", rename_all = "kebab-case")]
pub enum Control {
    /// **The tool cannot force a control and records its absence** (§5.3).
    NotRun { says: String },
    Ran {
        perturbation: String,
        /// Boxed for the same reason as `Reply::Report`: two verdicts are the
        /// heaviest thing in this vocabulary, and `NotRun` should not pay for
        /// them.
        plain: Box<Verdict>,
        perturbed: Box<Verdict>,
        /// Whether the comparison noticed. A control that was run and went
        /// unnoticed is a statement about the comparison, not a failure.
        noticed: bool,
        says: String,
    },
}

/// All of §5 in one value, which is what §5 requires of a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub routine: String,
    /// §5.1, **with everything that bears on it applied** — §4.8's caveat and
    /// §4.12's beginning included. This is the field a client should read.
    pub verdict: Verdict,
    /// What the comparison alone said, when the two differ. A client that wants
    /// to know what a caveat changed reads this; one that does not, ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_compared: Option<Verdict>,
    /// §5.2's movement, absent rather than zero where there is no count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved: Option<usize>,
    /// §5.4, when it was asked for and there was something to localise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub localisation: Option<Localised>,
    /// §5.3.
    pub control: Control,
    /// §5.3's first sentence: a measurement without a control that varies is
    /// incomplete.
    pub complete: bool,
    /// §10's coverage of the measurement, when a span was asked for.
    ///
    /// Absent means nobody asked, which is not the same as "nothing ran" and is
    /// kept apart on purpose: a report answering an empty reading to a question
    /// nobody put would say the routine executed nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<Coverage>,
    /// §4.12, next to the result rather than in a footnote.
    pub beginning: Beginning,
    pub took_ms: u64,
}

/// §5.4's answer for one byte.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Localised {
    pub region: String,
    pub offset: usize,
    pub wrote: Wrote,
    /// Whether the answer cost a replay, or the cheap filter settled it.
    pub replayed: bool,
    /// What the replay began from: an anchor's name, or absent for the
    /// reference's origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// §10's execution coverage, in the form a wire can afford — §8.3.
///
/// **The counts stay host-side.** The record is a count per byte, and a
/// cartridge's worth of them is megabytes of JSON for a question almost nobody
/// asks that way. What crosses is the answer §10 says coverage is for — *what
/// has not been seen* — plus the two totals that make it checkable.
///
/// If per-byte counts are ever wanted on the wire they belong in §8.3's binary
/// payload rather than here, and nobody has asked yet (§2.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    pub region: String,
    pub offset: usize,
    /// How many bytes the reading covers.
    pub length: usize,
    /// How many of them ran at least once.
    pub ran: usize,
    /// The stretches that never ran, as `[start, end)` in the region's own
    /// numbering — which is the answer, not the leftovers.
    pub never_ran: Vec<[usize; 2]>,
}

impl Coverage {
    fn of(c: &awaseru_core::ExecutionCoverage) -> Self {
        Coverage {
            region: c.region.clone(),
            offset: c.offset,
            length: c.executions.len(),
            ran: c.ran(),
            never_ran: c.never_ran().into_iter().map(|r| [r.start, r.end]).collect(),
        }
    }
}

/// Whether §4.8 has established an anchor, and whose establishing it was.
///
/// An enum and **not** an `Option<Cause>`, for the reason `Control` gives about
/// its own absence: a field reading `null` is read as "no problem". A client
/// that has to notice an absence in order to learn that nothing here is
/// evidence will sometimes not notice.
///
/// It is not a verdict and must not be read as one. An arrival compares nothing
/// — §2.3's three values are for comparisons — and this says what a comparison
/// made from this position **would** be worth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "established", rename_all = "kebab-case")]
pub enum Established {
    /// Shown here, by this session, with the number of replays it took (§4.9's
    /// `verify_from_origin`).
    Here { replays: u32, says: String },
    /// Shown by whoever packed the box this blob arrived in, and therefore not
    /// shown here. §4.8 makes a demonstration the property of the run that
    /// performed it, so this is reported as theirs and counts for nothing.
    Elsewhere { by: String, says: String },
    /// Not shown anywhere. The blob may well be right, and nothing has
    /// established that.
    Nowhere { says: String },
}

/// How the reference came up — §4.12.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Beginning {
    /// Whether a run from here can be compared with a run from anywhere else
    /// (§2.5). A report that did not say this would be a report somebody
    /// trusts.
    pub repeats: bool,
    pub settled: Vec<String>,
    pub says: String,
}

// ------------------------------------------------------------ conversions --

impl From<&awaseru_core::Position> for Position {
    fn from(p: &awaseru_core::Position) -> Self {
        match p {
            awaseru_core::Position::FrameBoundary { frame } => {
                Position::FrameBoundary { frame: *frame }
            }
            awaseru_core::Position::InstructionBoundary { pc } => {
                Position::InstructionBoundary { pc: *pc }
            }
            awaseru_core::Position::MidInstruction { pc } => Position::MidInstruction { pc: *pc },
            awaseru_core::Position::Unclassified { pc } => Position::Unclassified { pc: *pc },
        }
    }
}

impl From<&awaseru_core::Reason> for Reason {
    fn from(r: &awaseru_core::Reason) -> Self {
        match r {
            awaseru_core::Reason::BoundReached => Reason::BoundReached,
            awaseru_core::Reason::AddressHit { address } => {
                Reason::AddressHit { address: *address }
            }
            awaseru_core::Reason::BudgetExhausted => Reason::BudgetExhausted,
            awaseru_core::Reason::WriteHit { region, offset } => Reason::WriteHit {
                region: region.clone(),
                offset: *offset,
            },
            awaseru_core::Reason::Refused { why } => Reason::Refused { why: why.clone() },
            awaseru_core::Reason::CannotContinue { why } => {
                Reason::CannotContinue { why: why.clone() }
            }
        }
    }
}

impl From<&awaseru_core::Stop> for Stop {
    fn from(s: &awaseru_core::Stop) -> Self {
        Stop {
            reason: (&s.reason).into(),
            position: (&s.position).into(),
            arrived: s.arrived(),
            says: s.to_string(),
        }
    }
}

impl From<&awaseru_core::Undetermined> for Cause {
    fn from(u: &awaseru_core::Undetermined) -> Self {
        use awaseru_core::Undetermined as U;
        match u {
            U::RegionAbsent { .. } => Cause::RegionAbsent,
            U::ReferencesDisagree { .. } => Cause::ReferencesDisagree,
            U::DidNotArrive { .. } => Cause::DidNotArrive,
            U::Vacuous { .. } => Cause::Vacuous,
            U::CapabilityAbsent { .. } => Cause::CapabilityAbsent,
            U::MovementUnknown { .. } => Cause::MovementUnknown,
            U::SpansDiffer { .. } => Cause::SpansDiffer,
            U::StatesIncomparable { .. } => Cause::StatesIncomparable,
            U::NotRepeatable { .. } => Cause::NotRepeatable,
            U::AnchorNotDemonstrated { .. } => Cause::AnchorNotDemonstrated,
            U::SeededFromNoBoundary { .. } => Cause::SeededFromNoBoundary,
        }
    }
}

impl From<&awaseru_core::Wrote> for Wrote {
    fn from(w: &awaseru_core::Wrote) -> Self {
        match w {
            awaseru_core::Wrote::NotLooked => Wrote::NotLooked,
            awaseru_core::Wrote::NotAvailable { capability } => Wrote::NotAvailable {
                capability: capability.clone(),
            },
            awaseru_core::Wrote::NothingWrote => Wrote::NothingWrote,
            awaseru_core::Wrote::At { position, writes } => Wrote::At {
                position: position.into(),
                writes: *writes,
                symbol: None,
            },
        }
    }
}

impl Difference {
    /// A difference, told which region its offset belongs to.
    ///
    /// The region is a parameter because the core's value does not carry one —
    /// §13's Q16 — and the caller is what knows. A `None` here is the question
    /// being left open, not an omission.
    pub fn of(d: &awaseru_core::Difference, region: Option<String>) -> Self {
        Difference {
            region,
            first: d.first,
            expected: d.expected,
            found: d.found,
            differing: d.differing,
            compared: d.compared,
            wrote: (&d.wrote).into(),
            symbol: None,
        }
    }
}

impl Named {
    fn of(symbol: &crate::mapping::Symbol, into: usize) -> Self {
        Named {
            name: symbol.name.clone(),
            into,
            hypothesis: symbol.provenance.is_hypothesis(),
        }
    }
}

impl Wrote {
    /// Fills in what the mapping calls the instruction, where it has one.
    fn name_with(&mut self, mapping: &crate::mapping::Mapping) {
        if let Wrote::At {
            position, symbol, ..
        } = self
        {
            let pc = match position {
                Position::InstructionBoundary { pc } | Position::MidInstruction { pc } => Some(*pc),
                Position::Unclassified { pc } => Some(*pc),
                Position::FrameBoundary { .. } => None,
            };
            *symbol = pc.and_then(|pc| {
                mapping
                    .at_address(pc)
                    .map(|s| Named::of(s, (pc - s.address().unwrap_or(pc)) as usize))
            });
        }
    }
}

impl Difference {
    /// Fills in what the mapping calls the differing byte and the instruction
    /// that wrote it.
    fn name_with(&mut self, mapping: &crate::mapping::Mapping) {
        if let Some(region) = &self.region {
            self.symbol = mapping
                .at(region, self.first)
                .map(|s| Named::of(s, self.first - s.offset().unwrap_or(self.first)));
        }
        self.wrote.name_with(mapping);
    }
}

impl Verdict {
    fn name_with(&mut self, mapping: &crate::mapping::Mapping) {
        if let Verdict::Differs { difference } = self {
            difference.name_with(mapping);
        }
    }
}

impl Report {
    /// §M7's third clause: say the symbol as well as the address.
    ///
    /// A pass over a finished report rather than a parameter threaded through
    /// every conversion, which keeps the conversions pure and means a report
    /// built without a mapping is exactly a report built with an empty one.
    ///
    /// Not a switch (`doc/report-options.md`): naming costs a lookup over the
    /// symbols already in memory and not a run of the machine, so by the rule
    /// there it is sent always. Said here explicitly rather than by omission.
    pub fn name_with(&mut self, mapping: &crate::mapping::Mapping) {
        self.verdict.name_with(mapping);
        if let Some(as_compared) = &mut self.as_compared {
            as_compared.name_with(mapping);
        }
        if let Some(localisation) = &mut self.localisation {
            localisation.wrote.name_with(mapping);
        }
        if let Control::Ran {
            plain, perturbed, ..
        } = &mut self.control
        {
            plain.name_with(mapping);
            perturbed.name_with(mapping);
        }
    }
}

impl Verdict {
    /// A verdict, with the region a difference belongs to where the caller
    /// knows it.
    pub fn of(v: &awaseru_core::Verdict, region: Option<String>) -> Self {
        match v {
            awaseru_core::Verdict::Agrees { compared, moved } => Verdict::Agrees {
                compared: *compared,
                moved: *moved,
            },
            awaseru_core::Verdict::Differs(d) => Verdict::Differs {
                difference: Difference::of(d, region),
            },
            awaseru_core::Verdict::NotDetermined(cause) => Verdict::NotDetermined {
                cause: cause.into(),
                says: cause.to_string(),
            },
        }
    }
}

impl From<&awaseru_core::platform::Beginning> for Beginning {
    fn from(b: &awaseru_core::platform::Beginning) -> Self {
        Beginning {
            repeats: b.repeats(),
            settled: b.settled.clone(),
            says: b.to_string(),
        }
    }
}

impl From<&awaseru_core::Region> for Region {
    fn from(r: &awaseru_core::Region) -> Self {
        Region {
            name: r.name.clone(),
            size: r.size,
            readable: r.access.readable(),
            writable: r.access.writable(),
        }
    }
}

impl Control {
    pub fn of(c: &crate::perturb::Control) -> Self {
        match c {
            crate::perturb::Control::NotRun => Control::NotRun {
                says: c.to_string(),
            },
            crate::perturb::Control::Ran {
                perturbation,
                plain,
                perturbed,
            } => Control::Ran {
                perturbation: perturbation.clone(),
                // Each verdict with its own region, and not the report's:
                // a control can be noticed in a region the main comparison
                // agreed about, and naming the wrong one would be worse than
                // naming none. `Difference`'s region documents itself as
                // always present on a difference this tool produced, and until
                // the control compared region by region it was absent here.
                plain: Box::new(Verdict::of(&plain.verdict, plain.region.clone())),
                perturbed: Box::new(Verdict::of(
                    &perturbed.verdict,
                    perturbed.region.clone(),
                )),
                noticed: plain.verdict != perturbed.verdict,
                says: c.to_string(),
            },
        }
    }
}

impl Report {
    /// A report, as the wire carries it — all four parts of §5.
    ///
    /// `verdict` is the one with everything applied (§4.8's caveat, §4.12's
    /// beginning), because that is the field a client should read.
    /// `as_compared` is present only when the two differ, so a client that
    /// never looks at it is never misled by it.
    pub fn of(r: &crate::differ::Report) -> Self {
        let verdict = r.verdict();
        // §13's Q16, answered: the region comes from the report, which took it
        // from the comparison that produced the verdict — not from the
        // localisation, which is optional. A client that did not pay for §5.4
        // still gets an offset it can place.
        let region = r.differing_region().map(str::to_string);
        Report {
            routine: r.routine().to_string(),
            verdict: Verdict::of(&verdict, region.clone()),
            as_compared: (&verdict != r.as_compared())
                .then(|| Verdict::of(r.as_compared(), region)),
            moved: r.moved(),
            localisation: r.localisation().map(Localised::from),
            control: Control::of(r.control()),
            complete: r.complete(),
            coverage: r.coverage().map(Coverage::of),
            beginning: r.beginning().into(),
            took_ms: r.took().as_millis() as u64,
        }
    }
}

impl From<&crate::localise::Localised> for Localised {
    fn from(l: &crate::localise::Localised) -> Self {
        Localised {
            region: l.region.clone(),
            offset: l.offset,
            wrote: (&l.wrote).into(),
            replayed: l.replayed,
            from: l.from.clone(),
        }
    }
}

impl Span {
    pub fn of(s: &crate::routine::Span) -> Self {
        Span {
            region: s.region.clone(),
            offset: s.offset,
            length: s.length,
        }
    }

    pub fn into_routine_span(&self) -> crate::routine::Span {
        crate::routine::Span::new(self.region.clone(), self.offset, self.length)
    }
}

impl Bound {
    /// The bound a client asked for, as the one the tool runs.
    ///
    /// Infallible: every bound this vocabulary can express is one §4.2 has, and
    /// a bound the *backend* will not honour comes back as `Reason::Refused`
    /// from the run rather than as a refusal here (§4.3).
    pub fn into_core(self) -> awaseru_core::Bound {
        match self {
            Bound::Frames { count } => awaseru_core::Bound::Frames(count),
            Bound::Instructions { count } => awaseru_core::Bound::Instructions(count),
            Bound::Address { address, within } => {
                awaseru_core::Bound::Address { address, within }
            }
            Bound::Write {
                region,
                offset,
                until,
                within,
            } => awaseru_core::Bound::Write {
                region,
                offset,
                until,
                within,
            },
        }
    }
}

impl Routine {
    pub fn into_core(self, writes: Vec<crate::routine::Span>) -> crate::routine::Routine {
        crate::routine::Routine {
            name: self.name,
            entry: self.entry,
            returns_to: self.returns_to,
            within: self.within,
            reaching: self.reaching,
            from: self.from,
            writes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(value: &impl Serialize) -> serde_json::Value {
        serde_json::to_value(value).expect("it serializes")
    }

    /// §8.6's extensibility, in the direction it actually has to work.
    ///
    /// A client is talking to a tool that may be **newer than it is**, and a
    /// newer tool says more. Every reply shape must therefore skip a field it
    /// does not know, at every depth — because a field is added where it
    /// belongs, not at the top.
    ///
    /// Until M6 these were strict, so a field added to a report broke every
    /// older reader, including this host's own parent process reading its
    /// child. Nothing was additive and every addition would have been a version
    /// bump — which in practice means none happen.
    #[test]
    fn a_reply_from_a_newer_tool_parses_at_every_depth() {
        // Top level, and one nested object per level of the deepest reply
        // there is: report -> verdict -> difference -> wrote -> position.
        //
        // The invented names have to stay invented. This test first used
        // `coverage` for the report's unknown field, and M6 made `coverage`
        // real two units later — at which point the test failed, correctly,
        // because a known field with the wrong shape is a malformed reply and
        // not an extension. `call_tree` is §10's next unimplemented feature and
        // will need the same treatment the day somebody writes it.
        let from_the_future = r#"{
            "result": "report",
            "a_reply_field_from_2027": true,
            "report": {
                "routine": "running-total",
                "call_tree": {"calls": 12, "returns": 11},
                "verdict": {
                    "verdict": "differs",
                    "a_verdict_field": null,
                    "difference": {
                        "region": "work-ram",
                        "first": 1025,
                        "expected": 7,
                        "found": 3,
                        "differing": 2,
                        "compared": 64,
                        "how_confident": "very",
                        "wrote": {
                            "wrote": "at",
                            "writes": 1,
                            "cycles_ago": 900,
                            "position": {
                                "position": "mid-instruction",
                                "pc": 49152,
                                "bank_name": "a name from later"
                            }
                        }
                    }
                },
                "control": {"control": "not-run", "says": "none", "why_not": "nobody asked"},
                "complete": false,
                "beginning": {
                    "repeats": true, "settled": ["work-ram"], "says": "fine",
                    "how": "an answer this client has never heard of"
                },
                "took_ms": 3
            }
        }"#;

        let reply: Reply = serde_json::from_str(from_the_future)
            .expect("a reply from a newer tool must parse, skipping what it adds");

        // And what the old client DOES know came through unharmed — a tolerant
        // parser that dropped the fields it knows would pass the line above.
        let Reply::Report { report } = reply else {
            panic!("the tag is still read");
        };
        assert_eq!(report.routine, "running-total");
        assert!(!report.complete);
        assert_eq!(report.took_ms, 3);
        assert!(report.beginning.repeats);
        match &report.verdict {
            Verdict::Differs { difference } => {
                assert_eq!(difference.first, 1025);
                assert_eq!(difference.region.as_deref(), Some("work-ram"));
                assert_eq!(
                    difference.wrote,
                    Wrote::At {
                        position: Position::MidInstruction { pc: 0xC000 },
                        writes: 1,
                        symbol: None,
                    },
                    "the nested position survived two unknown fields around it"
                );
            }
            other => panic!("got {other:?}"),
        }
    }

    /// The other half of the asymmetry, and the half that must NOT change.
    ///
    /// A client misspelling a field is making a mistake, and §2.4 says the tool
    /// does not guess what was meant. Without this, `"localise"` written
    /// `"localize"` would be a measurement quietly made with a default nobody
    /// chose — which is exactly the class of silent wrong answer this project
    /// refuses everywhere else.
    #[test]
    fn a_command_with_a_field_nobody_declared_is_refused_by_name() {
        for (text, offender) in [
            (
                r#"{"command":"read","region":"work-ram","offset":0,"length":1,"lenght":64}"#,
                "lenght",
            ),
            (
                r#"{"command":"run","bound":{"bound":"frames","count":1,"untill":9}}"#,
                "untill",
            ),
            (
                r#"{"command":"reverify","anchor":"early","localize":true}"#,
                "localize",
            ),
        ] {
            let err = serde_json::from_str::<Command>(text)
                .expect_err("a field nobody declared is a mistake, not an extension");
            assert!(
                err.to_string().contains(offender),
                "the refusal names the field so its author can find it: {err}"
            );
        }

        // And the half that keeps this from being a blanket refusal: the same
        // commands without the typo parse.
        for text in [
            r#"{"command":"read","region":"work-ram","offset":0,"length":64}"#,
            r#"{"command":"run","bound":{"bound":"frames","count":1}}"#,
            r#"{"command":"reverify","anchor":"early"}"#,
        ] {
            serde_json::from_str::<Command>(text).expect("the correct spelling parses");
        }
    }

    /// Where tolerance stops, said out loud so nobody relies on more of it.
    ///
    /// A new **field** is additive; a new **variant** is not. An old client
    /// meeting a reply kind or a cause it has never heard of fails to parse —
    /// and should, because it has no idea what it is being told. That is what
    /// §8.6's version number is for, and it is a far rarer event than adding a
    /// field.
    #[test]
    fn a_reply_kind_from_the_future_is_not_silently_accepted() {
        let err = serde_json::from_str::<Reply>(r#"{"result":"coverage","executed":2}"#)
            .expect_err("an unknown reply kind is not something to shrug at");
        assert!(err.to_string().contains("coverage"), "said: {err}");

        let err = serde_json::from_str::<Verdict>(
            r#"{"verdict":"probably-agrees","compared":1,"moved":1}"#,
        )
        .expect_err("and neither is a verdict nobody has heard of (§2.3)");
        assert!(err.to_string().contains("probably-agrees"), "said: {err}");
    }

    /// A command round-trips through JSON, and the JSON is the shape a client's
    /// author would guess. Both halves matter: the first is the contract
    /// working, the second is the contract being usable.
    #[test]
    fn a_command_round_trips_and_reads_the_way_a_client_would_write_it() {
        let command = Command::Read {
            region: "work-ram".into(),
            offset: 0x400,
            length: 64,
        };
        let text = serde_json::to_string(&command).expect("it serializes");
        assert_eq!(
            text,
            r#"{"command":"read","region":"work-ram","offset":1024,"length":64}"#
        );
        assert_eq!(
            serde_json::from_str::<Command>(&text).expect("it parses"),
            command
        );
    }

    /// Every command and every reply survives the round trip. A vocabulary
    /// where one variant is write-only is a vocabulary with a hole in it that
    /// the client finds.
    #[test]
    fn every_command_and_reply_round_trips() {
        let commands = vec![
            Command::Hello {
                protocol: PROTOCOL,
                client: "a client".into(),
            },
            Command::Capabilities,
            Command::Regions,
            Command::Read {
                region: "work-ram".into(),
                offset: 0,
                length: 1,
            },
            Command::Write {
                region: "work-ram".into(),
                offset: 7,
            },
            Command::Run {
                bound: Bound::Frames { count: 2 },
            },
            Command::Run {
                bound: Bound::Address {
                    address: 0x8020,
                    within: 20_000,
                },
            },
            Command::Run {
                bound: Bound::Instructions { count: 10 },
            },
            Command::Run {
                bound: Bound::Write {
                    region: "work-ram".into(),
                    offset: 0x400,
                    until: 0x800F,
                    within: 20_000,
                },
            },
            Command::Examine {
                routine: Routine {
                    name: "r".into(),
                    entry: 0x8020,
                    returns_to: 0x800F,
                    within: 20_000,
                    reaching: None,
                    from: None,
                },
                given: vec![Span {
                    region: "work-ram".into(),
                    offset: 0x300,
                    length: 64,
                }],
                produced: vec![Span {
                    region: "work-ram".into(),
                    offset: 0x400,
                    length: 64,
                }],
                control: Some(Perturbation {
                    name: "the first input byte".into(),
                    span: Span {
                        region: "work-ram".into(),
                        offset: 0x300,
                        length: 64,
                    },
                }),
                coverage: Some(Span {
                    region: "program-rom".into(),
                    offset: 0,
                    length: 0x40,
                }),
                localise: true,
            },
        ];
        for command in &commands {
            let text = serde_json::to_string(command).expect("out");
            assert_eq!(
                &serde_json::from_str::<Command>(&text).expect("in"),
                command,
                "{text}"
            );
            // Every command names itself in a field a client can switch on.
            assert!(
                json(command).get("command").is_some(),
                "a command must say which it is: {text}"
            );
        }

        let replies = vec![
            Reply::Hello {
                protocol: PROTOCOL,
                tool: "0.0.0".into(),
                commands: COMMANDS.iter().map(|c| (*c).to_string()).collect(),
            },
            Reply::Capabilities {
                declared: vec!["stop-on-write".into()],
                absent: vec!["input-replay".into()],
            },
            Reply::Regions {
                regions: vec![Region {
                    name: "work-ram".into(),
                    size: 0x20000,
                    readable: true,
                    writable: true,
                }],
            },
            Reply::Bytes {
                region: "work-ram".into(),
                offset: 0,
                length: 64,
            },
            Reply::Written {
                region: "work-ram".into(),
                offset: 0,
                length: 64,
            },
            Reply::Stopped {
                stop: Stop {
                    reason: Reason::AddressHit { address: 0x8020 },
                    position: Position::InstructionBoundary { pc: 0x8020 },
                    arrived: true,
                    says: "stopped".into(),
                },
            },
            Reply::Refused {
                looking_for: "a region named `nowhere`".into(),
                found: "work-ram, palette-ram".into(),
            },
        ];
        for reply in &replies {
            let text = serde_json::to_string(reply).expect("out");
            assert_eq!(
                &serde_json::from_str::<Reply>(&text).expect("in"),
                reply,
                "{text}"
            );
            assert!(json(reply).get("result").is_some(), "{text}");
        }
    }

    /// **§2.3 on the wire.** Three values, each tagged differently, and the
    /// third one carrying both a cause a client can branch on and the sentence.
    /// A wire form with two states would be the lie this project exists to not
    /// tell, told in JSON.
    #[test]
    fn the_verdict_has_three_shapes_and_the_third_says_why() {
        let agrees = Verdict::of(
            &awaseru_core::Verdict::Agrees {
                compared: 64,
                moved: 64,
            },
            None,
        );
        let differs = Verdict::of(
            &awaseru_core::Verdict::Differs(awaseru_core::Difference::new(1025, 0x57, 0x50, 63, 64)),
            Some("work-ram".into()),
        );
        let undetermined = Verdict::of(
            &awaseru_core::Verdict::NotDetermined(awaseru_core::Undetermined::Vacuous {
                compared: 64,
            }),
            None,
        );

        assert_eq!(json(&agrees)["verdict"], "agrees");
        assert_eq!(json(&agrees)["moved"], 64);
        assert_eq!(json(&differs)["verdict"], "differs");
        assert_eq!(json(&undetermined)["verdict"], "not-determined");

        // The three tags are distinct, and none of them is a boolean.
        let tags: Vec<String> = [&agrees, &differs, &undetermined]
            .iter()
            .map(|v| json(v)["verdict"].as_str().expect("a tag").to_string())
            .collect();
        assert_eq!(tags.len(), 3);
        assert_ne!(tags[0], tags[1]);
        assert_ne!(tags[1], tags[2]);
        assert_ne!(tags[0], tags[2]);

        // The third carries a cause to branch on AND the sentence to print.
        let j = json(&undetermined);
        assert_eq!(j["cause"], "vacuous");
        assert!(
            j["says"].as_str().expect("a sentence").contains("neither side wrote"),
            "{j}"
        );

        // §5.4's first item is read against a region, and the wire says which.
        assert_eq!(json(&differs)["difference"]["region"], "work-ram");
        assert_eq!(json(&differs)["difference"]["first"], 1025);
    }

    /// Every cause the core can produce has a tag here. The conversion's match
    /// is exhaustive, so a new cause stops this file compiling — this test is
    /// the other half: that the tags are distinct and kebab-case, so a client
    /// can switch on them.
    #[test]
    fn every_cause_has_its_own_tag() {
        let causes = [
            awaseru_core::Undetermined::RegionAbsent {
                region: "x".into(),
            },
            awaseru_core::Undetermined::ReferencesDisagree {
                first: "a".into(),
                second: "b".into(),
            },
            awaseru_core::Undetermined::DidNotArrive {
                stopped: "x".into(),
            },
            awaseru_core::Undetermined::Vacuous { compared: 1 },
            awaseru_core::Undetermined::CapabilityAbsent {
                capability: "x".into(),
            },
            awaseru_core::Undetermined::MovementUnknown {
                region: "x".into(),
            },
            awaseru_core::Undetermined::SpansDiffer {
                region: "x".into(),
            },
            awaseru_core::Undetermined::StatesIncomparable { why: "x".into() },
            awaseru_core::Undetermined::NotRepeatable {
                first: "a".into(),
                second: "b".into(),
            },
            awaseru_core::Undetermined::AnchorNotDemonstrated {
                anchor: "x".into(),
            },
            awaseru_core::Undetermined::SeededFromNoBoundary {
                position: "x".into(),
            },
        ];

        let mut tags: Vec<String> = causes
            .iter()
            .map(|c| {
                let cause: Cause = c.into();
                serde_json::to_value(cause)
                    .expect("a tag")
                    .as_str()
                    .expect("a string")
                    .to_string()
            })
            .collect();
        let before = tags.len();
        tags.sort();
        tags.dedup();
        assert_eq!(tags.len(), before, "two causes share a tag: {tags:?}");
        for tag in &tags {
            assert!(
                tag.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "a client switches on this: {tag}"
            );
        }
    }

    /// §5.4's third item has four shapes on the wire, and "nobody asked" is not
    /// one of the other three. A client that could not tell them apart would
    /// read an absence as an answer.
    #[test]
    fn the_four_answers_about_a_write_are_four_shapes() {
        let wrote = |w: &awaseru_core::Wrote| json(&Wrote::from(w));

        assert_eq!(wrote(&awaseru_core::Wrote::NotLooked)["wrote"], "not-looked");
        assert_eq!(
            wrote(&awaseru_core::Wrote::NothingWrote)["wrote"],
            "nothing-wrote"
        );
        let absent = wrote(&awaseru_core::Wrote::NotAvailable {
            capability: "writing-position".into(),
        });
        assert_eq!(absent["wrote"], "not-available");
        assert_eq!(absent["capability"], "writing-position");

        let at = wrote(&awaseru_core::Wrote::At {
            position: awaseru_core::Position::MidInstruction { pc: 0x802C },
            writes: 1,
        });
        assert_eq!(at["wrote"], "at");
        assert_eq!(at["writes"], 1);
        // Mid-instruction, and the wire says so: §3.4 forbids seeding there,
        // and a client told only the number would not know.
        assert_eq!(at["position"]["position"], "mid-instruction");
        assert_eq!(at["position"]["pc"], 0x802C);
    }

    /// §5.3's absence is a shape, not a null. A client reading `null` would
    /// read "no problem".
    #[test]
    fn a_control_nobody_ran_says_so_on_the_wire() {
        let not_run = Control::of(&crate::perturb::Control::NotRun);
        let j = json(&not_run);
        assert_eq!(j["control"], "not-run");
        assert!(
            j["says"].as_str().expect("a sentence").contains("no control was run"),
            "{j}"
        );

        let ran = Control::of(&crate::perturb::Control::Ran {
            perturbation: "the first input byte".into(),
            plain: crate::perturb::Located::of(
                awaseru_core::Verdict::Agrees {
                    compared: 64,
                    moved: 64,
                },
                None,
            ),
            perturbed: crate::perturb::Located::of(
                awaseru_core::Verdict::Differs(awaseru_core::Difference::new(0, 1, 2, 64, 64)),
                Some("work-ram".into()),
            ),
        });
        let j = json(&ran);
        assert_eq!(j["control"], "ran");
        assert_eq!(j["noticed"], true);
        assert_eq!(j["plain"]["verdict"], "agrees");
        assert_eq!(j["perturbed"]["verdict"], "differs");
    }

    /// `Difference`'s region documents itself as always present on a difference
    /// this tool produced, and a control's difference is one it produced.
    ///
    /// It was absent, because the control compared through `compare`, which
    /// folds several regions into one verdict and keeps the difference while
    /// losing which region it came from — the same reason `differ::by_region`
    /// exists for the main verdict. A client reading a control to find out
    /// **where** the perturbation was noticed got an offset and nothing to
    /// place it against; with more than one span in `produced`, nothing to
    /// guess with either.
    #[test]
    fn a_control_says_which_region_it_was_noticed_in() {
        let ran = Control::of(&crate::perturb::Control::Ran {
            perturbation: "one byte of the span".into(),
            plain: crate::perturb::Located::of(
                awaseru_core::Verdict::Agrees {
                    compared: 544,
                    moved: 160,
                },
                None,
            ),
            perturbed: crate::perturb::Located::of(
                awaseru_core::Verdict::Differs(awaseru_core::Difference::new(
                    0x200, 0xA5, 0x5A, 1, 544,
                )),
                Some("a-region".into()),
            ),
        });
        let j = json(&ran);
        assert_eq!(
            j["perturbed"]["difference"]["region"], "a-region",
            "the offset has to be placeable: {j}"
        );
        assert_eq!(j["perturbed"]["difference"]["first"], 0x200);

        // And the agreeing half has none to carry, which is the only case
        // where absent is right.
        assert!(
            j["plain"].get("difference").is_none(),
            "agreement has no difference to place: {j}"
        );
    }

    /// §8.6's vocabulary, and the test is what keeps the list from drifting.
    ///
    /// The `match` below is exhaustive on purpose and has no `_` arm: a new
    /// command stops this file compiling until `COMMANDS` is told about it.
    /// That is the whole mechanism — a hand-written list nobody checks drifts
    /// the first time somebody is in a hurry, and this cannot be left to care.
    #[test]
    fn every_command_is_in_the_declared_vocabulary_under_the_name_it_travels_by() {
        let one_of_each = [
            Command::Hello {
                protocol: PROTOCOL,
                client: "c".into(),
            },
            Command::Capabilities,
            Command::Regions,
            Command::Read {
                region: "r".into(),
                offset: 0,
                length: 1,
            },
            Command::Write {
                region: "r".into(),
                offset: 0,
            },
            Command::Run {
                bound: Bound::Frames { count: 1 },
            },
            Command::Arrive { anchor: "a".into() },
            Command::Demonstrate { anchor: "a".into() },
            Command::Reverify { anchor: "a".into() },
            Command::Examine {
                routine: Routine {
                    name: "r".into(),
                    entry: 0,
                    returns_to: 1,
                    within: 1,
                    reaching: None,
                    from: None,
                },
                given: Vec::new(),
                produced: Vec::new(),
                control: None,
                coverage: None,
                localise: false,
            },
        ];

        // Exhaustive, so that adding a variant breaks the build here.
        for command in &one_of_each {
            let name = match command {
                Command::Hello { .. } => "hello",
                Command::Capabilities => "capabilities",
                Command::Regions => "regions",
                Command::Read { .. } => "read",
                Command::Write { .. } => "write",
                Command::Run { .. } => "run",
                Command::Arrive { .. } => "arrive",
                Command::Demonstrate { .. } => "demonstrate",
                Command::Reverify { .. } => "reverify",
                Command::Examine { .. } => "examine",
            };
            assert!(
                COMMANDS.contains(&name),
                "`{name}` is a command and is not declared"
            );
            // And the name is the one the wire uses, not a label beside it.
            assert_eq!(
                json(command)["command"], name,
                "the declared name must be what a client sends"
            );
        }
        assert_eq!(
            COMMANDS.len(),
            one_of_each.len(),
            "the list has something in it that is not a command"
        );
    }

    /// U1's done-condition: a client can tell whether a command exists
    /// **without sending it**.
    ///
    /// That is the thing the first outside client could not do. It wanted a
    /// command, the vocabulary did not say, and the only way to find out was to
    /// read this file.
    #[test]
    fn a_client_learns_the_vocabulary_from_the_greeting_rather_than_from_a_refusal() {
        let greeting = Reply::Hello {
            protocol: PROTOCOL,
            tool: "t".into(),
            commands: COMMANDS.iter().map(|c| (*c).to_string()).collect(),
        };
        let j = json(&greeting);
        let said = j["commands"].as_array().expect("a list of commands");

        assert!(
            said.iter().any(|c| c == "examine"),
            "a command it has is listed: {j}"
        );
        assert!(
            !said.iter().any(|c| c == "a-command-nobody-wrote"),
            "and one it does not have is not: {j}"
        );
        assert_eq!(said.len(), COMMANDS.len());
    }

    /// An arrival says what a comparison from it would be worth, in three
    /// answers rather than two.
    ///
    /// "Shown by somebody else" is neither shown nor unshown. Finding 29
    /// decided that a demonstration belongs to the run that performed it, so a
    /// blob that arrived in a box is reported as **theirs** — and folding that
    /// into "not established" would lose the only thing a person can act on,
    /// which is who to ask.
    ///
    /// An enum and not an `Option`, for the reason `Control` gives about its
    /// own absence: a field reading `null` is read as "no problem".
    #[test]
    fn an_arrival_says_whether_it_is_established_and_whose_establishing_it_was() {
        let here = Established::Here {
            replays: 3,
            says: "shown here".into(),
        };
        let elsewhere = Established::Elsewhere {
            by: "a backend 1.0.0".into(),
            says: "shown by them".into(),
        };
        let nowhere = Established::Nowhere {
            says: "nothing has shown it".into(),
        };

        assert_eq!(json(&here)["established"], "here");
        assert_eq!(json(&here)["replays"], 3);
        assert_eq!(json(&elsewhere)["established"], "elsewhere");
        assert_eq!(json(&elsewhere)["by"], "a backend 1.0.0");
        assert_eq!(json(&nowhere)["established"], "nowhere");

        // The three are distinguishable on the wire, which is the whole point:
        // a client must not have to tell them apart by reading a sentence.
        let tags: Vec<String> = [&here, &elsewhere, &nowhere]
            .iter()
            .map(|e| json(e)["established"].as_str().expect("a tag").to_string())
            .collect();
        assert_eq!(tags.len(), 3);
        assert_eq!(
            tags.iter().collect::<std::collections::BTreeSet<_>>().len(),
            3,
            "three answers, three tags: {tags:?}"
        );

        // And each carries its sentence, so a client that only prints has
        // something to print.
        for e in [&here, &elsewhere, &nowhere] {
            let says = json(e)["says"].as_str().expect("a sentence").to_string();
            assert!(!says.is_empty(), "{e:?}");
        }
    }

    /// §4.3's stop carries whether it arrived, because every client would
    /// otherwise write that match itself and the one that gets it wrong
    /// compares a state from the wrong place.
    #[test]
    fn a_stop_says_whether_it_arrived_rather_than_leaving_it_to_be_derived() {
        let arrived = Stop::from(&awaseru_core::Stop {
            reason: awaseru_core::Reason::AddressHit { address: 0x8020 },
            position: awaseru_core::Position::InstructionBoundary { pc: 0x8020 },
        });
        assert!(arrived.arrived);
        assert_eq!(json(&arrived)["reason"]["reason"], "address-hit");

        let exhausted = Stop::from(&awaseru_core::Stop {
            reason: awaseru_core::Reason::BudgetExhausted,
            position: awaseru_core::Position::InstructionBoundary { pc: 0x8015 },
        });
        assert!(!exhausted.arrived, "a budget that ran out did not arrive");
        assert!(exhausted.says.contains("budget"), "{}", exhausted.says);
    }

    /// §8.5 and §2.7: the vocabulary names no platform and no region. Every
    /// name on the wire came from the configuration or the backend.
    #[test]
    fn the_vocabulary_names_no_platform_and_no_console_memory() {
        // The serialized *shape* of every type here, with no values filled in
        // from a backend: the tags and field names are the vocabulary.
        let shapes = [
            serde_json::to_string(&Command::Capabilities).unwrap(),
            serde_json::to_string(&Command::Regions).unwrap(),
            serde_json::to_string(&Bound::Frames { count: 1 }).unwrap(),
            serde_json::to_string(&Cause::Vacuous).unwrap(),
            serde_json::to_string(&Wrote::NotLooked).unwrap(),
        ]
        .join(" ");

        for forbidden in [
            "snes", "nintendo", "mesen", "ppu", "apu", "vram", "cgram", "oam", "wram", "sfc",
            "cartridge",
        ] {
            assert!(
                !shapes.to_lowercase().contains(forbidden),
                "`{forbidden}` is a platform's word and must not be in the vocabulary: {shapes}"
            );
        }
    }

    /// A bound a client wrote is the bound the tool runs — all four of them,
    /// with the budget §4.4 requires carried through rather than defaulted.
    #[test]
    fn every_bound_converts_to_the_one_the_tool_runs() {
        assert_eq!(
            Bound::Frames { count: 3 }.into_core(),
            awaseru_core::Bound::Frames(3)
        );
        assert_eq!(
            Bound::Instructions { count: 3 }.into_core(),
            awaseru_core::Bound::Instructions(3)
        );
        assert_eq!(
            Bound::Address {
                address: 0x8020,
                within: 99
            }
            .into_core(),
            awaseru_core::Bound::Address {
                address: 0x8020,
                within: 99
            }
        );
        assert_eq!(
            Bound::Write {
                region: "work-ram".into(),
                offset: 4,
                until: 0x800F,
                within: 99
            }
            .into_core(),
            awaseru_core::Bound::Write {
                region: "work-ram".into(),
                offset: 4,
                until: 0x800F,
                within: 99
            }
        );
    }

    /// **A field this tool does not know is a refusal, not something to
    /// ignore.** A client sending one expects behaviour this tool does not
    /// have, and §8.6's negotiation is not settled — so the strict reading is
    /// the honest one until it is. Serde ignores unknown fields by default,
    /// which is why this is a decision and a test rather than a sentence in a
    /// document.
    #[test]
    fn a_field_this_tool_does_not_know_is_refused() {
        let unknown = r#"{"command":"read","region":"work-ram","offset":0,"length":1,"hurry":true}"#;
        let err = serde_json::from_str::<Command>(unknown).expect_err("an unknown field");
        assert!(
            err.to_string().contains("hurry"),
            "the refusal must name it: {err}"
        );

        // And the same message without the extra field parses, so the test is
        // about the field and not about the message.
        let known = r#"{"command":"read","region":"work-ram","offset":0,"length":1}"#;
        assert!(serde_json::from_str::<Command>(known).is_ok());

        // Nested, too: a span with a field nobody knows is refused.
        let nested = r#"{"command":"write","region":"work-ram","offset":0,"extra":1}"#;
        assert!(serde_json::from_str::<Command>(nested).is_err());
    }

    /// A refusal must say both halves. One that said only what went wrong would
    /// make a client's author guess, and §2.4 refuses guessing on this side of
    /// the line too.
    #[test]
    fn a_refusal_carries_what_was_looked_for_and_what_was_found() {
        let refused = Reply::Refused {
            looking_for: "a region named `nowhere`".into(),
            found: "work-ram, palette-ram".into(),
        };
        let j = json(&refused);
        assert_eq!(j["result"], "refused");
        assert!(j["looking_for"].as_str().unwrap().contains("nowhere"));
        assert!(j["found"].as_str().unwrap().contains("work-ram"));
    }
}
