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

/// The three readings that cannot report a failure, each exercised where it is
/// used — and each returning something rather than the zeros or filler a
/// failed call would leave behind.
///
/// This is the half that would pass trivially if the guards were removed, so
/// it is paired with the mutation recorded in the commit: with the sentinel
/// taken out, a backend that wrote nothing produces a frame boundary at frame
/// zero, an all-filler processor record and an access record saying nothing
/// ever happened — all three of which read as ordinary answers.
#[test]
fn the_three_silent_readings_come_back_as_readings() {
    let Some(mut reference) = open("awaseru-silent-reads") else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    reference.run(Bound::Frames(2)).expect("two frames");

    // The processor record: not filler, and that is what `wrote_nothing` is
    // about. A record of thirty-two 0xFF bytes is what a failed call leaves.
    let processor = reference.read_processor().expect("a processor record");
    assert!(
        !processor.bytes().iter().all(|&b| b == 0xFF),
        "an all-filler record is a call that wrote nothing, and is refused rather than \
         returned as a processor state"
    );
    assert_eq!(processor.bytes().len(), 32);

    // The video record, through the position it decides. After two frames this
    // is a frame boundary at a frame that is not zero — and a failed read would
    // have produced a frame boundary at frame zero, which is the same shape.
    let stop = reference.run(Bound::Frames(1)).expect("one more");
    match stop.position {
        awaseru_core::Position::FrameBoundary { frame } => assert!(
            frame > 0,
            "a frame boundary at frame zero is what an unwritten video record produces"
        ),
        other => panic!("expected a frame boundary, got {other}"),
    }

    // The access record: something was written during those frames, so a record
    // saying nothing ever happened is a record that was never filled in.
    let record = reference
        .access_record("work-ram", 0, 0x200)
        .expect("the record is readable");
    assert_eq!(record.len(), 0x200);
    assert!(
        record.iter().any(|c| c.writes > 0 || c.reads > 0),
        "three frames of this program touch work memory, so an all-zero record would be one \
         the backend never wrote"
    );
}
