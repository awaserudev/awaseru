//! §8.4's in-process binding: the vocabulary applied to a real reference.
//!
//! > For a client in the host's own language: a crate, calling in process, no
//! > IPC. For every other language: the subprocess and the protocol. Same
//! > semantics, two bindings.
//!
//! This is the one place a command turns into work. The subprocess binding
//! (§8.2) does not duplicate any of it: it decodes a frame into a `Command`,
//! calls `apply`, and encodes the answer. That is what makes "same semantics"
//! structural — there is one implementation and two ways of reaching it, rather
//! than two implementations that are meant to agree.
//!
//! # `apply` has no error type, and that is the design
//!
//! Every failure is a `Reply::Refused` carrying what was looked for and what
//! was found (§14.2). A binding that returned `Result` would make the server
//! above it decide how to turn an error into a reply, which is a second place
//! for the protocol's behaviour to live and the place the two bindings would
//! drift apart.
//!
//! # Nothing the client sends is trusted
//!
//! A region name, an offset, a length and a payload size are all input. Every
//! one of them is checked here or by the verb underneath, and a client cannot
//! make this process allocate, read out of bounds or run unbounded.

use awaseru_core::anchor::Anchors;
use awaseru_core::snapshot::Provenance;
use awaseru_core::{Capability, Platform};

use crate::arrive::Arriver;
use crate::cache::Cache;
use crate::config::AnchorPolicy;
use crate::differ::{self, Request};
use crate::perturb::Perturbation;
use crate::protocol::{self, Beginning, Command, Established, Position, Reply, PROTOCOL};
use crate::routine::{Given, Span};

/// One answer: the reply, and the bytes that go with it (§8.3's payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    pub reply: Reply,
    pub payload: Vec<u8>,
}

impl Answered {
    fn of(reply: Reply) -> Self {
        Answered {
            reply,
            payload: Vec::new(),
        }
    }

    fn with(reply: Reply, payload: Vec<u8>) -> Self {
        Answered { reply, payload }
    }

    /// A refusal, with both halves §14.2 requires.
    fn refuse(looking_for: impl Into<String>, found: impl Into<String>) -> Self {
        Answered::of(Reply::Refused {
            looking_for: looking_for.into(),
            found: found.into(),
        })
    }

    pub fn was_refused(&self) -> bool {
        matches!(self.reply, Reply::Refused { .. })
    }
}

/// The tool's own version, for the handshake (§16.1).
pub const TOOL: &str = env!("CARGO_PKG_VERSION");

/// A reference, driven by the vocabulary.
pub struct Binding<'a> {
    arriver: Arriver<'a>,
    provenance: Provenance,
    /// Whether the handshake has happened. Nothing else is answered before it:
    /// a client that has not said which protocol it speaks is a client whose
    /// next message cannot be trusted to mean what it looks like.
    greeted: bool,
    /// §9's mapping, so a report can say a name as well as a number (§M7).
    /// Empty is the ordinary case and changes nothing: a project with no
    /// mapping files gets exactly the report it got before §9 existed.
    mapping: crate::mapping::Mapping,
}

impl<'a> Binding<'a> {
    pub fn new(
        platform: &'a mut dyn Platform,
        anchors: &'a Anchors,
        cache: &'a Cache,
        provenance: Provenance,
        policy: AnchorPolicy,
    ) -> Self {
        Binding {
            arriver: Arriver::new(platform, anchors, cache, provenance.clone(), policy),
            provenance,
            greeted: false,
            mapping: crate::mapping::Mapping::default(),
        }
    }

    /// The mapping this binding names things with — §M7.
    ///
    /// Separate from `new` because a mapping is the user's and optional, and a
    /// constructor that demanded one would make every test and every caller
    /// that has none pass an empty value to say so.
    pub fn naming(mut self, mapping: crate::mapping::Mapping) -> Self {
        self.mapping = mapping;
        self
    }

    /// One command, one answer. Never fails; a failure is a refusal.
    pub fn apply(&mut self, command: Command, payload: &[u8]) -> Answered {
        // ---- the handshake, before anything else (§8.6) ------------------
        if let Command::Hello { protocol, client } = &command {
            if self.greeted {
                return Answered::refuse(
                    "one handshake per connection",
                    format!("a second hello, from `{client}`"),
                );
            }
            if *protocol != PROTOCOL {
                // Both numbers, so the mismatch is legible from either side.
                return Answered::refuse(
                    format!("protocol {PROTOCOL}, which this tool {TOOL} speaks"),
                    format!("protocol {protocol}, which the client `{client}` speaks"),
                );
            }
            self.greeted = true;
            return Answered::of(Reply::Hello {
                protocol: PROTOCOL,
                tool: TOOL.to_string(),
                // §8.6: what this server has, said rather than guessed at. A
                // client that wants a command it cannot see is told so here
                // instead of finding out by being refused later.
                commands: crate::protocol::COMMANDS
                    .iter()
                    .map(|c| (*c).to_string())
                    .collect(),
            });
        }
        if !self.greeted {
            return Answered::refuse(
                format!("a hello naming protocol {PROTOCOL}, before anything else"),
                "a command sent before the handshake",
            );
        }

        match command {
            Command::Hello { .. } => unreachable!("handled above"),

            Command::Capabilities => {
                let declared = self.arriver.capabilities();
                Answered::of(Reply::Capabilities {
                    declared: declared
                        .declared()
                        .iter()
                        .map(|c| c.name().to_string())
                        .collect(),
                    absent: declared
                        .absent()
                        .iter()
                        .map(|c| c.name().to_string())
                        .collect(),
                })
            }

            Command::Regions => Answered::of(Reply::Regions {
                regions: self
                    .arriver
                    .regions()
                    .iter()
                    .map(protocol::Region::from)
                    .collect(),
            }),

            Command::Read {
                region,
                offset,
                length,
            } => match self.arriver.read_span(&region, offset, length) {
                Ok(bytes) => Answered::with(
                    Reply::Bytes {
                        region,
                        offset,
                        length: bytes.len(),
                    },
                    bytes,
                ),
                Err(e) => Answered::refuse(
                    format!("{length} byte(s) of `{region}` from {offset}"),
                    e.to_string(),
                ),
            },

            Command::Write { region, offset } => {
                if payload.is_empty() {
                    return Answered::refuse(
                        format!("bytes to write into `{region}` at {offset}"),
                        "a payload of no bytes, which is not a write",
                    );
                }
                let length = payload.len();
                match self.arriver.write_span(&region, offset, payload) {
                    Ok(()) => Answered::of(Reply::Written {
                        region,
                        offset,
                        length,
                    }),
                    Err(e) => Answered::refuse(
                        format!("{length} byte(s) written into `{region}` at {offset}"),
                        e.to_string(),
                    ),
                }
            }

            Command::Run { bound } => {
                let asked = bound.clone();
                match self.arriver.run(bound.into_core()) {
                    Ok(stop) => Answered::of(Reply::Stopped {
                        stop: (&stop).into(),
                    }),
                    Err(e) => Answered::refuse(
                        format!("a run bounded by {asked:?}"),
                        e.to_string(),
                    ),
                }
            }

            // §4.7, and it demonstrates nothing: see
            // `Arriver::arrive_without_demonstrating` for why, and
            // `Established` for what the reply says instead of staying quiet.
            Command::Arrive { anchor } => {
                match self.arriver.arrive_without_demonstrating(&anchor) {
                    Ok(arrived) => {
                        let established = establishment(&arrived);
                        let at = self.arriver.at().clone();
                        Answered::of(Reply::Arrived {
                            anchor: arrived.anchor,
                            position: Position::from(&at),
                            how: arrived.how.to_string(),
                            took_ms: arrived.took.as_millis() as u64,
                            beginning: Beginning::from(&arrived.beginning),
                            established,
                        })
                    }
                    Err(e) => Answered::refuse(
                        format!("the anchor `{anchor}`"),
                        e.to_string(),
                    ),
                }
            }

            Command::Reverify { anchor } => match self.arriver.reverify(&anchor) {
                Ok(done) => Answered::of(Reply::Reverified {
                    anchor: done.anchor,
                    uses: done.uses,
                    took_ms: done.took.as_millis() as u64,
                }),
                // A disagreement is a refusal and not a verdict: it means every
                // comparison made from this anchor in this session is void, and
                // that is not something to hand back beside a result.
                Err(e) => Answered::refuse(
                    format!("`{anchor}` to still produce what replaying it produces (§4.9)"),
                    e.to_string(),
                ),
            },

            Command::Examine {
                routine,
                given,
                produced,
                control,
                coverage,
                localise,
            } => self.examine(routine, given, produced, control, coverage, localise, payload),
        }
    }

    /// §5.6's cycle, with the bytes cut out of the payload.
    ///
    /// The payload is the given spans' bytes, then the produced spans' bytes,
    /// then the control's span if there is one — each one as long as its span
    /// says. A payload that is not exactly that long is refused with both
    /// numbers, because a client whose arithmetic is off would otherwise seed a
    /// routine with bytes nobody chose.
    #[allow(clippy::too_many_arguments)]
    fn examine(
        &mut self,
        routine: protocol::Routine,
        given: Vec<protocol::Span>,
        produced: Vec<protocol::Span>,
        control: Option<protocol::Perturbation>,
        coverage: Option<protocol::Span>,
        localise: bool,
        payload: &[u8],
    ) -> Answered {
        let mut needed: usize = 0;
        for span in given.iter().chain(&produced) {
            needed = match needed.checked_add(span.length) {
                Some(n) => n,
                None => {
                    return Answered::refuse(
                        "spans whose lengths add up",
                        "lengths that overflow a count of bytes",
                    );
                }
            };
        }
        if let Some(perturbation) = &control {
            needed = match needed.checked_add(perturbation.span.length) {
                Some(n) => n,
                None => {
                    return Answered::refuse(
                        "spans whose lengths add up",
                        "lengths that overflow a count of bytes",
                    );
                }
            };
        }
        if payload.len() != needed {
            return Answered::refuse(
                format!("a payload of exactly {needed} byte(s), one per byte of every span named"),
                format!("{} byte(s)", payload.len()),
            );
        }

        let mut at = 0usize;
        let mut take = |length: usize| {
            let bytes = payload[at..at + length].to_vec();
            at += length;
            bytes
        };

        let seeds: Vec<Given> = given
            .iter()
            .map(|span| Given {
                span: span.into_routine_span(),
                bytes: take(span.length),
            })
            .collect();
        let candidate: Vec<Vec<u8>> = produced.iter().map(|span| take(span.length)).collect();
        let perturbation = control.map(|p| Perturbation {
            name: p.name.clone(),
            given: Given {
                span: p.span.into_routine_span(),
                bytes: take(p.span.length),
            },
        });

        let writes: Vec<Span> = produced.iter().map(|s| s.into_routine_span()).collect();
        let subject = routine.into_core(writes);

        let report = differ::examine(
            &mut self.arriver,
            &self.provenance,
            &Request {
                routine: &subject,
                given: &seeds,
                produced: &candidate,
                control: perturbation.as_ref(),
                localise,
                // §10's span is NOT part of the payload's arithmetic above: it
                // names bytes to look at afterwards, not bytes to seed.
                coverage: coverage.map(|s| differ::CoverageSpan {
                    region: s.region,
                    offset: s.offset,
                    length: s.length,
                }),
            },
        );

        match report {
            Ok(report) => {
                let mut answer = protocol::Report::of(&report);
                answer.name_with(&self.mapping);
                Answered::of(Reply::Report {
                    report: Box::new(answer),
                })
            }
            Err(e) => Answered::refuse(
                format!("a measurement of `{}`", subject.name),
                e.to_string(),
            ),
        }
    }

    /// Whether this reference could localise a difference if asked — §7.3.
    ///
    /// Here so that the server can answer `capabilities` without the client
    /// having to know which capability names localisation needs.
    pub fn can_localise(&self) -> bool {
        let declared = self.arriver.capabilities();
        declared.has(Capability::StopOnWrite) && declared.has(Capability::WritingPosition)
    }
}

/// What §4.8 has established about the anchor an arrival reached.
///
/// Three answers and not two, because "shown by somebody else" is neither shown
/// nor unshown: finding 29 decided that a demonstration belongs to the run that
/// performed it, so a blob that arrived in a box is reported as **theirs** and
/// counts for nothing here. Folding that into "not established" would lose the
/// only thing a person can act on, which is who to ask.
fn establishment(arrived: &crate::arrive::Arrived) -> Established {
    if let Some(whose) = &arrived.demonstrated_elsewhere {
        return Established::Elsewhere {
            by: whose.clone(),
            says: format!(
                "this anchor was shown to produce what replaying it produces by {whose}, and \
                 not here. §4.8 makes a demonstration the property of the run that performed \
                 it, so a comparison from this position is not evidence in this session until \
                 something establishes it here"
            ),
        };
    }
    match &arrived.caveat {
        Some(caveat) => Established::Nowhere {
            says: caveat.to_string(),
        },
        None => Established::Here {
            replays: arrived.demonstrated_with,
            says: format!(
                "this anchor has been shown to produce what replaying its definition produces, \
                 here, with {} replay(s) — so a comparison from this position is evidence",
                arrived.demonstrated_with
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use awaseru_core::platform::{Beginning, ReadError, RunError, WriteError};
    use awaseru_core::StateError;
    use awaseru_core::snapshot::Processor;
    use awaseru_core::{
        Access, BackendVersion, Blob, Bound, Capabilities, Position, Reason, Region, Regions, Stop,
        check_read, check_write,
    };

    /// A platform that does nothing but answer, so that the binding's own
    /// refusals can be tested without a backend.
    ///
    /// **It does not emulate**, it has no notion of a routine, and a comparison
    /// made against it means nothing. What it is for is the half of this module
    /// that is about the vocabulary rather than about a reference: the
    /// handshake, a region that does not exist, a span past the end of one, a
    /// payload whose length disagrees with the spans that describe it.
    struct Answers {
        regions: Regions,
        work: Vec<u8>,
    }

    impl Answers {
        fn new() -> Self {
            Answers {
                regions: Regions::new(vec![
                    Region::bytes("work", 64, Access::ReadWrite),
                    Region::bytes("rom", 16, Access::ReadOnly),
                ]),
                work: vec![0; 64],
            }
        }
    }

    impl Platform for Answers {
        fn version(&self) -> BackendVersion {
            BackendVersion {
                reported: "1.0.0".into(),
                built: None,
            }
        }
        fn beginning(&self) -> Beginning {
            Beginning {
                reproducible: true,
                settled: vec!["work".into()],
                by_input_log: None,
            }
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::of([Capability::StopOnExecution])
        }
        fn regions(&self) -> Regions {
            self.regions.clone()
        }
        fn read(&self, region: &str) -> Result<Vec<u8>, ReadError> {
            check_read(&self.regions, region, None)?;
            Ok(match region {
                "work" => self.work.clone(),
                _ => vec![0xEE; 16],
            })
        }
        fn read_span(&self, region: &str, offset: usize, len: usize) -> Result<Vec<u8>, ReadError> {
            check_read(&self.regions, region, Some((offset, len)))?;
            Ok(self.read(region)?[offset..offset + len].to_vec())
        }
        fn run(&mut self, _bound: Bound) -> Result<Stop, RunError> {
            Ok(Stop {
                reason: Reason::BoundReached,
                position: Position::InstructionBoundary { pc: 0x8000 },
            })
        }
        fn write(&mut self, region: &str, bytes: &[u8]) -> Result<(), WriteError> {
            check_write(&self.regions, region, None)?;
            self.work = bytes.to_vec();
            Ok(())
        }
        fn write_span(
            &mut self,
            region: &str,
            offset: usize,
            bytes: &[u8],
        ) -> Result<(), WriteError> {
            check_write(&self.regions, region, Some((offset, bytes.len())))?;
            self.work[offset..offset + bytes.len()].copy_from_slice(bytes);
            Ok(())
        }
        fn read_processor(&self) -> Result<Processor, ReadError> {
            Ok(Processor::opaque(vec![1, 2, 3, 4]))
        }
        fn write_processor(&mut self, _processor: &Processor) -> Result<(), WriteError> {
            Ok(())
        }
        fn return_to_origin(&mut self) -> Result<(), RunError> {
            Ok(())
        }
        fn save_state(&mut self) -> Result<Blob, StateError> {
            Err(StateError::Backend {
                why: "this platform answers questions and keeps no state".into(),
            })
        }
        fn load_state(&mut self, _blob: &Blob) -> Result<(), StateError> {
            Err(StateError::Backend {
                why: "this platform answers questions and keeps no state".into(),
            })
        }
    }

    struct Held {
        anchors: Anchors,
        cache: Cache,
    }

    fn held() -> Held {
        Held {
            anchors: Anchors::new(vec![]).expect("none"),
            cache: Cache::at(std::env::temp_dir().join("awaseru-binding-tests")),
        }
    }

    fn provenance() -> Provenance {
        Provenance {
            reference: "answers".into(),
            backend: "none".into(),
            version: "1.0.0".into(),
            software: "nothing".into(),
        }
    }

    fn hello() -> Command {
        Command::Hello {
            protocol: PROTOCOL,
            client: "a test".into(),
        }
    }

    /// Runs a conversation against the fake, returning every answer.
    fn converse(commands: Vec<(Command, Vec<u8>)>) -> Vec<Answered> {
        let mut platform = Answers::new();
        let held = held();
        let mut binding = Binding::new(
            &mut platform,
            &held.anchors,
            &held.cache,
            provenance(),
            AnchorPolicy {
                verify_from_origin: 0,
                reverify_at_end: false,
            },
        );
        commands
            .into_iter()
            .map(|(command, payload)| binding.apply(command, &payload))
            .collect()
    }

    /// **The handshake is first, and it is not advice.** A client that has not
    /// said which protocol it speaks is a client whose next message cannot be
    /// trusted to mean what it looks like.
    #[test]
    fn nothing_is_answered_before_the_handshake() {
        let answers = converse(vec![
            (Command::Regions, vec![]),
            (hello(), vec![]),
            (Command::Regions, vec![]),
        ]);

        assert!(answers[0].was_refused(), "{:?}", answers[0]);
        match &answers[0].reply {
            Reply::Refused { looking_for, found } => {
                assert!(looking_for.contains("hello"), "{looking_for}");
                assert!(found.contains("before the handshake"), "{found}");
            }
            other => panic!("got {other:?}"),
        }

        assert!(matches!(answers[1].reply, Reply::Hello { .. }));
        assert!(
            matches!(answers[2].reply, Reply::Regions { .. }),
            "and after it, work happens: {:?}",
            answers[2]
        );
    }

    /// §8.6: a mismatch is refused with **both** numbers, so it is legible from
    /// either side. Negotiation is not invented (§13's Q3).
    #[test]
    fn a_protocol_this_tool_does_not_speak_is_refused_with_both_numbers() {
        let answers = converse(vec![(
            Command::Hello {
                protocol: PROTOCOL + 7,
                client: "a client from the future".into(),
            },
            vec![],
        )]);
        match &answers[0].reply {
            Reply::Refused { looking_for, found } => {
                assert!(
                    looking_for.contains(&PROTOCOL.to_string()),
                    "the tool's number: {looking_for}"
                );
                assert!(
                    found.contains(&(PROTOCOL + 7).to_string()),
                    "and the client's: {found}"
                );
                assert!(looking_for.contains(TOOL), "and the tool's version");
            }
            other => panic!("got {other:?}"),
        }

        // And a second handshake is refused too: a connection has one.
        let answers = converse(vec![(hello(), vec![]), (hello(), vec![])]);
        assert!(!answers[0].was_refused());
        assert!(answers[1].was_refused(), "{:?}", answers[1]);
    }

    /// Nothing the client sends is trusted. Each of these is a refusal naming
    /// what was looked for and what was found, and none of them is a panic.
    #[test]
    fn a_region_a_span_and_a_payload_are_all_checked() {
        let answers = converse(vec![
            (hello(), vec![]),
            // A region nobody exposes.
            (
                Command::Read {
                    region: "nowhere".into(),
                    offset: 0,
                    length: 1,
                },
                vec![],
            ),
            // Past the end of one that exists.
            (
                Command::Read {
                    region: "work".into(),
                    offset: 60,
                    length: 10,
                },
                vec![],
            ),
            // A length that would overflow if it were added to an offset.
            (
                Command::Read {
                    region: "work".into(),
                    offset: usize::MAX,
                    length: 2,
                },
                vec![],
            ),
            // A write with nothing to write.
            (
                Command::Write {
                    region: "work".into(),
                    offset: 0,
                },
                vec![],
            ),
            // A write into a region that cannot be written.
            (
                Command::Write {
                    region: "rom".into(),
                    offset: 0,
                },
                vec![1],
            ),
        ]);

        for answer in &answers[1..] {
            assert!(answer.was_refused(), "got {:?}", answer.reply);
            match &answer.reply {
                Reply::Refused { looking_for, found } => {
                    assert!(!looking_for.is_empty() && !found.is_empty(), "both halves");
                }
                other => panic!("got {other:?}"),
            }
        }

        // The refusals are about different things, and say so.
        let said: Vec<String> = answers[1..]
            .iter()
            .map(|a| match &a.reply {
                Reply::Refused { found, .. } => found.clone(),
                other => panic!("got {other:?}"),
            })
            .collect();
        assert!(said[0].contains("nowhere"), "{}", said[0]);
        assert!(said[3].contains("not a write"), "{}", said[3]);
        assert!(said[4].contains("cannot be written"), "{}", said[4]);
    }

    /// A read answers with the bytes in the payload and the length it actually
    /// read, and a write puts the payload where it says. The round trip is the
    /// one thing a client does most.
    #[test]
    fn bytes_go_out_in_the_payload_and_come_back_in_it() {
        let mut platform = Answers::new();
        let held = held();
        let mut binding = Binding::new(
            &mut platform,
            &held.anchors,
            &held.cache,
            provenance(),
            AnchorPolicy {
                verify_from_origin: 0,
                reverify_at_end: false,
            },
        );
        assert!(!binding.apply(hello(), &[]).was_refused());

        let written = binding.apply(
            Command::Write {
                region: "work".into(),
                offset: 8,
            },
            &[1, 2, 3, 4],
        );
        assert_eq!(
            written.reply,
            Reply::Written {
                region: "work".into(),
                offset: 8,
                length: 4
            }
        );
        assert!(written.payload.is_empty(), "a write answers with no bytes");

        let read = binding.apply(
            Command::Read {
                region: "work".into(),
                offset: 8,
                length: 4,
            },
            &[],
        );
        assert_eq!(
            read.reply,
            Reply::Bytes {
                region: "work".into(),
                offset: 8,
                length: 4
            }
        );
        assert_eq!(read.payload, vec![1, 2, 3, 4], "what was written is there");
    }

    /// §8.3's payload is cut by the spans that describe it, so a payload that
    /// does not match them exactly is refused **with both numbers**. A client
    /// whose arithmetic is off would otherwise seed a routine with bytes nobody
    /// chose, and the comparison would be of something else entirely.
    #[test]
    fn a_payload_that_does_not_match_its_spans_is_refused_with_both_numbers() {
        let span = |offset, length| protocol::Span {
            region: "work".into(),
            offset,
            length,
        };
        let examine = |payload: Vec<u8>| {
            converse(vec![
                (hello(), vec![]),
                (
                    Command::Examine {
                        routine: protocol::Routine {
                            name: "r".into(),
                            entry: 0x8000,
                            returns_to: 0x8010,
                            within: 100,
                            from: None,
                        },
                        given: vec![span(0, 4)],
                        produced: vec![span(8, 4)],
                        coverage: None,
                        control: None,
                        localise: false,
                    },
                    payload,
                ),
            ])
            .pop()
            .expect("an answer")
        };

        let short = examine(vec![0; 7]);
        match &short.reply {
            Reply::Refused { looking_for, found } => {
                assert!(looking_for.contains('8'), "the number needed: {looking_for}");
                assert!(found.contains('7'), "and the number sent: {found}");
            }
            other => panic!("a short payload must be refused: {other:?}"),
        }

        // Too many bytes is just as wrong as too few: trailing bytes mean the
        // client and the tool disagree about the shape of the message.
        assert!(examine(vec![0; 9]).was_refused());
    }
}
