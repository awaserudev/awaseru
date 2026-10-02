//! §4.8's demonstration, run against the generated fixture.
//!
//! The fixture's program is this project's, so the assertions here may be
//! about content. The same demonstration against supplied software is the
//! done-condition test's, where they are about plumbing.
//!
//! One test, because only one reference may exist per process.

use std::path::PathBuf;
use std::time::Duration;

use awaseru::arrive::{Arriver, How};
use awaseru::cache::Cache;
use awaseru::config::AnchorPolicy;
use awaseru_core::anchor::{Anchor, Anchors, Definition, Start};
use awaseru_core::run::Bound;
use awaseru_core::snapshot::Provenance;
use awaseru_core::{Platform, Undetermined};
use awaseru_snes::{Reference, Startup, fixture};

fn provenance(version: String) -> Provenance {
    Provenance {
        reference: "fixture".into(),
        backend: "mesence".into(),
        version,
        software: "the generated fixture".into(),
    }
}

/// Two anchors, the second built on the first, so the chain is exercised and
/// not only a single definition. Shallow on purpose: these exist so there is
/// something to demonstrate.
fn anchors() -> Anchors {
    Anchors::new(vec![
        Anchor {
            name: "early".into(),
            definition: Definition {
                start: Start::PowerOn,
                bound: Bound::Frames(3),
                input: None,
            },
            covers: vec!["work-ram".into(), "palette-ram".into()],
        },
        Anchor {
            name: "later".into(),
            definition: Definition {
                start: Start::Anchor("early".into()),
                bound: Bound::Frames(2),
                input: None,
            },
            covers: vec!["work-ram".into(), "palette-ram".into()],
        },
    ])
    .expect("distinct names")
}

#[test]
fn an_anchor_is_demonstrated_and_then_resumed() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-demonstration");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("fixture.sfc");
    std::fs::write(&rom, fixture::image()).expect("write the image");
    let cache_dir = dir.join("cache");
    std::fs::create_dir_all(&cache_dir).expect("a directory");

    let mut reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(30));
    let provenance = provenance(reference.version().reported);
    let anchors = anchors();
    let cache = Cache::at(&cache_dir);
    let policy = AnchorPolicy {
        verify_from_origin: 3,
        reverify_after: 2,
    };

    let mut arriver = Arriver::new(
        &mut reference,
        &anchors,
        &cache,
        provenance.clone(),
        policy.clone(),
    );

    // ---- the first arrival replays, and demonstrates -------------------
    assert!(cache.is_empty(), "nothing is cached yet");
    let first = arriver.arrive("later").expect("it arrives");
    assert!(
        matches!(first.how, How::Replayed { anchors_run: 2 }),
        "with an empty cache the chain must be replayed, both links: {:?}",
        first.how
    );
    assert!(
        first.caveat.is_none(),
        "the policy asks for three replays, so the anchor is demonstrated and carries no \
         caveat: {:?}",
        first.caveat
    );
    eprintln!("first arrival: {} in {:?}", first.how, first.took);
    assert!(!cache.is_empty(), "and a blob was cached");

    // ---- the second arrival resumes -----------------------------------
    let second = arriver.arrive("later").expect("it arrives");
    assert_eq!(
        second.how,
        How::Resumed,
        "a cached blob whose key matches and whose cheap check passes must be resumed"
    );
    assert!(second.caveat.is_none());
    assert!(
        second.took < first.took,
        "resuming must be quicker than replaying: {:?} against {:?}",
        second.took,
        first.took
    );
    eprintln!("second arrival: {} in {:?}", second.how, second.took);

    // ---- the machine really is at the anchor --------------------------
    // The fixture's pattern is where its program puts it, which is content
    // this project owns (§11.3).
    let work = arriver.read("work-ram").expect("it reads");
    let at = fixture::expected::WORK_PATTERN_AT;
    let pattern = fixture::expected::work_pattern();
    assert_eq!(
        &work[at..at + pattern.len()],
        &pattern[..],
        "resuming the anchor must put the machine where the program had got to"
    );

    // ---- §4.9's reverify_after fires of its own accord ----------------
    // `reverify_after` is two, and the uses counter was reset by the
    // demonstration, so a couple more arrivals bring it due.
    let mut reverified = false;
    for _ in 0..4 {
        let arrived = arriver.arrive("later").expect("it arrives");
        reverified |= arrived.reverified;
    }
    assert!(
        reverified,
        "with reverify_after = 2, the tool must re-demonstrate of its own accord within four \
         arrivals (§4.9)"
    );

    // ---- §4.11: deleting the cache costs only time --------------------
    let before = arriver.read("work-ram").expect("it reads");
    assert!(cache.clear() >= 1, "there was something to clear");
    let after_clear = arriver.arrive("later").expect("it arrives anyway");
    assert!(
        matches!(after_clear.how, How::Replayed { .. }),
        "with the cache gone it must replay: {:?}",
        after_clear.how
    );
    assert_eq!(
        arriver.read("work-ram").expect("it reads"),
        before,
        "and arrive at the same machine — deleting the cache changes the clock and nothing else \
         (§4.11)"
    );

    // ---- an undemonstrated anchor hands back a caveat ----------------
    // The policy that asks for no demonstration is permitted (§4.9) and what
    // it costs is a verdict, not a warning.
    let lax = AnchorPolicy {
        verify_from_origin: 0,
        reverify_after: 0,
    };
    cache.clear();
    let mut lax_arriver = Arriver::new(&mut reference, &anchors, &cache, provenance, lax);
    let arrived = lax_arriver.arrive("early").expect("it arrives");
    match arrived.caveat {
        Some(Undetermined::AnchorNotDemonstrated { ref anchor }) => {
            assert_eq!(anchor, "early");
        }
        other => panic!(
            "an anchor nobody demonstrated must hand back a cause, got {other:?}"
        ),
    }
    let said = arrived.caveat.expect("there").to_string();
    assert!(said.contains("not evidence"), "said: {said}");

    // What this does NOT cover: §4.8's step three across *processes*. This
    // runs in one, and a library that spawned processes to prove a point would
    // be a surprising library. The done-condition test spawns them.
}
