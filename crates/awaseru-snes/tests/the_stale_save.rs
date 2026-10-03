//! What the host does when a call that cannot report failure fails.
//!
//! Twenty-one of this backend's thirty-one imported symbols return `void`.
//! Six of them sit where a wrong answer still looks like an answer, and the
//! audit before the first use pass gave each one something observable to check
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

use awaseru_core::{Bound, Platform};
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

/// A save leaves no stale file behind for the next save to be mistaken for.
///
/// `SaveStateFile` reports nothing. Before the audit, a save that silently did
/// nothing left the previous save of the same process on disk, the host read it
/// back and built a blob of a moment that had passed — and the error arrived
/// much later from a load, blaming the load.
#[test]
fn a_second_save_is_not_the_first_one_read_again() {
    let Some(mut reference) = open("awaseru-stale-save") else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    reference.run(Bound::Frames(2)).expect("frames");
    let first = reference.save_state().expect("a state");

    reference.run(Bound::Frames(30)).expect("further");
    let second = reference.save_state().expect("another state");

    assert_ne!(
        first.bytes(),
        second.bytes(),
        "two saves thirty frames apart are two different states — equal bytes would mean the \
         second save never happened and the first file was read again"
    );
    // Not the position: the break that ends a save comes from completing
    // whatever instruction was in progress, so where a save lands is not a
    // frame boundary and two saves may share a position legitimately. The bytes
    // are the evidence, and they are the thing a stale file would repeat.
}
