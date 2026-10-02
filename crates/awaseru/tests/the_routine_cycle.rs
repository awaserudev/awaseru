//! §5.6's cycle, as the tool offers it — and §4.5 kept by the tool rather than
//! by whoever calls it.
//!
//! Run against the generated fixture with a routine in it, so the assertions
//! may be about content (§11.3).
//!
//! # The assertion this file exists for
//!
//! The fixture's routine is followed by an instruction that writes `0xFF` over
//! the output's first byte. A measurement bounded to the return sees the
//! routine's answer. A measurement that ran one instruction further would see
//! the `0xFF` — and would report a difference at offset 0 that has nothing to
//! do with the routine. That is §4.5, and the test checks both sides of it.

use std::path::PathBuf;
use std::time::Duration;

use awaseru::arrive::Arriver;
use awaseru::cache::Cache;
use awaseru::config::AnchorPolicy;
use awaseru::routine::{self, Given, Routine, Span};
use awaseru_core::anchor::Anchors;
use awaseru_core::run::{Bound, Reason};
use awaseru_core::snapshot::Provenance;
use awaseru_core::Platform;
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
        // No anchor: this fixture reaches its routine in a few hundred
        // instructions, so an anchor would cost more than it saves. §4.10's
        // last piece of guidance, taken.
        from: None,
        writes: vec![Span::new(
            "work-ram",
            expected::ROUTINE_OUTPUT_AT,
            expected::ROUTINE_LENGTH,
        )],
    }
}

#[test]
fn a_routine_is_measured_between_its_entry_and_its_return() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-routine-cycle");
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
        AnchorPolicy::default(),
    );

    let input = input();
    let routine = the_routine();
    let given = vec![Given {
        span: Span::new("work-ram", expected::ROUTINE_INPUT_AT, input.len()),
        bytes: input.clone(),
    }];

    // ---- not reached is not measured --------------------------------------
    // First, because the refusal leaves the machine one instruction along and
    // the measurement below can still get to the routine from there. One test
    // in this file rather than two, because only one reference may exist per
    // process and the suite runs its test binaries in parallel — which is how
    // this was found.
    let mut unreachable = routine.clone();
    unreachable.within = 1;
    let err = routine::measure(&mut arriver, &provenance, &unreachable, &[])
        .expect_err("one instruction cannot reach a routine a few hundred away");
    match &err {
        routine::Error::NeverEntered { routine, .. } => assert_eq!(routine, "running-total"),
        other => panic!("got {other}"),
    }
    assert!(
        err.to_string().contains("different thing from measuring it"),
        "the message must say why an empty measurement would be worse, said: {err}"
    );

    // ---- and now the measurement itself -----------------------------------
    let measured = routine::measure(&mut arriver, &provenance, &routine, &given)
        .expect("the routine is measured");

    // ---- it stopped where §4.5 says ---------------------------------------
    assert_eq!(
        measured.stop.reason,
        Reason::AddressHit {
            address: expected::ROUTINE_RETURN
        },
        "a measurement of a routine ends at that routine's return: {}",
        measured.stop
    );
    assert!(
        measured.caveat().is_none(),
        "no anchor was used, so there is no anchor caveat to carry: {:?}",
        measured.caveat()
    );

    // ---- the seed is what the routine began from -------------------------
    let seed = measured
        .seed
        .get("work-ram")
        .expect("the written span was captured");
    assert_eq!(seed.offset, expected::ROUTINE_OUTPUT_AT);
    assert_eq!(seed.len(), expected::ROUTINE_LENGTH);
    assert!(
        seed.bytes().iter().all(|&b| b == 0),
        "the output span before the routine ran is the power-on zeros, which is what makes \
         §5.2's movement non-zero afterwards"
    );

    // ---- and the result is the routine's answer ---------------------------
    let result = measured
        .result
        .get("work-ram")
        .expect("captured")
        .bytes()
        .to_vec();
    assert_eq!(
        result,
        expected::routine(&input),
        "the measured result must be what the routine does"
    );
    assert_ne!(
        result[0],
        expected::ROUTINE_CLOBBER,
        "and must NOT be what the instruction after the return writes — this is §4.5 holding"
    );

    // ---- §4.5, from the other side ---------------------------------------
    // One instruction past the return, and the output's first byte is ruined.
    // So a measurement that did not stop at the return would report a
    // difference at offset 0 about something the routine never did.
    let stop = arriver
        .run(Bound::Instructions(2))
        .expect("two instructions more");
    assert!(stop.arrived(), "{stop}");
    let ruined = arriver
        .read("work-ram")
        .expect("it reads")[expected::ROUTINE_OUTPUT_AT];
    assert_eq!(
        ruined,
        expected::ROUTINE_CLOBBER,
        "two instructions past the return the output really is ruined, so the assertion above \
         is about something that could have gone wrong"
    );

    eprintln!(
        "measured `{}` in {:?}; {} bytes captured before and after; past the return the first \
         byte becomes {:#04x}",
        measured.routine,
        measured.took,
        expected::ROUTINE_LENGTH,
        expected::ROUTINE_CLOBBER
    );
}
