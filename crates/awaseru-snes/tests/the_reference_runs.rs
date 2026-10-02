//! Does this crate actually drive a reference and read it?
//!
//! # Why this is one test
//!
//! The backend's emulator is a single global object (see `reference`'s header),
//! so only one reference can exist in a process. `cargo test` runs the tests in
//! one binary on several threads, so a second test that opened a reference
//! would either collide with this one or spend its time refusing. Everything
//! that needs a running reference is therefore one test, run in order, and the
//! checks that need no reference live in the unit tests beside the code.
//!
//! # What it asserts, and what it refuses to assert
//!
//! The software this runs is supplied by whoever runs it and is **not** part of
//! this repository (§11.2). So nothing here says what is at any address, or what
//! the software does. The assertions are about the plumbing:
//!
//! - a region exists, and holds the number of bytes the backend declares;
//! - a read of it returns that many bytes, and they are not all zero;
//! - a span is the part of the whole it claims to be;
//! - a bound of *n* frames advances the frame counter by exactly *n*;
//! - a run advances the processor's cycle count;
//! - a frame boundary does not claim to be an instruction boundary;
//! - the refusals refuse.
//!
//! Only the second of those could be said to depend on the software at all, and
//! what it depends on is that a console with memory in it has something in its
//! memory.

use awaseru_core::snapshot::{Processor, Provenance};
use awaseru_core::{
    Blob, Bound, Comparison, Platform, Position, ReadError, Reason, StateError, Undetermined,
    Verdict, WriteError,
};
use awaseru_snes::{OpenError, Reference};
use std::path::PathBuf;
use std::time::Duration;

fn from_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

#[test]
fn the_reference_maps_regions_and_runs_bounded() {
    // Both paths are the runner's, not the repository's: the library is built,
    // not shipped (§1.5), and the software is the runner's own (§11.2).
    let Some(library) = from_env("AWASERU_TEST_BACKEND") else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };
    let Some(software) = from_env("AWASERU_TEST_SOFTWARE") else {
        eprintln!("SKIPPED: set AWASERU_TEST_SOFTWARE to something this backend can load");
        return;
    };

    let home = std::env::temp_dir().join("awaseru-test-home");
    std::fs::create_dir_all(&home).expect("a directory for the backend's own files");

    let mut reference = match Reference::open(&library, &home, &software) {
        Ok(r) => r,
        Err(e) => panic!("the reference did not open: {e}"),
    };
    reference.set_watchdog(Duration::from_secs(30));

    // ---- the backend says what it is -------------------------------------
    let version = reference.version();
    eprintln!("backend {} built {:?}", version.reported, version.built);
    assert!(
        !version.reported.is_empty(),
        "a backend that will not say what version it is cannot be pinned (§16.1)"
    );
    assert!(
        version.built.as_deref().is_some_and(|b| !b.is_empty()),
        "and it should say when it was built"
    );

    // ---- regions are named, enumerated, and non-empty --------------------
    let regions = reference.regions();
    assert!(!regions.is_empty(), "a reference with no regions reads nothing");
    eprintln!(
        "regions: {}",
        regions
            .iter()
            .map(|r| format!("{} ({} bytes)", r.name, r.size))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for region in regions.iter() {
        assert!(
            region.size > 0,
            "`{}` is present with no bytes; an empty memory is absent (§3.5), not a region",
            region.name
        );
        assert_eq!(region.unit, 1, "this backend hands out bytes");
        assert!(
            region.access.readable(),
            "`{}` cannot be read, so nothing could be compared over it",
            region.name
        );
    }

    // The host addresses regions by the names it is given, so a name it was not
    // given must not resolve.
    assert!(
        regions.get("work-ram").is_some(),
        "the console's own work memory should be among them"
    );
    assert!(regions.get("nowhere").is_none());
    assert_eq!(
        reference.backend_name_for("work-ram"),
        Some("SnesWorkRam"),
        "the report must be checkable against the backend's own source"
    );
    assert_eq!(reference.backend_name_for("nowhere"), None);

    // ---- a read returns what the region declares -------------------------
    let declared = regions.get("work-ram").expect("it is there").size;
    let bytes = reference.read("work-ram").expect("it reads");
    assert_eq!(
        bytes.len(),
        declared,
        "the read must be as long as the region says it is; a short read would compare equal \
         over bytes nobody looked at"
    );
    // At power-on with memory zeroed — which is how a reference comes up by
    // default now — the work memory is **exactly** zero, and that is an
    // assertion rather than an inconvenience: it says the zeroing happened.
    //
    // This check used to be the reverse, "not all zero", standing in for "the
    // read reached the memory". It failed the moment zeroing became the
    // default, correctly. Its job is now done by the pair: zero here, and not
    // zero once the software has run, a few lines below.
    assert!(
        !reference.origin().memory_zeroed.is_empty(),
        "this reference should have come up with memory zeroed"
    );
    assert!(
        bytes.iter().all(|&b| b == 0),
        "work memory at a zeroed power-on must be zero; {} bytes are not",
        bytes.iter().filter(|&&b| b != 0).count()
    );

    // ---- a span is part of the whole -------------------------------------
    let span = reference.read_span("work-ram", 0x100, 64).expect("it reads");
    assert_eq!(
        span,
        bytes[0x100..0x100 + 64],
        "a span must be the part of the region it names"
    );

    // ---- the refusals refuse ---------------------------------------------
    let err = reference.read("nowhere").expect_err("no such region");
    assert!(
        matches!(err, ReadError::Absent { .. }),
        "an unknown name is absence, which §3.5 turns into a verdict rather than an error: {err:?}"
    );
    let err = reference
        .read_span("work-ram", declared - 1, 2)
        .expect_err("one byte past the end");
    assert!(matches!(err, ReadError::Span(_)), "got {err:?}");

    // ---- a bound of nothing asks the backend nothing ---------------------
    let before = reference.position();
    let stop = reference.run(Bound::Frames(0)).expect("a bound of no frames");
    assert!(stop.arrived());
    assert_eq!(stop.position, before, "no frames means no movement");

    // ---- frames: the counter advances by exactly what was asked ----------
    let cycles_before = reference.cycles().expect("it is stopped");
    let stop = reference.run(Bound::Frames(1)).expect("one frame");
    assert!(stop.arrived(), "{stop}");
    let Position::FrameBoundary { frame: first } = stop.position else {
        panic!("a run bounded by frames should end at a frame boundary, ended at {stop}")
    };
    assert!(
        !stop.position.is_instruction_boundary(),
        "§3.4: a frame boundary is not an instruction boundary, and the position must not claim \
         to be one"
    );
    assert!(
        reference.cycles().expect("still stopped") > cycles_before,
        "a frame's worth of running must advance the processor's cycle count"
    );

    // The other half of the pair above: the software has now run, so the
    // memory is no longer the zeros we put there. A read that returned
    // something stale, or went nowhere, could not produce both halves.
    assert!(
        reference
            .read("work-ram")
            .expect("it reads")
            .iter()
            .any(|&b| b != 0),
        "after a frame of running, work memory should no longer be the zeros it started as"
    );

    let stop = reference.run(Bound::Frames(3)).expect("three frames");
    assert!(stop.arrived(), "{stop}");
    assert_eq!(
        stop.position,
        Position::FrameBoundary { frame: first + 3 },
        "three frames must advance the frame counter by three — not by two, and not by however \
         many fit in the time it took"
    );

    // How much the reference moved while it ran — printed, **not asserted**.
    //
    // §2.2 says every comparison must report this, because agreement over bytes
    // the reference never touched is not evidence. But what it is at a given
    // point is a fact about the software, and asserting it here would be
    // asserting that (§11.2).
    //
    // It is worth knowing what it looks like: measured on one cartridge, four
    // frames from power-on moved **nothing** — the software was in a loop
    // waiting for something — and by sixty-four frames it had moved tens of
    // thousands of bytes. A comparison made at frame four would have agreed
    // perfectly and meant nothing, which is the whole of §2.2 in one number.
    let after = reference.read("work-ram").expect("it reads");
    assert_eq!(after.len(), bytes.len(), "the second read is the same length");
    let moved = bytes.iter().zip(&after).filter(|(a, b)| a != b).count();
    eprintln!("the reference moved {moved} of {} bytes over four frames", bytes.len());

    // ---- instructions: a different kind of position ----------------------
    let cycles_before = reference.cycles().expect("it is stopped");
    let stop = reference.run(Bound::Instructions(1000)).expect("a thousand");
    assert!(stop.arrived(), "{stop}");
    assert!(
        matches!(stop.position, Position::InstructionBoundary { .. }),
        "a run bounded by instructions ends between instructions, ended at {stop}"
    );
    assert!(
        stop.position.is_instruction_boundary(),
        "and says so, because that is what makes it seedable (§3.4)"
    );
    assert!(
        reference.cycles().expect("still stopped") > cycles_before,
        "a thousand instructions must advance the cycle count"
    );

    // ---- an address bound, which M0 refused and M3 honours ---------------
    //
    // **This assertion is the opposite of the one M0 wrote.** M0 refused an
    // address bound, because the backend's breakpoint record was not among the
    // declarations transcribed then. It is now, so the refusal became an
    // arrival — and the test that asserted the refusal says so rather than
    // quietly disappearing.
    //
    // The assertions are about plumbing (§11.2): where the software goes is
    // read from the reference rather than written down here.
    // A mark of its own, because the blob the rest of this test uses is made
    // further down and an address bound has to be able to start twice from the
    // same place.
    let mark = reference.save_state().expect("it saves");
    let stop = reference
        .run(Bound::Instructions(1))
        .expect("one instruction");
    let Position::InstructionBoundary { pc: next } = stop.position else {
        panic!("expected an instruction boundary, got {stop}")
    };

    reference.load_state(&mark).expect("back to the mark");
    let stop = reference
        .run(Bound::Address {
            address: next,
            within: 10,
        })
        .expect("an address bound");
    assert_eq!(
        stop.reason,
        Reason::AddressHit { address: next },
        "an address one instruction away must be reached, and reported as reached rather than \
         as a budget running out: {stop}"
    );
    assert!(stop.arrived(), "{stop}");
    assert_eq!(
        stop.position,
        Position::InstructionBoundary { pc: next },
        "and the position is the address asked for"
    );

    // ---- and §4.4's budget, reachable for the first time -----------------
    // An address the software is not about to reach, with a budget of one
    // instruction. The budget wins, and that is a result rather than a failure
    // (§4.3).
    reference.load_state(&mark).expect("back to the mark");
    let unreachable = 0x00_0001;
    assert_ne!(unreachable, next, "the address must not be the next one");
    let stop = reference
        .run(Bound::Address {
            address: unreachable,
            within: 1,
        })
        .expect("a bounded look is still an answer");
    assert_eq!(
        stop.reason,
        Reason::BudgetExhausted,
        "one instruction is not enough to reach an arbitrary address, and running out is a stop \
         reason and not an error (§4.3, §4.4): {stop}"
    );
    assert!(
        !stop.arrived(),
        "and exhausting a budget is not arriving, so a comparison here is not determined (§2.3)"
    );

    // A budget of nothing is refused rather than always exhausting.
    let stop = reference
        .run(Bound::Address {
            address: next,
            within: 0,
        })
        .expect("still an answer");
    assert!(
        matches!(stop.reason, Reason::Refused { .. }),
        "a budget of no instructions cannot reach anything, and pretending to look is worse \
         than refusing (§2.4): {stop}"
    );

    let stop = reference
        .run(Bound::Instructions(u64::MAX))
        .expect("more than the backend can count");
    assert!(
        matches!(stop.reason, Reason::Refused { .. }),
        "a count the backend cannot hold must be refused rather than truncated: {stop}"
    );

    // ---- writing a region, and reading back what was written -------------
    let before = reference.read("work-ram").expect("it reads");
    let pattern: Vec<u8> = (0..before.len()).map(|i| (i * 7 + 13) as u8).collect();
    assert_ne!(
        pattern, before,
        "the pattern has to differ from what is there, or writing it proves nothing"
    );
    reference.write("work-ram", &pattern).expect("it writes");
    assert_eq!(
        reference.read("work-ram").expect("it reads"),
        pattern,
        "what was written must come back; a write that did nothing and a read that is cached \
         look the same from here, and this is what tells them apart"
    );

    // A span, and only the span.
    reference.write("work-ram", &before).expect("restore");
    let segment = [0xA5u8; 64];
    reference
        .write_span("work-ram", 0x1000, &segment)
        .expect("it writes");
    let after_span = reference.read("work-ram").expect("it reads");
    assert_eq!(&after_span[0x1000..0x1040], &segment, "the span arrived");
    assert_eq!(
        &after_span[..0x1000],
        &before[..0x1000],
        "and nothing before it moved"
    );
    assert_eq!(
        &after_span[0x1040..],
        &before[0x1040..],
        "and nothing after it moved"
    );

    // ---- the write refusals ----------------------------------------------
    let err = reference
        .write("work-ram", &pattern[..100])
        .expect_err("not the whole region");
    assert!(
        matches!(err, WriteError::Span(_)),
        "a short whole-region write must be refused, not padded with whatever was there: {err:?}"
    );
    let err = reference
        .write("program-rom", &[0u8; 16])
        .expect_err("read-only");
    assert!(
        matches!(err, WriteError::NotWritable { .. }),
        "the cartridge's program data is not writable; writing it would change the subject: {err:?}"
    );
    let err = reference
        .write("nowhere", &[0u8; 16])
        .expect_err("no such region");
    assert!(matches!(err, WriteError::Absent { .. }), "got {err:?}");

    // ---- the processor state round-trips ----------------------------------
    let processor = reference.read_processor().expect("it reads");
    assert!(
        !processor.is_empty(),
        "an empty processor state would make §3.3 unimplementable"
    );
    reference
        .write_processor(&processor)
        .expect("it writes it back");
    assert_eq!(
        reference.read_processor().expect("it reads").bytes(),
        processor.bytes(),
        "the state must survive being written back"
    );

    // And writing a *different* one arrives, so the round-trip above is not a
    // pair of no-ops agreeing with each other.
    let mut altered = processor.bytes().to_vec();
    altered[0] ^= 0xFF;
    reference
        .write_processor(&awaseru_core::snapshot::Processor::opaque(altered.clone()))
        .expect("it writes");
    assert_eq!(
        reference.read_processor().expect("it reads").bytes(),
        &altered[..],
        "a changed processor state must arrive, or `write_processor` is doing nothing"
    );
    reference.write_processor(&processor).expect("restore");

    // ---- the blob: save, disturb, load, and prove the disturbance is gone -
    reference.write("work-ram", &before).expect("restore");
    let blob = reference.save_state().expect("it saves");
    assert!(blob.len() > 1000, "a whole machine is not a few bytes");
    eprintln!(
        "blob of {} bytes, taken at {}, fingerprint of {}",
        blob.len(),
        blob.position(),
        blob.fingerprint().len()
    );
    let at_blob = reference.read("work-ram").expect("it reads");

    // **The anti-vacuous step.** Put the machine somewhere it demonstrably is
    // not, then load the blob and show that what was put there is gone. A load
    // that did nothing passes a read-load-read test; it cannot pass this one.
    reference.write("work-ram", &pattern).expect("disturb it");
    assert_eq!(
        reference.read("work-ram").expect("it reads"),
        pattern,
        "the disturbance is really there"
    );
    reference.load_state(&blob).expect("it loads");
    let restored = reference.read("work-ram").expect("it reads");
    assert_eq!(
        restored, at_blob,
        "the load must put back what the blob holds"
    );
    assert_ne!(
        restored, pattern,
        "and the disturbance must be gone — if this passes while the one above also passes, \
         the load is real"
    );

    // ---- a blob that does not describe this machine is refused ------------
    let wrong_position = Blob::new(
        blob.bytes().to_vec(),
        Position::FrameBoundary { frame: u64::MAX },
        blob.fingerprint().to_vec(),
    );
    let err = reference
        .load_state(&wrong_position)
        .expect_err("it did not land there");
    assert!(
        matches!(err, StateError::LandedElsewhere { .. }),
        "the position check is the only detector of a load that did nothing on this backend: \
         {err:?}"
    );

    let wrong_fingerprint = Blob::new(
        blob.bytes().to_vec(),
        reference.position(),
        vec![0xFF; blob.fingerprint().len()],
    );
    let err = reference
        .load_state(&wrong_fingerprint)
        .expect_err("not this machine");
    assert!(
        matches!(err, StateError::FingerprintDiffers),
        "got {err:?}"
    );

    // ---- §4.8's fourth step, in miniature --------------------------------
    // Run the same bound onward from two separate resumes of one blob. If a
    // blob restored only the memories this crate maps, these would part.
    reference.load_state(&blob).expect("it loads");
    reference.run(Bound::Frames(3)).expect("three frames");
    let first_way = reference.read("work-ram").expect("it reads");
    let first_cycles = reference.cycles().expect("stopped");

    reference.load_state(&blob).expect("it loads again");
    reference.run(Bound::Frames(3)).expect("three frames");
    assert_eq!(
        reference.read("work-ram").expect("it reads"),
        first_way,
        "running on from two resumes of one blob must agree; disagreement would mean the blob \
         restores some of the machine and not the rest"
    );
    assert_eq!(
        reference.cycles().expect("stopped"),
        first_cycles,
        "and arrive at the same cycle"
    );

    // ---- M1's done-condition, on supplied software ------------------------
    // The same round trip as `the_round_trip`, which runs it against the
    // generated fixture and asserts content. Here the assertions are about
    // the plumbing only, because the software is the runner's and is not in
    // this repository (§11.2): that what was read comes back, that the
    // disturbance in between was really there and is really gone, and that a
    // region this backend does not expose is not determined.
    let provenance = Provenance {
        reference: "supplied".into(),
        backend: "mesence".into(),
        version: reference.version().reported,
        // A fixed string rather than a hash: hashing is the host's job and
        // this test is below it. What §16.5's check needs is that the two
        // sides agree, and they do.
        software: "supplied by the runner".into(),
    };
    const ROUND_TRIP: [&str; 2] = ["work-ram", "palette-ram"];

    reference.load_state(&blob).expect("back to the blob");
    let stop = reference
        .run(Bound::Instructions(1))
        .expect("one instruction, for a position a state can be seeded at (§3.4)");
    let position = stop.position.clone();
    assert!(position.is_instruction_boundary(), "at {position}");

    let before = awaseru_core::capture(&reference, provenance.clone(), position.clone(), &ROUND_TRIP)
        .expect("it captures");
    let original = before.processor().expect("captured").clone();

    // Every region it carries is disturbed, or the comparison over the ones
    // left alone is vacuous and the fold comes back not determined (§2.2).
    for name in ROUND_TRIP {
        let held = before.get(name).expect("captured").bytes();
        let noise: Vec<u8> = (0..held.len()).map(|i| ((i * 29 + 11) as u8) & 0x7F).collect();
        assert_ne!(noise, held, "the noise must differ from what `{name}` holds");
        reference.write(name, &noise).expect("disturb it");
    }
    let mut flipped = original.bytes().to_vec();
    for byte in &mut flipped {
        *byte ^= 0xFF;
    }
    reference
        .write_processor(&Processor::opaque(flipped))
        .expect("disturb the registers");

    let disturbed =
        awaseru_core::capture(&reference, provenance.clone(), position.clone(), &ROUND_TRIP)
            .expect("it captures");
    for name in ROUND_TRIP {
        assert_ne!(
            disturbed.get(name).expect("captured").bytes(),
            before.get(name).expect("captured").bytes(),
            "the disturbance to `{name}` is really there"
        );
    }

    awaseru_core::seed(&mut reference, &before).expect("it seeds");
    let after = awaseru_core::capture(&reference, provenance.clone(), position.clone(), &ROUND_TRIP)
        .expect("it captures");

    for name in ROUND_TRIP {
        assert_eq!(
            after.get(name).expect("captured").bytes(),
            before.get(name).expect("captured").bytes(),
            "`{name}` did not come back the same"
        );
        assert_ne!(
            after.get(name).expect("captured").bytes(),
            disturbed.get(name).expect("captured").bytes(),
            "and `{name}` is not still disturbed, which is what says the seed did something"
        );
    }
    assert_eq!(
        after.processor().expect("captured").bytes(),
        original.bytes(),
        "the registers did not come back"
    );

    let comparison = Comparison {
        seed: &disturbed,
        reference: &before,
        candidate: &after,
    };
    match awaseru_core::compare_regions(comparison, &ROUND_TRIP).expect("comparable") {
        Verdict::Agrees { compared, moved } => {
            assert!(moved > 0, "a comparison over nothing moved is vacuous (§2.2)");
            eprintln!("round trip agrees over {compared} bytes, of which {moved} moved");
        }
        other => panic!("the round trip should agree, got {other}"),
    }

    assert_eq!(
        awaseru_core::compare_regions(comparison, &["nowhere"]).expect("comparable"),
        Verdict::NotDetermined(Undetermined::RegionAbsent {
            region: "nowhere".into()
        }),
        "a region this backend does not expose must be not determined, never agreement"
    );

    // ---- one reference per process ----------------------------------------
    let err = Reference::open(&library, &home, &software)
        .expect_err("the backend's emulator is one object");
    assert!(
        matches!(err, OpenError::AlreadyInUse),
        "a second reference must refuse rather than share the first one's emulator: {err:?}"
    );
}
