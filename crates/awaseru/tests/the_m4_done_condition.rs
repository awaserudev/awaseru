//! §M4's done-condition: **a client written in a language other than the
//! host's drives a full cycle — seed, run, compare — without linking the
//! host.**
//!
//! The client is `clients/python/`, standard library only. It is spawned as a
//! program, it spawns `awaseru serve` itself (§8.1 — the client drives), and it
//! links nothing of this project: no crate, no header, no shared library. What
//! passes between them is §8.3's frames.
//!
//! # Why this is not a test the protocol can pass vacuously
//!
//! Two numbers in the summary are the **tool's** and cannot be the client's:
//!
//! - **where the two implementations part.** The client is told where its output
//!   span begins and nothing about where it is wrong; the offset comes back from
//!   the comparison. One wrong implementation here is right about its first byte
//!   and wrong about its second, so an offset of 1024 would be a protocol
//!   guessing and 1025 is a reference measuring.
//! - **the instruction that wrote the reference's value** (§5.4's third item).
//!   The client never sees the program; the address of the store comes from a
//!   replay with a write breakpoint, and this test knows what it must be.
//!
//! And both halves are here, because either alone is passed by a stub: a
//! protocol that always differs fails the right implementation, one that always
//! agrees fails the wrong one.
//!
//! # What this does NOT cover
//!
//! A second language beyond Python, and a client written by somebody who did
//! not write the tool — which is what §13's Q3 says would settle §8.6's
//! versioning and negotiation. This client was written against
//! `doc/protocol.md`, which is the next best thing and not the same thing.
//!
//! Nor does it cover the in-process binding, which has its own test: the point
//! here is precisely that nothing is linked.

use std::path::{Path, PathBuf};
use std::process::Command;

use awaseru_snes::fixture::{self, expected};

fn python() -> Option<String> {
    let candidate = std::env::var("AWASERU_TEST_PYTHON").unwrap_or_else(|_| "python3".to_string());
    let found = Command::new(&candidate).arg("--version").output().ok()?;
    found.status.success().then_some(candidate)
}

fn client_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("clients")
        .join("python")
}

fn configure(dir: &Path, rom: &Path, library: &Path) {
    let digest = awaseru::digest::of_file(rom).expect("the image hashes");
    std::fs::write(
        dir.join("awaseru.toml"),
        format!(
            "[project]\nplatform = \"snes\"\n\n\
             [rom]\nsha256 = \"{digest}\"\n\n\
             [[emulator]]\nname = \"ref-a\"\nplatform = \"snes\"\nbackend = \"mesence\"\n\
             version = \"2.2.1\"\n\n\
             [reference]\nuse = \"ref-a\"\n\n\
             [anchors]\nverify_from_origin = 0\nreverify_at_end = false\n"
        ),
    )
    .expect("the shared half");
    std::fs::write(
        dir.join("awaseru.local.toml"),
        format!(
            "[rom]\npath = \"{}\"\n\n[emulator.ref-a]\npath = \"{}\"\n",
            rom.display(),
            library.display()
        ),
    )
    .expect("the machine-local half");
}

#[test]
fn a_client_in_another_language_drives_seed_run_and_compare() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };
    let Some(python) = python() else {
        eprintln!("SKIPPED: no python3 on this machine (set AWASERU_TEST_PYTHON to name one)");
        return;
    };
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_awaseru"));

    let dir = std::env::temp_dir().join("awaseru-m4-done");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");
    configure(&dir, &rom, &library);

    let output = Command::new(&python)
        .arg("cycle.py")
        .args(["--exe".into(), exe.display().to_string()])
        .args(["--config".into(), dir.join("awaseru.toml").display().to_string()])
        .args([
            "--local".into(),
            dir.join("awaseru.local.toml").display().to_string(),
        ])
        .args(["--home".into(), dir.join("home").display().to_string()])
        .args(["--cache".into(), dir.join("cache").display().to_string()])
        .args(["--entry".into(), expected::ROUTINE_ENTRY.to_string()])
        .args(["--returns-to".into(), expected::ROUTINE_RETURN.to_string()])
        .args(["--input-at".into(), expected::ROUTINE_INPUT_AT.to_string()])
        .args(["--output-at".into(), expected::ROUTINE_OUTPUT_AT.to_string()])
        .args(["--length".into(), expected::ROUTINE_LENGTH.to_string()])
        .current_dir(client_directory())
        .output()
        .expect("python runs");

    let said = String::from_utf8_lossy(&output.stdout).to_string();
    let complained = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "the client's own cycle must pass:\nstdout:\n{said}\nstderr:\n{complained}"
    );

    // ---- and the suite checks the numbers the client could not know -------
    let summary: serde_json::Value = serde_json::from_str(said.trim())
        .unwrap_or_else(|e| panic!("the client's summary must be JSON ({e}): {said}"));

    assert_eq!(
        summary["right"]["verdict"], "agrees",
        "the right reimplementation agrees: {summary}"
    );
    assert_eq!(
        summary["right"]["moved"],
        expected::ROUTINE_LENGTH,
        "§5.2's movement, reported always: {summary}"
    );

    assert_eq!(summary["wrong"]["verdict"], "differs", "{summary}");
    let difference = &summary["wrong"]["difference"];
    assert_eq!(
        difference["first"],
        expected::ROUTINE_OUTPUT_AT + 1,
        "the FIRST differing offset — this wrong implementation is right about the output's \
         first byte, and nothing the client sent says so: {summary}"
    );
    assert_eq!(
        difference["region"], "work-ram",
        "§5.4's offset is read against a region, and the wire names it (§13's Q16)"
    );
    assert_eq!(
        difference["wrote"]["position"]["pc"],
        expected::ROUTINE_STORE,
        "§5.4's third item: the routine's own store, and not {:#X}, the store after its \
         return (§4.5). The client never sees the program: {summary}",
        expected::ROUTINE_CLOBBER_STORE
    );
    assert_eq!(
        difference["wrote"]["position"]["position"], "mid-instruction",
        "a write is caught during the instruction performing it (§3.4)"
    );

    // §5.3's control, and §7.3's declaration, as the client saw them.
    assert_eq!(summary["control"]["control"], "ran", "{summary}");
    assert_eq!(summary["control"]["noticed"], true, "{summary}");
    assert_eq!(summary["control_complete"], true, "{summary}");
    assert_eq!(summary["read_matches_our_own"], true, "{summary}");
    assert!(
        summary["absent"]
            .as_array()
            .expect("a list")
            .iter()
            .any(|c| c == "input-replay"),
        "what the backend cannot do is on the wire too (§7.3): {summary}"
    );

    eprintln!("the client's summary: {said}");
}
