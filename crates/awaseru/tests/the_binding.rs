//! §8.4's in-process binding, against the real reference.
//!
//! The vocabulary driving an actual backend: the handshake, what it declares,
//! what regions it has, a routine examined with a right reimplementation and
//! with a wrong one, and the bytes read back afterwards.
//!
//! The wrong reimplementation is the test that matters, and the reason is in
//! §M4's warning: a binding that echoed what the client sent would pass a
//! careless test. The first differing offset here is a number **only the
//! reference knows** — the client sends sixty-four bytes of candidate and never
//! says where they are wrong — so an answer naming offset 1025 came from the
//! measurement and from nowhere else.
//!
//! One test, because only one reference may exist per process.
//!
//! # What this does NOT cover
//!
//! The subprocess binding, which is the next units': nothing here is framed,
//! nothing crosses a process boundary, and the child that owns the backend does
//! not exist yet. What is established is that the vocabulary applied to a real
//! reference produces the answers §5 requires — which is what the server will
//! then carry rather than reimplement.

use std::path::PathBuf;
use std::time::Duration;

use awaseru::binding::Binding;
use awaseru::cache::Cache;
use awaseru::config::AnchorPolicy;
use awaseru::protocol::{self, Command, Reply, PROTOCOL};
use awaseru_core::anchor::Anchors;
use awaseru_core::snapshot::Provenance;
use awaseru_core::Platform;
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::{Reference, Startup};

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

fn examine(produced: Vec<u8>, localise: bool) -> (Command, Vec<u8>) {
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
            control: None,
            localise,
        },
        payload,
    )
}

#[test]
fn the_vocabulary_drives_a_real_reference() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-binding");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");

    let mut reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(30));
    let provenance = Provenance {
        reference: "fixture".into(),
        backend: "mesence".into(),
        version: reference.version().reported,
        software: "the routine fixture".into(),
    };
    let anchors = Anchors::new(vec![]).expect("none needed");
    let cache = Cache::at(dir.join("cache"));
    let mut binding = Binding::new(
        &mut reference,
        &anchors,
        &cache,
        provenance,
        AnchorPolicy {
            verify_from_origin: 0,
            reverify_at_end: false,
        },
    );

    // ---- the handshake ---------------------------------------------------
    let answered = binding.apply(
        Command::Hello {
            protocol: PROTOCOL,
            client: "the binding test".into(),
        },
        &[],
    );
    match &answered.reply {
        Reply::Hello { protocol, tool } => {
            assert_eq!(*protocol, PROTOCOL);
            assert!(!tool.is_empty(), "the tool names its own version (§16.1)");
        }
        other => panic!("got {other:?}"),
    }

    // ---- §7.3: what it declares, and what it does not --------------------
    let answered = binding.apply(Command::Capabilities, &[]);
    match &answered.reply {
        Reply::Capabilities { declared, absent } => {
            for needed in ["stop-on-execution", "stop-on-write", "writing-position"] {
                assert!(declared.contains(&needed.to_string()), "{declared:?}");
            }
            assert!(
                absent.contains(&"input-replay".to_string()),
                "measured absent on this backend (§13's Q14): {absent:?}"
            );
            assert!(binding.can_localise(), "so §5.4 can be asked for");
        }
        other => panic!("got {other:?}"),
    }

    // ---- §3.1: the names are the backend's -------------------------------
    let answered = binding.apply(Command::Regions, &[]);
    match &answered.reply {
        Reply::Regions { regions } => {
            let work = regions
                .iter()
                .find(|r| r.name == "work-ram")
                .expect("the backend exposes it");
            assert!(work.readable && work.writable);
            assert!(work.size >= expected::ROUTINE_OUTPUT_AT + expected::ROUTINE_LENGTH);
            assert!(
                regions.iter().any(|r| !r.writable),
                "and at least one region cannot be written, which §3.1 wants said"
            );
        }
        other => panic!("got {other:?}"),
    }

    // ---- §4.2: a bounded run ---------------------------------------------
    let answered = binding.apply(
        Command::Run {
            bound: protocol::Bound::Address {
                address: expected::ROUTINE_ENTRY,
                within: BUDGET,
            },
        },
        &[],
    );
    match &answered.reply {
        Reply::Stopped { stop } => {
            assert!(stop.arrived, "{stop:?}");
            assert_eq!(
                stop.reason,
                protocol::Reason::AddressHit {
                    address: expected::ROUTINE_ENTRY
                }
            );
            assert_eq!(
                stop.position,
                protocol::Position::InstructionBoundary {
                    pc: expected::ROUTINE_ENTRY
                },
                "an address bound lands on an instruction boundary (§3.4)"
            );
        }
        other => panic!("got {other:?}"),
    }

    // ---- §5: the right reimplementation agrees ---------------------------
    let right = expected::routine(&input());
    let (command, payload) = examine(right.clone(), true);
    let answered = binding.apply(command, &payload);
    let report = match &answered.reply {
        Reply::Report { report } => report.clone(),
        other => panic!("got {other:?}"),
    };
    eprintln!("the right one: {}", serde_json::to_string(&*report).unwrap());
    assert!(
        matches!(report.verdict, protocol::Verdict::Agrees { moved, .. } if moved == expected::ROUTINE_LENGTH),
        "{:?}",
        report.verdict
    );
    assert!(
        report.as_compared.is_none(),
        "nothing took this verdict away from what the bytes said"
    );
    assert!(
        matches!(report.control, protocol::Control::NotRun { .. }),
        "no control was asked for, and §5.3 says the absence is recorded"
    );
    assert!(!report.complete, "so the measurement is incomplete (§5.3)");
    assert!(report.beginning.repeats, "{:?}", report.beginning);
    assert!(
        report.localisation.is_none(),
        "there is nothing to localise about an agreement"
    );

    // ---- and the wrong one is caught, with a number only the reference has
    let wrong = expected::routine_without_the_chain(&input());
    let (command, payload) = examine(wrong.clone(), true);
    let answered = binding.apply(command, &payload);
    let report = match &answered.reply {
        Reply::Report { report } => report.clone(),
        other => panic!("got {other:?}"),
    };
    eprintln!("the wrong one: {}", serde_json::to_string(&*report).unwrap());

    match &report.verdict {
        protocol::Verdict::Differs { difference } => {
            assert_eq!(
                difference.first,
                expected::ROUTINE_OUTPUT_AT + 1,
                "this wrong one is right about the first byte and wrong about the second, and \
                 nothing in the request said so"
            );
            assert_eq!(difference.expected, right[1], "what the reference produced");
            assert_eq!(difference.found, wrong[1], "and what the client sent");
            assert_eq!(
                difference.region.as_deref(),
                Some("work-ram"),
                "§5.4's offset is read against a region, and the wire says which"
            );
            // §5.4's third item, through the binding: the routine's own store.
            assert_eq!(
                difference.wrote,
                protocol::Wrote::At {
                    position: protocol::Position::MidInstruction {
                        pc: expected::ROUTINE_STORE
                    },
                    writes: 1,
                },
                "and not {:#X}, the store after the return (§4.5)",
                expected::ROUTINE_CLOBBER_STORE
            );
        }
        other => panic!("a wrong reimplementation must be caught: {other:?}"),
    }
    assert_eq!(
        report.moved, None,
        "§5.2 has no count to report for a difference"
    );

    // ---- and the bytes themselves, read back through the vocabulary ------
    let answered = binding.apply(
        Command::Read {
            region: "work-ram".into(),
            offset: expected::ROUTINE_OUTPUT_AT,
            length: expected::ROUTINE_LENGTH,
        },
        &[],
    );
    assert!(
        matches!(answered.reply, Reply::Bytes { ref region, .. } if region == "work-ram"),
        "{:?}",
        answered.reply
    );
    assert_eq!(
        answered.payload, right,
        "the reference's own output, in the payload — the examine before this left the machine \
         at the routine's return, so this is what it produced"
    );
}
