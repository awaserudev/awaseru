//! **This file is M0's done-condition.**
//!
//! > The host reads a configuration, selects a platform backend, drives a
//! > reference to a position, and prints one region's bytes.
//!
//! It runs the host — `session::run`, the same function `main` calls — against
//! a real configuration, so that every link in the chain is the real one: two
//! configuration files resolved (§6.1, §6.2), the software's identity checked
//! against what the shared file declares (§6.6), a backend found by name
//! (§7.1), its version checked against what the library reports (§16.1), a
//! bounded run (§4.2), and one region's bytes.
//!
//! # Where the configuration comes from, and why not from here
//!
//! `AWASERU_TEST_CONFIG_DIR` names a directory holding `awaseru.toml` and
//! `awaseru.local.toml`. The test skips when it is unset — §11.3's third
//! bullet, which is how this project tests against software it may not
//! redistribute.
//!
//! The configuration is **not** written by this test, and that is the point. A
//! test that generated the shared file would have to put a hash in it, and the
//! only hash available would be one computed from the same file it is
//! checking — so §6.6 would pass by construction and prove nothing. Reading a
//! configuration somebody else wrote is what makes the identity check real.
//!
//! # What it asserts, and what it will not
//!
//! Nothing here says what is at any address. The software is the runner's and
//! is not in this repository (§11.2). What is asserted is that the plumbing
//! carries: the regions the backend declares exist and have the sizes it
//! declares, a read is as long as the region says, the bytes are not all zero,
//! the run arrives and reports a position of the kind the bound implies, and
//! more frames get further than fewer.

use std::path::{Path, PathBuf};

use awaseru::session::{self, Plan};
use awaseru_core::{Bound, Position};

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("AWASERU_TEST_CONFIG_DIR").map(PathBuf::from)
}

fn home() -> PathBuf {
    std::env::temp_dir().join("awaseru-done-condition-home")
}

fn plan(dir: &Path, bound: Bound) -> Plan {
    Plan {
        shared: dir.join("awaseru.toml"),
        local: dir.join("awaseru.local.toml"),
        home: home(),
        bound,
        // Not a name chosen here: `None` means the first region the backend
        // reports, because §3.1 says the host must not assume which names
        // exist.
        region: None,
        offset: 0,
        length: 256,
    }
}

/// The whole chain, once.
///
/// One test, because the backend's emulator is a single global object and only
/// one reference can exist in a process — the backend crate's `reference`
/// module says why, and §13's Q10 is what would change it.
#[test]
fn the_host_reads_a_configuration_drives_a_reference_and_produces_a_regions_bytes() {
    let Some(dir) = config_dir() else {
        eprintln!(
            "SKIPPED: set AWASERU_TEST_CONFIG_DIR to a directory holding awaseru.toml and \
             awaseru.local.toml (§11.3)"
        );
        return;
    };
    std::fs::create_dir_all(home()).expect("a directory for the backend's own files");

    // ---- the chain, end to end -------------------------------------------
    let outcome = match session::run(&plan(&dir, Bound::Frames(1))) {
        Ok(outcome) => outcome,
        Err(e) => panic!("the host did not get through: {e}"),
    };

    // Getting here at all means the software's hash matched what the shared
    // file declares (§6.6) and the library's version matched what the
    // configuration declares (§16.1). Both of those refuse rather than warn,
    // so there is no quieter way to have arrived.
    assert!(
        !outcome.emulator.is_empty(),
        "the reference is named by the configuration, and reports call it that (§6.4)"
    );
    assert!(!outcome.backend.is_empty());
    assert!(
        !outcome.version.reported.is_empty(),
        "the backend says what version it is, which is what §16.1 checked"
    );

    // ---- regions are the backend's, named and enumerated (§3.1) ----------
    assert!(
        !outcome.regions.is_empty(),
        "a reference exposing nothing has nothing to compare"
    );
    for region in outcome.regions.iter() {
        assert!(
            region.size > 0,
            "`{}` is present with no bytes — an empty memory is absent (§3.5)",
            region.name
        );
    }
    eprintln!(
        "regions: {}",
        outcome
            .regions
            .iter()
            .map(|r| format!("{}={}", r.name, r.size))
            .collect::<Vec<_>>()
            .join(" ")
    );

    assert!(
        outcome.regions.get(&outcome.region.name).is_some(),
        "the region read must be one the backend declared, got `{}`",
        outcome.region.name
    );
    assert_eq!(
        outcome.regions.iter().next().map(|r| r.name.clone()),
        Some(outcome.region.name.clone()),
        "with no name asked for, it must be the backend's first"
    );

    // ---- one region's bytes came out -------------------------------------
    assert_eq!(
        outcome.bytes.len(),
        256.min(outcome.region.size),
        "the read must be as long as it was asked for; a short read would compare equal over \
         bytes nobody looked at"
    );
    assert!(
        outcome.bytes.iter().any(|&b| b != 0),
        "every byte came back zero, which means the read went somewhere other than the memory"
    );

    // And they print. This is the literal "prints one region's bytes".
    let dumped = session::hexdump(&outcome.bytes, outcome.offset);
    assert_eq!(
        dumped.lines().count(),
        outcome.bytes.len().div_ceil(16),
        "sixteen bytes to a line"
    );
    eprintln!(
        "{} from {:#x}:\n{}",
        outcome.region.name,
        outcome.offset,
        dumped.lines().take(4).collect::<Vec<_>>().join("\n")
    );

    // ---- it was driven to a position, and the position says what kind ----
    assert!(outcome.stop.arrived(), "{}", outcome.stop);
    let Position::FrameBoundary { frame } = outcome.stop.position else {
        panic!(
            "a run bounded by frames must end at a frame boundary, ended at {}",
            outcome.stop.position
        )
    };
    assert!(
        !outcome.stop.position.is_instruction_boundary(),
        "§3.4: it must not claim to be an instruction boundary as well"
    );
    eprintln!("started {} and stopped {}", outcome.started, outcome.stop);

    // ---- and the bound is honoured, not approximated ---------------------
    // Dropped first, because only one reference may exist at a time.
    drop(outcome);
    let further = session::run(&plan(&dir, Bound::Frames(4))).expect("a second pass");
    let Position::FrameBoundary {
        frame: further_frame,
    } = further.stop.position
    else {
        panic!("expected a frame boundary, got {}", further.stop.position)
    };
    assert!(
        further_frame > frame,
        "four frames must get further than one: {further_frame} is not past {frame}"
    );

    // What this does NOT cover, stated rather than left to be assumed:
    //
    // - that the two passes start from the same place. They do not, and §13's
    //   Q9 is why: this backend begins executing when software loads, so the
    //   frame a session first stops at is not reproducible. That is exactly why
    //   the assertion above is `>` and not an equality — an equality here would
    //   be writing down a number that is not a property of anything.
    // - that the regions hold what the software is supposed to put there. That
    //   is a fact about software this repository does not contain (§11.2), and
    //   asserting it is M3's work with its own fixtures (§11.3).
    // - that a library inside the supported version list behaves as measured
    //   (§16.2). The version check compares labels; §16.5's conformance
    //   fixtures are what would compare behaviour.
    // - §16.1's *mismatch* leg, against a real library. While the backend
    //   crate's supported list has one entry, a declared version that is
    //   supported and yet differs from what the library reports cannot be
    //   written — so the only refusal a real run can produce is the
    //   unsupported one. The mismatch leg is covered by a unit test over the
    //   rule, and by nothing here; it is said rather than counted.
}

/// A configuration that is not there is refused, naming the file. This one
/// needs nothing supplied, so it runs everywhere — including on a machine with
/// no backend, where the test above only skips.
#[test]
fn a_configuration_that_is_not_there_is_refused_by_name() {
    let plan = Plan {
        shared: PathBuf::from("/nonexistent/awaseru/awaseru.toml"),
        local: PathBuf::from("/nonexistent/awaseru/awaseru.local.toml"),
        home: home(),
        bound: Bound::Frames(1),
        region: None,
        offset: 0,
        length: 16,
    };
    let err = session::run(&plan).expect_err("there is no configuration there");
    let said = err.to_string();
    assert!(
        said.contains("awaseru.toml"),
        "the refusal must name the file it could not read, said: {said}"
    );
}
