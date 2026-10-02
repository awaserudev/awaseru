//! Writes the gated fixture (§11.3) to a file — the one that will not proceed
//! until a button is held.
//!
//!     cargo run -p awaseru-snes --example dump_gated -- /tmp/gated.sfc

fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: dump_gated <path>");
        return std::process::ExitCode::FAILURE;
    };
    let image = awaseru_snes::fixture::image_needing_input();
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
