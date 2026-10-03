//! What this backend declares, and whether the declaration is true — §7.3.
//!
//! A declaration nothing checks is a promise, and §7.3's whole purpose is that
//! the host can rely on one. So this file reads the declaration back and then
//! exercises what it can of it against the real library.
//!
//! # What this file does NOT cover
//!
//! Only one of the four declared capabilities is exercised here:
//! `stop-on-execution`, through `Bound::Address`. The other three —
//! `stop-on-write`, `writing-position` and `write-recency` — are declared on
//! the strength of the measurements recorded in `doc/backend.md`, each with the
//! numbers it produced, and this crate has no verb that uses them yet. §5.4's
//! localisation is what will, and the test that exercises two of them is the
//! one that comes with it.
//!
//! So a reader should take this file as establishing that the declaration is
//! *consistent with what the backend was measured doing*, and not as
//! establishing that every declared capability works today through a public
//! verb. That difference is recorded rather than smoothed over, because a
//! declaration is exactly the kind of claim that rots quietly.

use std::path::PathBuf;
use std::time::Duration;

use awaseru_core::{Bound, Capabilities, Capability, Platform, Reason};
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::{Reference, Startup};

const BUDGET: u64 = 20_000;

#[test]
fn the_declaration_says_what_was_measured_and_nothing_more() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-capabilities");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");

    let mut reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(30));

    // ---- the declaration, exactly ---------------------------------------
    let declared = reference.capabilities();
    assert_eq!(
        declared,
        Capabilities::of([
            Capability::StopOnExecution,
            Capability::StopOnWrite,
            Capability::WritingPosition,
            Capability::WriteRecency,
            Capability::ExecutionCoverage,
            Capability::InputReplay,
        ]),
        "M3's four, M6's fifth and Q14's sixth, and not a seventh: {declared}"
    );

    // ---- and what it refuses to claim -----------------------------------
    // What is left are routes that exist and have not been taken, which §7.3
    // says is not a declaration.
    //
    // Two names have left this list, each the only way a capability may:
    // `execution-coverage` in M6 and `input-replay` when §13's Q14 closed. The
    // second was the interesting one — the machine could do it for two
    // milestones while the host had no verb to ask, and declaring it then would
    // have let §7.3's gate pass an anchor whose log went unreplayed.
    for absent in [
        Capability::StopOnRead,
        Capability::CallAndReturnEvents,
        Capability::RegisterWrites,
    ] {
        assert!(
            !declared.has(absent),
            "`{absent}` is not declared, and must not be: {}",
            absent.means()
        );
        let cause = declared.require(absent).expect_err("absent");
        assert!(
            cause.to_string().contains(absent.name()),
            "the refusal names it: {cause}"
        );
    }

    // ---- the one this crate exercises today ------------------------------
    // A bound by address is `stop-on-execution` and nothing else, so arriving
    // at one is the declaration being true rather than a restatement of it.
    assert!(declared.has(Capability::StopOnExecution));
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
        "a declared `stop-on-execution` that did not stop would be a false declaration: {stop}"
    );

    // ---- and M6's, which is a declaration only because this runs ---------
    // The machine is at the routine's entry, so the setup before it has run and
    // the routine itself has not. Both halves, because a coverage that answered
    // the same thing everywhere would pass either one alone.
    assert!(declared.has(Capability::ExecutionCoverage));
    let covered = reference
        .coverage("program-rom", 0, 0x40)
        .expect("a declared capability answers");
    assert_eq!(covered.region, "program-rom");
    assert_eq!(covered.executions.len(), 0x40);
    assert_eq!(
        covered.ran_at(0),
        Some(true),
        "the first instruction of the program has run"
    );
    assert_eq!(
        covered.ran_at(usize::try_from(expected::ROUTINE_ENTRY - 0x8000).unwrap()),
        Some(false),
        "and the routine's own entry has not, because the machine is stopped ON it"
    );
    assert_eq!(
        covered.ran_at(0x40),
        None,
        "one byte past the span is not an answer about that byte (§2.3)"
    );
    assert!(
        covered.ran() > 0 && covered.untouched() > 0,
        "both halves are non-empty, or the assertions above are about a constant: \
         {} ran, {} did not",
        covered.ran(),
        covered.untouched()
    );
    let gaps = covered.never_ran();
    assert!(
        gaps.iter().any(|g| g.contains(&0x20)),
        "the routine's entry falls in a stretch nothing reached: {gaps:?}"
    );

    // And forgetting works through the verb, not only through the backend.
    reference.forget_coverage().expect("declared, so it answers");
    let cleared = reference.coverage("program-rom", 0, 0x40).expect("readable");
    assert_eq!(cleared.ran(), 0, "forgetting leaves nothing behind");
}
