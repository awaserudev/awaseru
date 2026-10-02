//! Writes the generated fixture (§11.3) to a file, for looking at it with a
//! disassembler or pointing an emulator at it by hand.
//!
//!     cargo run -p awaseru-snes --example dump_fixture -- /tmp/fixture.sfc
//!
//! The tests build the image in memory and do not need this; it exists because
//! a fixture nobody can open is a fixture nobody will check.

fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: dump_fixture <path>");
        return std::process::ExitCode::FAILURE;
    };
    let image = awaseru_snes::fixture::image();
    match std::fs::write(&path, &image) {
        Ok(()) => {
            println!("{} bytes written to {path}", image.len());
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("could not write {path}: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
