//! Does the fixture of §11.3 actually run, and write what it says it writes?
//!
//! This is the test that needs no supplied software: the program is this
//! project's, so its expected values are this project's too, and the
//! assertions can be about **content** rather than only about plumbing
//! (§11.2). It still needs a built backend, and skips without one.
//!
//! One test, because the backend's emulator is a single global object — the
//! `reference` module says why — and this file is its own binary, so it does
//! not collide with the other integration tests.

use awaseru_core::{Bound, Platform};
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::Reference;
use std::path::PathBuf;

#[test]
fn the_fixture_writes_the_patterns_it_says_it_does() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-fixture-test");
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("fixture.sfc");
    std::fs::write(&rom, fixture::image()).expect("write the image");

    let mut reference = match Reference::open(&library, &dir, &rom) {
        Ok(r) => r,
        Err(e) => panic!("the backend would not take the fixture: {e}"),
    };

    // The program's two fills finish well inside one frame; a few frames is
    // room to spare and leaves the spin loop running.
    let stop = reference.run(Bound::Frames(3)).expect("three frames");
    assert!(stop.arrived(), "{stop}");

    // ---- work memory -----------------------------------------------------
    let work = reference.read("work-ram").expect("it reads");
    let at = expected::WORK_PATTERN_AT;
    let pattern = expected::work_pattern();
    assert_eq!(
        &work[at..at + pattern.len()],
        &pattern[..],
        "the fill loop's pattern is not where the program puts it"
    );

    // The sentinels say the pattern's boundaries are where the program put
    // them. Asserting that the bytes *outside* the pattern are zero would be
    // wrong here and was: this backend fills work memory pseudo-randomly at
    // power-on, differently in every process (§13's Q13), so nothing the
    // program did not write can be asserted at all.
    assert_eq!(
        work[expected::SENTINEL_BEFORE_AT],
        expected::SENTINEL_BEFORE,
        "the sentinel before the pattern is missing, so the fill did not start where it should"
    );
    assert_eq!(
        work[expected::SENTINEL_AFTER_AT],
        expected::SENTINEL_AFTER,
        "the sentinel after the pattern is missing, so the fill did not end where it should"
    );

    // ---- the palette -----------------------------------------------------
    let palette = reference.read("palette-ram").expect("it reads");
    let at = expected::PALETTE_PATTERN_AT;
    let expected_palette = expected::palette_pattern();
    assert_eq!(
        &palette[at..at + expected_palette.len()],
        &expected_palette[..],
        "the palette fill is not what the program writes — which would mean the write goes \
         through the hardware differently from how the listing reads"
    );

    // ---- the counter moves ----------------------------------------------
    // §2.2: a region that never changes makes every comparison over it
    // vacuous, so the fixture has something that moves and this is it. The
    // program zeroes it before the spin loop, so where it starts is the
    // program's choice; what it reaches depends on cycle timing and is not
    // asserted.
    let before = work[expected::WORK_COUNTER_AT];
    reference.run(Bound::Frames(1)).expect("one more frame");
    let after = reference.read("work-ram").expect("it reads")[expected::WORK_COUNTER_AT];
    assert_ne!(
        before, after,
        "the spin loop's counter must move between frames, or a comparison over this region \
         has nothing in it"
    );

    // And nothing else in the pattern moved while it did, so the counter is
    // the only thing running.
    let later = reference.read("work-ram").expect("it reads");
    let at = expected::WORK_PATTERN_AT;
    assert_eq!(
        &later[at..at + pattern.len()],
        &pattern[..],
        "the pattern must stay put while the counter runs"
    );

    eprintln!(
        "fixture: work pattern at {:#x}, counter at {:#x} went {before} -> {after}, palette \
         pattern at {:#x}",
        expected::WORK_PATTERN_AT,
        expected::WORK_COUNTER_AT,
        expected::PALETTE_PATTERN_AT
    );
}
