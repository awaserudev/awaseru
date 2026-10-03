//! §9's mapping: what the user knows about the software, in files the user
//! writes by hand.
//!
//! The tool holds no mapping of its own (§9.4, §11.1). It loads what it is
//! given, and this module is the shape of what may be given.
//!
//! # The shape was read off a real mapping before it was written
//!
//! §9.1 lists what a symbol has: a name, a location, zero or more groups, a
//! description, relations to other symbols, and a provenance. Before inventing
//! fields, M7 read the one real mapping this project has — the table a person
//! wrote while measuring actual software — and compared the two.
//!
//! **What §9.1 does not give, and nothing works without: an extent.** A
//! location in §9.1 is a point: a region and an offset, or an address. Every
//! row of the real mapping that carries weight is a *span*. The output buffer
//! is twelve kilobytes. The input span is six. The routine's hot loop is
//! fifty-eight bytes. And the length is not decoration: the spans in an
//! `examine` — what to seed and what to compare — are made of exactly these
//! numbers, so a mapping without lengths cannot supply the one thing a
//! measurement needs most. `length` is therefore here, and it is optional,
//! because a symbol that genuinely is a point (an entry address) should not
//! have to invent one.
//!
//! **What §9.1 offers that nothing has wanted: nested groups.** Groups
//! themselves are wanted, and for exactly the reason §9.1 gives — the routine,
//! the buffer it reads, the buffer it writes and its hot loop are one concept
//! scattered across two regions, and naming that concept is the point. What no
//! mapping has ever wanted is a group *inside* a group. Seven symbols have no
//! hierarchy. So a group is a name here and nothing more, and nesting can be
//! added the day something asks for it — which is the safe direction, since a
//! field added later breaks nobody and a field removed later breaks everybody.
//!
//! # Every name here is permanent
//!
//! A mapping is written by hand, in files that outlive any version of this
//! tool, so a field renamed is every user's file broken. The list is therefore
//! as short as it can be while saying what §9.1 requires.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Where a symbol is — §9.1's "a region and an offset, or an address".
///
/// Two forms rather than one, because both are honest in different places: a
/// buffer is a region and an offset, and an entry point is an address the
/// program counter takes. Giving both, or neither, is refused rather than
/// resolved (§2.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// A place in a named region (§3.1, §8.5 — the name is the backend's).
    In { region: String, offset: usize },
    /// An address as the machine would use it.
    At(u64),
}

/// One entry of §9.1's mapping.
///
/// Not `Eq`: the provenance is a TOML table, whose float values have no total
/// equality. `PartialEq` is what tests need and is what the shape honestly has.
#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub at: Where,
    /// How many bytes it covers. `None` is a point, which an entry address is.
    pub length: Option<usize>,
    /// §9.1's groups, flat. See the module's note on nesting.
    pub groups: Vec<String>,
    /// §9.1: read by the API, "so that a consumer — human or program — obtains
    /// the context without reconstructing it from raw code". Required: a symbol
    /// nobody can explain is one nobody should rely on.
    pub description: String,
    /// §9.2, mandatory. Its contents are M7's third unit; what is fixed here is
    /// that no symbol may exist without one, so that no example is ever written
    /// that a later unit has to go back and fix.
    pub provenance: toml::Table,
}

/// What a mapping file holds, before any of §9.3's checks.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct File {
    pub symbols: Vec<Symbol>,
}

// ------------------------------------------------------------- the parsing --

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SymbolForm {
    name: String,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    address: Option<i64>,
    #[serde(default)]
    length: Option<usize>,
    #[serde(default)]
    groups: Vec<String>,
    description: String,
    provenance: toml::Table,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileForm {
    #[serde(default, rename = "symbol")]
    symbols: Vec<SymbolForm>,
}

/// Why a mapping file was refused — §2.4: never resolved, always named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The file is not the shape a mapping file has.
    Malformed { why: String },
    /// A symbol gives no location at all.
    NoLocation { symbol: String },
    /// A symbol gives two, which is two instructions.
    TwoLocations { symbol: String },
    /// Half of a location: a region without an offset, or the reverse.
    HalfLocation { symbol: String, given: &'static str },
    /// A field that must say something says nothing.
    Empty { symbol: String, field: &'static str },
    /// A length of zero, which is a point written as though it were a span.
    NoLength { symbol: String },
    /// An address that is not one.
    BadAddress { symbol: String, given: i64 },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Malformed { why } => write!(f, "this is not a mapping file: {why}"),
            Error::NoLocation { symbol } => write!(
                f,
                "the symbol `{symbol}` says where it is in no way at all. Give `region` and \
                 `offset`, or give `address`"
            ),
            Error::TwoLocations { symbol } => write!(
                f,
                "the symbol `{symbol}` gives both `address` and a region with an offset. Those \
                 are two answers to one question and this tool does not pick between them (§2.4)"
            ),
            Error::HalfLocation { symbol, given } => write!(
                f,
                "the symbol `{symbol}` gives `{given}` and not the other half. A place in a \
                 region is a region AND an offset"
            ),
            Error::Empty { symbol, field } => {
                write!(f, "the symbol `{symbol}` has an empty `{field}`")
            }
            Error::NoLength { symbol } => write!(
                f,
                "the symbol `{symbol}` has a `length` of zero. A symbol that covers no bytes is \
                 a point: leave `length` out and say so"
            ),
            Error::BadAddress { symbol, given } => write!(
                f,
                "the symbol `{symbol}` gives the address {given}, which is not an address"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Reads one mapping file. §9.3's checks across files are the next unit's.
pub fn parse(text: &str) -> Result<File, Error> {
    let form: FileForm = toml::from_str(text).map_err(|e| Error::Malformed {
        why: e.message().to_string(),
    })?;

    let mut symbols = Vec::with_capacity(form.symbols.len());
    for s in form.symbols {
        if s.name.trim().is_empty() {
            return Err(Error::Empty {
                symbol: s.name,
                field: "name",
            });
        }
        if s.description.trim().is_empty() {
            return Err(Error::Empty {
                symbol: s.name,
                field: "description",
            });
        }
        if s.provenance.is_empty() {
            return Err(Error::Empty {
                symbol: s.name,
                field: "provenance",
            });
        }

        let at = match (s.region, s.offset, s.address) {
            (Some(region), Some(offset), None) => Where::In { region, offset },
            (None, None, Some(address)) => Where::At(u64::try_from(address).map_err(|_| {
                Error::BadAddress {
                    symbol: s.name.clone(),
                    given: address,
                }
            })?),
            (None, None, None) => return Err(Error::NoLocation { symbol: s.name }),
            (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => {
                return Err(Error::TwoLocations { symbol: s.name });
            }
            (Some(_), None, None) => {
                return Err(Error::HalfLocation {
                    symbol: s.name,
                    given: "region",
                });
            }
            (None, Some(_), None) => {
                return Err(Error::HalfLocation {
                    symbol: s.name,
                    given: "offset",
                });
            }
        };

        if s.length == Some(0) {
            return Err(Error::NoLength { symbol: s.name });
        }

        symbols.push(Symbol {
            name: s.name,
            at,
            length: s.length,
            groups: s.groups,
            description: s.description,
            provenance: s.provenance,
        });
    }
    Ok(File { symbols })
}

/// The symbols of a file, by name — a convenience the next unit's graph uses.
pub fn by_name(file: &File) -> BTreeMap<&str, &Symbol> {
    file.symbols.iter().map(|s| (s.name.as_str(), s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A symbol with everything a symbol must have, so that each refusal below
    /// can take exactly one thing away.
    fn whole() -> String {
        r#"
[[symbol]]
name = "expand"
region = "program-rom"
offset = 64
length = 18
groups = ["the-decompressor"]
description = "Expands each input byte into two output bytes."
provenance = { how = "measured" }
"#
        .to_string()
    }

    fn without(line_starting: &str) -> String {
        whole()
            .lines()
            .filter(|l| !l.trim_start().starts_with(line_starting))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_whole_symbol_parses_and_keeps_what_it_was_given() {
        let file = parse(&whole()).expect("it parses");
        assert_eq!(file.symbols.len(), 1);
        let s = &file.symbols[0];
        assert_eq!(s.name, "expand");
        assert_eq!(
            s.at,
            Where::In {
                region: "program-rom".into(),
                offset: 64
            }
        );
        assert_eq!(s.length, Some(18), "§9.1 has no extent and a mapping needs one");
        assert_eq!(s.groups, vec!["the-decompressor"]);
        assert!(s.description.starts_with("Expands"));
        assert_eq!(s.provenance.get("how").and_then(|v| v.as_str()), Some("measured"));
        assert_eq!(by_name(&file).get("expand").map(|s| &s.name), Some(&"expand".to_string()));
    }

    /// The other form of a location, and the point case §9.1's wording is about.
    #[test]
    fn an_address_without_a_length_is_a_point_and_is_allowed() {
        let file = parse(
            r#"
[[symbol]]
name = "entry"
address = 0x8020
description = "Where the routine begins."
provenance = { how = "measured" }
"#,
        )
        .expect("it parses");
        let s = &file.symbols[0];
        assert_eq!(s.at, Where::At(0x8020));
        assert_eq!(s.length, None, "an entry address covers no span and invents none");
        assert!(s.groups.is_empty(), "§9.1 says zero or more");
    }

    /// Two locations is two instructions, and §2.4 does not pick between them.
    /// The near miss that must still parse is each half on its own.
    #[test]
    fn a_symbol_in_two_places_is_refused_and_each_place_alone_is_not() {
        let both = whole().replace(
            "offset = 64",
            "offset = 64\naddress = 0x8020",
        );
        assert_eq!(
            parse(&both),
            Err(Error::TwoLocations {
                symbol: "expand".into()
            })
        );
        assert!(parse(&whole()).is_ok(), "a region and an offset alone");
        assert!(
            parse(&whole().replace("region = \"program-rom\"\noffset = 64", "address = 0x8020")).is_ok(),
            "an address alone"
        );
    }

    /// Half a location, both ways round, each naming which half was given.
    #[test]
    fn half_a_location_is_refused_and_says_which_half() {
        assert_eq!(
            parse(&without("offset =")),
            Err(Error::HalfLocation {
                symbol: "expand".into(),
                given: "region"
            })
        );
        assert_eq!(
            parse(&without("region =")),
            Err(Error::HalfLocation {
                symbol: "expand".into(),
                given: "offset"
            })
        );
        let neither = without("region =");
        let neither = neither
            .lines()
            .filter(|l| !l.trim_start().starts_with("offset ="))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            parse(&neither),
            Err(Error::NoLocation {
                symbol: "expand".into()
            })
        );
    }

    /// The fields that may not be absent, each taken away on its own — and the
    /// whole thing parsing beside them, so this is not a blanket refusal.
    #[test]
    fn the_fields_a_symbol_cannot_do_without() {
        assert_eq!(
            parse(&without("description =")).unwrap_err().to_string(),
            "this is not a mapping file: missing field `description`",
            "absent is a parse error; present and empty is the next one"
        );
        assert_eq!(
            parse(&whole().replace(
                "description = \"Expands each input byte into two output bytes.\"",
                "description = \"   \""
            )),
            Err(Error::Empty {
                symbol: "expand".into(),
                field: "description"
            }),
            "§9.1: the description is read by the API, so it has to say something"
        );
        assert_eq!(
            parse(&whole().replace("provenance = { how = \"measured\" }", "provenance = {}")),
            Err(Error::Empty {
                symbol: "expand".into(),
                field: "provenance"
            }),
            "§9.2 is mandatory, and an empty table is not a provenance"
        );
        assert!(parse(&whole()).is_ok());
    }

    /// A zero length is a point written as though it were a span. Refused
    /// rather than quietly treated as either.
    #[test]
    fn a_length_of_zero_is_refused_and_one_of_one_is_not() {
        assert_eq!(
            parse(&whole().replace("length = 18", "length = 0")),
            Err(Error::NoLength {
                symbol: "expand".into()
            })
        );
        assert_eq!(
            parse(&whole().replace("length = 18", "length = 1"))
                .expect("one byte is a span")
                .symbols[0]
                .length,
            Some(1)
        );
        assert_eq!(
            parse(&without("length =")).expect("absent is a point").symbols[0].length,
            None,
            "and leaving it out is the way to say point"
        );
    }

    /// A field nobody declared is a mistake in somebody's mapping file, and the
    /// refusal has to name it or they will never find it — the same rule the
    /// protocol's commands follow.
    #[test]
    fn a_field_nobody_declared_is_refused_by_name() {
        let typo = whole().replace("length = 18", "lenght = 18");
        let err = parse(&typo).expect_err("a typo is not an extension");
        assert!(
            err.to_string().contains("lenght"),
            "the refusal names the field: {err}"
        );
    }

    /// Several symbols in one file, which is the ordinary case and the one the
    /// next unit's graph is built on.
    #[test]
    fn a_file_may_hold_more_than_one_and_an_empty_file_is_not_an_error() {
        let two = format!("{}\n{}", whole(), whole().replace("\"expand\"", "\"expand-2\""));
        let file = parse(&two).expect("two symbols");
        assert_eq!(file.symbols.len(), 2);
        assert_eq!(by_name(&file).len(), 2);

        assert_eq!(
            parse("").expect("an empty file is empty, not wrong"),
            File::default(),
            "a mapping split across files may have a file that holds nothing yet"
        );
    }
}
