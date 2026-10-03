//! §10's coverage against the hardest case: a routine that **exists** and is
//! never reached.
//!
//! Padding and dead bytes are easy — they are not code, and a tool could tell
//! them apart by decoding. This fixture's two routines are byte for byte the
//! same program apart from the constant each applies and the buffer each
//! writes, and the program calls one of them. Nothing but having been reached
//! can tell them apart.
//!
//! Two witnesses, which is what makes this more than a restatement of
//! `the_access_record.rs`:
//!
//! 1. **coverage** says one ran and the other did not;
//! 2. **memory** agrees — the called routine's output holds what this project
//!    says it should, and the other buffer was never touched.
//!
//! If coverage claimed the uncalled routine ran, the second witness would
//! contradict it. That is the shape §10 asks for: "there is always a routine I
//! did not know about" is answered by what has *not* been seen, and a coverage
//! that says everything ran answers nothing at all.
//!
//! One test, because only one reference may exist per process.

use std::path::PathBuf;
use std::time::Duration;

use awaseru_core::{Bound, Platform};
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::{Reference, Startup};

const ORIGIN: u64 = 0x8000;
const BUDGET: u64 = 20_000;

fn offset_of(address: u64) -> usize {
    usize::try_from(address - ORIGIN).expect("the program is mapped at the origin")
}

fn input() -> Vec<u8> {
    (0..expected::TWO_LENGTH)
        .map(|i| (i as u8).wrapping_mul(11).wrapping_add(5))
        .collect()
}

#[test]
fn coverage_tells_a_routine_that_ran_from_one_that_only_exists() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };
    let dir = std::env::temp_dir().join("awaseru-uncalled");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("coverage.sfc");
    std::fs::write(&rom, fixture::image_with_two_routines()).expect("write the image");

    let mut reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(60));

    // Seed the input both routines read, so that the one which does not run has
    // everything it would need — and still leaves nothing behind.
    reference
        .write_span("work-ram", expected::TWO_INPUT_AT, &input())
        .expect("the input is seeded");

    reference.forget_coverage().expect("declared, so it answers");
    let stop = reference
        .run(Bound::Address {
            address: expected::TWO_RETURN,
            within: BUDGET,
        })
        .expect("the called routine returns");
    assert!(stop.arrived(), "{stop:?}");

    // ---- witness one: coverage -------------------------------------------
    let covered = reference
        .coverage("program-rom", 0, 0x80)
        .expect("declared, so it answers");

    let called = offset_of(expected::TWO_CALLED_ENTRY);
    let uncalled = offset_of(expected::TWO_UNCALLED_ENTRY);
    for i in 0..18 {
        assert_eq!(
            covered.ran_at(called + i),
            Some(true),
            "byte {i} of the called routine ran"
        );
        assert_eq!(
            covered.ran_at(uncalled + i),
            Some(false),
            "byte {i} of the uncalled routine is the same instruction as byte {i} \
             of the other, and nothing reached it"
        );
    }

    // The whole of it falls inside one stretch nothing reached, which a
    // coverage wrong about a byte here and there could not produce.
    let gaps = covered.never_ran();
    assert!(
        gaps.iter()
            .any(|g| g.start <= uncalled && g.end >= uncalled + 18),
        "the uncalled routine is one unbroken gap: {gaps:?}"
    );
    assert!(
        covered.ran() > 0 && covered.untouched() > 0,
        "{} ran and {} did not — both halves non-empty, or the assertions above \
         are about a constant",
        covered.ran(),
        covered.untouched()
    );

    // ---- witness two: the memory -----------------------------------------
    // Coverage could be lying. The buffers cannot both agree with it by
    // accident: the one that ran holds what this project says it should, and
    // the one that did not is untouched.
    let produced = reference
        .read_span(
            "work-ram",
            expected::TWO_CALLED_OUTPUT_AT,
            expected::TWO_LENGTH,
        )
        .expect("the output is readable");
    assert_eq!(
        produced,
        expected::two_called(&input()),
        "the called routine did what this project assembled it to do"
    );

    let never = reference
        .read_span(
            "work-ram",
            expected::TWO_UNCALLED_OUTPUT_AT,
            expected::TWO_LENGTH,
        )
        .expect("readable");
    assert!(
        never.iter().all(|b| *b == 0),
        "the uncalled routine's buffer is untouched, which is the memory \
         agreeing with coverage rather than coverage being believed: {never:?}"
    );
    assert_ne!(
        never,
        expected::two_uncalled(&input()),
        "and it is specifically NOT what that routine would have written"
    );
}
