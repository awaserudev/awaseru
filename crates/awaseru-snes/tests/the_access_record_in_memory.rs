//! The access record over memory that is **not** the cartridge.
//!
//! §10's execution coverage says "which bytes of the software executed", and
//! the word software hides an assumption: that the code is in the cartridge. On
//! real software it need not be — M5 measured a routine running out of the
//! machine's own memory — so a record that covered only the cartridge would
//! answer "nothing ran" about the most interesting code there is.
//!
//! One test, because only one reference may exist per process. The rest of the
//! record's measurements are in `the_access_record.rs`.

use std::path::PathBuf;
use std::time::Duration;

use awaseru_core::{Bound, Platform};
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::{Reference, Startup};

/// The record is not the cartridge's alone — which matters, because real
/// software runs code from the machine's own memory.
///
/// Nothing in this fixture executes from work memory, so what is checked is
/// that the record **exists** there and is telling the truth about it: the
/// routine's output span is written and not executed. A record that did not
/// cover the region would report nothing at all, and a record that answered
/// "executed" everywhere would fail the second half.
#[test]
fn the_record_covers_memory_that_is_not_the_cartridge() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };
    let dir = std::env::temp_dir().join("awaseru-access-record-ram");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");

    let mut reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");
    reference.set_watchdog(Duration::from_secs(60));
    reference
        .run(Bound::Address {
            address: expected::ROUTINE_RETURN,
            within: 20_000,
        })
        .expect("the routine returns");

    let output = reference
        .access_record(
            "work-ram",
            expected::ROUTINE_OUTPUT_AT,
            expected::ROUTINE_LENGTH,
        )
        .expect("work memory has a record too");
    assert_eq!(output.len(), expected::ROUTINE_LENGTH);
    assert!(
        output.iter().all(|c| c.writes > 0),
        "the routine wrote every byte of its output"
    );
    assert!(
        output.iter().all(|c| c.executions == 0),
        "and executed none of them — a record that said otherwise would be \
         answering about the region rather than about what happened in it"
    );
}
