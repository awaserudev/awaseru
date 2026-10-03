//! §7.3's gate, and the proof that it reads a declaration rather than a
//! constant.
//!
//! This test once asserted that an anchor needing an input log is **refused**,
//! because the only backend there was could not replay one. §13's Q14 closed
//! and the backend declares `input-replay`, so that half inverted: the anchor
//! now passes the gate.
//!
//! **What the test is for did not change.** Its point was never that the answer
//! is "no" — it was that the answer comes from asking the reference. So the
//! halves have swapped jobs: the real reference now shows the gate letting
//! something through on the strength of a declaration, and a synthetic
//! declaration **without** the capability shows the refusal still happening and
//! still naming both the anchor and what it needs.
//!
//! Had both halves asked a backend that says yes, this would have become a test
//! that passes everything, which is what a gate failing open looks like from
//! the inside.
//!
//! One test, because only one reference may exist per process.
//!
//! # What this does NOT cover
//!
//! Where the log actually takes the machine. That is the backend's test and the
//! real anchor's; here the log is three bytes of nothing in particular, and what
//! matters is that the gate did not stand in the way.

use std::path::PathBuf;
use std::time::Duration;

use awaseru::arrive::{ArriveError, Arriver, How};
use awaseru::cache::Cache;
use awaseru::config::AnchorPolicy;
use awaseru_core::anchor::{Anchor, AnchorError, Anchors, Definition, InputLog, Start};
use awaseru_core::capability::{Capabilities, Capability};
use awaseru_core::run::Bound;
use awaseru_core::snapshot::Provenance;
use awaseru_core::Platform;
use awaseru_snes::{Reference, Startup, fixture};

/// One anchor that needs input and one that does not, so that a gate which
/// refused everything would fail as loudly as one that refused nothing.
fn anchors() -> Anchors {
    Anchors::new(vec![
        Anchor {
            name: "plain".into(),
            definition: Definition {
                start: Start::PowerOn,
                bound: Bound::Frames(2),
                input: None,
            },
            covers: vec!["work-ram".into()],
        },
        Anchor {
            name: "after-the-button".into(),
            definition: Definition {
                start: Start::PowerOn,
                bound: Bound::Frames(2),
                input: Some(InputLog {
                    name: "press-start".into(),
                    path: std::path::PathBuf::from("/a/log.rec"),
                    recorded: vec![0x10, 0x00, 0x00],
                }),
            },
            covers: vec!["work-ram".into()],
        },
    ])
    .expect("distinct names")
}

#[test]
fn an_anchor_needing_input_is_refused_by_what_the_reference_declares() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-capability-gate");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("fixture.sfc");
    std::fs::write(&rom, fixture::image()).expect("write the image");
    let cache_dir = dir.join("cache");
    std::fs::create_dir_all(&cache_dir).expect("a directory");

    let mut reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(30));

    let declared = reference.capabilities();
    assert!(
        declared.has(Capability::InputReplay),
        "§13's Q14 is closed and this backend replays a recorded log: {declared}"
    );

    let provenance = Provenance {
        reference: "fixture".into(),
        backend: "mesence".into(),
        version: reference.version().reported,
        software: "the generated fixture".into(),
    };
    let anchors = anchors();
    let cache = Cache::at(&cache_dir);
    let policy = AnchorPolicy {
        verify_from_origin: 0,
        reverify_at_end: false,
    };
    let mut arriver = Arriver::new(&mut reference, &anchors, &cache, provenance, policy);

    // ---- the anchor that needs a button now gets PAST the gate -----------
    // The gate's job is to ask the reference, and the reference says yes. This
    // anchor's log is three bytes of nothing at a path that does not exist, so
    // the arrival still fails — but it fails at the REPLAY and not at the gate,
    // which is the whole distinction this test is about.
    let err = arriver
        .arrive("after-the-button")
        .expect_err("three bytes of nothing is not a recording");
    assert!(
        !matches!(
            err,
            ArriveError::Anchors(AnchorError::NeedsCapability { .. })
        ),
        "the gate let it through; what stopped it was the log itself: {err}"
    );
    assert!(
        err.to_string().contains("not playing"),
        "and the backend says nothing about a log it could not open, so the host checks the \
         one thing it can see: {err}"
    );
    assert!(
        cache.is_empty(),
        "an arrival that did not arrive must not have cached anything"
    );

    // ---- and the anchor that needs nothing still arrives ------------------
    // The half that keeps the gate from being a blanket refusal. If the check
    // were wrong about which definitions need something, this is what fails.
    let arrived = arriver.arrive("plain").expect("it arrives");
    assert!(
        matches!(arrived.how, How::Replayed { anchors_run: 1 }),
        "an empty cache replays: {:?}",
        arrived.how
    );

    // ---- the gate answers the declaration, not a constant -----------------
    // Same anchors, same question, a declaration that has the capability: the
    // chain resolves. Without this a hard-coded refusal would pass everything
    // above.
    assert!(
        anchors
            .chain_for(
                "after-the-button",
                &Capabilities::of([Capability::InputReplay])
            )
            .is_ok(),
        "a reference declaring input replay must not be refused"
    );

    // ---- and the refusal, asked of a declaration that lacks it -----------
    // This half carried a supporting role until §13's Q14 closed; it is now
    // the whole of the proof that the gate can say no. Without it, both halves
    // would ask a backend that says yes, and a gate that failed open would
    // pass this file from end to end.
    let err = anchors
        .chain_for("after-the-button", &Capabilities::of([]))
        .expect_err("a reference that cannot press a button is still refused");
    match &err {
        AnchorError::NeedsCapability {
            anchor,
            capability,
            needed_for,
        } => {
            assert_eq!(anchor, "after-the-button");
            assert_eq!(*capability, Capability::InputReplay);
            assert!(needed_for.contains("press-start"), "said: {needed_for}");
        }
        other => panic!("the refusal must name the capability: {other}"),
    }
    assert!(
        err.to_string().contains("input-replay"),
        "and say it in words: {err}"
    );

    // And the anchor that needs nothing is not refused by an empty
    // declaration, or the line above would be about a blanket refusal.
    assert!(anchors.chain_for("plain", &Capabilities::of([])).is_ok());
    assert!(
        anchors.chain_for("plain", &declared).is_ok(),
        "and this reference's own declaration is enough for a definition needing nothing"
    );
}
