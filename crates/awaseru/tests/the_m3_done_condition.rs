//! §M3's done-condition, on the generated fixture — where the assertions may
//! be about content, because the program is this project's (§11.3).
//!
//! Three claims, and each one is paired with its opposite, because a differ is
//! the easiest thing in this project to write vacuously:
//!
//! 1. **A deliberately wrong reimplementation is caught with the first
//!    differing offset named.** Two wrong ones, with two different first
//!    offsets — one that gets the first byte right and the second wrong, and one
//!    that is wrong from the first byte. A differ reporting a constant offset
//!    passes one and fails the other. And the right implementation agrees, so a
//!    differ that always differs fails too.
//! 2. **A vacuous comparison is reported as vacuous.** A span the routine never
//!    writes, with a candidate that matches it byte for byte. Every byte is
//!    equal and the honest answer is *not determined* — §2.2's lie-by-passing,
//!    which is the one this project exists to not tell.
//! 3. **A perturbation that should be noticed is noticed.** And one that should
//!    not is not: changing a byte past the input the routine reads must leave
//!    the verdict exactly where it was, and the tool must say it cannot
//!    discriminate it. A perturbation nobody would notice proves nothing, so
//!    both are here.
//!
//! One test, because only one reference may exist per process.
//!
//! # What this does NOT cover
//!
//! The done-condition is about a *generated* ROM, which is what this is. The
//! same cycle against supplied software is M1's done-condition and is
//! necessarily about plumbing rather than content — nothing here establishes
//! that the differ is right about software this project did not write.
//!
//! Nor does anything here exercise §5.5's cross-check, which is blocked, or
//! localisation from an anchor, since this routine is reached in a few hundred
//! instructions.

use std::path::PathBuf;
use std::time::Duration;

use awaseru::arrive::Arriver;
use awaseru::cache::Cache;
use awaseru::config::AnchorPolicy;
use awaseru::differ::{self, Report, Request};
use awaseru::perturb::Perturbation;
use awaseru::routine::{Given, Routine, Span};
use awaseru_core::anchor::Anchors;
use awaseru_core::snapshot::Provenance;
use awaseru_core::{Platform, Undetermined, Verdict, Wrote};
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::{Reference, Startup};

const BUDGET: u64 = 20_000;

/// A span the routine never writes, for the vacuous claim. Clear of the
/// input at `0x300`, the output at `0x400` and the counter the spin loop
/// increments at `0x10`.
const UNTOUCHED_AT: usize = 0x0600;

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

fn output_span() -> Span {
    Span::new(
        "work-ram",
        expected::ROUTINE_OUTPUT_AT,
        expected::ROUTINE_LENGTH,
    )
}

fn routine_writing(span: Span) -> Routine {
    Routine {
        name: "running-total".into(),
        entry: expected::ROUTINE_ENTRY,
        returns_to: expected::ROUTINE_RETURN,
        within: BUDGET,
        reaching: None,
        from: None,
        writes: vec![span],
    }
}

/// Where two implementations of the routine first part, and how many bytes
/// differ in all — computed from the fixture's own Rust versions rather than
/// written down, so that a change to either is a change to what the test
/// expects.
fn parting(right: &[u8], wrong: &[u8]) -> (usize, usize) {
    let first = right
        .iter()
        .zip(wrong)
        .position(|(a, b)| a != b)
        .expect("the wrong one must be wrong somewhere");
    let differing = right.iter().zip(wrong).filter(|(a, b)| a != b).count();
    (first, differing)
}

#[test]
fn the_differ_catches_a_wrong_reimplementation_reports_the_vacuous_and_notices_a_control() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-m3-done");
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

    let given = vec![Given {
        span: input_span(),
        bytes: input(),
    }];
    let right = expected::routine(&input());

    let examine = |arriver: &mut Arriver<'_>,
                   routine: &Routine,
                   produced: Vec<Vec<u8>>,
                   control: Option<&Perturbation>,
                   localise: bool|
     -> Report {
        differ::examine(
            arriver,
            &provenance,
            &Request {
                routine,
                given: &given,
                produced: &produced,
                control,
                coverage: None,
                localise,
            },
        )
        .expect("it examines")
    };

    let routine = routine_writing(output_span());

    // ---- claim 1a: wrong from the second byte --------------------------
    // The reimplementation that forgets to carry the running total forward.
    // Its first byte is right, which is what makes "the FIRST differing
    // offset" a claim worth testing: a differ that reported offset zero
    // whenever anything differed would pass claim 1b below and fail here.
    let wrong = expected::routine_without_the_chain(&input());
    let (first, differing) = parting(&right, &wrong);
    assert_eq!(first, 1, "the fixture's own description of this wrong one");

    let report = examine(&mut arriver, &routine, vec![wrong.clone()], None, true);
    eprintln!("a wrong reimplementation: {report}");
    match report.verdict() {
        Verdict::Differs(d) => {
            assert_eq!(
                d.first,
                expected::ROUTINE_OUTPUT_AT + first,
                "the first differing offset, against the region and not the span: {d}"
            );
            assert_eq!(d.expected, right[first], "what the reference produced");
            assert_eq!(d.found, wrong[first], "and what the candidate did");
            assert_eq!(d.differing, differing, "how many differ in all");
            assert_eq!(d.compared, expected::ROUTINE_LENGTH);
            assert!(
                differing < expected::ROUTINE_LENGTH,
                "this wrong one agrees somewhere, which is the point of it"
            );
        }
        other => panic!("a wrong reimplementation must be caught: {other}"),
    }

    // §5.4's third item, on the same finding: the instruction that wrote the
    // reference's value there.
    let located = report
        .localisation()
        .expect("localisation was asked for and there is a difference to localise");
    assert_eq!(
        located.wrote,
        Wrote::At {
            position: awaseru_core::Position::MidInstruction {
                pc: expected::ROUTINE_STORE
            },
            writes: 1,
        },
        "the routine's own store, and not the one after its return (§4.5): {located}"
    );
    assert_eq!(located.offset, expected::ROUTINE_OUTPUT_AT + first);
    assert_eq!(located.region, "work-ram");

    // ---- claim 1b: wrong from the first byte ---------------------------
    // The one that forgets the exclusive-or. Same routine, same inputs, a
    // different mistake — and the offset follows the mistake.
    let wrong = expected::routine_without_the_mask(&input());
    let (first_of_mask, _) = parting(&right, &wrong);
    assert_eq!(first_of_mask, 0, "the fixture's own description of this one");

    let report = examine(&mut arriver, &routine, vec![wrong.clone()], None, false);
    eprintln!("the other wrong reimplementation: {report}");
    match report.verdict() {
        Verdict::Differs(d) => {
            assert_eq!(
                d.first,
                expected::ROUTINE_OUTPUT_AT + first_of_mask,
                "a differ reporting a constant offset would have failed one of these two: {d}"
            );
            assert_eq!(d.found, wrong[first_of_mask]);
        }
        other => panic!("got {other}"),
    }

    // ---- claim 1c: and the right one agrees ----------------------------
    // Without this, every assertion above is passed by a differ that always
    // differs.
    let report = examine(
        &mut arriver,
        &routine,
        vec![right.clone()],
        None,
        true,
    );
    eprintln!("the right reimplementation: {report}");
    assert!(
        matches!(report.verdict(), Verdict::Agrees { .. }),
        "the right implementation of the fixture's own routine must agree: {report}"
    );
    assert_eq!(
        report.moved(),
        Some(expected::ROUTINE_LENGTH),
        "§5.2: the reference moved every byte it was compared over"
    );
    assert!(
        report.localisation().is_none(),
        "nothing to localise about an agreement"
    );

    // ---- claim 2: a vacuous comparison is reported as vacuous ----------
    // A span the routine never writes, and a candidate that matches it byte
    // for byte. Every compared byte is equal. "Agrees" here would be the lie
    // §2.2 describes — the one that passes.
    let untouched = routine_writing(Span::new(
        "work-ram",
        UNTOUCHED_AT,
        expected::ROUTINE_LENGTH,
    ));
    let report = examine(
        &mut arriver,
        &untouched,
        vec![vec![0u8; expected::ROUTINE_LENGTH]],
        None,
        true,
    );
    eprintln!("a vacuous comparison: {report}");
    assert_eq!(
        report.verdict(),
        Verdict::NotDetermined(Undetermined::Vacuous {
            compared: expected::ROUTINE_LENGTH
        }),
        "the reference changed none of these bytes, so agreement over them is agreement about \
         data neither side wrote: {report}"
    );
    assert!(
        !matches!(report.verdict(), Verdict::Agrees { .. }),
        "§2.3: never collapsed into agreement"
    );
    assert_eq!(
        report.moved(),
        None,
        "§5.2 has no count to report here, and reporting zero would be stating something else"
    );
    assert!(
        report.to_string().contains("neither side wrote"),
        "and it says why in words: {report}"
    );

    // ---- claim 3a: a perturbation that should be noticed is noticed -----
    let mut changed = input();
    changed[0] = changed[0].wrapping_add(1);
    let sensitive = Perturbation::new("the first input byte", input_span(), changed);
    let report = examine(
        &mut arriver,
        &routine,
        vec![right.clone()],
        Some(&sensitive),
        false,
    );
    eprintln!("with a control: {report}");
    assert_eq!(
        report.control().noticed(),
        Some(true),
        "every output byte depends on the first input byte: {report}"
    );
    assert!(
        report.complete(),
        "a control that discriminates is what makes a measurement complete (§5.3): {report}"
    );
    assert!(
        matches!(report.verdict(), Verdict::Agrees { .. }),
        "and the verdict itself is unchanged by the control: {report}"
    );

    // ---- claim 3b: and one that should not be, is not -------------------
    let past_the_end = Perturbation::new(
        "a byte past the input the routine reads",
        Span::new(
            "work-ram",
            expected::ROUTINE_INPUT_AT + expected::ROUTINE_LENGTH,
            1,
        ),
        vec![0xA5],
    );
    let report = examine(
        &mut arriver,
        &routine,
        vec![right.clone()],
        Some(&past_the_end),
        false,
    );
    eprintln!("with a control nobody would notice: {report}");
    assert_eq!(
        report.control().noticed(),
        Some(false),
        "the routine reads {} bytes and this changes the one after them: {report}",
        expected::ROUTINE_LENGTH
    );
    assert!(
        !report.complete(),
        "nothing has been shown about this comparison's sensitivity: {report}"
    );
    assert!(
        report.to_string().contains("incomplete"),
        "and the report says so rather than looking green: {report}"
    );
    assert!(
        matches!(report.verdict(), Verdict::Agrees { .. }),
        "§5.3 does not withdraw a finding: {report}"
    );
}
