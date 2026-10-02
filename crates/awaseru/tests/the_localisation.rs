//! §5.4's third item, against the real reference — and §4.5 again, one level
//! down.
//!
//! The fixture's routine has exactly one store, and the instruction after the
//! routine's return has another one that writes over the output's first byte.
//! So the output's first byte is written **twice** in the life of the program,
//! by two instructions four addresses apart, and which of the two a
//! localisation names is the whole question:
//!
//! - bounded to the return, as §4.5 requires, the answer is the routine's own
//!   store;
//! - one instruction further and the answer is the clobber, which has nothing
//!   to do with the routine being measured and is indistinguishable from a
//!   correct answer unless you already knew.
//!
//! One test, because only one reference may exist per process.
//!
//! # What this does NOT cover
//!
//! A byte written more than once *inside* one routine: this fixture's routine
//! writes each output byte once, so `writes > 1` and the hedge that comes with
//! it are exercised only in the unit tests. A routine with a loop that revisits
//! its output would be a second fixture, and nothing in M3 needs one.
//!
//! Nor does it cover localisation with an anchor in front of it: the routine is
//! reached in a few hundred instructions, so `from` is `None` here. The replay
//! machinery underneath is M2's and is tested there.

use std::path::PathBuf;
use std::time::Duration;

use awaseru::arrive::Arriver;
use awaseru::cache::Cache;
use awaseru::config::AnchorPolicy;
use awaseru::localise::localise;
use awaseru::routine::{self, Given, Routine, Span};
use awaseru_core::anchor::Anchors;
use awaseru_core::snapshot::Provenance;
use awaseru_core::{Capability, Difference, Platform, Position, Recency, Wrote};
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::{Reference, Startup};

const BUDGET: u64 = 20_000;

fn input() -> Vec<u8> {
    (0..expected::ROUTINE_LENGTH)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(3))
        .collect()
}

fn the_routine() -> Routine {
    Routine {
        name: "running-total".into(),
        entry: expected::ROUTINE_ENTRY,
        returns_to: expected::ROUTINE_RETURN,
        within: BUDGET,
        from: None,
        writes: vec![Span::new(
            "work-ram",
            expected::ROUTINE_OUTPUT_AT,
            expected::ROUTINE_LENGTH,
        )],
    }
}

fn given() -> Vec<Given> {
    vec![Given {
        span: Span::new(
            "work-ram",
            expected::ROUTINE_INPUT_AT,
            expected::ROUTINE_LENGTH,
        ),
        bytes: input(),
    }]
}

#[test]
fn a_difference_is_localised_to_the_instruction_that_wrote_it() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-localisation");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");

    let mut reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(30));
    let provenance = Provenance {
        reference: "fixture".into(),
        backend: "mesence".into(),
        version: reference.version().reported,
        software: "the routine fixture".into(),
    };

    // The declaration first: everything below rests on it, and §7.3 says the
    // host asks rather than assumes.
    let declared = reference.capabilities();
    assert!(
        declared.has(Capability::StopOnWrite) && declared.has(Capability::WritingPosition),
        "this backend was measured doing both: {declared}"
    );

    let anchors = Anchors::new(vec![]).expect("none needed");
    let cache = Cache::at(dir.join("cache"));
    let mut arriver = Arriver::new(
        &mut reference,
        &anchors,
        &cache,
        provenance.clone(),
        AnchorPolicy {
            verify_from_origin: 0,
            reverify_after: 0,
        },
    );

    let routine = the_routine();
    let given = given();

    // ---- a measurement, so there is something to localise ----------------
    let measured =
        routine::measure(&mut arriver, &provenance, &routine, &given).expect("it measures");
    let produced = measured
        .result
        .get("work-ram")
        .expect("the output was captured")
        .bytes()
        .to_vec();
    assert_eq!(
        produced,
        expected::routine(&input()),
        "the routine must do what the Rust version says before anything is localised"
    );

    // ---- the cheap filter, on a byte and on a neighbour -------------------
    // The output was just written, so the backend has a record for it. The
    // byte after the output span was not, which is the filter's own
    // discriminating case.
    let written = arriver
        .write_recency("work-ram", expected::ROUTINE_OUTPUT_AT)
        .expect("a reading");
    assert!(
        matches!(written, Recency::Stamp(_)),
        "a byte the routine just wrote must carry a write record: {written}"
    );
    let untouched = arriver
        .write_recency(
            "work-ram",
            expected::ROUTINE_OUTPUT_AT + expected::ROUTINE_LENGTH + 0x40,
        )
        .expect("a reading");
    assert_eq!(
        untouched,
        Recency::NeverWritten,
        "a byte nothing has written must say so, or the filter filters nothing"
    );

    // ---- §5.4's third item, and §4.5 deciding which of two stores ---------
    let located = localise(
        &mut arriver,
        &routine,
        &given,
        "work-ram",
        expected::ROUTINE_OUTPUT_AT,
    )
    .expect("it localises");
    eprintln!("localised: {located}");
    assert!(located.replayed, "the exact answer costs a replay: {located}");
    assert_eq!(
        located.wrote,
        Wrote::At {
            position: Position::MidInstruction {
                pc: expected::ROUTINE_STORE
            },
            writes: 1,
        },
        "the routine's own store, named exactly — and not {:#X}, the store after the return, \
         which a measurement that ran past its subject would have found instead (§4.5)",
        expected::ROUTINE_CLOBBER_STORE
    );

    // A write is caught before it commits, so the position is mid-instruction
    // — the exact answer, and a place §3.4 forbids seeding from. A localisation
    // reporting an instruction boundary here would be reporting a position
    // somebody could try to start a comparison from.
    assert!(
        !located.wrote_at_an_instruction_boundary(),
        "a write is caught during the instruction performing it: {located}"
    );

    // ---- a byte in the same region that the routine does not write --------
    // The input span. Not an absence of information: the answer is that
    // nothing wrote it, which for a difference there would mean the
    // reimplementation wrote something the reference never did.
    let not_written = localise(
        &mut arriver,
        &routine,
        &given,
        "work-ram",
        expected::ROUTINE_INPUT_AT,
    )
    .expect("it localises");
    eprintln!("the input span: {not_written}");
    assert_eq!(
        not_written.wrote,
        Wrote::NothingWrote,
        "the routine reads its input and never writes it"
    );
    // And the filter earned its place: the answer cost no replay. The input
    // span was seeded through the debugger, and a debugger write leaves no
    // write record (`doc/backend.md`), so the filter can settle it — which is
    // what makes it a filter rather than a second opinion.
    assert!(
        !not_written.replayed,
        "the cheap filter must settle a byte nothing wrote: {not_written}"
    );

    // ---- the same thing, from a difference ------------------------------
    // `Difference::first` is an offset into its region, which is what
    // `localise_difference` passes through. Built by hand here: U10's
    // done-condition test is where a real comparison produces one.
    let difference = Difference::new(expected::ROUTINE_OUTPUT_AT, 0x01, 0x02, 1, 0x40);
    let from_difference = awaseru::localise::localise_difference(
        &mut arriver,
        &routine,
        &given,
        "work-ram",
        &difference,
    )
    .expect("it localises");
    assert_eq!(
        from_difference.wrote, located.wrote,
        "the same byte, asked for the other way round, must give the same answer"
    );

    // And a difference carrying it prints both halves of §5.4 in one line.
    let said = difference.localised(from_difference.wrote.clone()).to_string();
    assert!(said.contains("first at 1024"), "said: {said}");
    assert!(said.contains("802C"), "said: {said}");
}
