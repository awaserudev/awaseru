//! Does the fixture's routine do what the Rust version of it says? — §11.3.
//!
//! §M3's done-condition compares a reimplementation of a routine against the
//! reference running that routine. That rests on the two being the same
//! transformation, and this is what establishes it: the cycle of §5.6, done by
//! hand, with the assertions about content because the content is ours.
//!
//! What the tool will offer is the same sequence as a function (§5.6's unit of
//! work); doing it by hand first is what proves the assembly before anything is
//! built on it.

use awaseru_core::{Bound, Platform, Reason};
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::{Reference, Startup};
use std::path::PathBuf;
use std::time::Duration;

/// Enough for the routine's sixty-four rounds of ten-odd instructions, and the
/// setup before it, with room to spare. §4.4 wants a number; this is one chosen
/// to be comfortably larger than the work rather than tight.
const BUDGET: u64 = 20_000;

#[test]
fn the_routine_transforms_its_input_the_way_the_rust_version_does() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-routine-test");
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");

    let mut reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(30));

    // ---- to the routine's first instruction ------------------------------
    let stop = reference
        .run(Bound::Address {
            address: expected::ROUTINE_ENTRY,
            within: BUDGET,
        })
        .expect("an address bound");
    assert_eq!(
        stop.reason,
        Reason::AddressHit {
            address: expected::ROUTINE_ENTRY
        },
        "the setup must reach the routine within {BUDGET} instructions: {stop}"
    );

    // ---- seed its input --------------------------------------------------
    // Written here rather than left as the power-on zeros, because a routine
    // fed zeros gives an output that several wrong implementations would also
    // give.
    let input: Vec<u8> = (0..expected::ROUTINE_LENGTH)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(3))
        .collect();
    assert!(
        !input.contains(&0),
        "no zero in the input, so that a dropped addition is visible"
    );
    reference
        .write_span("work-ram", expected::ROUTINE_INPUT_AT, &input)
        .expect("it writes");

    // The output is still the zeros power-on left, which is what says the
    // routine has not run yet.
    let before = reference
        .read_span(
            "work-ram",
            expected::ROUTINE_OUTPUT_AT,
            expected::ROUTINE_LENGTH,
        )
        .expect("it reads");
    assert!(
        before.iter().all(|&b| b == 0),
        "the routine has not run, so its output is the zeros it started with"
    );

    // ---- run until it returns, and no further (§4.5) ---------------------
    let stop = reference
        .run(Bound::Address {
            address: expected::ROUTINE_RETURN,
            within: BUDGET,
        })
        .expect("an address bound");
    assert_eq!(
        stop.reason,
        Reason::AddressHit {
            address: expected::ROUTINE_RETURN
        },
        "the routine must return within {BUDGET} instructions: {stop}"
    );

    // ---- and it did what the Rust version says ---------------------------
    let output = reference
        .read_span(
            "work-ram",
            expected::ROUTINE_OUTPUT_AT,
            expected::ROUTINE_LENGTH,
        )
        .expect("it reads");
    let right = expected::routine(&input);
    assert_eq!(
        output, right,
        "the assembled routine and the Rust version of it must be the same transformation, or \
         every comparison built on them is comparing two different things"
    );
    assert_ne!(output, input, "and it is not the identity");

    // The two wrong implementations differ from what the reference produced,
    // at the two offsets the fixture's own test predicts. This is the fixture
    // earning its keep: the done-condition needs a wrong answer whose first
    // differing offset is somewhere other than zero.
    let without_mask = expected::routine_without_the_mask(&input);
    let without_chain = expected::routine_without_the_chain(&input);
    assert_eq!(
        output.iter().zip(&without_mask).position(|(a, b)| a != b),
        Some(0),
        "forgetting the mask differs from the reference at offset 0"
    );
    assert_eq!(
        output.iter().zip(&without_chain).position(|(a, b)| a != b),
        Some(1),
        "and forgetting the chain differs at offset 1, having agreed at 0"
    );

    eprintln!(
        "the routine ran over {} bytes; the two wrong versions first differ at 0 and 1",
        expected::ROUTINE_LENGTH
    );

    // What this does NOT cover: that the measurement stopped at the return
    // rather than somewhere after it. The bound says so and the stop reason
    // agrees, but nothing here reads a byte the routine would have written
    // later — §4.5's rule is kept by the bound, and U5 is where the tool keeps
    // it rather than the test.
}
