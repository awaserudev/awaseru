//! §8.2's transport, driven the way a client drives it.
//!
//! The test spawns `awaseru serve`, writes framed commands to its standard
//! input and reads framed replies from its standard output — exactly what the
//! Python client will do two units from now, in Rust so that the transport is
//! tested before a second language is added to the picture.
//!
//! Two tests here, and the one-reference-per-process rule is not broken by it:
//! each test spawns its **own** server, and each server spawns its own
//! reference process. The reference is a grandchild, which is the shape §13's
//! Q10 forces and the reason the server exists.
//!
//! # What it establishes
//!
//! - §8.2: the whole protocol on the client's stdin and stdout, with the
//!   emulator's voice nowhere near them;
//! - the server relays and decides nothing — the handshake it enforces is the
//!   child's;
//! - a frame it cannot parse is a refusal **and the conversation continues**;
//! - a reference that dies is reported, and **is not replaced**: every command
//!   after it gets the same refusal rather than a fresh machine that looks like
//!   the old one;
//! - closing the client's end is how a conversation ends, and the server leaves
//!   with a success status.

use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Spawn, Stdio};

use awaseru::frame::{self, Frame};
use awaseru::protocol::{self, Command, Reply, PROTOCOL};
use awaseru_snes::fixture::{self, expected};

const BUDGET: u64 = 20_000;

fn input() -> Vec<u8> {
    (0..expected::ROUTINE_LENGTH)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(3))
        .collect()
}

fn span(offset: usize) -> protocol::Span {
    protocol::Span {
        region: "work-ram".into(),
        offset,
        length: expected::ROUTINE_LENGTH,
    }
}

fn hello() -> Command {
    Command::Hello {
        protocol: PROTOCOL,
        client: "the server test".into(),
    }
}

/// A client: framed commands in, framed replies out, over a child's stdio.
struct Client {
    server: Child,
    to: Option<ChildStdin>,
    from: BufReader<std::process::ChildStdout>,
}

impl Client {
    fn spawn(exe: &Path, dir: &Path) -> Self {
        let mut server = Spawn::new(exe)
            .arg("serve")
            .arg("--config")
            .arg(dir.join("awaseru.toml"))
            .arg("--local")
            .arg(dir.join("awaseru.local.toml"))
            .arg("--home")
            .arg(dir.join("home"))
            .arg("--cache")
            .arg(dir.join("cache"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // The server's own diagnostics, which are not the protocol. Kept so
            // that a failure in this test can be read.
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the server starts");
        let to = server.stdin.take();
        let from = BufReader::new(server.stdout.take().expect("its output"));
        Client { server, to, from }
    }

    fn ask(&mut self, command: &Command, payload: &[u8]) -> (Reply, Vec<u8>) {
        let envelope = serde_json::to_string(command).expect("our own command");
        frame::write_frame(
            self.to.as_mut().expect("the pipe"),
            &Frame::with_payload(envelope, payload.to_vec()),
        )
        .expect("it takes the frame");
        self.read()
    }

    /// Sends bytes that are not a frame this tool can parse.
    fn send_raw(&mut self, bytes: &[u8]) {
        let to = self.to.as_mut().expect("the pipe");
        to.write_all(bytes).expect("it takes the bytes");
        to.flush().expect("flushed");
    }

    fn read(&mut self) -> (Reply, Vec<u8>) {
        let frame = frame::read_frame(&mut self.from)
            .expect("a frame comes back")
            .expect("the server has not closed");
        let reply: Reply = serde_json::from_str(&frame.envelope).expect("our vocabulary");
        (reply, frame.payload)
    }

    /// Closes the client's end and waits, which is how §8.2's conversation ends.
    fn finish(mut self) -> std::process::ExitStatus {
        self.to = None;
        self.server.wait().expect("it leaves")
    }
}

fn configure(dir: &Path, rom: &Path, library: &Path) {
    let digest = awaseru::digest::of_file(rom).expect("the image hashes");
    std::fs::write(
        dir.join("awaseru.toml"),
        format!(
            "[project]\nplatform = \"snes\"\n\n\
             [rom]\nsha256 = \"{digest}\"\n\n\
             [[emulator]]\nname = \"ref-a\"\nplatform = \"snes\"\nbackend = \"mesence\"\n\
             version = \"2.2.1\"\n\n\
             [reference]\nuse = \"ref-a\"\n\n\
             [anchors]\nverify_from_origin = 0\nreverify_at_end = false\n"
        ),
    )
    .expect("the shared half");
    std::fs::write(
        dir.join("awaseru.local.toml"),
        format!(
            "[rom]\npath = \"{}\"\n\n[emulator.ref-a]\npath = \"{}\"\n",
            rom.display(),
            library.display()
        ),
    )
    .expect("the machine-local half");
}

#[test]
fn a_client_drives_the_whole_cycle_over_stdin_and_stdout() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_awaseru"));

    let dir = std::env::temp_dir().join("awaseru-server");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");
    configure(&dir, &rom, &library);

    let mut client = Client::spawn(&exe, &dir);

    // ---- the handshake, which the server does not answer itself -----------
    let (reply, payload) = client.ask(&hello(), &[]);
    match &reply {
        Reply::Hello {
            protocol,
            tool,
            commands,
        } => {
            assert_eq!(*protocol, PROTOCOL);
            assert!(!tool.is_empty());
            // §8.6: the vocabulary arrives with the greeting, so a client can
            // tell what exists without sending it and being refused.
            assert!(
                commands.iter().any(|c| c == "examine"),
                "the greeting lists what this server has: {commands:?}"
            );
        }
        other => panic!("got {other:?}"),
    }
    assert!(payload.is_empty());

    // ---- a command before the handshake would have been refused ----------
    // Proved by the order: a second hello is refused by the binding in the
    // child, which is the same code the in-process binding runs.
    let (reply, _) = client.ask(&hello(), &[]);
    assert!(
        matches!(reply, Reply::Refused { .. }),
        "the child's binding enforces one handshake per connection: {reply:?}"
    );

    // ---- §5 across the transport, with the right reimplementation --------
    let right = expected::routine(&input());
    let mut payload = input();
    payload.extend(right.clone());
    let command = Command::Examine {
        routine: protocol::Routine {
            name: "running-total".into(),
            entry: expected::ROUTINE_ENTRY,
            returns_to: expected::ROUTINE_RETURN,
            within: BUDGET,
            reaching: None,
            from: None,
        },
        given: vec![span(expected::ROUTINE_INPUT_AT)],
        produced: vec![span(expected::ROUTINE_OUTPUT_AT)],
        coverage: None,
        control: None,
        localise: true,
    };
    let (reply, _) = client.ask(&command, &payload);
    match &reply {
        Reply::Report { report } => assert!(
            matches!(report.verdict, protocol::Verdict::Agrees { moved, .. } if moved == expected::ROUTINE_LENGTH),
            "{:?}",
            report.verdict
        ),
        other => panic!("got {other:?}"),
    }

    // ---- and the wrong one, with the number only the reference knows ------
    let wrong = expected::routine_without_the_chain(&input());
    let mut payload = input();
    payload.extend(wrong.clone());
    let (reply, _) = client.ask(&command, &payload);
    match &reply {
        Reply::Report { report } => match &report.verdict {
            protocol::Verdict::Differs { difference } => {
                assert_eq!(difference.first, expected::ROUTINE_OUTPUT_AT + 1);
                assert_eq!(difference.expected, right[1]);
                assert_eq!(difference.found, wrong[1]);
                assert_eq!(difference.region.as_deref(), Some("work-ram"));
                assert_eq!(
                    difference.wrote,
                    protocol::Wrote::At {
                        position: protocol::Position::MidInstruction {
                            pc: expected::ROUTINE_STORE
                        },
                        writes: 1,
                        symbol: None,
                    }
                );
            }
            other => panic!("a wrong reimplementation must be caught: {other:?}"),
        },
        other => panic!("got {other:?}"),
    }

    // ---- bytes in the payload, over the transport -------------------------
    let (reply, payload) = client.ask(
        &Command::Read {
            region: "work-ram".into(),
            offset: expected::ROUTINE_OUTPUT_AT,
            length: expected::ROUTINE_LENGTH,
        },
        &[],
    );
    assert!(matches!(reply, Reply::Bytes { .. }), "{reply:?}");
    assert_eq!(payload, right, "the reference's own output");

    // ---- something that is not a command, and the conversation goes on ----
    client.send_raw(&{
        let mut bytes = Vec::new();
        let envelope = b"{\"command\":\"dance\"}";
        bytes.extend_from_slice(&(envelope.len() as u32).to_be_bytes());
        bytes.extend_from_slice(envelope);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes
    });
    let (reply, _) = client.read();
    match &reply {
        Reply::Refused { looking_for, found } => {
            assert!(looking_for.contains("vocabulary"), "{looking_for}");
            assert!(found.contains("dance"), "it quotes what came: {found}");
        }
        other => panic!("got {other:?}"),
    }
    let (reply, _) = client.ask(&Command::Capabilities, &[]);
    assert!(
        matches!(reply, Reply::Capabilities { .. }),
        "a refused command must not end the conversation: {reply:?}"
    );

    // ---- closing the client's end is how it ends --------------------------
    let status = client.finish();
    assert!(
        status.success(),
        "the client closing its end is not a failure: {status:?}"
    );
}

/// Seeding and comparing across the boundary, in the three shapes the earlier
/// tests do not use: a difference nobody localised, a control, and inputs cut
/// out of the payload in more than one piece.
///
/// Its own server, because a reference is one per process.
#[test]
fn the_wire_carries_every_part_of_section_five_and_cuts_the_payload_by_its_spans() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_awaseru"));

    let dir = std::env::temp_dir().join("awaseru-server-section-five");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");
    configure(&dir, &rom, &library);

    let mut client = Client::spawn(&exe, &dir);
    let (reply, _) = client.ask(&hello(), &[]);
    assert!(matches!(reply, Reply::Hello { .. }), "{reply:?}");

    let right = expected::routine(&input());
    let wrong = expected::routine_without_the_chain(&input());
    let routine = protocol::Routine {
        name: "running-total".into(),
        entry: expected::ROUTINE_ENTRY,
        returns_to: expected::ROUTINE_RETURN,
        within: BUDGET,
        reaching: None,
        from: None,
    };

    // ---- §13's Q16 on the wire: a difference nobody localised --------------
    // `localise` is false, so nothing replayed and §5.4's third item is
    // `not-looked` — and the offset must STILL be placeable, which is the
    // question. Taken from the localisation, as the first version did, this
    // comes back null.
    let mut payload = input();
    payload.extend(wrong.clone());
    let (reply, _) = client.ask(
        &Command::Examine {
            routine: routine.clone(),
            given: vec![span(expected::ROUTINE_INPUT_AT)],
            produced: vec![span(expected::ROUTINE_OUTPUT_AT)],
            coverage: None,
            control: None,
            localise: false,
        },
        &payload,
    );
    match &reply {
        Reply::Report { report } => {
            assert!(report.localisation.is_none(), "nobody asked for one");
            match &report.verdict {
                protocol::Verdict::Differs { difference } => {
                    assert_eq!(
                        difference.region.as_deref(),
                        Some("work-ram"),
                        "an offset a client cannot place is not §5.4's first item"
                    );
                    assert_eq!(difference.first, expected::ROUTINE_OUTPUT_AT + 1);
                    assert_eq!(
                        difference.wrote,
                        protocol::Wrote::NotLooked,
                        "and nobody looked, which is said rather than implied"
                    );
                }
                other => panic!("got {other:?}"),
            }
        }
        other => panic!("got {other:?}"),
    }

    // ---- §5.3 across the boundary: a control, with its bytes in the payload
    // The payload is given, then produced, then the control's span — the third
    // segment, which nothing has exercised until now.
    let mut changed = input();
    changed[0] = changed[0].wrapping_add(1);
    let mut payload = input();
    payload.extend(right.clone());
    payload.extend(changed);
    let (reply, _) = client.ask(
        &Command::Examine {
            routine: routine.clone(),
            given: vec![span(expected::ROUTINE_INPUT_AT)],
            produced: vec![span(expected::ROUTINE_OUTPUT_AT)],
            control: Some(protocol::Perturbation {
                name: "the first input byte".into(),
                span: span(expected::ROUTINE_INPUT_AT),
            }),
            coverage: None,
            localise: false,
        },
        &payload,
    );
    match &reply {
        Reply::Report { report } => {
            assert!(
                matches!(report.verdict, protocol::Verdict::Agrees { .. }),
                "the right implementation still agrees: {:?}",
                report.verdict
            );
            match &report.control {
                protocol::Control::Ran {
                    perturbation,
                    noticed,
                    plain,
                    perturbed,
                    ..
                } => {
                    assert_eq!(perturbation, "the first input byte");
                    assert!(*noticed, "every output byte depends on it");
                    assert!(matches!(**plain, protocol::Verdict::Agrees { .. }));
                    assert!(matches!(**perturbed, protocol::Verdict::Differs { .. }));
                }
                other => panic!("a control was asked for: {other:?}"),
            }
            assert!(
                report.complete,
                "a control that discriminates is what completes a measurement (§5.3)"
            );
        }
        other => panic!("got {other:?}"),
    }

    // ---- the payload is cut by its spans, however many there are ----------
    // The same input, seeded as two halves. If the cutting followed anything
    // but the spans' own lengths in order, the routine would read different
    // bytes and the right implementation would stop agreeing.
    let half = expected::ROUTINE_LENGTH / 2;
    let mut payload = input();
    payload.extend(right.clone());
    let (reply, _) = client.ask(
        &Command::Examine {
            routine: routine.clone(),
            given: vec![
                protocol::Span {
                    region: "work-ram".into(),
                    offset: expected::ROUTINE_INPUT_AT,
                    length: half,
                },
                protocol::Span {
                    region: "work-ram".into(),
                    offset: expected::ROUTINE_INPUT_AT + half,
                    length: expected::ROUTINE_LENGTH - half,
                },
            ],
            produced: vec![span(expected::ROUTINE_OUTPUT_AT)],
            coverage: None,
            control: None,
            localise: false,
        },
        &payload,
    );
    match &reply {
        Reply::Report { report } => assert!(
            matches!(report.verdict, protocol::Verdict::Agrees { .. }),
            "two spans holding the same bytes as one must give the same answer: {:?}",
            report.verdict
        ),
        other => panic!("got {other:?}"),
    }

    // And a payload one byte short of what those spans claim is refused.
    let mut short = input();
    short.extend(right.clone());
    short.pop();
    let (reply, _) = client.ask(
        &Command::Examine {
            routine,
            given: vec![span(expected::ROUTINE_INPUT_AT)],
            produced: vec![span(expected::ROUTINE_OUTPUT_AT)],
            coverage: None,
            control: None,
            localise: false,
        },
        &short,
    );
    match &reply {
        Reply::Refused { looking_for, found } => {
            assert!(looking_for.contains("128"), "the number needed: {looking_for}");
            assert!(found.contains("127"), "and the number sent: {found}");
        }
        other => panic!("a payload that disagrees with its spans must be refused: {other:?}"),
    }

    let status = client.finish();
    assert!(status.success(), "{status:?}");
}

/// A server whose reference cannot be opened refuses, and keeps refusing.
///
/// Its own test because it needs its own server, and the refusal is relayed
/// from a child that never got as far as a reference.
#[test]
fn a_reference_that_cannot_open_is_refused_and_never_replaced() {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_awaseru"));
    let dir = std::env::temp_dir().join("awaseru-server-no-library");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");
    configure(&dir, &rom, Path::new("/nowhere/at/all/MesenCore.so"));

    let mut client = Client::spawn(&exe, &dir);

    // The child answers its framed refusal, which the server relays.
    let (reply, _) = client.ask(&hello(), &[]);
    match &reply {
        Reply::Refused { found, .. } => {
            assert!(found.contains("nowhere"), "it names the path: {found}");
        }
        other => panic!("expected the child's refusal, got {other:?}"),
    }

    // The next command finds the reference gone, and says so.
    let (second, _) = client.ask(&Command::Capabilities, &[]);
    let said = match &second {
        Reply::Refused { looking_for, found } => {
            assert!(looking_for.contains("reference"), "{looking_for}");
            assert!(
                found.contains("gone") || found.contains("status") || found.contains("signal"),
                "it says what happened to it: {found}"
            );
            found.clone()
        }
        other => panic!("a gone reference is a refusal, not a verdict: {other:?}"),
    };

    // **And the one after that gets the same answer.** This is the assertion
    // the unit is about: a server that quietly started a second reference would
    // hand the client a different machine at a different position wearing the
    // same name, and the client could not tell. The first version of this test
    // stopped one command too early to notice — a mutation that forgot the death
    // passed it — so it goes one further.
    let (third, _) = client.ask(&Command::Capabilities, &[]);
    match &third {
        Reply::Refused { found, .. } => assert_eq!(
            found, &said,
            "the same death, remembered, and not a fresh reference's answer"
        ),
        other => panic!("a dead reference must never be replaced: {other:?}"),
    }

    let _ = client.finish();
}
