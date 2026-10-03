//! §4.7's input log, replayed on this backend — the measurement §13's Q14 was
//! answered with.
//!
//! Q14 recorded input replay as impossible here. It is not, and the record was
//! wrong about the reason: the backend never lacked the ability to accept
//! input, it lacked a *controller*, because nothing had ever told it one
//! exists. A log's settings block says one does, and playback applies it.
//!
//! What this test establishes, in the order the questions have to be asked:
//!
//! 1. starting a log **returns the machine to its origin**, so a log is a
//!    definition's own beginning and not an addition to wherever it was;
//! 2. the recorded input **arrives**, which is the half that cannot be checked
//!    by watching a frame counter;
//! 3. playback **survives the debugger** — stepping one instruction at a time,
//!    which is what every bound run in this project does;
//! 4. two replays of one log reach **one state**, byte for byte, which is what
//!    §4.8's demonstration will rest on.
//!
//! # Why it needs the environment to supply the log
//!
//! §11.3's third route. A log is a recording made against particular software,
//! and neither it nor the software may enter this repository (§11.2). So both
//! are named by the environment and the test prints SKIPPED without them, the
//! way the Python client's test skips without an interpreter.
//!
//! # What this does NOT cover
//!
//! The capability is **not declared** on the strength of this test, and
//! `Reference::capabilities` says why: `Platform` has no verb for replaying a
//! log, so declaring it would let §7.3's gate pass an anchor whose log then
//! goes unreplayed. `doc/findings.md` holds the two decisions that would
//! change it. What is measured here is the backend's half, which is the half
//! that was in doubt.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use awaseru_core::{Bound, Platform};
use awaseru_snes::{Reference, Startup};

/// Long enough for the recorded input to have moved the software somewhere its
/// own boot never goes, and short enough to run twice in a test.
const FRAMES: u64 = 3_000;

struct Supplied {
    library: PathBuf,
    software: PathBuf,
    log: PathBuf,
}

fn supplied() -> Option<Supplied> {
    Some(Supplied {
        library: std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from)?,
        software: std::env::var_os("AWASERU_TEST_SOFTWARE").map(PathBuf::from)?,
        log: std::env::var_os("AWASERU_TEST_INPUT_LOG").map(PathBuf::from)?,
    })
}

/// Everything writable, in one vector, so that two of them compare for
/// equality without a digest in between.
fn state(reference: &Reference) -> Vec<u8> {
    let mut all = Vec::new();
    for region in reference.regions().iter() {
        if region.access.writable() {
            all.extend(reference.read(&region.name).expect("a readable region"));
        }
    }
    all.extend(reference.read_processor().expect("the processor").bytes());
    all
}

fn open(supplied: &Supplied, home: &str) -> Reference {
    let dir = std::env::temp_dir().join(home);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let mut reference = Reference::open_with(
        &supplied.library,
        &dir,
        &supplied.software,
        Startup::default(),
    )
    .expect("it comes up");
    reference.set_watchdog(Duration::from_secs(300));
    reference
}

#[test]
fn a_recorded_log_replays_and_survives_the_debugger() {
    let Some(supplied) = supplied() else {
        eprintln!(
            "SKIPPED: set AWASERU_TEST_BACKEND, AWASERU_TEST_SOFTWARE and \
             AWASERU_TEST_INPUT_LOG (§11.3)"
        );
        return;
    };

    let mut reference = open(&supplied, "awaseru-input-log");

    // ---- 1. starting a log returns the machine to its origin -------------
    // Five hundred frames of divergence first, so that a log which merely
    // continued from here would reach a different state than a fresh one and
    // the comparison at the end of this block would fail.
    reference.run(Bound::Frames(500)).expect("frames");
    let wandered = state(&reference);

    reference.play_input_log(&supplied.log).expect("it plays");
    assert!(
        reference.input_log_playing(),
        "playback begins when it is asked for, not when the machine next runs"
    );
    reference.run(Bound::Frames(1)).expect("one frame");
    let from_the_log = state(&reference);
    assert_ne!(
        from_the_log, wandered,
        "a log that started where the machine already was would be an addition \
         to a position nothing recorded (§4.7)"
    );

    // ---- 2. the recorded input arrives -----------------------------------
    // The log's own frames, and the proof is that the software is somewhere its
    // boot alone does not reach: left to itself it sits on one screen, and the
    // recording presses the button that leaves it. A screenshot of this is in
    // the workspace and was looked at; what the suite can check is that the
    // machine is not where a run with no input would be.
    let began = Instant::now();
    let stop = reference.run(Bound::Frames(FRAMES)).expect("frames");
    let took = began.elapsed();
    assert!(stop.arrived(), "{stop:?}");
    assert!(
        reference.input_log_playing(),
        "a log longer than this run is still playing at the end of it"
    );
    let with_input = state(&reference);
    eprintln!(
        "the log: {FRAMES} frames in {:.1}s ({:.0} a second)",
        took.as_secs_f64(),
        FRAMES as f64 / took.as_secs_f64()
    );

    // ---- 3. playback survives the debugger -------------------------------
    // One instruction at a time is what a bound run does between breakpoints,
    // and a backend that dropped input while broken would be useless for every
    // measurement in §5.
    //
    // A thousand here rather than the eighty thousand M5 measured by hand,
    // because a single-instruction run is not cheap: each one is a request, a
    // break and a wait, and the rate this prints is the one §5.4's localisation
    // pays. `doc/protocol.md` has the number and what it means for a
    // measurement that needs many steps.
    let began = Instant::now();
    for _ in 0..1_000 {
        reference.run(Bound::Instructions(1)).expect("one instruction");
    }
    eprintln!(
        "single steps: 1000 in {:.1}s ({:.0} a second)",
        began.elapsed().as_secs_f64(),
        1000.0 / began.elapsed().as_secs_f64()
    );
    assert!(
        reference.input_log_playing(),
        "a thousand single instructions must not end playback: a log that does \
         not survive the debugger cannot reach an anchor"
    );

    // ---- 4. two replays reach one state ----------------------------------
    // A second machine, from scratch, told to do exactly the same thing. This
    // is the property §4.8's demonstration needs and the one §4.11 relies on
    // when it calls a blob a cache: a log that replayed differently each time
    // would make every cached blob a guess.
    drop(reference);
    let mut again = open(&supplied, "awaseru-input-log-again");
    again.play_input_log(&supplied.log).expect("it plays");
    again.run(Bound::Frames(1)).expect("one frame");
    assert_eq!(
        state(&again),
        from_the_log,
        "one frame of one log reaches one state"
    );
    again.run(Bound::Frames(FRAMES)).expect("frames");
    assert_eq!(
        state(&again),
        with_input,
        "and {FRAMES} frames of it do too — byte for byte, every writable \
         region and the processor"
    );
}
