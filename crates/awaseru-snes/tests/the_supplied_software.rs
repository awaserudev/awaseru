//! Is a *boot* reproducible on software this repository does not contain?
//!
//! §2.5 wants the same run to stop the same way every time, and §13's Q13
//! measured that the first backend fills work memory pseudo-randomly, so
//! nothing downstream of a power-on repeats — until `Startup`'s declared
//! divergence writes zeros over it. That was measured on a generated fixture,
//! which is a program this project wrote.
//!
//! This asks the same question of a program it did not write: real software,
//! supplied by whoever is running the test, booted with no input at all and
//! left to itself for a thousand-odd frames. Two processes, compared byte for
//! byte over every writable region and the processor.
//!
//! It matters because of what it makes possible. An anchor that needs an input
//! log cannot be reached today — `doc/findings.md` has why — and an anchor
//! that needs nothing but a power-on and a frame count can, **if** the frames
//! after that power-on repeat. This is the test of that if.
//!
//! # Why it skips without the environment
//!
//! §11.3's third route: the software is not in this repository and will not be
//! (§11.2). It is named by the environment and the test prints SKIPPED without
//! it.
//!
//! # What this does NOT cover
//!
//! Reproducibility across *machines*, or across builds of the backend. Two
//! processes on one machine is what §2.5 needs for a comparison to mean
//! anything; the rest is §4.11's cache key, which includes the backend's
//! version precisely because this test cannot speak for another one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use awaseru_core::{Bound, Platform};
use awaseru_snes::{Reference, Startup};

/// Far enough in that the software has initialised itself and is running its
/// own code every frame, rather than still clearing memory.
const FRAMES: u64 = 1_200;

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

fn booted(library: &Path, software: &Path, home: &str, startup: Startup) -> Reference {
    let dir = std::env::temp_dir().join(home);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let mut reference =
        Reference::open_with(library, &dir, software, startup).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(120));
    reference
}

#[test]
fn a_boot_repeats_on_software_this_repository_does_not_contain() {
    let (Some(library), Some(software)) = (
        std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from),
        std::env::var_os("AWASERU_TEST_SOFTWARE").map(PathBuf::from),
    ) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND and AWASERU_TEST_SOFTWARE (§11.3)");
        return;
    };

    let mut first = booted(&library, &software, "awaseru-supplied-a", Startup::default());
    let stop = first.run(Bound::Frames(FRAMES)).expect("frames");
    assert!(stop.arrived(), "{stop:?}");
    let after_one = state(&first);
    assert!(
        after_one.iter().any(|b| *b != 0),
        "a boot that wrote nothing would make the comparison below vacuous (§2.2)"
    );
    drop(first);

    let mut second = booted(&library, &software, "awaseru-supplied-b", Startup::default());
    second.run(Bound::Frames(FRAMES)).expect("frames");
    assert_eq!(
        state(&second),
        after_one,
        "{FRAMES} frames from a power-on must reach one state, or no anchor \
         defined on this software means anything (§2.5)"
    );
    drop(second);

    // ---- and the half that says the declared divergence is doing the work --
    // Without it, §13's Q13's pseudo-random fill is back and the same two runs
    // disagree. If this ever stops disagreeing, the backend changed and the
    // divergence above has become unnecessary — which is worth knowing.
    let as_found = Startup {
        at_power_on: true,
        zero_memory: false,
    };
    let mut third = booted(&library, &software, "awaseru-supplied-c", as_found);
    third.run(Bound::Frames(FRAMES)).expect("frames");
    let without_zeroing = state(&third);
    drop(third);
    let mut fourth = booted(&library, &software, "awaseru-supplied-d", as_found);
    fourth.run(Bound::Frames(FRAMES)).expect("frames");
    assert_ne!(
        state(&fourth),
        without_zeroing,
        "two boots WITHOUT the declared zeroing must disagree — if they agree, \
         `Startup::zero_memory` is no longer what makes the test above pass, \
         and the reason this project diverges from the hardware has changed"
    );
}
