//! What the backend's access record says about **execution** — §13's Q15's
//! route, taken.
//!
//! §10 wants execution coverage: which bytes of the software ran under this
//! input. The record this backend keeps has carried an execute count and a
//! stamp all along, and this project had read only the write half, which is
//! why §7.3 declared `write-recency` and not `execution-coverage`. A route is
//! not a declaration; this is the measurement that turns one into the other.
//!
//! Every claim here has a **negative half**, because coverage is the easiest
//! thing in this project to fake: a record that answered "executed" for
//! everything would pass any test that only asks whether something ran.
//!
//! It runs on the generated fixture, whose program this project wrote, so what
//! did and did not execute is known before the backend is asked.
//!
//! One test, because only one reference may exist per process. The record over
//! memory that is not the cartridge is `the_access_record_in_memory.rs`.

use std::path::PathBuf;
use std::time::Duration;

use awaseru_core::{Bound, Platform};
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::{Reference, Startup};

/// Where the fixture's program is mapped. Its own constants are addresses, and
/// the record is indexed by offset into the region.
const ORIGIN: u64 = 0x8000;

/// The fixture's program, as this project assembled it:
///
/// ```text
/// 0x8000 .. 0x800E   setup, then JSR into the routine
/// 0x800F .. 0x8014   the clobber — runs only AFTER the routine returns
/// 0x8015 .. 0x8017   the spin
/// 0x8018 .. 0x801F   padding, which nothing ever executes
/// 0x8020 .. 0x8036   the routine, ending in RTS
/// 0x8037 ..          nothing
/// ```
///
/// A measurement bounded by the routine's return (§4.5) therefore executes the
/// first block and the last, and none of the three in between.
const SETUP: std::ops::RangeInclusive<u64> = 0x8000..=0x800E;
const ROUTINE: std::ops::RangeInclusive<u64> = 0x8020..=0x8036;
const NEVER: [std::ops::RangeInclusive<u64>; 2] = [0x800F..=0x801F, 0x8037..=0x803F];

fn record(r: &Reference, pc: u64, n: usize) -> Vec<awaseru_snes::ffi::AccessCounts> {
    r.access_record("program-rom", (pc - ORIGIN) as usize, n)
        .expect("the record is readable")
}

fn executed(r: &Reference, pc: u64) -> u32 {
    record(r, pc, 1)[0].executions
}

#[test]
fn the_record_says_which_bytes_ran_and_which_did_not() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };
    let dir = std::env::temp_dir().join("awaseru-access-record");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");

    let mut reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(60));

    // ---- nothing has run, and the record says nothing ran ----------------
    // §2.5's starting position, visible in the record: if this were not empty,
    // every count below would be measuring the bring-up as well.
    let fresh = record(&reference, ORIGIN, 0x40);
    assert!(
        fresh.iter().all(|c| c.executions == 0),
        "at the reproducible power-on nothing has executed yet"
    );

    // ---- run to the routine's return, and no further (§4.5) --------------
    reference
        .run(Bound::Address {
            address: expected::ROUTINE_RETURN,
            within: 20_000,
        })
        .expect("the routine returns");

    // ---- the positive half ------------------------------------------------
    for pc in SETUP.chain(ROUTINE) {
        assert!(
            executed(&reference, pc) > 0,
            "{pc:#06X} is in the program this project wrote and it must have run"
        );
    }

    // ---- the negative half, which is the one that matters ----------------
    // Without this, a record that answered "executed" for every byte would pass
    // everything above. Two of these three blocks sit BETWEEN executed ones.
    for block in NEVER {
        for pc in block {
            assert_eq!(
                executed(&reference, pc),
                0,
                "{pc:#06X} is never reached before the routine returns, and the \
                 record must not claim it ran"
            );
        }
    }

    // ---- and it is execution, not fetching -------------------------------
    // The 65816 has no prefetch queue, but a counter that incremented on a read
    // of the instruction stream would say yes about the byte after the last one
    // executed. Both boundaries are checked, because one of them is the byte
    // after a JSR and the other the byte after an RTS.
    assert_eq!(executed(&reference, SETUP.end() + 1), 0, "after the JSR");
    assert_eq!(executed(&reference, ROUTINE.end() + 1), 0, "after the RTS");

    // ---- per byte, counted once per execution ----------------------------
    // The routine's loop runs 0x40 times. Its first byte and the operand that
    // follows must both show that, which says the unit is the byte and the
    // event is the instruction running — not "this byte was seen once".
    let loop_top = 0x8024;
    assert_eq!(
        executed(&reference, loop_top),
        expected::ROUTINE_LENGTH as u32,
        "the loop's own count"
    );
    assert_eq!(
        executed(&reference, loop_top + 1),
        expected::ROUTINE_LENGTH as u32,
        "and its operand, which is the same instruction"
    );

    // ---- the host's own reading does not count as execution --------------
    // The question that decides whether coverage is about the software or about
    // the tool. Reading a whole region through the API is the heaviest thing the
    // host does to memory.
    let before = executed(&reference, *ROUTINE.start());
    let _ = reference.read("program-rom").expect("a whole-region read");
    let _ = reference.read_span("program-rom", 0, 64).expect("a span read");
    let after = record(&reference, *ROUTINE.start(), 1)[0];
    assert_eq!(after.executions, before, "a debugger read is not an execution");
    assert_eq!(after.reads, 0, "nor a read by the software");

    // ---- "this run" is a reset and a reading, not a subtraction ----------
    let stamp_before = record(&reference, ORIGIN, 0x40)
        .iter()
        .map(|c| c.execute_stamp)
        .max()
        .expect("a stamp");
    reference.forget_access_counts();
    let cleared = record(&reference, ORIGIN, 0x40);
    assert!(
        cleared.iter().all(|c| c.executions == 0 && c.execute_stamp == 0),
        "the counts and the stamps clear together"
    );

    reference
        .run(Bound::Instructions(40))
        .expect("forty instructions");
    let again = record(&reference, ORIGIN, 0x40);
    let ran_again = again.iter().filter(|c| c.executions > 0).count();
    assert!(
        (1..0x40).contains(&ran_again),
        "forty instructions touch some of the program and not all of it: {ran_again}"
    );
    let stamp_after = again
        .iter()
        .map(|c| c.execute_stamp)
        .max()
        .expect("a stamp");
    assert!(
        stamp_after > stamp_before,
        "the counts reset and the clock does not ({stamp_after} after {stamp_before}), so a \
         stamp is comparable with other stamps and with nothing else"
    );
}
