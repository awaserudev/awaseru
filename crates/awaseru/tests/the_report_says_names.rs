//! §M7's third clause: the API reports a divergence by symbol name rather than
//! by address.
//!
//! "Rather than" is the part that needs care. A report that *replaced* the
//! numbers with names would make a mistyped mapping unfalsifiable — a mapping
//! is written by hand and can be wrong, and the offset and the address are what
//! check it. So the name arrives **in addition**, and three things this must
//! never do have a test each:
//!
//! 1. invent a name where no symbol covers the place;
//! 2. lose the number;
//! 3. present a hypothesis as established (§9.2).

use awaseru::mapping::{self, Mapping};
use awaseru::protocol::{self, Named, Position, Report, Verdict, Wrote};

const WORK: &str = "work-ram";

fn mapping_of(text: &str) -> Mapping {
    let file = mapping::parse(text).expect("the mapping parses");
    mapping::load([("a.toml", &file)]).expect("it is a graph")
}

/// A mapping covering the byte a difference is at, and the instruction that
/// wrote it — one measured, one not, so the hypothesis half has something to
/// be about.
fn a_mapping() -> Mapping {
    mapping_of(
        r#"
[[symbol]]
name = "tile-buffer"
region = "work-ram"
offset = 1024
length = 64
description = "Where the expanded tiles land."
provenance = { how = "measured", note = "the span the memory actually changed" }

[[symbol]]
name = "tile-expand.store"
address = 32812
length = 4
description = "The store that fills the buffer."
provenance = { how = "inferred", note = "one observation, never confirmed" }
"#,
    )
}

fn a_difference(first: usize, pc: u64) -> Report {
    Report {
        routine: "tile-expand".into(),
        verdict: Verdict::Differs {
            difference: protocol::Difference {
                region: Some(WORK.into()),
                first,
                expected: 0x20,
                found: 0x00,
                differing: 1,
                compared: 64,
                wrote: Wrote::At {
                    position: Position::MidInstruction { pc },
                    writes: 1,
                    symbol: None,
                },
                symbol: None,
            },
        },
        as_compared: None,
        moved: None,
        localisation: None,
        control: protocol::Control::NotRun { says: "none".into() },
        complete: false,
        coverage: None,
        beginning: protocol::Beginning {
            repeats: true,
            settled: vec![],
            says: "fine".into(),
        },
        took_ms: 1,
    }
}

fn difference_of(report: &Report) -> &protocol::Difference {
    match &report.verdict {
        Verdict::Differs { difference } => difference,
        other => panic!("expected a difference: {other:?}"),
    }
}

#[test]
fn a_divergence_is_reported_by_name_and_keeps_its_numbers() {
    let mut report = a_difference(1024 + 40, 32812);
    report.name_with(&a_mapping());
    let difference = difference_of(&report);

    // ---- the name, and how far into the symbol ---------------------------
    assert_eq!(
        difference.symbol,
        Some(Named {
            name: "tile-buffer".into(),
            into: 40,
            hypothesis: false,
        }),
        "`tile-buffer+40` and `tile-buffer` are different answers, so the offset into the \
         symbol is part of the name"
    );

    // ---- and §5.4's instruction, named too -------------------------------
    match &difference.wrote {
        Wrote::At { symbol, position, writes } => {
            assert_eq!(
                symbol.as_ref().map(|s| s.name.as_str()),
                Some("tile-expand.store"),
                "the instruction that wrote it is a symbol too"
            );
            assert_eq!(
                *position,
                Position::MidInstruction { pc: 32812 },
                "**the address is still there**. A mapping can be wrong, and the number is \
                 what checks it — a report that replaced it would make a mistyped mapping \
                 unfalsifiable"
            );
            assert_eq!(*writes, 1, "and nothing else was disturbed");
        }
        other => panic!("got {other:?}"),
    }

    // ---- the numbers of the difference, untouched ------------------------
    assert_eq!(difference.first, 1024 + 40);
    assert_eq!(difference.region.as_deref(), Some(WORK));
    assert_eq!((difference.expected, difference.found), (0x20, 0x00));
}

#[test]
fn a_hypothesis_is_said_to_be_one() {
    let mut report = a_difference(1024, 32812);
    report.name_with(&a_mapping());
    let difference = difference_of(&report);

    assert_eq!(
        difference.symbol.as_ref().map(|s| s.hypothesis),
        Some(false),
        "the buffer was measured"
    );
    match &difference.wrote {
        Wrote::At { symbol, .. } => assert_eq!(
            symbol.as_ref().map(|s| s.hypothesis),
            Some(true),
            "§9.2: the store was inferred from one observation and never confirmed, and a \
             client that printed the name without this would present a guess as a fact"
        ),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn a_name_is_never_invented() {
    let mapping = a_mapping();

    // Outside the buffer, and at an address nothing covers.
    let mut report = a_difference(1024 + 64, 0x9999);
    report.name_with(&mapping);
    let difference = difference_of(&report);
    assert_eq!(
        difference.symbol, None,
        "one byte past the symbol is not in it, and absent is the answer rather than the \
         nearest name"
    );
    match &difference.wrote {
        Wrote::At { symbol, position, .. } => {
            assert_eq!(*symbol, None, "nothing covers that address");
            assert_eq!(
                *position,
                Position::MidInstruction { pc: 0x9999 },
                "and the number is still reported, which is what a reader has left"
            );
        }
        other => panic!("got {other:?}"),
    }

    // And the near miss: one byte earlier IS in it, so this is not a mapping
    // that names nothing.
    let mut inside = a_difference(1024 + 63, 32812);
    inside.name_with(&mapping);
    assert!(difference_of(&inside).symbol.is_some(), "the last byte is inside");
}

#[test]
fn a_report_with_no_mapping_says_no_names_and_is_otherwise_the_same() {
    let bare = a_difference(1024 + 40, 32812);
    let mut named = bare.clone();
    named.name_with(&Mapping::default());

    assert_eq!(
        named, bare,
        "an empty mapping changes nothing at all, so a project with no mapping files gets \
         exactly the report it got before §9 existed"
    );
    assert_eq!(difference_of(&named).symbol, None);
}

/// Overlapping symbols are legal and useful — a field inside a structure — and
/// the most specific is the one worth saying.
#[test]
fn the_most_specific_symbol_wins() {
    let mapping = mapping_of(
        r#"
[[symbol]]
name = "save-data"
region = "work-ram"
offset = 1024
length = 512
description = "All of it."
provenance = { how = "measured", note = "seen" }

[[symbol]]
name = "save-data.checksum"
region = "work-ram"
offset = 1040
length = 2
description = "Two bytes of it."
provenance = { how = "measured", note = "seen" }
"#,
    );

    let mut inner = a_difference(1040, 0);
    inner.name_with(&mapping);
    assert_eq!(
        difference_of(&inner).symbol.as_ref().map(|s| s.name.as_str()),
        Some("save-data.checksum"),
        "naming the outer one would answer `somewhere in the save data` where the mapping \
         could say which field"
    );

    let mut outer = a_difference(1030, 0);
    outer.name_with(&mapping);
    assert_eq!(
        difference_of(&outer).symbol.as_ref().map(|s| s.name.as_str()),
        Some("save-data"),
        "and a byte the inner one does not cover still gets the outer one"
    );
}
