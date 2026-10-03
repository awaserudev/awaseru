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

    /// `image_with_a_routine`'s routine: where its input is, where it puts its
    /// output, how long both are, and the two addresses that bound it.
    ///
    /// All five are numbers this project chose, which is what lets a test
    /// assert against them (§11.3).
    pub const ROUTINE_INPUT_AT: usize = 0x0300;
    pub const ROUTINE_OUTPUT_AT: usize = 0x0400;
    pub const ROUTINE_LENGTH: usize = 0x40;
    /// The routine's first instruction — where §5.6's cycle seeds.
    pub const ROUTINE_ENTRY: u64 = 0x8020;
    /// Where it returns to — where §4.5's bound ends, so that a measurement
    /// does not run past its subject.
    pub const ROUTINE_RETURN: u64 = 0x800F;
    /// What the instruction *after* the return writes over the output's first
    /// byte, so that running past the subject is visible rather than
    /// theoretical (§4.5).
    pub const ROUTINE_CLOBBER: u8 = 0xFF;
    /// The routine's only store, and so §5.4's answer for any difference in its
    /// output: the instruction a localisation must name.
    pub const ROUTINE_STORE: u64 = 0x802C;
    /// The store *after* the return — §4.5's trap, and what a localisation
    /// bounded one instruction too generously would name instead.
    pub const ROUTINE_CLOBBER_STORE: u64 = 0x8011;

    /// What the routine does, in Rust.
    ///
    /// The **reference implementation of the fixture's own behaviour**, which
    /// is what §M3's done-condition compares a wrong one against. It is a
    /// running total, eight bits and wrapping, exclusive-or'd with a constant
    /// on the way out — so every output depends on every input before it, and
    /// two different mistakes have two different first differing offsets.
    pub fn routine(input: &[u8]) -> Vec<u8> {
        let mut total = 0u8;
        input
            .iter()
            .map(|&byte| {
                total = total.wrapping_add(byte);
                total ^ 0x5A
            })
            .collect()
    }

    /// A wrong one: it forgets the exclusive-or. **Differs at offset 0.**
    pub fn routine_without_the_mask(input: &[u8]) -> Vec<u8> {
        let mut total = 0u8;
        input
            .iter()
            .map(|&byte| {
                total = total.wrapping_add(byte);
                total
            })
            .collect()
    }

    /// Another wrong one: it forgets to carry the total forward, so each output
    /// depends only on its own input. **Agrees at offset 0 and differs at
    /// offset 1**, which is what makes "the first differing offset" worth
    /// reporting.
    pub fn routine_without_the_chain(input: &[u8]) -> Vec<u8> {
        input.iter().map(|&byte| byte ^ 0x5A).collect()
    }
    // ---- the two-routine image, for §10's coverage -------------------
    //
    // One routine is called and one is not. Both are real: a loop, an indexed
    // load, an indexed store, a comparison and a branch. Neither can be told
    // from the other by looking at its bytes, which is the point — a tool that
    // confused *present in the image* with *executed* would flag both.

    /// The routine the program calls.
    pub const TWO_CALLED_ENTRY: u64 = 0x8040;
    /// The routine nothing calls. It is assembled, reachable by address, and
    /// never reached by this program.
    pub const TWO_UNCALLED_ENTRY: u64 = 0x8060;
    /// Where a measurement of the called routine is bounded (§4.5).
    pub const TWO_RETURN: u64 = 0x800F;

    /// What the called routine reads, and what it writes.
    pub const TWO_INPUT_AT: usize = 0x0300;
    pub const TWO_CALLED_OUTPUT_AT: usize = 0x0400;
    /// Where the uncalled routine WOULD write. Nothing ever does, so this span
    /// is a second witness: if coverage claimed that routine ran, the memory
    /// here would contradict it.
    pub const TWO_UNCALLED_OUTPUT_AT: usize = 0x0500;
    pub const TWO_LENGTH: usize = 0x20;

    /// What the called routine leaves in its output, given `two_input`.
    pub fn two_called(input: &[u8]) -> Vec<u8> {
        input.iter().map(|b| b ^ 0x5A).collect()
    }

    /// What the uncalled one would leave, which is different — so a test that
    /// confused the two buffers would fail rather than pass.
    pub fn two_uncalled(input: &[u8]) -> Vec<u8> {
        input.iter().map(|b| b ^ 0xA5).collect()
    }

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

/// A third program: one with a **routine** in it — §5.6's unit of work.
///
/// §M3's done-condition is a deliberately wrong reimplementation of a routine
/// being caught. That needs a routine whose behaviour is defined here, and the
/// first two fixtures have none: they run straight through, or wait.
///
/// ```text
///         SEI : CLC : XCE : SEP #$30
///         LDA #$00 : PHA : PLB     ; data bank zero
///         LDX #$FF : TXS           ; a stack, because the routine pushes
///         JSR routine
/// after:  LDA #$FF : STA $7E0400   ; and then ruins the output, on purpose
/// spin:   INC $0010                ; something moves once it has returned
///         BRA spin
///
/// routine:
///         LDX #$00
///         LDA #$00                 ; the accumulator starts at zero
/// loop:   CLC
///         ADC $7E0300,X            ; running total, eight bits, wrapping
///         PHA                      ; keep the total
///         EOR #$5A
///         STA $7E0400,X            ; and write the total exclusive-or'd
///         PLA                      ; restore the total for the next round
///         INX : CPX #$40 : BNE loop
///         RTS
/// ```
///
/// # Why a *chained* transformation
///
/// Because a byte-by-byte one would make every wrong reimplementation differ
/// at offset zero, and §5.4's "first differing offset" would then say nothing.
/// Here each output depends on every input before it, so:
///
/// - forgetting the exclusive-or differs at **offset 0**;
/// - forgetting to carry the total forward differs at **offset 1** — the first
///   place the chaining matters — and agrees at offset 0.
///
/// Two wrong implementations, two different first offsets, and a right one that
/// matches throughout. That is what makes a test of the localisation a test.
///
/// # Why it ruins its own output afterwards
///
/// §4.5 says a measurement must not run past its subject, because the next
/// thing to run writes over the data being compared and the comparison
/// silently becomes a reading of something else. A fixture whose routine is
/// followed by something harmless would let that rule be *respected* and never
/// *tested*. So the instruction after the call writes `0xFF` over the output's
/// first byte: bounded to the return, a measurement sees the routine's answer;
/// one instruction further and it does not.
pub fn image_with_a_routine() -> Vec<u8> {
    let mut rom = vec![0u8; ROM_BYTES];
    let code = routine_program();
    rom[..code.len()].copy_from_slice(&code);
    finish(
        &mut rom,
        b"AWASERU ROUTINE      ",
        ORIGIN + ROUTINE_SPIN as u16,
    );
    rom
}

/// An image with two routines, one of which is never called — §10's coverage.
///
/// The question coverage answers is "what has **not** been seen", and the
/// hardest version of it is a routine that exists, is correct, and is never
/// reached. Padding and dead bytes are easy: they are not code. This one is.
///
/// Both routines have the same shape — load indexed, transform, store indexed,
/// increment, compare, branch, return — and write different buffers, so the
/// memory is a second witness to what coverage says.
pub fn image_with_two_routines() -> Vec<u8> {
    let mut rom = vec![0u8; ROM_BYTES];
    let code = two_routine_program();
    rom[..code.len()].copy_from_slice(&code);
    finish(
        &mut rom,
        b"AWASERU COVERAGE     ",
        ORIGIN + TWO_SPIN as u16,
    );
    rom
}

const TWO_SPIN: usize = 0x15;
const TWO_CALLED: usize = 0x40;
const TWO_UNCALLED: usize = 0x60;

fn two_routine_program() -> Vec<u8> {
    let mut code: Vec<u8> = Vec::new();
    code.extend([0x78]); // SEI
    code.extend([0x18, 0xFB]); // CLC : XCE
    code.extend([0xE2, 0x30]); // SEP #$30
    code.extend([0xA9, 0x00]); // LDA #$00
    code.extend([0x48, 0xAB]); // PHA : PLB
    code.extend([0xA2, 0xFF]); // LDX #$FF
    code.extend([0x9A]); // TXS
    code.extend([
        0x20,
        (ORIGIN + TWO_CALLED as u16) as u8,
        ((ORIGIN + TWO_CALLED as u16) >> 8) as u8,
    ]); // JSR the called routine — and NO call to the other one
    debug_assert_eq!(code.len(), expected::TWO_RETURN as usize - ORIGIN as usize);

    // The instruction a measurement bounded by the return stops before.
    code.extend([0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0xEA]); // NOP x6
    debug_assert_eq!(code.len(), TWO_SPIN);
    code.extend([0xEE, 0x10, 0x00]); // INC $0010
    code.extend([0x80, branch_to(code.len() + 2, TWO_SPIN)]); // BRA spin

    while code.len() < TWO_CALLED {
        code.push(0x00);
    }
    // called: out[i] = in[i] ^ 0x5A
    code.extend([0xA2, 0x00]); // LDX #$00
    let loop_at = code.len();
    code.extend([0xBF, 0x00, 0x03, 0x7E]); // LDA $7E0300,X
    code.extend([0x49, 0x5A]); // EOR #$5A
    code.extend([0x9F, 0x00, 0x04, 0x7E]); // STA $7E0400,X
    code.extend([0xE8]); // INX
    code.extend([0xE0, 0x20]); // CPX #$20
    code.extend([0xD0, branch_to(code.len() + 2, loop_at)]); // BNE loop
    code.extend([0x60]); // RTS
    assert!(code.len() <= TWO_UNCALLED, "the called routine overran the other");

    while code.len() < TWO_UNCALLED {
        code.push(0x00);
    }
    // uncalled: out[i] = in[i] + 1, into a different buffer. Assembled, correct
    // and never reached.
    code.extend([0xA2, 0x00]); // LDX #$00
    let loop_at = code.len();
    code.extend([0xBF, 0x00, 0x03, 0x7E]); // LDA $7E0300,X
    code.extend([0x49, 0xA5]); // EOR #$A5 — two bytes, like the other one's
    code.extend([0x9F, 0x00, 0x05, 0x7E]); // STA $7E0500,X
    code.extend([0xE8]); // INX
    code.extend([0xE0, 0x20]); // CPX #$20
    code.extend([0xD0, branch_to(code.len() + 2, loop_at)]); // BNE loop
    code.extend([0x60]); // RTS
    code
}

/// Offsets within the routine program, from the listing above.
const ROUTINE_AFTER: usize = 0x0F;
const ROUTINE_SPIN: usize = 0x15;
const ROUTINE_ENTRY: usize = 0x20;
const ROUTINE_LOOP: usize = 0x24;

fn routine_program() -> Vec<u8> {
    let mut code: Vec<u8> = Vec::new();
    code.extend([0x78]); // SEI
    code.extend([0x18, 0xFB]); // CLC : XCE
    code.extend([0xE2, 0x30]); // SEP #$30
    code.extend([0xA9, 0x00]); // LDA #$00
    code.extend([0x48, 0xAB]); // PHA : PLB      -> data bank 0
    code.extend([0xA2, 0xFF]); // LDX #$FF
    code.extend([0x9A]); // TXS                  -> a stack at $00FF downward
    code.extend([
        0x20,
        (ORIGIN + ROUTINE_ENTRY as u16) as u8,
        ((ORIGIN + ROUTINE_ENTRY as u16) >> 8) as u8,
    ]); // JSR routine
    debug_assert_eq!(code.len(), ROUTINE_AFTER);
    // after: the instruction a measurement bounded to the return stops before.
    code.extend([0xA9, 0xFF]); // LDA #$FF
    code.extend([0x8F, 0x00, 0x04, 0x7E]); // STA $7E0400  -> ruins the output
    debug_assert_eq!(code.len(), ROUTINE_SPIN);
    // spin:
    code.extend([0xEE, 0x10, 0x00]); // INC $0010
    code.extend([0x80, branch_to(code.len() + 2, ROUTINE_SPIN)]); // BRA spin

    // Padding to the routine's own address, so that the entry is a number this
    // project chose rather than one that moved when the setup changed.
    while code.len() < ROUTINE_ENTRY {
        code.push(0x00);
    }
    debug_assert_eq!(code.len(), ROUTINE_ENTRY);

    // routine:
    code.extend([0xA2, 0x00]); // LDX #$00
    code.extend([0xA9, 0x00]); // LDA #$00
    debug_assert_eq!(code.len(), ROUTINE_LOOP);
    // loop:
    code.extend([0x18]); // CLC
    code.extend([0x7F, 0x00, 0x03, 0x7E]); // ADC $7E0300,X
    code.extend([0x48]); // PHA
    code.extend([0x49, 0x5A]); // EOR #$5A
    code.extend([0x9F, 0x00, 0x04, 0x7E]); // STA $7E0400,X
    code.extend([0x68]); // PLA
    code.extend([0xE8]); // INX
    code.extend([0xE0, 0x40]); // CPX #$40
    code.extend([0xD0, branch_to(code.len() + 2, ROUTINE_LOOP)]); // BNE loop
    code.extend([0x60]); // RTS
    code
}

fn write_word(rom: &mut [u8], at: usize, value: u16) {
    rom[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two routines are where the constants say, are the same shape, and
    /// exactly one of them is called.
    ///
    /// Checked against the bytes rather than trusted, because every coverage
    /// assertion in this project rests on "that one is never reached" — and a
    /// fixture that quietly called both would make those tests pass for the
    /// wrong reason.
    #[test]
    fn one_of_the_two_routines_is_called_and_the_other_is_not() {
        let code = two_routine_program();
        let called = expected::TWO_CALLED_ENTRY as usize - ORIGIN as usize;
        let uncalled = expected::TWO_UNCALLED_ENTRY as usize - ORIGIN as usize;

        // Both begin by clearing the index, both end in RTS, and both are the
        // same length — so neither can be told from the other by its shape.
        assert_eq!(&code[called..called + 2], &[0xA2, 0x00], "the called one");
        assert_eq!(&code[uncalled..uncalled + 2], &[0xA2, 0x00], "the other one");
        // Byte for byte the same program, apart from the constant it applies
        // and the buffer it writes — so nothing but having been reached can
        // tell them apart.
        const LENGTH: usize = 18;
        assert_eq!(code[called + LENGTH - 1], 0x60, "RTS ends the called routine");
        assert_eq!(code[uncalled + LENGTH - 1], 0x60, "and the uncalled one");
        let differ: Vec<usize> = (0..LENGTH)
            .filter(|i| code[called + i] != code[uncalled + i])
            .collect();
        assert_eq!(
            differ,
            vec![7, 10],
            "exactly two bytes differ: the constant and the output buffer's page"
        );

        // Exactly one JSR in the whole program, and it names the called one.
        let jsrs: Vec<usize> = (0..code.len() - 2)
            .filter(|i| code[*i] == 0x20)
            .filter(|i| {
                let target = u16::from_le_bytes([code[i + 1], code[i + 2]]);
                target == ORIGIN + called as u16 || target == ORIGIN + uncalled as u16
            })
            .collect();
        assert_eq!(jsrs.len(), 1, "one call, at {jsrs:?}");
        assert_eq!(
            u16::from_le_bytes([code[jsrs[0] + 1], code[jsrs[0] + 2]]),
            ORIGIN + called as u16,
            "and it is the called routine that is called"
        );

        // Nothing anywhere names the uncalled one — not a JSR, not a JMP, not
        // a branch. This is the assertion the coverage tests lean on.
        let its_address = (ORIGIN + uncalled as u16).to_le_bytes();
        assert!(
            !code.windows(2).any(|w| w == its_address),
            "the uncalled routine's address appears nowhere in the program"
        );

        // And they write different buffers, so a test that confused the two
        // would fail rather than pass.
        let input: Vec<u8> = (0..expected::TWO_LENGTH as u8).collect();
        assert_ne!(expected::two_called(&input), expected::two_uncalled(&input));
    }

    /// §5.4's answer is a constant written by hand, so it is checked against
    /// the bytes rather than trusted. Both stores are the only ones in their
    /// part of the program, which is what makes "the instruction that wrote
    /// it" a single address at all.
    #[test]
    fn the_two_stores_are_where_the_constants_say_and_are_the_only_ones() {
        let code = routine_program();
        let store = (expected::ROUTINE_STORE - u64::from(ORIGIN)) as usize;
        let clobber = (expected::ROUTINE_CLOBBER_STORE - u64::from(ORIGIN)) as usize;

        assert_eq!(code[store], 0x9F, "the routine's store, absolute indexed");
        assert_eq!(code[clobber], 0x8F, "the clobber's store, absolute");

        // Within the routine, that opcode appears once: a second store would
        // make "the instruction that wrote this byte" ambiguous and every
        // assertion about it weaker than it reads.
        let routine = &code[ROUTINE_ENTRY..];
        assert_eq!(
            routine.iter().filter(|b| **b == 0x9F).count(),
            1,
            "the routine must have exactly one indexed store"
        );
        assert_eq!(
            code[..ROUTINE_ENTRY].iter().filter(|b| **b == 0x8F).count(),
            1,
            "and the setup exactly one absolute store, the clobber"
        );
    }

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

    /// The routine program's three branches, recomputed from the bytes
    /// emitted — the same check the first fixture gets, for the same reason.
    #[test]
    fn the_routines_branches_land_where_the_labels_are() {
        let code = routine_program();
        for (opcode_at, label) in [(0x18, ROUTINE_SPIN), (0x34, ROUTINE_LOOP)] {
            let operand = code[opcode_at + 1] as i8 as isize;
            let after = opcode_at as isize + 2;
            assert_eq!(
                after + operand,
                label as isize,
                "the branch at {opcode_at:#04X} lands at {:#04X} and should reach {label:#04X}",
                after + operand
            );
        }
        // And the call reaches the routine, which a branch check would miss
        // because a call is absolute.
        assert_eq!(code[0x0C], 0x20, "a call at the end of the setup");
        assert_eq!(
            u16::from_le_bytes([code[0x0D], code[0x0E]]),
            ORIGIN + ROUTINE_ENTRY as u16,
            "and it calls the routine's own address"
        );
        assert_eq!(code[0x36], 0x60, "the routine ends by returning");
        assert_eq!(
            (code[ROUTINE_AFTER], code[ROUTINE_AFTER + 1]),
            (0xA9, expected::ROUTINE_CLOBBER),
            "and the instruction after the return loads what it will ruin the output with — \
             §4.5 is testable because of it"
        );
        assert_eq!(
            expected::ROUTINE_ENTRY,
            u64::from(ORIGIN + ROUTINE_ENTRY as u16),
            "what the tests are told the entry is must be where it is"
        );
        assert_eq!(
            expected::ROUTINE_RETURN,
            u64::from(ORIGIN + ROUTINE_AFTER as u16),
            "and the return address must be the instruction after the call"
        );
    }

    /// **The test that makes the done-condition possible.** Two wrong
    /// implementations, two different first differing offsets, and a right one
    /// that matches throughout.
    ///
    /// Without this, a differ test could pass against a transformation whose
    /// every wrong version differs everywhere — which would prove the differ
    /// says "differs" and nothing about *where*.
    #[test]
    fn the_two_wrong_implementations_differ_in_two_different_places() {
        // An input with no zero at the front, so that forgetting the chain is
        // visible from the second byte rather than later by luck.
        let input: Vec<u8> = (0..expected::ROUTINE_LENGTH)
            .map(|i| (i as u8).wrapping_mul(7).wrapping_add(3))
            .collect();
        let right = expected::routine(&input);

        let first_difference = |wrong: &[u8]| {
            right
                .iter()
                .zip(wrong)
                .position(|(a, b)| a != b)
                .expect("a wrong implementation must differ somewhere")
        };

        assert_eq!(
            first_difference(&expected::routine_without_the_mask(&input)),
            0,
            "forgetting the mask is wrong from the very first byte"
        );
        assert_eq!(
            first_difference(&expected::routine_without_the_chain(&input)),
            1,
            "forgetting the chain agrees at offset 0 — one input makes one output there — and \
             differs at offset 1, which is the first place the chaining matters"
        );
        assert_eq!(
            expected::routine(&input),
            right,
            "and the right one agrees with itself"
        );

        // The transformation is neither the identity nor a constant, or a
        // reimplementation that did nothing at all would pass.
        assert_ne!(right, input, "not the identity");
        assert!(
            right.iter().collect::<std::collections::HashSet<_>>().len() > 1,
            "not a constant"
        );
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
