//! §M6's done-condition: **a write to an address can be attributed to the
//! position that made it, and coverage distinguishes executed from unexecuted
//! ROM, on the generated fixtures.**
//!
//! Both halves, on a program this project assembled, so what wrote what and
//! what ran is known before the backend is asked.
//!
//! # Why the two halves are checked together and not only apart
//!
//! Each is already exercised on its own: `the_localisation.rs` attributes a
//! write, and `the_access_record.rs` measures the execute counter. What neither
//! can establish is that **they are talking about the same machine**.
//!
//! Write provenance says "the byte at this offset was written by the
//! instruction at *that* address". Coverage says "the instruction at that
//! address ran, and this other one did not". If the first names an address the
//! second says never executed, one of them is lying and no test that looks at
//! one at a time can see it. The fixture makes the cross-check sharp: the
//! output's first byte is written by two different instructions in the life of
//! the program, and a measurement bounded by the routine's return (§4.5)
//! reaches exactly one of them.
//!
//! One test, because only one reference may exist per process.
//!
//! # What this does NOT cover
//!
//! Coverage of code running outside the cartridge. The record keeps it —
//! `the_access_record_in_memory.rs` shows the region is covered — but this
//! fixture's program runs from ROM, so nothing here executes elsewhere. M5
//! measured real software doing it, and a fixture that copies a routine into
//! memory and calls it there is owed rather than written (§12).

use std::path::PathBuf;
use std::time::Duration;

use awaseru::arrive::Arriver;
use awaseru::cache::Cache;
use awaseru::config::AnchorPolicy;
use awaseru::localise::localise;
use awaseru::routine::{self, Given, Routine, Span};
use awaseru_core::anchor::Anchors;
use awaseru_core::snapshot::Provenance;
use awaseru_core::{Capability, Platform, Position, Wrote};
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::{Reference, Startup};

const BUDGET: u64 = 20_000;
/// Where the fixture's program is mapped, so that an address becomes an offset
/// into the region coverage is read from.
const ORIGIN: u64 = 0x8000;

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

fn offset_of(address: u64) -> usize {
    usize::try_from(address - ORIGIN).expect("the program is mapped at the origin")
}

#[test]
fn a_write_is_attributed_and_coverage_agrees_about_the_instruction() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-m6-done");
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

    // §7.3 first: both halves of this milestone are declared capabilities, and
    // a measurement that did not ask would be relying on a header.
    let declared = reference.capabilities();
    for needed in [
        Capability::StopOnWrite,
        Capability::WritingPosition,
        Capability::ExecutionCoverage,
    ] {
        assert!(
            declared.has(needed),
            "§M6 rests on `{needed}` and this backend must declare it: {declared}"
        );
    }

    fn provenance_again() -> Provenance {
        Provenance {
            reference: "fixture".into(),
            backend: "mesence".into(),
            version: String::new(),
            software: "the routine fixture".into(),
        }
    }

    let anchors = Anchors::new(vec![]).expect("none needed here");
    let cache = Cache::at(dir.join("cache"));
    let mut arriver = Arriver::new(
        &mut reference,
        &anchors,
        &cache,
        provenance,
        AnchorPolicy {
            verify_from_origin: 0,
            reverify_at_end: false,
        },
    );

    // ---- the measurement itself ------------------------------------------
    // Both halves below are about what THIS run did, so coverage is forgotten
    // first and the routine is run once. `localise` needs the run to have
    // happened as well: its cheap filter reads the backend's record of the
    // current machine, so on a machine where nothing has run it answers
    // `NothingWrote` — which is true of the machine and reads like a statement
    // about the routine (`doc/findings.md`).
    let routine = the_routine();
    let given = given();
    arriver.forget_coverage().expect("declared, so it answers");
    let measured = routine::measure(&mut arriver, &provenance_again(), &routine, &given)
        .expect("it measures");
    assert_eq!(
        measured
            .result
            .get("work-ram")
            .expect("the output was captured")
            .bytes(),
        expected::routine(&input()),
        "the fixture must do what this project says it does before anything is \
         attributed to it"
    );

    // ---- half two: coverage, of the plain measurement --------------------
    // Read BEFORE localising, because localising is a replay and would execute
    // the routine a second time.
    let covered = arriver
        .coverage("program-rom", 0, 0x40)
        .expect("declared, so it answers");
    assert!(
        covered.ran() > 0 && covered.untouched() > 0,
        "both halves non-empty, or everything below is about a constant: \
         {} ran, {} did not",
        covered.ran(),
        covered.untouched()
    );

    // ---- half one: the write, attributed -------------------------------
    // The output's first byte. In the life of this program it is written twice:
    // once by the routine's own store, and once by the instruction after the
    // routine returns, which overwrites it. Bounded by the return (§4.5), only
    // the first has happened.
    let located = localise(
        &mut arriver,
        &routine,
        &given,
        "work-ram",
        expected::ROUTINE_OUTPUT_AT,
    )
    .expect("it localises");

    assert_eq!(
        located.wrote,
        Wrote::At {
            position: Position::MidInstruction {
                pc: expected::ROUTINE_STORE
            },
            writes: 1,
        },
        "the routine's own store, and NOT {:#X}, which writes the same byte \
         after the measurement's subject has ended (§4.5)",
        expected::ROUTINE_CLOBBER_STORE
    );

    // ---- and the two agree about the same instruction -------------------
    // This is what neither half establishes alone. The address write
    // provenance named must be one coverage says ran; the address it refused
    // to name must be one coverage says did not.
    assert_eq!(
        covered.ran_at(offset_of(expected::ROUTINE_STORE)),
        Some(true),
        "the instruction credited with the write must be one that ran"
    );
    assert_eq!(
        covered.ran_at(offset_of(expected::ROUTINE_CLOBBER_STORE)),
        Some(false),
        "and the one §4.5 kept out of the answer must be one that did not — if \
         this ran, the measurement overshot its subject and the attribution \
         above was right by luck"
    );

    // The gap containing the clobber is a real stretch and not a single byte,
    // so a coverage that happened to be wrong about one address could not
    // produce it.
    let gaps = covered.never_ran();
    assert!(
        gaps.iter()
            .any(|g| g.contains(&offset_of(expected::ROUTINE_CLOBBER_STORE))),
        "the clobber falls inside a stretch nothing reached: {gaps:?}"
    );
    assert!(
        gaps.iter()
            .all(|g| !g.contains(&offset_of(expected::ROUTINE_STORE))),
        "and the routine's store falls inside none of them: {gaps:?}"
    );

    // ---- the negative half of attribution --------------------------------
    // A byte nothing wrote is `NothingWrote`, not an instruction. Without this,
    // a localisation that always named the last store would pass everything
    // above. The byte chosen is in the region the routine writes nothing to.
    let untouched = localise(
        &mut arriver,
        &routine,
        &given,
        "work-ram",
        expected::ROUTINE_OUTPUT_AT + expected::ROUTINE_LENGTH + 0x40,
    )
    .expect("it answers");
    assert_eq!(
        untouched.wrote,
        Wrote::NothingWrote,
        "a byte the routine never wrote has no position to name, and inventing \
         one would be the silent wrong answer §2.3 refuses"
    );
}
