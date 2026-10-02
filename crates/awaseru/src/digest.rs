//! SHA-256, because the configuration declares the software's identity by it
//! (§6.6) and a hash crate is a dependency this project has not taken.
//!
//! # Why this is written out rather than depended on
//!
//! §17.2 makes every dependency a decision recorded before it is used, and
//! `doc/dependencies.md` says what was weighed here. The short of it: this hash
//! is used as an **identity**, not as a security primitive. Nothing here
//! defends against an adversary choosing the input; it answers "is this the
//! same file the configuration was written against", and a wrong answer stops
//! the tool rather than letting something through.
//!
//! That makes it a function with a published specification and published test
//! vectors, which is the rare case where writing it out is cheaper than owning
//! a dependency — and it is checkable to a certainty, which is the only reason
//! this is acceptable at all. The vectors in the tests below are the ones from
//! the standard, and one more measured against the system's own tool.
//!
//! If the project ever needs this hash to resist an adversary, that is a
//! different requirement and a crate becomes a case worth making.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// The eight words the state starts at: the fractional parts of the square
/// roots of the first eight primes.
const INITIAL_STATE: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// One per round: the fractional parts of the cube roots of the first
/// sixty-four primes.
const ROUND_CONSTANTS: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const BLOCK: usize = 64;

/// A hash being computed.
#[derive(Debug, Clone)]
pub struct Sha256 {
    state: [u32; 8],
    /// Bytes not yet part of a whole block.
    pending: [u8; BLOCK],
    pending_len: usize,
    /// Total bytes seen, which the padding has to state.
    total: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Sha256 {
            state: INITIAL_STATE,
            pending: [0; BLOCK],
            pending_len: 0,
            total: 0,
        }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.total = self.total.wrapping_add(bytes.len() as u64);
        let mut bytes = bytes;

        if self.pending_len > 0 {
            let room = BLOCK - self.pending_len;
            let take = room.min(bytes.len());
            self.pending[self.pending_len..self.pending_len + take].copy_from_slice(&bytes[..take]);
            self.pending_len += take;
            bytes = &bytes[take..];
            if self.pending_len < BLOCK {
                // Still short of a block. Returning here matters: the tail of
                // this function sets `pending_len` from what is left of
                // `bytes`, which is now nothing — and falling through would
                // therefore throw away the bytes just buffered.
                return;
            }
            let block = self.pending;
            self.compress(&block);
            self.pending_len = 0;
        }

        let (blocks, rest) = bytes.as_chunks::<BLOCK>();
        for block in blocks {
            self.compress(block);
        }

        self.pending[..rest.len()].copy_from_slice(rest);
        self.pending_len = rest.len();
    }

    /// The digest, as thirty-two bytes.
    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.total.wrapping_mul(8);

        // A single one bit, then zeros, then the length. The length needs eight
        // bytes at the end of a block, so sometimes a whole extra block.
        let mut tail = [0u8; BLOCK * 2];
        tail[0] = 0x80;
        let pad = if self.pending_len < BLOCK - 8 {
            BLOCK - 8 - self.pending_len
        } else {
            BLOCK * 2 - 8 - self.pending_len
        };
        tail[pad..pad + 8].copy_from_slice(&bits.to_be_bytes());

        // `update` would add these to the total, which must not change now, so
        // the blocks are fed to the compression directly.
        let mut block = self.pending;
        let mut filled = self.pending_len;
        for &byte in &tail[..pad + 8] {
            block[filled] = byte;
            filled += 1;
            if filled == BLOCK {
                self.compress(&block);
                filled = 0;
            }
        }
        debug_assert_eq!(filled, 0, "padding must end exactly on a block boundary");

        let mut digest = [0u8; 32];
        let (words, _) = digest.as_chunks_mut::<4>();
        for (out, word) in words.iter_mut().zip(self.state) {
            *out = word.to_be_bytes();
        }
        digest
    }

    fn compress(&mut self, block: &[u8; BLOCK]) {
        let mut w = [0u32; 64];
        let (words, _) = block.as_chunks::<4>();
        for (slot, bytes) in w[..16].iter_mut().zip(words) {
            *slot = u32::from_be_bytes(*bytes);
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = s1
                .wrapping_add(w[t - 7])
                .wrapping_add(s0)
                .wrapping_add(w[t - 16]);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;

        for t in 0..64 {
            let big_s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(big_s1)
                .wrapping_add(choose)
                .wrapping_add(ROUND_CONSTANTS[t])
                .wrapping_add(w[t]);
            let big_s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = big_s0.wrapping_add(majority);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        for (slot, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
}

/// A digest written the way a configuration writes it: lower-case hexadecimal.
pub fn hex(digest: [u8; 32]) -> String {
    use std::fmt::Write;
    digest.iter().fold(String::with_capacity(64), |mut s, byte| {
        // Writing to a String cannot fail.
        let _ = write!(s, "{byte:02x}");
        s
    })
}

/// The digest of a file, read in pieces so that a cartridge-sized file does not
/// have to be held in memory twice.
pub fn of_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(hasher.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex(hasher.finish())
    }

    /// The published vectors. These are the whole justification for writing
    /// this out instead of depending on it: an implementation that passes them
    /// is the function, and one that does not is caught here rather than by a
    /// configuration that refuses a file it should have accepted.
    #[test]
    fn the_published_vectors_come_out() {
        assert_eq!(
            of(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            of(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            of(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            "fifty-six bytes: the case where the length does not fit in the last block \
             and the padding needs a second one"
        );
        assert_eq!(
            of(&[b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
            "a million bytes, which is many blocks"
        );
    }

    /// The boundaries where the padding decides between one block and two.
    ///
    /// Off-by-one here is the classic way to write a hash that is right for
    /// most inputs, which is worse than one that is wrong for all of them. The
    /// comparison is against the system's own `sha256sum` — a second
    /// implementation, written by somebody else — rather than against numbers
    /// recorded here, because a number recorded here would have come from this
    /// code and proved nothing.
    ///
    /// **What this does not cover**: anything, on a machine with no
    /// `sha256sum`. It says so rather than passing quietly.
    #[test]
    fn the_lengths_around_a_block_boundary_agree_with_another_implementation() {
        let Some(reference) = system_sha256(b"abc") else {
            eprintln!("NOT COVERED: no system sha256sum to compare against");
            return;
        };
        assert_eq!(
            reference, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "the system tool must itself be sha-256 before it is worth comparing to"
        );

        // 55 and 56 are the two sides of the padding's decision; 63, 64 and 65
        // are the two sides of a whole block.
        for len in [0usize, 1, 54, 55, 56, 57, 63, 64, 65, 119, 120, 127, 128, 129] {
            let input = vec![b'a'; len];
            assert_eq!(
                of(&input),
                system_sha256(&input).expect("the tool was there a moment ago"),
                "for {len} bytes"
            );
        }
    }

    /// Feeding the same bytes in awkward pieces must give the same digest.
    /// This is the buffering, which is the part a published vector does not
    /// exercise: a vector arrives in one call.
    #[test]
    fn the_digest_does_not_depend_on_how_the_bytes_arrive() {
        let input: Vec<u8> = (0..500u32).map(|i| (i * 7 % 251) as u8).collect();
        let whole = of(&input);

        for piece in [1usize, 3, 63, 64, 65, 127, 128, 499] {
            let mut hasher = Sha256::new();
            for chunk in input.chunks(piece) {
                hasher.update(chunk);
            }
            assert_eq!(
                hex(hasher.finish()),
                whole,
                "fed in pieces of {piece} it came out different"
            );
        }
    }

    /// `of_file` reads in pieces, which is a different path from `update`
    /// called once — and the one the configuration actually uses, over a file
    /// of a few megabytes. A file larger than the read buffer is what makes
    /// this cover the loop rather than a single read.
    #[test]
    fn hashing_a_file_agrees_with_hashing_the_same_bytes_in_memory() {
        let dir = std::env::temp_dir().join("awaseru-digest-tests");
        std::fs::create_dir_all(&dir).expect("a directory");
        let path = dir.join("a-few-buffers-worth.bin");

        // Over two of the 64 KiB reads, and not a whole number of them.
        let bytes: Vec<u8> = (0..200_003u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        std::fs::write(&path, &bytes).expect("write");

        assert_eq!(of_file(&path).expect("it reads"), of(&bytes));
        let _ = std::fs::remove_file(&path);
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
