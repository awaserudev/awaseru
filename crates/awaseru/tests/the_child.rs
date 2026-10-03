//! The reference in a child process, driven over §8.3's framing.
//!
//! The configuration here is **generated** beside the fixture, so this test
//! needs no machine-local setup beyond the backend library: the software is the
//! generated image of §11.3 and its digest is computed rather than written down.
//!
//! One test, because the child owns a reference and a reference is one per
//! process — which is the very reason this child exists.
//!
//! # What it establishes
//!
//! - the same answers as the in-process binding gave, now across a process
//!   boundary and through the framing;
//! - the emulator's voice is **in the log**, where it cannot corrupt the
//!   protocol — counted, not assumed;
//! - a child killed mid-conversation is reported as **dead**, with the signal,
//!   and not as a verdict;
//! - a child that cannot open its reference refuses **in a frame** rather than
//!   writing English into the answer channel.
//!
//! # What it does NOT cover
//!
//! The server (§8.2) is the next unit: nothing here reads the *client's* stdin
//! or writes the client's stdout. Nor does anything here exercise two children,
//! which is what §5.5 would need and what §13's Q10 records.

use std::path::{Path, PathBuf};

use awaseru::child::{Child, ChildError, Where};
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
        client: "the child test".into(),
    }
}

fn examine(produced: Vec<u8>) -> (Command, Vec<u8>) {
    let mut payload = input();
    payload.extend(produced);
    (
        Command::Examine {
            routine: protocol::Routine {
                name: "running-total".into(),
                entry: expected::ROUTINE_ENTRY,
                returns_to: expected::ROUTINE_RETURN,
                within: BUDGET,
                from: None,
            },
            given: vec![span(expected::ROUTINE_INPUT_AT)],
            produced: vec![span(expected::ROUTINE_OUTPUT_AT)],
            coverage: None,
            control: None,
            localise: true,
        },
        payload,
    )
}

/// Writes the two halves of §6.1's configuration beside the fixture.
fn configure(dir: &Path, rom: &Path, library: &Path) -> Where {
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

    Where {
        shared: dir.join("awaseru.toml"),
        local: dir.join("awaseru.local.toml"),
        home: dir.join("home"),
        cache: dir.join("cache"),
        log: dir.join("home").join("reference.log"),
    }
}

#[test]
fn a_reference_in_a_child_answers_and_its_voice_goes_to_the_log() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_awaseru"));

    let dir = std::env::temp_dir().join("awaseru-child");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");
    let places = configure(&dir, &rom, &library);

    let mut child = Child::spawn(&exe, &places).expect("it starts");
    // Shorter than the default, so that a child which stops answering fails
    // this test in a minute rather than holding the suite.
    child.set_watchdog(std::time::Duration::from_secs(60));

    // ---- the handshake, across the boundary ------------------------------
    let answered = child.ask(&hello(), &[]).expect("it answers");
    match &answered.reply {
        Reply::Hello { protocol, tool } => {
            assert_eq!(*protocol, PROTOCOL);
            assert!(!tool.is_empty());
        }
        other => panic!("got {other:?}"),
    }

    // ---- §7.3, through the framing ---------------------------------------
    let answered = child.ask(&Command::Capabilities, &[]).expect("it answers");
    match &answered.reply {
        Reply::Capabilities { declared, absent } => {
            assert!(declared.contains(&"writing-position".to_string()), "{declared:?}");
            assert!(absent.contains(&"input-replay".to_string()), "{absent:?}");
        }
        other => panic!("got {other:?}"),
    }

    // ---- the right reimplementation agrees -------------------------------
    let right = expected::routine(&input());
    let (command, payload) = examine(right.clone());
    let answered = child.ask(&command, &payload).expect("it answers");
    let report = match &answered.reply {
        Reply::Report { report } => report.clone(),
        other => panic!("got {other:?}"),
    };
    assert!(
        matches!(report.verdict, protocol::Verdict::Agrees { moved, .. } if moved == expected::ROUTINE_LENGTH),
        "{:?}",
        report.verdict
    );

    // ---- and the wrong one is caught, with the reference's own numbers ----
    let wrong = expected::routine_without_the_chain(&input());
    let (command, payload) = examine(wrong.clone());
    let answered = child.ask(&command, &payload).expect("it answers");
    let report = match &answered.reply {
        Reply::Report { report } => report.clone(),
        other => panic!("got {other:?}"),
    };
    match &report.verdict {
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
                },
                "§5.4's third item survives the boundary"
            );
        }
        other => panic!("a wrong reimplementation must be caught: {other:?}"),
    }

    // ---- bytes in the payload, across the boundary ------------------------
    let answered = child
        .ask(
            &Command::Read {
                region: "work-ram".into(),
                offset: expected::ROUTINE_OUTPUT_AT,
                length: expected::ROUTINE_LENGTH,
            },
            &[],
        )
        .expect("it answers");
    assert_eq!(answered.payload, right, "the reference's own output");

    // ---- the emulator's voice is in the log, and nowhere else -------------
    // If it were on the answer channel, every assertion above would have failed
    // at the first frame. Counting it here is what proves it went *somewhere*
    // rather than being thrown away.
    let log = std::fs::read_to_string(child.log()).expect("the log exists");
    let lines = log
        .lines()
        .filter(|l| l.contains("Uninitialized memory read"))
        .count();
    assert!(
        lines > 0,
        "the emulator talks, and the log is where it talks to: {} byte(s) of log",
        log.len()
    );
    eprintln!("the log has {lines} line(s) of the emulator's own output");

    // ---- a child killed mid-conversation is dead, not a verdict ------------
    child.kill();
    let err = child
        .ask(&Command::Capabilities, &[])
        .expect_err("it is gone");
    match &err {
        ChildError::Died { how, log } => {
            assert!(
                how.contains("signal") || how.contains("status"),
                "what `wait` said: {how}"
            );
            assert!(log.ends_with("reference.log"), "{log:?}");
        }
        other => panic!("a dead child must be reported as dead: {other}"),
    }
    assert!(
        err.to_string().contains("no answer rather than a wrong one"),
        "said: {err}"
    );
}

/// A child that has already left is still allowed to have spoken.
///
/// The race this makes deterministic: a child that cannot open its reference
/// answers a framed refusal and exits, so by the time the parent asks anything,
/// the pipe for commands is closed and the refusal is sitting in the answer
/// channel. Waiting first turns "sometimes" into "always".
///
/// Before this was fixed, the failed write reported `Died` over the top of that
/// refusal: the parent said "the reference is gone" where the child had said
/// *why* it was gone, and it did so intermittently — about once in ten runs,
/// which is the worst frequency a fault can have.
#[test]
fn a_parting_refusal_survives_a_write_to_a_closed_pipe() {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_awaseru"));
    let dir = std::env::temp_dir().join("awaseru-child-parting-word");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");
    let places = configure(&dir, &rom, Path::new("/nowhere/at/all/MesenCore.so"));

    let mut child = Child::spawn(&exe, &places).expect("the process starts");

    // Long enough that the child has certainly refused and gone. Nothing is
    // being timed here; the wait is what removes the timing from the test.
    std::thread::sleep(std::time::Duration::from_millis(300));

    let answered = child
        .ask(&hello(), &[])
        .expect("the child's parting word is an answer, not a death");
    match &answered.reply {
        Reply::Refused { looking_for, found } => {
            assert!(looking_for.contains("reference"), "{looking_for}");
            assert!(
                found.contains("nowhere"),
                "the child's own reason, not the parent's report of a corpse: {found}"
            );
        }
        other => panic!("expected the refusal it sent on its way out: {other:?}"),
    }

    // And once that is read, the next question is the death — there is nothing
    // left in the channel to mistake for an answer.
    let err = child.ask(&Command::Capabilities, &[]).expect_err("gone now");
    assert!(matches!(err, ChildError::Died { .. }), "{err}");
}

/// A child that cannot open its reference says so **in a frame** and leaves.
///
/// Separate from the test above because it needs its own child, and a child is
/// a reference: this one never gets as far as having one.
#[test]
fn a_child_that_cannot_start_refuses_in_a_frame() {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_awaseru"));
    let dir = std::env::temp_dir().join("awaseru-child-no-library");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");

    // A library that is not there. Everything else is well-formed, so the only
    // thing wrong is the one thing being tested.
    let places = configure(&dir, &rom, Path::new("/nowhere/at/all/MesenCore.so"));
    let mut child = Child::spawn(&exe, &places).expect("the process starts");

    let answered = child.ask(&hello(), &[]).expect("a framed answer");
    match &answered.reply {
        Reply::Refused { looking_for, found } => {
            assert!(looking_for.contains("reference"), "{looking_for}");
            assert!(found.contains("nowhere"), "it names the path it tried: {found}");
        }
        other => panic!("a child that cannot start must refuse, not die silently: {other:?}"),
    }

    // And then it is gone, which the next question reports as a death.
    let err = child.ask(&Command::Capabilities, &[]).expect_err("gone");
    assert!(matches!(err, ChildError::Died { .. }), "{err}");
}
