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
//! mapping has ever wanted is a group *inside* a group: seven symbols have no
//! hierarchy.
//!
//! It is here anyway, and the reason is §9.3 rather than §9.1. "Group
//! references resolve to groups that exist" means a group has to be **declared**
//! somewhere, or a typo in a hand-written file quietly invents a group with one
//! member and nobody ever notices. Once a group is a declared thing with a
//! description, nesting is one optional field reusing the resolution that had to
//! exist anyway — not a second concept. That is a much smaller invention than
//! the first unit expected, and it is why this is written down rather than
//! deferred.
//!
//! # What a hypothesis does, and what it deliberately does not
//!
//! §9.2 says entries with weak provenance "are not treated as fact". Where that
//! bites has to be decided once rather than discovered later, so: **a hypothesis
//! changes what a report SAYS and never what a verdict IS.**
//!
//! The distinction that makes this safe is which side supplies the numbers.
//!
//! - Where the mapping only **names** something the measurement already found —
//!   which is all it does today — a hypothesis is a label. The verdict rests on
//!   the client's spans and the reference's bytes, neither of which the mapping
//!   touched, so folding a label into §2.3's three values would be refusing to
//!   answer a question that was answered. What the report owes is to say the
//!   name is a hypothesis, because presenting one beside names confirmed twice
//!   over is the well-formatted guess §9.2 exists to prevent.
//! - Where the mapping would **supply** a number the measurement rests on — the
//!   span to seed, the span to compare, the address to stop at — a hypothesis
//!   would bear on the verdict directly: a comparison over a span that may be
//!   the wrong span is not determined (§2.3), however neatly the bytes match.
//!
//! **Nothing in this project does the second yet**, and that is why this is
//! written here rather than built. The day a measurement takes its spans from
//! the mapping, a hypothesis among them stops being a label — and that is a
//! change to §2 and §5, which is the user's to make and not a thing to arrive
//! at by accident.
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
    /// The groups this symbol belongs to, by name. Every one of them must be
    /// declared (§9.3).
    pub groups: Vec<String>,
    /// §9.1: read by the API, "so that a consumer — human or program — obtains
    /// the context without reconstructing it from raw code". Required: a symbol
    /// nobody can explain is one nobody should rely on.
    pub description: String,
    /// §9.2, mandatory.
    pub provenance: Provenance,
    /// §9.1's relations to other symbols.
    pub relations: Vec<Relation>,
}

/// How a symbol was established — §9.2's first half, the epistemic one.
///
/// §9.2: "A value established by measurement and a value someone remembered are
/// not the same value, and a mapping that cannot tell them apart decays." These
/// are the three this project has actually needed, read off the real mapping
/// rather than imagined:
///
/// - **measured** — observed on the machine. The real mapping's strongest rows
///   are measured *twice by two different means*, which is stronger still and
///   is what the note is for;
/// - **inferred** — derived from something observed, but not observed. The real
///   mapping has exactly one of these, and it is the only row its author said
///   he would not trust again without checking;
/// - **assumed** — neither. Somebody said so, or it came from elsewhere.
///
/// §2.4 keeps the list at three. A fourth is added the day something needs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Established {
    Measured,
    Inferred,
    Assumed,
}

/// §9.2's provenance, mandatory on every symbol.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub how: Established,
    /// What was done, in the writer's own words. Required, and it carries both
    /// of §9.2's purposes:
    ///
    /// - the **epistemic** one, because "measured" alone does not say measured
    ///   how, and the real mapping's useful rows say "two sources: the
    ///   instruction's operand, and the span the memory actually changed";
    /// - the **distribution** one. §9.2 wants an entry derived from material
    ///   that may not be redistributable to be *identifiable rather than mixed
    ///   in*, and a mandatory note is what makes that findable by reading or by
    ///   searching. A structured source field would be a vocabulary nobody has
    ///   asked for (§2.4) and can be added later, which is the safe direction.
    pub note: String,
}

impl Provenance {
    /// §9.2: "Entries with weak provenance are marked as hypotheses and are not
    /// treated as fact."
    ///
    /// Anything not measured is a hypothesis. That is deliberately harsh on
    /// `inferred`: the real mapping's one inferred row was inferred from a
    /// single observation, and presenting it beside rows confirmed twice over
    /// is exactly the well-formatted guess §9.2 exists to prevent.
    pub fn is_hypothesis(&self) -> bool {
        !matches!(self.how, Established::Measured)
    }
}

/// §9.1's relation from one symbol to another.
///
/// `kind` is whatever the person writing the mapping says it is. This project
/// has no vocabulary of relation kinds and will not invent one (§2.4): the
/// real mapping's are "reads" and "writes", a different binary's would be
/// something else, and a closed list would make the format wrong for everybody
/// it had not thought of. What the tool checks is the half it can: that `to`
/// names a symbol that exists.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub kind: String,
    pub to: String,
}

/// §9.1's group: a concept, which the symbols belonging to it are scattered
/// parts of.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub name: String,
    pub description: String,
    /// Groups this one is part of — §9.1's nesting. Empty is the ordinary case.
    #[serde(default)]
    pub inside: Vec<String>,
}

/// What a mapping file holds, before any of §9.3's checks.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct File {
    pub symbols: Vec<Symbol>,
    pub groups: Vec<Group>,
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
    provenance: Provenance,
    #[serde(default, rename = "relation")]
    relations: Vec<Relation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileForm {
    #[serde(default, rename = "symbol")]
    symbols: Vec<SymbolForm>,
    #[serde(default, rename = "group")]
    groups: Vec<Group>,
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
        if s.provenance.note.trim().is_empty() {
            return Err(Error::Empty {
                symbol: s.name,
                field: "provenance.note",
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
            relations: s.relations,
        });
    }
    for g in &form.groups {
        if g.name.trim().is_empty() {
            return Err(Error::Empty {
                symbol: g.name.clone(),
                field: "name",
            });
        }
        if g.description.trim().is_empty() {
            return Err(Error::Empty {
                symbol: g.name.clone(),
                field: "description",
            });
        }
    }
    Ok(File {
        symbols,
        groups: form.groups,
    })
}

// --------------------------------------------------------------- the graph --

/// §9.1's mapping as one graph, however many files it was written in.
///
/// Built by `load`, which is the only way to make one: every check §9.3 names
/// happens there, so a `Mapping` that exists is one whose names are unique,
/// whose group references resolve and whose relations point at symbols that are
/// there. Nothing downstream re-checks, and nothing downstream has to.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mapping {
    symbols: BTreeMap<String, Symbol>,
    groups: BTreeMap<String, Group>,
}

impl Mapping {
    pub fn symbol(&self, name: &str) -> Option<&Symbol> {
        self.symbols.get(name)
    }

    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.get(name)
    }

    pub fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.values()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty() && self.groups.is_empty()
    }

    /// Every symbol in a group, **including** those in groups nested inside it.
    ///
    /// This is what §9.1 means by navigating a concept: asking for "the
    /// decompressor" gives its parts whether they were filed directly under it
    /// or under something that is part of it.
    pub fn members(&self, group: &str) -> Vec<&Symbol> {
        let mut wanted: Vec<&str> = vec![group];
        let mut seen: Vec<&str> = vec![group];
        let mut at = 0;
        while at < wanted.len() {
            let here = wanted[at];
            at += 1;
            for g in self.groups.values() {
                if g.inside.iter().any(|p| p == here) && !seen.contains(&g.name.as_str()) {
                    seen.push(&g.name);
                    wanted.push(&g.name);
                }
            }
        }
        self.symbols
            .values()
            .filter(|s| s.groups.iter().any(|g| wanted.contains(&g.as_str())))
            .collect()
    }
}

/// Why a mapping could not be made one graph — §9.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    /// §9.3: no two symbols share a name, **including across files**. The two
    /// files are named because a mapping split across several is exactly where
    /// this happens and where it is hardest to see.
    TwoSymbols {
        name: String,
        first: String,
        second: String,
    },
    TwoGroups {
        name: String,
        first: String,
        second: String,
    },
    /// §9.3: group references resolve to groups that exist.
    NoSuchGroup {
        group: String,
        wanted_by: String,
        file: String,
    },
    /// The same rule for relations, which §9.1 has and §9.3 does not mention.
    NoSuchSymbol {
        symbol: String,
        wanted_by: String,
        kind: String,
        file: String,
    },
    /// A group inside itself, however many steps round.
    GroupCycle { through: Vec<String> },
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::TwoSymbols { name, first, second } => write!(
                f,
                "the symbol `{name}` is declared in `{first}` and again in `{second}`. A mapping \
                 is one graph, so one name is one thing (§9.3)"
            ),
            GraphError::TwoGroups { name, first, second } => write!(
                f,
                "the group `{name}` is declared in `{first}` and again in `{second}` (§9.3)"
            ),
            GraphError::NoSuchGroup { group, wanted_by, file } => write!(
                f,
                "`{wanted_by}` in `{file}` is in the group `{group}`, which nothing declares. A \
                 group has to be declared somewhere, or a mistyped name quietly invents one with \
                 a single member (§9.3)"
            ),
            GraphError::NoSuchSymbol { symbol, wanted_by, kind, file } => write!(
                f,
                "`{wanted_by}` in `{file}` says it `{kind}` `{symbol}`, which no file declares"
            ),
            GraphError::GroupCycle { through } => write!(
                f,
                "these groups are inside each other: {}. A concept cannot be part of itself",
                through.join(" -> ")
            ),
        }
    }
}

impl std::error::Error for GraphError {}

/// Loads several files as one graph — §M7's first clause.
///
/// Each file arrives with the name it came from, because every refusal below
/// is one somebody has to go and find in a file, and "a duplicate name" without
/// the two files is a message that costs an afternoon.
pub fn load<'a>(files: impl IntoIterator<Item = (&'a str, &'a File)>) -> Result<Mapping, GraphError> {
    let mut symbols: BTreeMap<String, Symbol> = BTreeMap::new();
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    let mut whence: BTreeMap<String, String> = BTreeMap::new();

    for (file, held) in files {
        for symbol in &held.symbols {
            if let Some(first) = whence.get(&symbol.name) {
                return Err(GraphError::TwoSymbols {
                    name: symbol.name.clone(),
                    first: first.clone(),
                    second: file.to_string(),
                });
            }
            whence.insert(symbol.name.clone(), file.to_string());
            symbols.insert(symbol.name.clone(), symbol.clone());
        }
        for group in &held.groups {
            if let Some(first) = whence.get(&group.name) {
                return Err(GraphError::TwoGroups {
                    name: group.name.clone(),
                    first: first.clone(),
                    second: file.to_string(),
                });
            }
            whence.insert(group.name.clone(), file.to_string());
            groups.insert(group.name.clone(), group.clone());
        }
    }

    let found = |name: &str| whence.get(name).cloned().unwrap_or_default();

    for symbol in symbols.values() {
        for group in &symbol.groups {
            if !groups.contains_key(group) {
                return Err(GraphError::NoSuchGroup {
                    group: group.clone(),
                    wanted_by: symbol.name.clone(),
                    file: found(&symbol.name),
                });
            }
        }
        for relation in &symbol.relations {
            if !symbols.contains_key(&relation.to) {
                return Err(GraphError::NoSuchSymbol {
                    symbol: relation.to.clone(),
                    wanted_by: symbol.name.clone(),
                    kind: relation.kind.clone(),
                    file: found(&symbol.name),
                });
            }
        }
    }

    for group in groups.values() {
        for parent in &group.inside {
            if !groups.contains_key(parent) {
                return Err(GraphError::NoSuchGroup {
                    group: parent.clone(),
                    wanted_by: group.name.clone(),
                    file: found(&group.name),
                });
            }
        }
    }

    // A group inside itself, however many steps round. Walked rather than
    // counted, so that the refusal can print the way round.
    //
    // The walk is bounded by the number of groups as well as by the cycle it is
    // looking for, and the second bound is not redundant: a mapping file is a
    // stranger's input, and a loader whose termination depends on its own
    // detection being right is one a malformed file can hang. Found by mutation
    // — removing the cycle check made the test suite hang rather than fail.
    for start in groups.keys() {
        let mut path = vec![start.clone()];
        let mut at = start.clone();
        for _ in 0..groups.len() {
            let Some(next) = groups[&at].inside.first().cloned() else {
                break;
            };
            if path.contains(&next) {
                path.push(next);
                return Err(GraphError::GroupCycle { through: path });
            }
            path.push(next.clone());
            at = next;
        }
    }

    Ok(Mapping { symbols, groups })
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
provenance = { how = "measured", note = "seen on the machine" }

[[group]]
name = "the-decompressor"
description = "Everything that turns the packed stream into bytes."
"#
        .to_string()
    }

    /// One file, named, as `load` takes them.
    fn one(text: &str) -> Result<Mapping, GraphError> {
        let file = parse(text).expect("it parses");
        load([("a.toml", &file)])
    }

    /// Everything but the lines beginning with this — used to take exactly one
    /// field away at a time.
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
        assert_eq!(s.provenance.how, Established::Measured);
        assert!(!s.provenance.is_hypothesis(), "measured is not a hypothesis");
        assert!(s.relations.is_empty(), "§9.1 says relations, and none is a number");
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
provenance = { how = "measured", note = "seen on the machine" }
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
            parse(&whole().replace("note = \"seen on the machine\"", "note = \"   \"")),
            Err(Error::Empty {
                symbol: "expand".into(),
                field: "provenance.note"
            }),
            "§9.2 wants an audit trail, and a blank note is not one"
        );
        assert!(
            parse(&without("provenance ="))
                .unwrap_err()
                .to_string()
                .contains("provenance"),
            "and a symbol with no provenance at all is refused by name (§9.2 is mandatory)"
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

        assert_eq!(
            parse("").expect("an empty file is empty, not wrong"),
            File::default(),
            "a mapping split across files may have a file that holds nothing yet"
        );
    }

    // ------------------------------------------------------- the graph --

    /// §M7's first clause: several files load as one graph, and a symbol in
    /// one file may belong to a group declared in another.
    #[test]
    fn several_files_become_one_graph() {
        let concepts = parse(
            r#"
[[group]]
name = "the-decompressor"
description = "Everything that turns the packed stream into bytes."

[[group]]
name = "its-inner-loop"
description = "The part that runs once per byte."
inside = ["the-decompressor"]
"#,
        )
        .expect("groups parse");

        let code = parse(
            r#"
[[symbol]]
name = "expand"
address = 0x8040
groups = ["the-decompressor"]
description = "Entered once per screen."
provenance = { how = "measured", note = "seen on the machine" }
[[symbol.relation]]
kind = "writes"
to = "out-buffer"

[[symbol]]
name = "hot-loop"
address = 0x8044
groups = ["its-inner-loop"]
description = "Runs once per byte."
provenance = { how = "measured", note = "seen on the machine" }
"#,
        )
        .expect("symbols parse");

        let data = parse(
            r#"
[[symbol]]
name = "out-buffer"
region = "work-ram"
offset = 1024
length = 64
groups = ["the-decompressor"]
description = "Where the expanded bytes land."
provenance = { how = "measured", note = "seen on the machine" }
"#,
        )
        .expect("data parses");

        let map = load([
            ("concepts.toml", &concepts),
            ("code.toml", &code),
            ("data.toml", &data),
        ])
        .expect("three files, one graph");

        assert!(map.symbol("expand").is_some());
        assert!(map.symbol("out-buffer").is_some(), "declared in another file");
        assert_eq!(map.symbols().count(), 3);

        // The relation crossed a file boundary and resolved.
        assert_eq!(map.symbol("expand").expect("it").relations[0].to, "out-buffer");

        // §9.1's point: a concept scattered across files is navigable as a
        // concept — and the nested group's member comes with it.
        let mut members: Vec<&str> = map
            .members("the-decompressor")
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        members.sort_unstable();
        assert_eq!(
            members,
            vec!["expand", "hot-loop", "out-buffer"],
            "including the one filed under a group nested inside it"
        );
        assert_eq!(
            map.members("its-inner-loop")
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec!["hot-loop"],
            "and asking for the inner one does not drag the outer one in"
        );
    }

    /// §M7's second clause, and the whole reason it is stated separately: a
    /// duplicate within one file is easy to see, and one ACROSS files is not.
    #[test]
    fn one_name_is_one_thing_across_files_and_two_names_are_two() {
        let first = parse(
            r#"
[[symbol]]
name = "expand"
address = 0x8040
description = "One."
provenance = { how = "measured", note = "seen on the machine" }
"#,
        )
        .expect("parses");
        let again = parse(
            r#"
[[symbol]]
name = "expand"
address = 0x9000
description = "Another, and somebody is wrong."
provenance = { how = "measured", note = "seen on the machine" }
"#,
        )
        .expect("parses");

        let err = load([("code.toml", &first), ("more.toml", &again)])
            .expect_err("two files, one name");
        assert_eq!(
            err,
            GraphError::TwoSymbols {
                name: "expand".into(),
                first: "code.toml".into(),
                second: "more.toml".into()
            }
        );
        let said = err.to_string();
        assert!(
            said.contains("code.toml") && said.contains("more.toml"),
            "both files, or somebody spends an afternoon: {said}"
        );

        // The near miss: the same two files with different names load.
        let renamed = parse(
            r#"
[[symbol]]
name = "expand-2"
address = 0x9000
description = "Another."
provenance = { how = "measured", note = "seen on the machine" }
"#,
        )
        .expect("parses");
        assert!(load([("code.toml", &first), ("more.toml", &renamed)]).is_ok());
    }

    /// A group and a symbol share one namespace, because a mapping is one
    /// graph and a name in it is one thing.
    #[test]
    fn a_group_may_not_take_a_name_a_symbol_has() {
        let a = parse(
            r#"
[[symbol]]
name = "expand"
address = 0x8040
description = "One."
provenance = { how = "measured", note = "seen on the machine" }
"#,
        )
        .expect("parses");
        let b = parse(
            r#"
[[group]]
name = "expand"
description = "A concept with a symbol's name."
"#,
        )
        .expect("parses");
        assert!(matches!(
            load([("code.toml", &a), ("concepts.toml", &b)]),
            Err(GraphError::TwoGroups { .. })
        ));
    }

    /// §9.3: a group reference resolves to a group that exists. This is the
    /// check that makes a typo in a hand-written file findable.
    #[test]
    fn a_group_nothing_declares_is_refused_and_a_declared_one_is_not() {
        let text = |group: &str| {
            format!(
                r#"
[[group]]
name = "the-decompressor"
description = "A concept."

[[symbol]]
name = "expand"
address = 0x8040
groups = ["{group}"]
description = "One."
provenance = {{ how = "measured", note = "seen on the machine" }}
"#
            )
        };

        let err = one(&text("the-decompressr")).expect_err("a typo invents nothing");
        assert_eq!(
            err,
            GraphError::NoSuchGroup {
                group: "the-decompressr".into(),
                wanted_by: "expand".into(),
                file: "a.toml".into()
            }
        );
        assert!(one(&text("the-decompressor")).is_ok(), "spelled right");
    }

    /// The same rule for relations, which §9.1 has and §9.3 forgot to mention.
    #[test]
    fn a_relation_to_nothing_is_refused_and_one_to_something_is_not() {
        let text = |to: &str| {
            format!(
                r#"
[[symbol]]
name = "expand"
address = 0x8040
description = "One."
provenance = {{ how = "measured", note = "seen on the machine" }}
[[symbol.relation]]
kind = "writes"
to = "{to}"

[[symbol]]
name = "out-buffer"
region = "work-ram"
offset = 0
length = 4
description = "Two."
provenance = {{ how = "measured", note = "seen on the machine" }}
"#
            )
        };
        let err = one(&text("out-bufer")).expect_err("nothing of that name");
        assert_eq!(
            err,
            GraphError::NoSuchSymbol {
                symbol: "out-bufer".into(),
                wanted_by: "expand".into(),
                kind: "writes".into(),
                file: "a.toml".into()
            }
        );
        assert!(one(&text("out-buffer")).is_ok());
    }

    /// A concept cannot be part of itself, however many steps round — and the
    /// refusal prints the way round, because a cycle through four groups is
    /// not something anybody finds by reading.
    #[test]
    fn a_group_inside_itself_is_refused_at_any_distance() {
        let direct = one(
            r#"
[[group]]
name = "a"
description = "One."
inside = ["a"]
"#,
        )
        .expect_err("itself");
        assert!(matches!(direct, GraphError::GroupCycle { .. }), "{direct}");

        let round = one(
            r#"
[[group]]
name = "a"
description = "One."
inside = ["b"]

[[group]]
name = "b"
description = "Two."
inside = ["c"]

[[group]]
name = "c"
description = "Three."
inside = ["a"]
"#,
        )
        .expect_err("three steps round");
        match &round {
            GraphError::GroupCycle { through } => {
                assert!(through.len() >= 4, "the way round is printed: {through:?}");
            }
            other => panic!("got {other}"),
        }

        // The near miss, and it is NOT a cycle: a chain, and a group two
        // different groups are both inside.
        assert!(
            one(r#"
[[group]]
name = "a"
description = "One."

[[group]]
name = "b"
description = "Two."
inside = ["a"]

[[group]]
name = "c"
description = "Three."
inside = ["a"]
"#)
            .is_ok(),
            "two groups inside one is a shape, not a cycle"
        );
    }

    /// An empty mapping is a mapping. A project that has written no files yet
    /// is not a project with a broken one.
    #[test]
    fn no_files_is_an_empty_graph_and_not_a_refusal() {
        let map = load([]).expect("nothing is not wrong");
        assert!(map.is_empty());
        assert!(map.symbol("anything").is_none());
        assert!(map.members("anything").is_empty());
    }

    // --------------------------------------------------- §9.2's provenance --

    /// The three values, and which of them §9.2 calls a hypothesis.
    #[test]
    fn measured_is_fact_and_everything_else_is_a_hypothesis() {
        let of = |how: &str| {
            parse(&whole().replace("how = \"measured\"", &format!("how = \"{how}\""))) 
                .expect("it parses")
                .symbols
                .swap_remove(0)
                .provenance
        };

        assert_eq!(of("measured").how, Established::Measured);
        assert!(!of("measured").is_hypothesis(), "observed on the machine");

        assert!(
            of("inferred").is_hypothesis(),
            "derived from an observation is not an observation — the real mapping's one \
             inferred row is the only one its author would not trust again"
        );
        assert!(of("assumed").is_hypothesis(), "somebody said so");

        // And the three are three, not two: a reader branching on `how` must be
        // able to tell inferred from assumed, even though both are hypotheses.
        assert_ne!(of("inferred").how, of("assumed").how);
    }

    /// A value nobody declared is a mistake in somebody's file, and §2.4 does
    /// not guess which of the three was meant. The near miss is the right
    /// spelling parsing.
    #[test]
    fn a_provenance_nobody_declared_is_refused_and_names_what_it_wanted() {
        let err = parse(&whole().replace("how = \"measured\"", "how = \"measuered\""))
            .expect_err("a typo is not a fourth kind of evidence");
        let said = err.to_string();
        assert!(
            said.contains("measured") && said.contains("inferred") && said.contains("assumed"),
            "the refusal lists what it would have accepted: {said}"
        );
        assert!(parse(&whole()).is_ok());
    }

    /// §9.2's second purpose, the one the specification calls not obvious: the
    /// note is the audit trail, so it may not be blank even when `how` is the
    /// strongest value there is.
    #[test]
    fn the_strongest_provenance_still_needs_its_note() {
        assert_eq!(
            parse(&whole().replace("note = \"seen on the machine\"", "note = \"\"")),
            Err(Error::Empty {
                symbol: "expand".into(),
                field: "provenance.note"
            }),
            "`measured` does not say measured HOW, and §9.2 wants an entry whose derivation \
             may be unshareable to be identifiable rather than mixed in"
        );
    }
}
