//! **M1's done-condition**, on software this project owns.
//!
//! > A snapshot round-trips — read, seed, read again, identical bytes — and a
//! > comparison over an unexposed region reports *not determined*.
//!
//! Run against the generated fixture (§11.3), so the assertions can be about
//! content: the program is this project's and so are its expected values. The
//! same round trip against supplied software is in `the_reference_runs`, where
//! the assertions are about plumbing only (§11.2).
//!
//! # Why there is a disturbance in the middle
//!
//! Because read-seed-read is the easiest test in the world to write
//! vacuously. A `seed` that does nothing and a `read` that returns a cached
//! buffer both pass it. So the machine is deliberately put somewhere it
//! demonstrably is not between the two reads — a different pattern in memory
//! and a different processor state — and the test asserts that the
//! disturbance was really there before asserting that it is gone.

use awaseru_core::snapshot::{Processor, Provenance};
use awaseru_core::{Bound, Comparison, Platform, Position, Undetermined, Verdict};
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::Reference;
use std::path::PathBuf;

/// Both snapshots carry the same provenance, which is what makes them
/// comparable (§16.5). The software's identity is a fixed string here rather
/// than a hash: hashing is the host's and this test is below it, and what the
/// check needs is that the two sides agree.
fn provenance(version: String) -> Provenance {
    Provenance {
        reference: "fixture".into(),
        backend: "mesence".into(),
        version,
        software: "the generated fixture".into(),
    }
}

const CAPTURED: [&str; 2] = ["work-ram", "palette-ram"];

#[test]
fn a_snapshot_round_trips_and_an_unexposed_region_is_not_determined() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-round-trip");
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("fixture.sfc");
    std::fs::write(&rom, fixture::image()).expect("write the image");

    let mut reference = Reference::open(&library, &dir, &rom).expect("the fixture loads");
    let provenance = provenance(reference.version().reported);

    // Far enough in that the fixture's fills are done, then one instruction so
    // that the position is a boundary a state can be written at (§3.4). A
    // frame boundary would be refused, and the end of this test checks that it
    // is.
    reference.run(Bound::Frames(3)).expect("three frames");
    let stop = reference.run(Bound::Instructions(1)).expect("one instruction");
    let position = stop.position.clone();
    assert!(
        position.is_instruction_boundary(),
        "a run bounded by instructions must end somewhere a state can be seeded, ended at \
         {position}"
    );

    // ---- read -------------------------------------------------------------
    let before = awaseru_core::capture(&reference, provenance.clone(), position.clone(), &CAPTURED)
        .expect("it captures");

    // The fixture's own patterns, which is the thing a generated fixture buys:
    // these are assertions about content, and the content is ours (§11.3).
    let work = before.get("work-ram").expect("captured").bytes();
    let at = expected::WORK_PATTERN_AT;
    let pattern = expected::work_pattern();
    assert_eq!(
        &work[at..at + pattern.len()],
        &pattern[..],
        "the snapshot must hold what the program wrote"
    );
    assert_eq!(
        before.get("palette-ram").expect("captured").bytes()[..256],
        expected::palette_pattern()[..],
        "and the palette too"
    );
    assert!(
        before.can_be_seeded(),
        "a snapshot at an instruction boundary is seedable (§3.4)"
    );

    // ---- disturb ----------------------------------------------------------
    // Everything the snapshot carries is moved somewhere else, so that a seed
    // which does nothing cannot pass what follows.
    // **Every** region the snapshot carries, not just one. The first version
    // of this test disturbed only work memory, and the comparison below came
    // back *not determined* — correctly, because nothing had moved in the
    // palette and §2.2 calls agreement over untouched bytes vacuous. The
    // machinery caught the test, which is the right way round.
    let noise: Vec<u8> = (0..work.len()).map(|i| (i * 31 + 7) as u8).collect();
    assert_ne!(noise, work, "the noise must differ from what is there");
    reference.write("work-ram", &noise).expect("disturb memory");

    let palette_held = before.get("palette-ram").expect("captured").bytes().to_vec();
    let palette_noise: Vec<u8> = (0..palette_held.len())
        .map(|i| ((i * 13 + 3) as u8) & 0x7F)
        .collect();
    assert_ne!(palette_noise, palette_held);
    reference
        .write("palette-ram", &palette_noise)
        .expect("disturb the palette");

    let original_processor = before.processor().expect("captured").clone();
    let mut altered = original_processor.bytes().to_vec();
    for byte in &mut altered {
        *byte ^= 0xFF;
    }
    reference
        .write_processor(&Processor::opaque(altered))
        .expect("disturb the registers");

    let disturbed =
        awaseru_core::capture(&reference, provenance.clone(), position.clone(), &CAPTURED)
            .expect("it captures");
    assert_ne!(
        disturbed.get("work-ram").expect("captured").bytes(),
        work,
        "the disturbance is really in memory"
    );
    assert_ne!(
        disturbed.processor().expect("captured").bytes(),
        original_processor.bytes(),
        "and in the registers"
    );

    // ---- seed -------------------------------------------------------------
    awaseru_core::seed(&mut reference, &before).expect("it seeds");

    // ---- read again -------------------------------------------------------
    let after = awaseru_core::capture(&reference, provenance.clone(), position.clone(), &CAPTURED)
        .expect("it captures");

    for name in CAPTURED {
        assert_eq!(
            after.get(name).expect("captured").bytes(),
            before.get(name).expect("captured").bytes(),
            "`{name}` did not come back the same"
        );
    }
    assert_eq!(
        after.processor().expect("captured").bytes(),
        original_processor.bytes(),
        "the registers did not come back the same"
    );
    assert_ne!(
        after.get("work-ram").expect("captured").bytes(),
        &noise[..],
        "and the disturbance is gone, which is what says the seed did something"
    );

    // ---- the same thing through the comparison machinery ------------------
    // Seeded with the disturbed state, so `moved` is the distance between
    // where the machine was put and where the snapshot says it should be —
    // non-zero, which is what makes this comparison mean anything (§2.2).
    let verdict = awaseru_core::compare_regions(
        Comparison {
            seed: &disturbed,
            reference: &before,
            candidate: &after,
        },
        &CAPTURED,
    )
    .expect("same provenance, so comparable");
    match verdict {
        Verdict::Agrees { compared, moved } => {
            assert!(moved > 0, "a comparison over nothing moved is vacuous (§2.2)");
            assert!(compared > 0);
            eprintln!("round trip agrees over {compared} bytes, of which {moved} moved");
        }
        other => panic!("the round trip should agree, got {other}"),
    }

    // ---- an unexposed region is not determined ---------------------------
    // The other half of the done-condition. `nowhere` is a name this backend
    // does not expose, and §3.5 turns that into a verdict rather than an error.
    let verdict = awaseru_core::compare_regions(
        Comparison {
            seed: &disturbed,
            reference: &before,
            candidate: &after,
        },
        &["nowhere"],
    )
    .expect("comparable");
    assert_eq!(
        verdict,
        Verdict::NotDetermined(Undetermined::RegionAbsent {
            region: "nowhere".into()
        }),
        "a comparison over a region the backend does not expose must be not determined, never \
         agreement"
    );
    assert_ne!(
        verdict,
        Verdict::Agrees {
            compared: 0,
            moved: 0
        }
    );

    // ---- §3.4, against the real thing ------------------------------------
    // A snapshot taken at a frame boundary cannot be written back, and the
    // refusal happens before anything is written.
    let stop = reference.run(Bound::Frames(1)).expect("one frame");
    assert!(matches!(stop.position, Position::FrameBoundary { .. }));
    let at_frame = awaseru_core::capture(
        &reference,
        provenance,
        stop.position.clone(),
        &["work-ram"],
    )
    .expect("it captures");
    assert!(!at_frame.can_be_seeded());

    let held = reference.read("work-ram").expect("it reads");
    let err = awaseru_core::seed(&mut reference, &at_frame).expect_err("not seedable");
    assert!(
        err.to_string().contains("resume a blob"),
        "the refusal must say what to do instead, said: {err}"
    );
    assert_eq!(
        reference.read("work-ram").expect("it reads"),
        held,
        "a refused seed must leave the machine untouched"
    );
}
