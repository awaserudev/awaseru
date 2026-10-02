//! Does a reference come up where it says it does? — §4.7's origin.
//!
//! Run against the generated fixture, so the position can be asserted against
//! a number this project chose: the fixture's reset vector points at `$8000`,
//! and that is where a reference at power-on must be.
//!
//! One test, because only one reference may exist per process. The other
//! startup — as the backend was found, with memory left random — is checked
//! where it can be: in a unit test over what `Origin` says, and by U8's
//! separate processes.

use awaseru_core::{Bound, Platform};
use awaseru_snes::memory::ZEROED_AT_POWER_ON;
use awaseru_snes::{Reference, Startup, fixture};
use std::path::PathBuf;

#[test]
fn a_reference_comes_up_at_power_on_with_memory_settled() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };

    let dir = std::env::temp_dir().join("awaseru-origin-test");
    std::fs::create_dir_all(&dir).expect("a directory");
    let rom = dir.join("fixture.sfc");
    std::fs::write(&rom, fixture::image()).expect("write the image");

    let reference =
        Reference::open_with(&library, &dir, &rom, Startup::default()).expect("it comes up");

    // ---- where it is ------------------------------------------------------
    let origin = reference.origin().clone();
    eprintln!("origin: {origin}");
    assert!(origin.at_power_on, "the default is the reproducible position");
    assert_eq!(
        reference.cycles(),
        Some(0),
        "power-on means cycle zero: nothing has executed yet"
    );

    // The fixture's reset vector is `$8000` because this project put it there,
    // so this is an assertion about content we own (§11.3).
    let position = reference.position().to_string();
    assert!(
        position.contains("8000"),
        "the reference should be at the fixture's reset vector, and is at {position}"
    );

    // ---- what it settled --------------------------------------------------
    // Every memory in the set that exists for this software was zeroed, and
    // the ones that do not exist are absent rather than silently counted.
    assert!(
        !origin.memory_zeroed.is_empty(),
        "the default startup zeroes memory"
    );
    for name in &origin.memory_zeroed {
        assert!(
            ZEROED_AT_POWER_ON.iter().any(|(_, n)| n == name),
            "`{name}` is reported zeroed and is not in the declared set"
        );
    }
    // The fixture declares no battery memory, so that one should be absent
    // from the report rather than claimed.
    assert!(
        !origin.memory_zeroed.contains(&"save-ram"),
        "this fixture has no battery memory, so nothing should claim to have zeroed it: {:?}",
        origin.memory_zeroed
    );

    for region in reference.regions().iter().filter(|r| r.access.writable()) {
        let bytes = reference.read(&region.name).expect("it reads");
        assert!(
            bytes.iter().all(|&b| b == 0),
            "`{}` should be zero at a zeroed power-on; {} of {} bytes are not",
            region.name,
            bytes.iter().filter(|&&b| b != 0).count(),
            bytes.len()
        );
    }

    // And the cartridge's program data was **not** touched, which is what says
    // the zeroing went to the memories it names and not to everything.
    let program = reference.read("program-rom").expect("it reads");
    assert!(
        program.iter().any(|&b| b != 0),
        "the program data must not have been zeroed"
    );
    assert_eq!(
        program[0], 0x78,
        "and it is this project's program: the fixture begins by disabling interrupts"
    );

    // ---- and it runs from there ------------------------------------------
    let mut reference = reference;
    let stop = reference.run(Bound::Frames(3)).expect("three frames");
    assert!(stop.arrived(), "{stop}");
    let work = reference.read("work-ram").expect("it reads");
    let at = fixture::expected::WORK_PATTERN_AT;
    let pattern = fixture::expected::work_pattern();
    assert_eq!(
        &work[at..at + pattern.len()],
        &pattern[..],
        "running from power-on must execute the fixture from its beginning, so its pattern is \
         where the program puts it"
    );

    // What this does NOT cover: that two *processes* agree. One reference per
    // process means this test cannot answer §2.5's across-processes half, and
    // the done-condition test is where that is answered by spawning them.
}
