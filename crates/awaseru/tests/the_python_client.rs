//! The second-language client's own checks, run from the suite.
//!
//! §M4's done-condition needs a client in a language other than the host's, and
//! the next unit drives the whole cycle with it. This one runs the client's
//! self-test: the framing against the documented bytes, and the two wrong
//! reimplementations being wrong where they claim to be.
//!
//! Both are things that would otherwise fail inside a protocol exchange, where
//! the cause is hard to see. A client one byte out produces a refusal from the
//! tool and a puzzled afternoon for whoever wrote it.
//!
//! It needs **no backend**: nothing here opens a reference. It needs `python3`,
//! and prints SKIPPED without one — §11.3's third route, applied to a tool
//! rather than to a ROM.
//!
//! # What this does NOT cover
//!
//! Any conversation at all. The client's transport is exercised against a real
//! server by the done-condition test, which is the next unit; this is the half
//! that can be checked without one.

use std::path::PathBuf;
use std::process::Command;

fn python() -> Option<String> {
    // `python3` by name, and the environment's override first, because a machine
    // may keep several.
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

#[test]
fn the_python_client_agrees_with_the_contract() {
    let Some(python) = python() else {
        eprintln!("SKIPPED: no python3 on this machine (set AWASERU_TEST_PYTHON to name one)");
        return;
    };

    let directory = client_directory();
    assert!(
        directory.join("awaseru.py").is_file(),
        "the client is at {}",
        directory.display()
    );

    let output = Command::new(&python)
        .arg("selftest.py")
        .current_dir(&directory)
        .output()
        .expect("python runs");

    let said = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "the client's own checks must pass before it is trusted with a conversation:\n{said}"
    );
    assert!(
        output.stdout.is_empty(),
        "a self-test that passes says nothing:\n{said}"
    );
}
