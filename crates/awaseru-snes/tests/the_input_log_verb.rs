//! §4.7's input log through `Platform`'s own verb — what §7.3 needs before
//! `input-replay` may be declared.
//!
//! `the_input_log.rs` measured the backend's ability directly, in this crate's
//! own vocabulary. That is not a declaration: §7.3 says a capability is claimed
//! when something has run through the verb a host would use, and a host uses
//! `Platform::replay_input_log`. This is that.
//!
//! What it asserts is **where the machine is**, never that the call returned.
//! A replay that silently did nothing leaves the machine at power-on, and
//! power-on is a perfectly good-looking position — so a test that only checked
//! for `Ok` would pass against a verb that does nothing at all.
//!
//! # Why it skips without the environment
//!
//! §11.3's third route: a recording is made against particular software and
//! neither may enter this repository (§11.2).

use std::path::PathBuf;
use std::time::Duration;

use awaseru_core::anchor::InputLog;
use awaseru_core::{Bound, Capability, Platform};
use awaseru_snes::{Reference, Startup};

/// Far enough into the recording that the software is somewhere its own boot
/// never reaches, and short enough to run twice.
const FRAMES: u64 = 3_000;

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

#[test]
fn a_host_replays_a_log_through_the_verb_and_the_machine_moves() {
    let (Some(library), Some(software), Some(log)) = (
        std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from),
        std::env::var_os("AWASERU_TEST_SOFTWARE").map(PathBuf::from),
        std::env::var_os("AWASERU_TEST_INPUT_LOG").map(PathBuf::from),
    ) else {
        eprintln!(
            "SKIPPED: set AWASERU_TEST_BACKEND, AWASERU_TEST_SOFTWARE and \
             AWASERU_TEST_INPUT_LOG (§11.3)"
        );
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-input-log-verb");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");

    let recorded = std::fs::read(&log).expect("the log is readable");
    let log = InputLog {
        name: "the recording".into(),
        path: log,
        recorded,
    };

    let mut reference =
        Reference::open_with(&library, &dir, &software, Startup::default()).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(300));

    // ---- the machine is somewhere before the verb is called --------------
    // Five hundred frames of its own boot, so that "it moved" below cannot be
    // satisfied by a verb that did nothing from a machine that had done
    // nothing either.
    reference.run(Bound::Frames(500)).expect("frames");
    let wandered = state(&reference);

    // ---- the verb ---------------------------------------------------------
    reference.replay_input_log(&log).expect("the host's own verb");
    reference.run(Bound::Frames(1)).expect("one frame");

    assert_ne!(
        state(&reference),
        wandered,
        "a replay that silently did nothing would leave the machine where it was, and this \
         test would pass on `Ok` alone"
    );

    // ---- §4.12: and the origin says the log is what put it here ----------
    // After a replay the reproducibility is the recording's doing — it carries
    // the settings the console comes up with — so crediting this crate's
    // declared divergence would credit the wrong thing.
    let origin = reference.origin();
    assert_eq!(
        origin.input_log.as_deref(),
        Some("the recording"),
        "the origin names the log: {origin}"
    );
    assert!(
        origin.to_string().contains("recording's doing"),
        "and says whose doing the repetition is: {origin}"
    );

    // ---- it is the same place every time ---------------------------------
    // The property §4.8 rests on, asked of the verb rather than of the backend.
    let stop = reference.run(Bound::Frames(FRAMES)).expect("frames");
    assert!(stop.arrived(), "{stop:?}");
    let reached = state(&reference);
    drop(reference);

    let twice = std::env::temp_dir().join("awaseru-input-log-verb-again");
    let _ = std::fs::remove_dir_all(&twice);
    std::fs::create_dir_all(&twice).expect("a directory");
    let mut again =
        Reference::open_with(&library, &twice, &software, Startup::default()).expect("it comes up");
    again.set_watchdog(Duration::from_secs(300));
    again.replay_input_log(&log).expect("the verb again");
    again.run(Bound::Frames(1)).expect("one frame");
    again.run(Bound::Frames(FRAMES)).expect("frames");

    assert_eq!(
        state(&again),
        reached,
        "two machines, one verb, one state — byte for byte over every writable region and the \
         processor. Note the first one ran five hundred frames of its own before replaying and \
         the second did not: starting a log returns the machine to its origin, so where it had \
         been makes no difference"
    );

    // ---- and only now may anything be declared ---------------------------
    assert!(
        again.capabilities().has(Capability::InputReplay),
        "§7.3: the declaration is earned by the lines above, and this assertion is last on \
         purpose — it is the conclusion, not the premise"
    );
}
