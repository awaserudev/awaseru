//! The differ as one thing, against the real reference — §5 entire.
//!
//! One `examine`, with a right reimplementation and a control that should be
//! noticed, and the assertions are about the report being **whole**: §5.1's
//! verdict, §5.2's movement, §5.3's control and §4.12's beginning, all present
//! and all from the same measurement.
//!
//! §5.4's localisation is absent here and that is the correct answer: there is
//! nothing to localise about an agreement. The report says so by carrying
//! `None`, and the test asserts that rather than letting it pass unexamined.
//!
//! One test, because only one reference may exist per process.
//!
//! # What this does NOT cover
//!
//! The three claims of §M3's done-condition, which are the next unit's — a
//! wrong reimplementation, a vacuous comparison and a perturbation that should
//! be noticed. This one covers the shape of the report and that the happy path
//! produces a verdict that stands.
//!
//! Nor does it cover a report whose measurement did not repeat (§2.5): both
//! readings here agree, which is what a working reference does, and arranging
//! the other case on a real backend would mean breaking the reference on
//! purpose. The unit tests cover the reading.

use std::path::PathBuf;
use std::time::Duration;

use awaseru::arrive::Arriver;
use awaseru::cache::Cache;
use awaseru::config::AnchorPolicy;
use awaseru::differ::{self, Request};
use awaseru::perturb::Perturbation;
use awaseru::routine::{Given, Routine, Span};
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

fn input_span() -> Span {
    Span::new(
        "work-ram",
        expected::ROUTINE_INPUT_AT,
        expected::ROUTINE_LENGTH,
    )
}

#[test]
fn one_examination_answers_every_part_of_section_five() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-report");
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

    let routine = Routine {
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
    };
    let given = vec![Given {
        span: input_span(),
        bytes: input(),
    }];
    let produced = vec![expected::routine(&input())];

    let mut changed = input();
    changed[0] = changed[0].wrapping_add(1);
    let control = Perturbation::new("the first input byte", input_span(), changed);

    let report = differ::examine(
        &mut arriver,
        &provenance,
        &Request {
            routine: &routine,
            given: &given,
            produced: &produced,
            control: Some(&control),
            localise: true,
            coverage: None,
        },
    )
    .expect("it examines");
    eprintln!("{report}");

    // §5.1 — and it stands: no caveat, a beginning that repeats, and the same
    // measurement twice gave the same answer (§2.5).
    assert!(
        matches!(report.verdict(), Verdict::Agrees { .. }),
        "the right reimplementation must agree, and the verdict must survive everything that \
         bears on it: {report}"
    );
    assert_eq!(
        &report.verdict(),
        report.as_compared(),
        "nothing should have taken this verdict away from what the bytes said"
    );

    // §5.2 — reported always, not on request.
    assert_eq!(
        report.moved(),
        Some(expected::ROUTINE_LENGTH),
        "the routine writes every byte of its output, so the reference moved all of them"
    );

    // §5.4 — nothing to localise, said as `None` rather than as a blank.
    assert!(
        report.localisation().is_none(),
        "there is nothing to localise about an agreement: {:?}",
        report.localisation()
    );

    // §5.3 — run, and noticed, so the measurement is complete.
    assert_eq!(report.control().noticed(), Some(true), "{report}");
    assert!(
        report.complete(),
        "a control that discriminates is what makes a measurement complete (§5.3): {report}"
    );

    // §4.12 — next to the result, not in a footnote.
    assert!(
        report.beginning().repeats(),
        "the origin settles memory, so runs from it repeat: {}",
        report.beginning()
    );

    // And the whole thing in one line, with every part in it.
    let said = report.to_string();
    for part in [
        "running-total",
        "agrees",
        "the first input byte",
        "The reference began",
    ] {
        assert!(said.contains(part), "the report must carry `{part}`: {said}");
    }
    assert!(
        !said.contains("incomplete"),
        "this measurement is complete: {said}"
    );
}
