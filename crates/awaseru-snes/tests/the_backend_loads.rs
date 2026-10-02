//! Does the binding actually reach a built backend?
//!
//! The library is supplied, not shipped (§1.5, §11.1): these read its path from
//! `AWASERU_TEST_BACKEND` and skip when it is unset, so the suite runs for
//! someone who has not built one. The two failure paths need no backend at all
//! and always run.

use awaseru_snes::Backend;
use std::path::{Path, PathBuf};

/// The backend to test against, or `None` when nobody said where one is.
fn backend_path() -> Option<PathBuf> {
    std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from)
}

macro_rules! or_skip {
    ($e:expr, $why:literal) => {
        match $e {
            Some(v) => v,
            None => {
                eprintln!("SKIPPED: {}", $why);
                return;
            }
        }
    };
}

/// A path with no library at it fails as a library error, naming the path.
///
/// This one needs no backend, so it runs everywhere. It is the check that says
/// the error type distinguishes "there is nothing here" from "what is here is
/// the wrong thing" — which the next test covers.
#[test]
fn a_missing_library_fails_as_a_library_error() {
    let err = Backend::open(Path::new("/nonexistent/awaseru/not-a-backend.so"))
        .expect_err("there is no library at that path");
    assert!(
        matches!(err, awaseru_snes::LoadError::Library { .. }),
        "expected a library error, got {err:?}"
    );
    let said = err.to_string();
    assert!(
        said.contains("not-a-backend.so"),
        "the message must name the path that failed, said: {said}"
    );
}

/// A real library that is not this backend fails as a **symbol** error, naming
/// the symbol that is missing.
///
/// This is the one that earns its place: without it, `open` could resolve
/// nothing and report success, and the failure would arrive much later as a
/// call into a pointer nobody set.
#[test]
fn a_library_that_is_not_the_backend_fails_as_a_symbol_error() {
    // Any real shared library will do. Several candidates, because the path of
    // the C library is a property of the distribution, not of this test.
    let candidates = [
        "/lib/x86_64-linux-gnu/libc.so.6",
        "/usr/lib/x86_64-linux-gnu/libc.so.6",
        "/lib64/libc.so.6",
    ];
    let found = candidates.iter().map(Path::new).find(|p| p.exists());
    let library = or_skip!(found, "no system C library found at the usual paths");

    let err = Backend::open(library).expect_err("that library is not a backend");
    match err {
        awaseru_snes::LoadError::Symbol { name, .. } => {
            assert_eq!(
                name, "GetMesenVersion",
                "the first symbol missing should be the first one asked for"
            );
        }
        other => panic!("expected a symbol error, got {other:?}"),
    }
}

/// The backend opens, and answers.
///
/// **What this does not cover**: that the version packing is the right reading
/// of that number. The decomposition is arithmetic checked in the unit tests,
/// and this shows the library returns something non-zero that survives a round
/// trip — but whether the three bytes *mean* major, minor and revision is
/// settled where the configuration declares a version and a mismatch refuses
/// (§16.1), not here.
#[test]
fn the_backend_reports_a_version_and_a_build_date() {
    let path = or_skip!(backend_path(), "set AWASERU_TEST_BACKEND to a built backend library");
    let backend = Backend::open(&path).expect("the backend opens");

    let version = backend.version();
    eprintln!("version {version} (raw 0x{:08X})", version.raw);
    eprintln!("built   {}", backend.build_date());

    assert_ne!(version.raw, 0, "a backend reporting version zero is not one");
    assert_eq!(
        version.to_raw(),
        version.raw,
        "the decode must account for every bit the backend set; a difference here \
         means the version carries something this binding does not understand"
    );
    assert!(
        version.major > 0,
        "a major version of zero would mean the packing is read wrong"
    );

    let built = backend.build_date();
    assert!(!built.is_empty(), "the build date must say something");
    assert!(
        built.is_ascii(),
        "the build date is a C string from the library and should be plain ASCII, got {built:?}"
    );
}
