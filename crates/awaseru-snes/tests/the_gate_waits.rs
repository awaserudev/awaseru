//! The gated fixture waits, and nothing this project can do makes it stop
//! waiting — §4.7's input logs, as far as this backend allows.
//!
//! # What was measured, and why this test asserts so little
//!
//! §4.7 says a definition may carry a recorded input log, for anchors behind
//! software that waits for input. Reaching one needs two things: software that
//! waits, and a way to press a button. This fixture is the first. The second
//! **is not available** on this backend through any surface this project is
//! willing to bind, and that was measured rather than assumed:
//!
//! - the backend reports **no control device at any of its eight indices**, so
//!   there is nothing for an input to arrive at;
//! - setting an input override therefore stores a state that nothing reads,
//!   and the gate below stays shut with one set;
//! - attaching a controller means the configuration record, passed by value,
//!   containing ten controller configurations each holding a key-mapping set of
//!   its own. §13's Q14 has the whole of it.
//!
//! So what this test can assert is that the gate is a working gate: the program
//! reaches it, waits there, and writes nothing. That is worth having — it is
//! what will prove an input log works the day there is a way to press a button,
//! and a fixture that merely *looked* like a gate would prove nothing then.

use awaseru_core::{Bound, Platform};
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::{Reference, Startup};
use std::path::PathBuf;

/// The wait loop's addresses, from the listing in `fixture`.
const WAIT_FROM: u64 = 0x8013;
const WAIT_TO: u64 = 0x801A;

#[test]
fn the_gated_fixture_reaches_its_gate_and_waits_there() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-gate-test");
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("gated.sfc");
    std::fs::write(&rom, fixture::image_needing_input()).expect("write the image");

    let mut reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");

    let stop = reference.run(Bound::Frames(10)).expect("ten frames");
    assert!(stop.arrived(), "{stop}");

    // ---- it waited, and wrote nothing --------------------------------
    let work = reference.read("work-ram").expect("it reads");
    assert_eq!(
        work[expected::GATE_SENTINEL_AT], 0,
        "the gate's sentinel must still be zero: nothing pressed the button"
    );
    assert_eq!(
        work[expected::GATE_COUNTER_AT], 0,
        "and the counter past the gate must not have moved"
    );

    // ---- and it is waiting at the gate, not crashed ------------------
    // The negative above would also hold for a program that never started.
    // This is what tells those apart: after one instruction the processor is
    // inside the wait loop, which is where the listing says it should be.
    let stop = reference
        .run(Bound::Instructions(1))
        .expect("one instruction");
    let position = stop.position.to_string();
    let pc = match stop.position {
        awaseru_core::Position::InstructionBoundary { pc } => pc,
        other => panic!("expected an instruction boundary, got {other}"),
    };
    assert!(
        (WAIT_FROM..WAIT_TO).contains(&pc),
        "the processor should be in the gate's wait loop, between {WAIT_FROM:#x} and \
         {WAIT_TO:#x}, and is at {position}. Outside it, this program did not reach its gate \
         and the test above was asserting that nothing happened for the wrong reason"
    );

    // Ten more frames and it is still there, which says the loop is a loop.
    reference.run(Bound::Frames(10)).expect("ten more frames");
    let stop = reference
        .run(Bound::Instructions(1))
        .expect("one instruction");
    let pc = match stop.position {
        awaseru_core::Position::InstructionBoundary { pc } => pc,
        other => panic!("expected an instruction boundary, got {other}"),
    };
    assert!(
        (WAIT_FROM..WAIT_TO).contains(&pc),
        "still in the wait loop twenty frames in, and is at {pc:#x}"
    );

    eprintln!(
        "the gate is shut and the processor is waiting at {pc:#x}, between {WAIT_FROM:#x} and \
         {WAIT_TO:#x}"
    );

    // What this does NOT cover: the gate opening. Nothing here can press a
    // button — §13's Q14 — so the one assertion that would make this fixture
    // earn its keep cannot be written yet.
}
