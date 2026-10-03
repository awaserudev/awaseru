//! §7.3's gate, with a real reference on the other side of it.
//!
//! An anchor whose definition needs an input log cannot be replayed by a
//! reference that cannot press a button. Until this unit, the refusal was
//! written into `awaseru-core` as a fact about every backend — which was true
//! of the only backend there was, and not something the platform-independent
//! half can know. Now the reference is asked (§7.3), and this is the test that
//! the asking happens where it matters: on the way in to `arrive`, before the
//! cache, because an anchor nothing can replay is an anchor nothing can ever
//! demonstrate (§4.8), and a blob for one would be a blob resumed on trust.
//!
//! One test, because only one reference may exist per process.
//!
//! # What this does NOT cover
//!
//! Nothing here exercises an input log being *replayed*: no backend in this
//! project can, and §13's Q14 has what would change that. What is checked is
//! that the refusal comes from the declaration rather than from a constant —
//! which the second half of the test establishes by asking the same question
//! of a declaration that does contain the capability.

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
        !declared.has(Capability::InputReplay),
        "this reference exposes no control device (§13's Q14): {declared}"
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

    // ---- the anchor that needs a button ----------------------------------
    let err = arriver
        .arrive("after-the-button")
        .expect_err("nothing here can press it");
    match &err {
        ArriveError::Anchors(AnchorError::NeedsCapability {
            anchor,
            capability,
            needed_for,
        }) => {
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
    assert!(
        cache.is_empty(),
        "a refused anchor must not have gone as far as caching anything"
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
    assert!(
        anchors.chain_for("plain", &declared).is_ok(),
        "and this reference's own declaration is enough for a definition needing nothing"
    );
}
