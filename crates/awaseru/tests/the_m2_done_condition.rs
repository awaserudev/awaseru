//! **M2's done-condition**, the half that needs more than one process.
//!
//! > The same run from the same seed produces the same stop reason and the same
//! > state three times, in three separate processes (§2.5) — and an anchor's
//! > demonstration passes end to end (§4.8).
//!
//! §2.5 asks for determinism in one process, across processes, and across
//! machines. The first is covered where the demonstration runs
//! (`the_demonstration`, on the fixture). **The second is this file**, and it
//! is why the host grew `--state-digest`: a line a machine can compare is the
//! only way to compare two machines that are not in the same process.
//!
//! The third — across machines — is not testable from here, and saying so is
//! better than counting it. What would settle it is somebody else running this
//! and comparing the digest.
//!
//! # Why it spawns the host rather than calling into it
//!
//! Because "in three separate processes" is the requirement. A test that called
//! `session::run` three times would be testing one process three times, which
//! is the thing §2.5 distinguishes. Each child here is the binary a user runs,
//! with its own address space, its own loaded backend, and — in the first three
//! cases — its own empty cache.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What a child printed, as the fields of its one line.
#[derive(Debug, PartialEq, Eq)]
struct Line {
    stop: String,
    state: String,
    how: String,
    evidence: String,
}

fn field(line: &str, name: &str) -> String {
    // The fields are `name=value` separated by spaces, and `stop`'s value
    // contains spaces — so each field runs up to the next ` name=`, not to the
    // next space.
    let after = line
        .split_once(&format!("{name}="))
        .unwrap_or_else(|| panic!("no `{name}=` in {line:?}"))
        .1;
    for next in ["stop=", "state=", "how=", "evidence="] {
        if let Some((before, _)) = after.split_once(&format!(" {next}")) {
            return before.to_string();
        }
    }
    after.trim().to_string()
}

fn run_child(config_dir: &Path, cache: &Path, anchor: &str) -> Line {
    let output = Command::new(env!("CARGO_BIN_EXE_awaseru"))
        .arg("--config")
        .arg(config_dir.join("awaseru.toml"))
        .arg("--local")
        .arg(config_dir.join("awaseru.local.toml"))
        .arg("--anchor")
        .arg(anchor)
        .arg("--cache")
        .arg(cache)
        .arg("--state-digest")
        .output()
        .expect("the host runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the host failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        stdout
    );
    // The backend writes its own notes to the same stream, so the line wanted
    // is the one that is the digest.
    let line = stdout
        .lines()
        .find(|l| l.starts_with("stop="))
        .unwrap_or_else(|| panic!("no digest line in {stdout:?}"));

    Line {
        stop: field(line, "stop"),
        state: field(line, "state"),
        how: field(line, "how"),
        evidence: field(line, "evidence"),
    }
}

#[test]
fn the_same_run_gives_the_same_state_in_three_separate_processes() {
    let Some(config_dir) = std::env::var_os("AWASERU_TEST_CONFIG_DIR").map(PathBuf::from) else {
        eprintln!(
            "SKIPPED: set AWASERU_TEST_CONFIG_DIR to a directory holding awaseru.toml and \
             awaseru.local.toml, with at least one anchor declared"
        );
        return;
    };

    let scratch = std::env::temp_dir().join("awaseru-m2-done-condition");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("a directory");

    // ---- three processes, three empty caches, all replaying --------------
    let mut lines = Vec::new();
    for which in 0..3 {
        let cache = scratch.join(format!("cache-{which}"));
        std::fs::create_dir_all(&cache).expect("a directory");
        let line = run_child(&config_dir, &cache, "later");
        eprintln!("process {which}: {line:?}");
        assert_eq!(
            line.how, "replayed",
            "process {which} had an empty cache, so it must have replayed"
        );
        lines.push(line);
    }

    assert_eq!(
        lines[0].state, lines[1].state,
        "two separate processes replaying the same definition must reach the same state (§2.5)"
    );
    assert_eq!(lines[1].state, lines[2].state);
    assert_eq!(
        lines[0].stop, lines[1].stop,
        "and report the same stop position"
    );
    assert_eq!(lines[1].stop, lines[2].stop);

    // A digest of nothing would also agree three times, so it has to be a
    // digest of something.
    assert_eq!(lines[0].state.len(), 64, "a sha-256 in hexadecimal");
    assert_ne!(
        lines[0].state,
        "0".repeat(64),
        "a state digest of all zeros would mean nothing was hashed"
    );

    // ---- and the anchor was demonstrated along the way -----------------
    for (which, line) in lines.iter().enumerate() {
        assert_eq!(
            line.evidence, "yes",
            "process {which} must have demonstrated the anchor it arrived at, or its run is \
             not evidence (§4.8)"
        );
    }

    // ---- a fourth process resumes, and lands in the same place ---------
    // §4.7: which of the two happened must never change the result. This is
    // that sentence as a test, across processes.
    let resumed = run_child(&config_dir, &scratch.join("cache-0"), "later");
    eprintln!("fourth process (warm cache): {resumed:?}");
    assert_eq!(
        resumed.how, "resumed",
        "the cache from the first process is warm, so this must resume"
    );
    assert_eq!(
        resumed.state, lines[0].state,
        "resuming a blob another process cached must give the state replaying gives — §4.7 says \
         which of the two happened must never change the result"
    );
    assert_eq!(resumed.stop, lines[0].stop);
    assert_eq!(resumed.evidence, "yes");

    // ---- §4.11: and deleting the cache costs only the clock ------------
    std::fs::remove_dir_all(scratch.join("cache-0")).expect("it goes");
    std::fs::create_dir_all(scratch.join("cache-0")).expect("a directory");
    let after_clear = run_child(&config_dir, &scratch.join("cache-0"), "later");
    assert_eq!(
        after_clear.how, "replayed",
        "with the cache gone it must replay"
    );
    assert_eq!(
        after_clear.state, lines[0].state,
        "and arrive at the same state — deleting the cache changes the clock and nothing else \
         (§4.11)"
    );

    // What this does NOT cover: §2.5's third, across machines. Nothing here
    // can run on another machine. What would settle it is somebody else
    // running this and comparing the digest, which is why the digest is a
    // single line a person can paste.
}

/// A chain's first link is an anchor too, and arriving at it must agree with
/// arriving at it from a process that cached it.
///
/// Separate from the test above because it is a different claim: that an anchor
/// part way along a chain is as good as the one at its end.
#[test]
fn an_anchor_part_way_along_a_chain_agrees_across_processes_too() {
    let Some(config_dir) = std::env::var_os("AWASERU_TEST_CONFIG_DIR").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_CONFIG_DIR");
        return;
    };

    let scratch = std::env::temp_dir().join("awaseru-m2-chain");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("a directory");

    let first = run_child(&config_dir, &scratch.join("a"), "early");
    let second = run_child(&config_dir, &scratch.join("b"), "early");
    assert_eq!(first.state, second.state, "§2.5, one link in");
    assert_eq!(first.how, "replayed");

    // And it is not the same state as the end of the chain, or the test above
    // would be agreeing about the wrong thing.
    let later = run_child(&config_dir, &scratch.join("c"), "later");
    assert_ne!(
        first.state, later.state,
        "two anchors at different positions must not have the same state, or these tests are \
         comparing something that does not depend on where the machine is"
    );
}
