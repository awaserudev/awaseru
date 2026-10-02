//! SHA-256, because the configuration declares the software's identity by it
//! (§6.6).
//!
//! The function comes from `sha2`; what is here is the two things the
//! configuration actually needs — a digest of a file, and a digest written the
//! way a configuration writes one.
//!
//! # Why this is a dependency and not written out
//!
//! It was written out, once, and `doc/dependencies.md` carries why that was
//! wrong. The short of it: the argument for writing it out was that the
//! function has a published specification and published test vectors, so it is
//! checkable to a certainty. The hand-written one passed those vectors **and
//! was wrong** — a short incremental update threw away bytes it had just
//! buffered, which a vector arriving in one call cannot reach. The thing that
//! caught it was an extra test somebody thought to write, which is not a
//! property a project can rely on.
//!
//! The tests below stay, and are now about the integration rather than the
//! arithmetic: that this is SHA-256 and not one of its relatives, that the
//! hexadecimal comes out in the order a configuration writes it, and that
//! reading a file in pieces gives the same answer as hashing it whole — which
//! is this module's own loop and the one part still worth checking.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

/// How much is read at a time. A cartridge-sized file should not have to be
/// held in memory to be identified.
const CHUNK: usize = 64 * 1024;

/// The digest of some bytes, as a configuration writes it.
pub fn of_bytes(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The digest of a file, read in pieces.
pub fn of_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Lower-case hexadecimal, most significant byte first — which is how every
/// tool that prints a digest prints one, and therefore how a configuration
/// written by hand will have it.
fn hex(digest: &[u8]) -> String {
    use std::fmt::Write;
    digest.iter().fold(String::with_capacity(64), |mut s, byte| {
        // Writing to a String cannot fail.
        let _ = write!(s, "{byte:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two published vectors, which here say *which* function this is and that
    /// the hexadecimal is in the right order — not that the arithmetic is
    /// right, which is `sha2`'s business and not this module's.
    ///
    /// Without this, swapping `Sha256` for `Sha512_256` or reversing the hex
    /// would compile and produce a confident, wrong identity for every file.
    #[test]
    fn this_is_sha_256_and_the_hexadecimal_reads_the_usual_way() {
        assert_eq!(
            of_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            of_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(of_bytes(b"abc").len(), 64, "thirty-two bytes in hexadecimal");
        assert!(
            of_bytes(b"abc").chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
            "lower case, because that is what a person writing a configuration will paste"
        );
    }

    /// **This module's own code.** `of_file` reads in pieces; hashing the same
    /// bytes whole must give the same answer. The file is deliberately larger
    /// than the read buffer and not a whole number of them, so the loop and its
    /// last short read are both covered.
    #[test]
    fn hashing_a_file_in_pieces_agrees_with_hashing_the_bytes_whole() {
        let dir = std::env::temp_dir().join("awaseru-digest-tests");
        std::fs::create_dir_all(&dir).expect("a directory");
        let path = dir.join("more-than-two-buffers.bin");

        let bytes: Vec<u8> = (0..CHUNK * 2 + 3)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        std::fs::write(&path, &bytes).expect("write");

        assert_eq!(of_file(&path).expect("it reads"), of_bytes(&bytes));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_that_is_not_there_is_an_error_and_not_the_digest_of_nothing() {
        let err = of_file(Path::new("/nonexistent/awaseru/no-such-file")).expect_err("not there");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    /// Against the system's own implementation, which is a second one written
    /// by somebody else. Lengths either side of a block boundary, because that
    /// is where a hash is most often wrong.
    ///
    /// **What this does not cover**: anything, on a machine with no
    /// `sha256sum`. It says so rather than passing quietly.
    #[test]
    fn it_agrees_with_another_implementation() {
        let Some(reference) = system_sha256(b"abc") else {
            eprintln!("NOT COVERED: no system sha256sum to compare against");
            return;
        };
        assert_eq!(
            reference, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "the system tool must itself be sha-256 before it is worth comparing to"
        );

        for len in [0usize, 1, 55, 56, 63, 64, 65, 127, 128, 129] {
            let input = vec![b'a'; len];
            assert_eq!(
                of_bytes(&input),
                system_sha256(&input).expect("the tool was there a moment ago"),
                "for {len} bytes"
            );
        }
    }

    /// Runs the system's `sha256sum` over the bytes, if there is one.
    fn system_sha256(bytes: &[u8]) -> Option<String> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut child = Command::new("sha256sum")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        child.stdin.take()?.write_all(bytes).ok()?;
        let out = child.wait_with_output().ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        Some(text.split_whitespace().next()?.to_string())
    }
}
