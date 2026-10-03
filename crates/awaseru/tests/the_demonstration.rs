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
        reverify_at_end: true,
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

    // ---- §4.9: nothing is re-verified in the MIDDLE of a session ------
    // The old policy replayed from the origin every N uses. On a definition
    // that replays in five minutes, that is fifty hours of verification against
    // seventeen minutes of work (§4.9 has the arithmetic), so the middle of a
    // session is now left alone and the session is bracketed instead.
    for _ in 0..8 {
        let arrived = arriver.arrive("later").expect("it arrives");
        assert_eq!(
            arrived.how,
            How::Resumed,
            "every arrival after the first resumes — nothing replays mid-session"
        );
        assert!(
            !arrived.reverified,
            "and nothing re-verifies of its own accord in the middle (§4.9)"
        );
    }

    // ---- and the closing half of the bracket does the checking --------
    // One replay, not three: the definition's determinism was established by
    // the demonstration and cannot change while the key holds (§4.11).
    let closed = arriver.reverify("later").expect("the blob still stands");
    assert_eq!(closed.anchor, "later");
    assert!(
        closed.uses >= 8,
        "it says how much rested on the blob: {} use(s)",
        closed.uses
    );
    assert!(
        closed.to_string().contains("still produces"),
        "said: {closed}"
    );
    eprintln!("closing check: {closed}");

    // Cheaper than the arrival that demonstrated it, which is the whole point:
    // that one replayed the chain three times and ran onward twice; this one
    // replays once.
    assert!(
        closed.took < first.took,
        "one replay must cost less than a demonstration: {:?} against {:?}",
        closed.took,
        first.took
    );

    // ---- and it CATCHES a blob that is not what replaying gives --------
    // Without this half the closing check is decoration: one that always passed
    // would pass every assertion above. The blob under the key is replaced with
    // a state from further on — the same anchor, the same key, a machine that
    // ran further — which is exactly the shape of the failure §4.9 is paid to
    // catch.
    let key = anchors
        .key("later", &provenance)
        .expect("the anchor resolves");
    let honest = cache.get(&key).expect("a blob is cached");

    // The blob is made wrong in a region the anchor does NOT declare — it
    // covers `work-ram` and `palette-ram`, so a difference in `video-ram`
    // sails past §4.8's cheap check and is caught only by a comparison over
    // every writable region. That is the exact failure the closing check is
    // paid for; a blob wrong in a declared region would be caught one step
    // earlier, by the cheap check, which is the correct order and tests
    // something else.
    arriver.arrive("later").expect("back to the anchor");
    arriver
        .write_span("video-ram", 0x100, &[0xA5; 64])
        .expect("scribble somewhere the anchor does not watch");
    let tampered = arriver.save_state().expect("a state with the scribble in it");
    cache
        .put(
            &key,
            &awaseru::cache::Stored {
                check: awaseru_core::CheapCheck {
                    // The blob's own landing place, so the position half of the
                    // cheap check agrees...
                    position: tampered.position().clone(),
                    // ...and the declared regions are untouched, so the digest
                    // half agrees too.
                    coverage: honest.check.coverage.clone(),
                },
                blob: tampered,
                ..honest.clone()
            },
        )
        .expect("the cache takes it");

    let err = arriver
        .reverify("later")
        .expect_err("that blob is not what replaying gives");
    match &err {
        awaseru::arrive::ArriveError::ResumeDisagrees { anchor, differences } => {
            assert_eq!(anchor, "later");
            assert!(!differences.is_empty(), "it says where they parted");
            eprintln!("caught: {err}");
        }
        other => panic!("the closing check must catch this: {other}"),
    }

    // Put the honest one back, so what follows is about what it is about.
    cache.put(&key, &honest).expect("restored");

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
        reverify_at_end: false,
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
