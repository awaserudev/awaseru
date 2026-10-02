//! A program this project owns — §11.3.
//!
//! Expected values taken from software this project may not redistribute cannot
//! appear in its tests, so the primary fixture is a program assembled here,
//! whose behaviour is defined here. This is that program: a few dozen bytes of
//! machine code that writes patterns nobody has to guess at into the memories
//! the tests read, and then spins while incrementing a counter so that a
//! bounded run always has somewhere to be and something that moves.
//!
//! # Why it is assembled byte by byte
//!
//! An assembler would be a dependency, and `doc/dependencies.md` records why it
//! was not taken: a fixture whose correctness depends on a tool nobody reads is
//! a worse fixture. So the bytes are written out with the mnemonic beside each
//! one and the branch offsets worked out by hand — and checked by a test that
//! recomputes them, because by hand is exactly where that goes wrong.
//!
//! # What the program does
//!
//! ```text
//!         SEI                  ; no interrupts
//!         CLC : XCE            ; native mode
//!         SEP #$30             ; eight-bit accumulator and index registers
//!         LDA #$00 : PHA : PLB ; data bank zero, so the register writes below
//!                              ; reach the hardware and not the memory mirror
//!         STZ $10              ; the counter starts at a value we chose
//!         LDX #$00
//! fill:   TXA : EOR #$5A
//!         STA $7E0100,X        ; work memory, 0x100 through 0x1FF
//!         INX : BNE fill
//!         LDA #$C3 : STA $7E00FF   ; a sentinel either side of the pattern
//!         LDA #$3C : STA $7E0200
//!         STZ $2121            ; palette address := 0
//!         LDX #$00
//! pal:    TXA : EOR #$A5
//!         STA $2122            ; palette data, 256 writes. A colour is fifteen
//!                              ; bits, so every odd byte reads back masked.
//!         INX : BNE pal
//! spin:   INC $0010            ; work memory 0x10, through the bank-zero mirror
//!         BRA spin
//! ```
//!
//! Nothing in it is a title, a trademark or anybody's copyright: it is an
//! exclusive-or, a store and a loop.
//!
//! # Why it writes its own margins, and zeroes its own counter
//!
//! Because the memory it writes into **does not start empty, and does not start
//! the same way twice**. Measured on the first backend: the work memory at
//! power-on is filled pseudo-randomly, and two processes loading the same image
//! see different bytes outside what the program writes. That is the case §2.5
//! names, and §13's Q13 carries it.
//!
//! So nothing in this fixture's expectations may rest on memory the program did
//! not write. The sentinels either side of the pattern are what let a test
//! assert the pattern's *boundaries* — a store off by a page would put pattern
//! bytes where a sentinel should be — without reading a single byte the program
//! left alone. The counter is zeroed for the same reason: it is a value this
//! program chose, not one the backend happened to leave.

/// Where the program is assembled to, and what the reset vector points at.
const ORIGIN: u16 = 0x8000;

/// 32 KiB. The smallest size this mapping is defined for, and small enough that
/// the whole image can sit in a test's memory without anybody thinking about it.
const ROM_BYTES: usize = 0x8000;

/// Where the header goes in the image, for the mapping this uses.
const HEADER_AT: usize = 0x7FC0;

/// What the program leaves behind, so that a test asserts a value written here
/// rather than one read off a run.
pub mod expected {
    /// Work memory from here holds the pattern below.
    pub const WORK_PATTERN_AT: usize = 0x0100;
    /// Work memory here holds a counter the spin loop increments, so that a
    /// comparison over this region has something that moves (§2.2). The
    /// program zeroes it first, so where it starts is this program's choice
    /// and not whatever the backend's power-on fill left (§13's Q13).
    pub const WORK_COUNTER_AT: usize = 0x0010;

    /// The byte immediately before the pattern, and the byte immediately
    /// after. The program writes both, so a test can assert where the pattern
    /// ends without reading memory the program left alone — which on this
    /// backend is not the same twice.
    pub const SENTINEL_BEFORE_AT: usize = WORK_PATTERN_AT - 1;
    pub const SENTINEL_BEFORE: u8 = 0xC3;
    pub const SENTINEL_AFTER_AT: usize = WORK_PATTERN_AT + 0x100;
    pub const SENTINEL_AFTER: u8 = 0x3C;

    /// Where `image_needing_input`'s gate writes its sentinel once the button
    /// it waits for has been held, and what it writes.
    ///
    /// Zero until then — which the program itself writes, so the test does not
    /// rest on memory nobody wrote.
    pub const GATE_SENTINEL_AT: usize = 0x0210;
    pub const GATE_SENTINEL: u8 = 0x5A;
    /// The gated program's counter is the same address as the first
    /// fixture's, and it does **not** move until the gate opens.
    pub const GATE_COUNTER_AT: usize = WORK_COUNTER_AT;
    /// The palette from here holds the pattern below.
    pub const PALETTE_PATTERN_AT: usize = 0x0000;

    /// 256 bytes, `i ^ 0x5A`.
    ///
    /// Chosen so that no byte is zero where its index is zero and no byte
    /// equals its own index: a pattern of `i` alone would agree with a
    /// correctly-working read of an incrementing buffer somewhere else, and a
    /// pattern of a constant would agree with any region that happened to hold
    /// it.
    pub fn work_pattern() -> Vec<u8> {
        (0..256u32).map(|i| (i as u8) ^ 0x5A).collect()
    }

    /// 256 bytes — a different constant from the work memory's, so that a read
    /// of the wrong region cannot pass for a read of the right one.
    ///
    /// The program writes `i ^ 0xA5`, and **what comes back is not quite that**.
    /// A colour here is fifteen bits, not sixteen, so the top bit of each
    /// colour's high byte is not stored: every odd byte reads back masked with
    /// `0x7F`.
    ///
    /// This was got wrong first, by writing down what the program stores and
    /// calling it what the memory holds. The test failed, which is the right
    /// outcome — the hardware is not a detail of the program and a fixture's
    /// expectations have to account for both.
    pub fn palette_pattern() -> Vec<u8> {
        (0..256u32)
            .map(|i| {
                let written = (i as u8) ^ 0xA5;
                if i % 2 == 1 { written & 0x7F } else { written }
            })
            .collect()
    }
}

/// The program, as bytes, with the mnemonic for each.
///
/// Offsets are from `ORIGIN`. The three branch offsets are the only arithmetic
/// here and `tests::the_branches_land_where_the_labels_are` recomputes all of
/// them.
const FILL: usize = 0x0D;
const PAL: usize = 0x28;
const SPIN: usize = 0x31;

fn program() -> Vec<u8> {
    let mut code: Vec<u8> = Vec::new();
    // 0x00
    code.extend([0x78]); // SEI
    code.extend([0x18, 0xFB]); // CLC : XCE          -> native
    code.extend([0xE2, 0x30]); // SEP #$30           -> 8-bit A, X, Y
    code.extend([0xA9, 0x00]); // LDA #$00
    code.extend([0x48, 0xAB]); // PHA : PLB          -> data bank 0
    code.extend([0x64, 0x10]); // STZ $10            -> the counter starts at 0
    code.extend([0xA2, 0x00]); // LDX #$00
    debug_assert_eq!(code.len(), FILL);
    // fill:
    code.extend([0x8A]); // TXA
    code.extend([0x49, 0x5A]); // EOR #$5A
    code.extend([0x9F, 0x00, 0x01, 0x7E]); // STA $7E0100,X
    code.extend([0xE8]); // INX
    code.extend([0xD0, branch_to(code.len() + 2, FILL)]); // BNE fill
    code.extend([0xA9, 0xC3]); // LDA #$C3
    code.extend([0x8F, 0xFF, 0x00, 0x7E]); // STA $7E00FF    -> sentinel before
    code.extend([0xA9, 0x3C]); // LDA #$3C
    code.extend([0x8F, 0x00, 0x02, 0x7E]); // STA $7E0200    -> sentinel after
    code.extend([0x9C, 0x21, 0x21]); // STZ $2121
    code.extend([0xA2, 0x00]); // LDX #$00
    debug_assert_eq!(code.len(), PAL);
    // pal:
    code.extend([0x8A]); // TXA
    code.extend([0x49, 0xA5]); // EOR #$A5
    code.extend([0x8D, 0x22, 0x21]); // STA $2122
    code.extend([0xE8]); // INX
    code.extend([0xD0, branch_to(code.len() + 2, PAL)]); // BNE pal
    debug_assert_eq!(code.len(), SPIN);
    // spin:
    code.extend([0xEE, 0x10, 0x00]); // INC $0010
    code.extend([0x80, branch_to(code.len() + 2, SPIN)]); // BRA spin
    code
}

/// A relative branch's operand: the distance from the instruction *after* the
/// branch to the target.
///
/// `after` is where the program counter will be, which is the thing that is
/// easy to get wrong by one.
fn branch_to(after: usize, target: usize) -> u8 {
    let distance = target as isize - after as isize;
    assert!(
        (-128..=127).contains(&distance),
        "a relative branch cannot reach {distance} bytes"
    );
    distance as i8 as u8
}

/// The whole image: the program, a header, and vectors.
///
/// Everything not otherwise written is `0x00`, which decodes as an instruction
/// that would stop the processor — a deliberate choice, so that a program that
/// runs off its own end goes somewhere obvious rather than wandering through
/// a landscape of no-operations.
pub fn image() -> Vec<u8> {
    let mut rom = vec![0u8; ROM_BYTES];
    let code = program();
    rom[..code.len()].copy_from_slice(&code);
    finish(&mut rom, b"AWASERU FIXTURE      ", ORIGIN + SPIN as u16);
    rom
}

/// The header, the vectors and the checksum, which both programs share.
fn finish(rom: &mut [u8], title: &[u8; 21], vectors_to: u16) {
    // --- the header ---------------------------------------------------
    // Nothing here is anybody's name (§11.2).
    rom[HEADER_AT..HEADER_AT + 21].copy_from_slice(title);
    rom[HEADER_AT + 0x15] = 0x20; // mapping: the low-bank one, slow
    rom[HEADER_AT + 0x16] = 0x00; // no coprocessor, no battery
    rom[HEADER_AT + 0x17] = 0x05; // size: 2^5 KiB = 32 KiB
    rom[HEADER_AT + 0x18] = 0x00; // no save memory
    rom[HEADER_AT + 0x19] = 0x01; // region
    rom[HEADER_AT + 0x1A] = 0x00; // maker
    rom[HEADER_AT + 0x1B] = 0x00; // version

    // --- the vectors ---------------------------------------------------
    // Everything points at the spin loop. Interrupts are off, so none of
    // these should be taken; a vector of zero would send the processor
    // somewhere undefined if one ever were, and "should not happen" is not a
    // reason to leave a hole.
    for at in [0x7FE4, 0x7FE6, 0x7FE8, 0x7FEA, 0x7FEE, 0x7FF4, 0x7FFA, 0x7FFE] {
        write_word(rom, at, vectors_to);
    }
    write_word(rom, 0x7FFC, ORIGIN); // reset

    // --- the checksum --------------------------------------------------
    // Written last, over an image whose checksum bytes are still zero, which
    // is the convention the fields describe.
    let sum: u16 = rom.iter().fold(0u16, |acc, &b| acc.wrapping_add(u16::from(b)));
    write_word(rom, HEADER_AT + 0x1C, !sum); // complement
    write_word(rom, HEADER_AT + 0x1E, sum);
}

/// A second program, which **will not proceed until a button is pressed**.
///
/// §4.7's definitions may carry a recorded input log, for anchors behind
/// software that waits for input. Testing that needs software that waits, and
/// nothing this project owned did — the first fixture runs to its end on its
/// own. So this one waits.
///
/// ```text
///         SEI : CLC : XCE : SEP #$30
///         LDA #$00 : PHA : PLB     ; data bank zero
///         STZ $10                  ; the counter
///         STZ $0210                ; and the gate's sentinel
///         LDA #$01 : STA $4200     ; have the hardware read the controller
/// wait:   LDA $4219
///         AND #$10                 ; the button this gate is behind
///         BEQ wait                 ; ...and go no further until it is held
///         LDA #$5A : STA $0210     ; the gate is open, and says so
/// spin:   INC $0010
///         BRA spin
/// ```
///
/// The difference from the first fixture is the whole point: **its counter does
/// not move until the gate opens.** A test can therefore tell "the input
/// arrived" from "the input did not" without reading anything but memory this
/// program wrote.
pub fn image_needing_input() -> Vec<u8> {
    let mut rom = vec![0u8; ROM_BYTES];
    let code = gated_program();
    rom[..code.len()].copy_from_slice(&code);
    finish(&mut rom, b"AWASERU GATED        ", ORIGIN + GATE_SPIN as u16);
    rom
}

/// Where the gated program's labels are.
const GATE_WAIT: usize = 0x13;
const GATE_SPIN: usize = 0x1F;

fn gated_program() -> Vec<u8> {
    let mut code: Vec<u8> = Vec::new();
    code.extend([0x78]); // SEI
    code.extend([0x18, 0xFB]); // CLC : XCE
    code.extend([0xE2, 0x30]); // SEP #$30
    code.extend([0xA9, 0x00]); // LDA #$00
    code.extend([0x48, 0xAB]); // PHA : PLB        -> data bank 0
    code.extend([0x64, 0x10]); // STZ $10          -> the counter
    code.extend([0x9C, 0x10, 0x02]); // STZ $0210  -> the gate's sentinel
    code.extend([0xA9, 0x01]); // LDA #$01
    code.extend([0x8D, 0x00, 0x42]); // STA $4200  -> read the controller each frame
    debug_assert_eq!(code.len(), GATE_WAIT);
    // wait:
    code.extend([0xAD, 0x19, 0x42]); // LDA $4219
    code.extend([0x29, 0x10]); // AND #$10
    code.extend([0xF0, branch_to(code.len() + 2, GATE_WAIT)]); // BEQ wait
    code.extend([0xA9, 0x5A]); // LDA #$5A
    code.extend([0x8D, 0x10, 0x02]); // STA $0210  -> the gate is open
    debug_assert_eq!(code.len(), GATE_SPIN);
    // spin:
    code.extend([0xEE, 0x10, 0x00]); // INC $0010
    code.extend([0x80, branch_to(code.len() + 2, GATE_SPIN)]); // BRA spin
    code
}

fn write_word(rom: &mut [u8], at: usize, value: u16) {
    rom[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The test this module needs most.** Three branch offsets were worked
    /// out by hand; this recomputes every one from the bytes that were
    /// actually emitted, so a slip in the arithmetic or an instruction whose
    /// length was mis-counted fails here instead of producing a program that
    /// runs somewhere unintended and writes nothing.
    #[test]
    fn the_branches_land_where_the_labels_are() {
        let code = program();
        // Each entry: where the branch's opcode is, and the label it should
        // reach. Derived from the listing in this module's header.
        for (opcode_at, label) in [(0x15, FILL), (0x2F, PAL), (0x34, SPIN)] {
            let operand = code[opcode_at + 1] as i8 as isize;
            let after = opcode_at as isize + 2;
            assert_eq!(
                after + operand,
                label as isize,
                "the branch at {opcode_at:#04X} lands at {:#04X} and should reach {label:#04X}",
                after + operand
            );
        }
        assert_eq!(code.len(), SPIN + 5, "the program is as long as the listing");
    }

    /// The labels are where the emitted bytes put them. `program` asserts this
    /// in a debug build; this makes it a test, so a release-mode run of the
    /// suite checks it too.
    #[test]
    fn the_labels_are_at_the_offsets_the_listing_says() {
        let code = program();
        assert_eq!(code[FILL], 0x8A, "fill: should begin with TXA");
        assert_eq!(code[PAL], 0x8A, "pal: should begin with TXA");
        assert_eq!(code[SPIN], 0xEE, "spin: should begin with INC absolute");
    }

    #[test]
    fn the_image_is_the_size_and_shape_the_header_claims() {
        let rom = image();
        assert_eq!(rom.len(), ROM_BYTES);
        assert_eq!(
            1usize << rom[HEADER_AT + 0x17],
            ROM_BYTES / 1024,
            "the size field must describe the image"
        );
        assert_eq!(
            &rom[HEADER_AT..HEADER_AT + 7],
            b"AWASERU",
            "the title is this project's and nobody else's (§11.2)"
        );
        assert_eq!(
            u16::from_le_bytes([rom[0x7FFC], rom[0x7FFD]]),
            ORIGIN,
            "reset must point at the program"
        );
    }

    /// The two halves of the checksum field are complements, which is what the
    /// field pair means. A reader that checks one against the other is the
    /// reason to get this right.
    #[test]
    fn the_checksum_and_its_complement_are_complements() {
        let rom = image();
        let complement = u16::from_le_bytes([rom[HEADER_AT + 0x1C], rom[HEADER_AT + 0x1D]]);
        let checksum = u16::from_le_bytes([rom[HEADER_AT + 0x1E], rom[HEADER_AT + 0x1F]]);
        assert_eq!(complement, !checksum);
        assert_ne!(checksum, 0, "a checksum of zero would mean nothing was summed");
    }

    /// The two patterns must not be mistakable for each other, or for a buffer
    /// that was never written. A test asserting the work pattern against the
    /// palette would otherwise pass.
    /// A store that went to the wrong place by a plausible amount puts a
    /// pattern byte where a sentinel belongs, and the sentinels are chosen so
    /// that it does not match.
    ///
    /// The pattern takes every byte value exactly once, so no value is *absent*
    /// from it — requiring that was this test's first form and it failed, which
    /// is the right outcome for an assertion that was simply wrong. What can be
    /// required is narrower and is what actually matters: for each shift a
    /// mistake would plausibly produce, the byte the pattern would leave at a
    /// sentinel's offset is not that sentinel.
    ///
    /// **What this does not cover**: a shift chosen to defeat it. One exists —
    /// the pattern is a bijection, so for every byte there is some offset that
    /// produces it. The shifts here are the ones a wrong operand, a wrong page
    /// or a wrong bank would give.
    #[test]
    fn a_misplaced_store_would_show_up_at_a_sentinel() {
        let pattern = expected::work_pattern();
        let plausible: [isize; 10] = [-0x100, -0x10, -2, -1, 1, 2, 0x10, 0x100, 0x1000, -0x1000];

        for shift in plausible {
            for (offset, sentinel) in [
                (expected::SENTINEL_BEFORE_AT, expected::SENTINEL_BEFORE),
                (expected::SENTINEL_AFTER_AT, expected::SENTINEL_AFTER),
            ] {
                // Where in a pattern written `shift` bytes off this offset
                // would fall.
                let index = offset as isize - (expected::WORK_PATTERN_AT as isize + shift);
                let Some(&would_be) =
                    usize::try_from(index).ok().and_then(|i| pattern.get(i))
                else {
                    continue;
                };
                assert_ne!(
                    would_be, sentinel,
                    "a store {shift} bytes out would put {would_be:#04X} at {offset:#x}, which \
                     is the sentinel {sentinel:#04X} and would pass unnoticed"
                );
            }
        }

        assert_eq!(expected::SENTINEL_BEFORE_AT, expected::WORK_PATTERN_AT - 1);
        assert_eq!(
            expected::SENTINEL_AFTER_AT,
            expected::WORK_PATTERN_AT + pattern.len()
        );
        assert_ne!(expected::SENTINEL_BEFORE, expected::SENTINEL_AFTER);
    }

    /// Every odd byte of the palette has its top bit clear, because a colour
    /// is fifteen bits. An expectation that did not account for that is what
    /// this test would have caught had it existed first; it exists now.
    #[test]
    fn the_palette_expectation_accounts_for_a_fifteen_bit_colour() {
        let palette = expected::palette_pattern();
        for (i, &byte) in palette.iter().enumerate() {
            if i % 2 == 1 {
                assert_eq!(
                    byte & 0x80,
                    0,
                    "byte {i} of the palette has its top bit set, and the hardware does not \
                     store it"
                );
            }
        }
        assert!(
            palette.iter().step_by(2).any(|&b| b & 0x80 != 0),
            "the even bytes keep all eight bits, so some of them should have the top one set — \
             otherwise this test would pass over a pattern that is simply all small numbers"
        );
    }

    #[test]
    fn the_two_patterns_cannot_be_mistaken_for_each_other() {
        let work = expected::work_pattern();
        let palette = expected::palette_pattern();
        assert_eq!(work.len(), 256);
        assert_eq!(palette.len(), 256);
        assert_ne!(work, palette);
        assert!(work.iter().any(|&b| b != 0), "not all zero");
        assert!(
            work.iter().enumerate().all(|(i, &b)| b != i as u8),
            "no byte equals its own index, so an incrementing buffer cannot pass for this"
        );
        assert_eq!(
            work.iter().collect::<std::collections::HashSet<_>>().len(),
            256,
            "every byte value appears once, so a read at the wrong offset is visible"
        );
    }
}
