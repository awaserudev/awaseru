//! What the host does when a call that cannot report failure fails.
//!
//! Twenty-one of this backend's thirty-one imported symbols return `void`.
//! Six of them sit where a wrong answer still looks like an answer, and the
//! audit before the FF5 use pass gave each one something observable to check
//! instead of a status to believe. These are those checks, exercised by making
//! the failure happen rather than by asserting the happy path still works.
//!
//! **These do not prove a guard fires.** No test can make a working backend
//! silently do nothing, so what is proved here is the other half: the guards do
//! **not** fire when the call worked. A check too strict would refuse every
//! honest write, and that is the way for a guard like this to be wrong that a
//! test can reach. The decisions themselves are unit-tested with both answers,
//! and `doc/findings.md`'s twenty-second entry is about this limit — found when
//! the first version of these tests survived every mutation.
//!
//! One test per file, because only one reference may exist per process.
//!
//! # Why this matters more than it looks
//!
//! §13's Q12 and `doc/findings.md`'s nineteenth entry were two instances of
//! this shape, found months apart and by accident, and each cost an afternoon.
//! The pattern they form is in the twenty-first entry: the host checked where
//! it had already been burned and nowhere else.

use std::path::PathBuf;
use std::time::Duration;

use awaseru_core::Platform;
use awaseru_snes::fixture;
use awaseru_snes::{Reference, Startup};

fn open(home: &str) -> Option<Reference> {
    let library = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from)?;
    let dir = std::env::temp_dir().join(home);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");
    let mut reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(30));
    Some(reference)
}

/// A write that does not land is refused, and one that lands is not.
///
/// The backend will not store into read-only memory, and reports nothing about
/// having declined. Before the audit this came back as success and the next
/// comparison was about input nobody chose.
#[test]
fn a_write_that_does_not_land_is_refused() {
    let Some(mut reference) = open("awaseru-silent-write") else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    // The near miss first, so that what follows is not a blanket refusal: a
    // write to memory that accepts one lands, and the bytes are there.
    let seed: Vec<u8> = (0..64u8).map(|i| i.wrapping_mul(7).wrapping_add(3)).collect();
    reference
        .write_span("work-ram", 0x300, &seed)
        .expect("work memory takes a write");
    assert_eq!(
        reference.read_span("work-ram", 0x300, seed.len()).expect("readable"),
        seed,
        "and the bytes are what was written, which is what the check compares"
    );

    // And the one that cannot land. The region is declared read-only, so the
    // refusal arrives before the backend is asked — which is the cheap guard,
    // and the point here is that it IS a refusal.
    let refused = reference
        .write_span("program-rom", 0, &[0xFF; 4])
        .expect_err("the cartridge does not take a write");
    assert!(
        !refused.to_string().is_empty(),
        "a write that cannot land is a refusal and not a silent success: {refused}"
    );
}
