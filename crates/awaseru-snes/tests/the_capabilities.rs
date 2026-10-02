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
        ]),
        "the four measured in M3's first unit, and not a fifth: {declared}"
    );

    // ---- and what it refuses to claim -----------------------------------
    // `input-replay` is the one absent in the machine rather than in this
    // crate: the library exposes no control device for an input to arrive at
    // (§13's Q14). The other three are routes that exist and have not been
    // taken, which §7.3 says is not a declaration.
    for absent in [
        Capability::InputReplay,
        Capability::StopOnRead,
        Capability::ExecutionCoverage,
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
}
