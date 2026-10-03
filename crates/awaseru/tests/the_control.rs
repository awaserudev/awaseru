//! §5.3's control, against the real reference.
//!
//! Two perturbations, and the second is the one that makes the first mean
//! anything:
//!
//! - **the first input byte.** The routine is a running total, so every output
//!   byte depends on the first input byte. Changing it must move the verdict,
//!   and a comparison that did not notice would be a comparison measuring
//!   nothing.
//! - **a byte past the input the routine reads.** The routine reads sixty-four
//!   bytes; this changes the sixty-fifth. The verdict must stay exactly where
//!   it was, and the tool must say that it cannot discriminate this — which is
//!   §5.3's own sentence and not a failure.
//!
//! A test with only the first would pass against a control that always reports
//! "noticed". A test with only the second would pass against one that always
//! reports "not noticed". Both halves, or neither is evidence.
//!
//! One test, because only one reference may exist per process.
//!
//! # What this does NOT cover
//!
//! A perturbation that makes the comparison *vacuous* rather than merely
//! different — the unit tests cover that reading of `noticed`, and arranging it
//! on the reference would need a routine whose output can be made not to move,
//! which this fixture's cannot: it writes all sixty-four bytes every time.
//!
//! Nor does it cover a control over several perturbations at once. §5.3 asks
//! for "a named input changed", singular, and a set of them is a design
//! question nothing has needed yet.

use std::path::PathBuf;
use std::time::Duration;

use awaseru::arrive::Arriver;
use awaseru::cache::Cache;
use awaseru::config::AnchorPolicy;
use awaseru::perturb::{self, Control, Perturbation};
use awaseru::routine::{self, Given, Routine, Span};
use awaseru_core::anchor::Anchors;
use awaseru_core::snapshot::Provenance;
use awaseru_core::{Platform, Verdict};
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
        reaching: None,
        from: None,
        writes: vec![Span::new(
            "work-ram",
            expected::ROUTINE_OUTPUT_AT,
            expected::ROUTINE_LENGTH,
        )],
    }
}

fn input_span() -> Span {
    Span::new(
        "work-ram",
        expected::ROUTINE_INPUT_AT,
        expected::ROUTINE_LENGTH,
    )
}

#[test]
fn a_control_that_should_be_noticed_is_and_one_that_should_not_is_not() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-control");
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
    let anchors = Anchors::new(vec![]).expect("none needed");
    let cache = Cache::at(dir.join("cache"));
    let mut arriver = Arriver::new(
        &mut reference,
        &anchors,
        &cache,
        provenance.clone(),
        AnchorPolicy {
            verify_from_origin: 0,
            reverify_at_end: false,
        },
    );

    let routine = the_routine();
    let given = vec![Given {
        span: input_span(),
        bytes: input(),
    }];

    // ---- a right candidate, so the plain verdict is agreement -------------
    let measured =
        routine::measure(&mut arriver, &provenance, &routine, &given).expect("it measures");
    let candidate = measured
        .candidate(&[expected::routine(&input())])
        .expect("the same shape as the result");

    // ---- the control that must be noticed --------------------------------
    // The first input byte, which every output byte depends on.
    let mut changed = input();
    changed[0] = changed[0].wrapping_add(1);
    let sensitive = Perturbation::new("the first input byte", input_span(), changed);

    let noticed = perturb::control(
        &mut arriver,
        &provenance,
        &routine,
        &given,
        &candidate,
        &sensitive,
    )
    .expect("it runs");
    eprintln!("{noticed}");
    assert_eq!(
        noticed.noticed(),
        Some(true),
        "every output byte depends on the first input byte: {noticed}"
    );
    assert!(noticed.discriminates());
    match &noticed {
        Control::Ran {
            plain, perturbed, ..
        } => {
            assert!(
                matches!(plain.verdict, Verdict::Agrees { .. }),
                "the candidate is the right implementation, so the plain verdict agrees: {plain}"
            );
            assert!(
                matches!(perturbed.verdict, Verdict::Differs(_)),
                "and with the input changed the reference must leave the candidate behind: \
                 {perturbed}"
            );
            // The region travels with the difference, so a client reading a
            // control can place the offset it is given.
            assert!(
                perturbed.region.is_some(),
                "a difference this tool produced names its region: {perturbed:?}"
            );
            assert!(
                plain.region.is_none(),
                "and agreement has none to name: {plain:?}"
            );
        }
        Control::NotRun => panic!("a control was run"),
    }

    // ---- the control that must NOT be noticed ----------------------------
    // The byte just past the input the routine reads. Changing it is a real
    // change to the machine and no change to the routine's answer, so the
    // verdict must not move — and the tool must say it cannot discriminate it.
    let past_the_end = Perturbation::new(
        "a byte past the input the routine reads",
        Span::new(
            "work-ram",
            expected::ROUTINE_INPUT_AT + expected::ROUTINE_LENGTH,
            1,
        ),
        vec![0xA5],
    );

    let unnoticed = perturb::control(
        &mut arriver,
        &provenance,
        &routine,
        &given,
        &candidate,
        &past_the_end,
    )
    .expect("it runs");
    eprintln!("{unnoticed}");
    assert_eq!(
        unnoticed.noticed(),
        Some(false),
        "the routine reads {} bytes and this changes the one after them: {unnoticed}",
        expected::ROUTINE_LENGTH
    );
    assert!(
        !unnoticed.discriminates(),
        "and nothing about this comparison has been shown by it"
    );
    let said = unnoticed.to_string();
    assert!(said.contains("NOT noticed"), "said: {said}");
    assert!(said.contains("cannot discriminate"), "said: {said}");
    assert!(
        said.contains("past the input the routine reads"),
        "an unnoticed control must name itself: {said}"
    );

    // ---- and the absence, which is not either of those -------------------
    // §5.3's last sentence. Nothing above produces this; a report that carries
    // no control carries this, and it must not read like an unnoticed one.
    let none = Control::NotRun;
    assert_eq!(none.noticed(), None);
    assert_ne!(none.to_string(), said);
}
