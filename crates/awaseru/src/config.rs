//! The configuration — §6.
//!
//! # The two halves
//!
//! `awaseru.toml` names things and declares invariants; `awaseru.local.toml`
//! says where they are on this machine (§6.1). The first travels in a
//! repository, which is why it cannot carry an absolute path.
//!
//! # How they resolve
//!
//! The local file overrides the shared one **per key**, last file read wins —
//! the `.env` / `.env.local` convention (§6.2). Per key, not per section: a
//! local file that sets one key of a table leaves the rest of that table alone.
//! The merge is therefore done on the parsed documents, before either is given
//! a shape, which is also what makes the invariant rule possible.
//!
//! Two things are not a per-key merge, and both for reasons the spec gives:
//!
//! - **An invariant may not be overridden** (§6.2). The attempt is reported and
//!   the tool stops. Overriding the software's identity annuls the reason it is
//!   declared, and §6.6 is explicit that meaningless comparisons which pass are
//!   worse than a stopped run.
//! - **Emulator locations are keyed by name** (§6.4), so the local half's
//!   `emulator` is a table of names and the shared half's is a list of
//!   declarations. They are joined by name rather than merged, which is the
//!   same reason §6.4 gives for `use` being explicit: appending a declaration
//!   to a shared file must not silently re-pair somebody's local paths.
//!
//! # What is refused rather than ignored
//!
//! A key neither file is supposed to contain is an error. A configuration key
//! that is quietly ignored is §6.3's rule 5 in another costume: the typo does
//! nothing, and nobody ever finds out. That includes keys for parts of the tool
//! that do not exist yet — a `[mapping]` section is refused today, because
//! accepting one and not expanding it would be worse than saying so.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::Value;

use awaseru_core::anchor::{Anchor, AnchorError, Anchors, Definition, InputLog, Start};
use awaseru_core::run::Bound;

use crate::digest;

/// Keys the shared file owns and the local file may not override (§6.2).
///
/// A list because §6.2 speaks of invariants in general, with exactly one named:
/// the software's identity. Adding to this list is a decision about the
/// specification, not about the code, so nothing here grows it on its own.
pub const INVARIANTS: &[&str] = &["rom.sha256"];

/// The shared half, after the local half has been merged over it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub project: Project,
    pub rom: Rom,
    /// The declarations, in the order they are written. Named rather than
    /// positional everywhere else (§6.4).
    #[serde(default, rename = "emulator")]
    pub emulators: Vec<Emulator>,
    pub reference: Reference,
    /// §4.9's verification policy. Absent means the defaults, which are the
    /// careful ones.
    #[serde(default)]
    pub anchors: AnchorPolicy,
    /// §4.7's anchors, declared in the shared file because they are things the
    /// project names and so travel with it (§6.7). The *cache* of their blobs
    /// is machine-local and is not configuration at all.
    #[serde(default, rename = "anchor")]
    pub anchor_declarations: Vec<AnchorDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Which platform boundary to go through. A name the host matches against
    /// the backends it was built with (§7.1); nothing interprets it further.
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rom {
    /// The invariant: what the software *is* (§6.6).
    pub sha256: String,
    /// Where it is, which only the local half can say (§6.1).
    #[serde(default)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Emulator {
    /// What reports call it. Distinct from `backend`, because two builds of one
    /// backend must be distinguishable (§6.4).
    pub name: String,
    pub platform: String,
    /// Which backend implementation this is.
    pub backend: String,
    /// Which version of it, checked against the loaded library (§16.1).
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    /// The emulator whose readings are the ground, by name (§6.4).
    #[serde(rename = "use")]
    pub uses: String,
    /// Others to cross-check against (§5.5). Not used yet; declared because
    /// refusing an unknown key means every key a configuration may carry has
    /// to be declared somewhere.
    #[serde(default)]
    pub crosscheck: Vec<String>,
    /// Begin at the reproducible power-on rather than wherever the backend had
    /// got to (§2.5). On by default, because a reference that does not start
    /// the same way twice cannot be the ground for anything.
    #[serde(default = "yes")]
    pub start_at_power_on: bool,
    /// Write zeros over every writable memory there.
    ///
    /// On by default, and it is **a divergence from the hardware**: a real
    /// console has rubbish in its memory at power-on and software that reads it
    /// behaves differently. Turning it off makes runs stop repeating between
    /// processes, which every report then says.
    #[serde(default = "yes")]
    pub zero_memory: bool,
}

fn yes() -> bool {
    true
}

/// §4.9's policy: how much verification, and how often.
///
/// Both numbers are the user's, because the trade is theirs — replaying from
/// the origin is the expensive thing an anchor exists to avoid.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorPolicy {
    /// Replays of a definition before an anchor is trusted (§4.8). Three is the
    /// default. **Zero means never demonstrated** — permitted, and every
    /// verdict made from that anchor says so.
    #[serde(default = "three")]
    pub verify_from_origin: u32,
    /// After this many uses, replay from the origin again and check the blob
    /// still produces the same state. Zero means never.
    ///
    /// Fifty is the default, which is the number §4.9 illustrates. The cheap
    /// check covers only what an anchor declares; this is the audit of the
    /// cheap check, and switching it off is a decision rather than an absence.
    #[serde(default = "fifty")]
    pub reverify_after: u64,
}

fn three() -> u32 {
    3
}

fn fifty() -> u64 {
    50
}

impl Default for AnchorPolicy {
    fn default() -> Self {
        AnchorPolicy {
            verify_from_origin: three(),
            reverify_after: fifty(),
        }
    }
}

/// One `[[anchor]]` as the shared file writes it.
///
/// The bound is given as exactly one of three keys rather than as a tagged
/// value, because that is how a person writes it. Giving none, or more than
/// one, is refused: a default bound would be a number nobody chose, and two
/// would make the anchor mean whichever the code happened to read first.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorDeclaration {
    pub name: String,
    /// The anchor this one begins at. **Absent means power-on.**
    ///
    /// A single `from = "power-on"` key would be ambiguous the day somebody
    /// names an anchor `power-on`, and a configuration that reads two ways is
    /// worse than one with two keys.
    #[serde(default)]
    pub after: Option<String>,
    /// Run to the end of this many frames.
    #[serde(default)]
    pub frames: Option<u64>,
    /// Run for this many instructions.
    #[serde(default)]
    pub instructions: Option<u64>,
    /// Run until the program counter reaches this address, in hexadecimal
    /// because that is how addresses are written.
    #[serde(default)]
    pub address: Option<String>,
    /// The regions §4.8's cheap check digests on every load. §4.10's guidance
    /// is to declare the ones the comparisons read.
    #[serde(default)]
    pub covers: Vec<String>,
    /// A recorded input log, where the software needs input before it will
    /// proceed (§4.7). Carried and **refused**: nothing can replay one yet,
    /// and reaching the anchor without it would arrive somewhere else.
    #[serde(default)]
    pub input: Option<PathBuf>,
}

/// Where one emulator is on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub path: PathBuf,
}

/// A configuration that has been read, merged, and checked as far as can be
/// checked without loading anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub configuration: Configuration,
    /// Keyed by emulator name (§6.4).
    pub locations: BTreeMap<String, Location>,
    /// The software's path, already checked to exist and to hash to what the
    /// shared half declares (§6.6).
    pub software: PathBuf,
    /// The declared anchors, already checked to name each other sensibly
    /// (§4.7, §4.11).
    pub anchors: Anchors,
}

impl Loaded {
    /// The emulator the reference uses, and where it is.
    ///
    /// Both are known to exist: `load` refuses a configuration where they do
    /// not, so this cannot fail and nothing downstream has to handle a case
    /// that was already decided.
    pub fn reference(&self) -> (&Emulator, &Location) {
        let name = &self.configuration.reference.uses;
        let emulator = self
            .configuration
            .emulators
            .iter()
            .find(|e| &e.name == name)
            .expect("load refuses a reference naming no declared emulator");
        let location = self
            .locations
            .get(name)
            .expect("load refuses a reference with no location on this machine");
        (emulator, location)
    }
}

/// Why a configuration was not accepted.
#[derive(Debug)]
pub enum Error {
    Unreadable {
        path: PathBuf,
        why: std::io::Error,
    },
    Unparsable {
        path: PathBuf,
        why: toml::de::Error,
    },
    /// The merged document does not have the shape a configuration has.
    Malformed {
        why: toml::de::Error,
    },
    /// §6.2's exception: the local half tried to override an invariant.
    InvariantOverridden {
        key: &'static str,
    },
    /// The local half gives a location for an emulator nothing declares. A
    /// typo, under any other treatment, is a path that is never used and never
    /// missed.
    LocationForUnknownEmulator {
        name: String,
        declared: Vec<String>,
    },
    /// Two declarations share a name, so `use` would be ambiguous.
    DuplicateEmulator {
        name: String,
    },
    ReferenceNotDeclared {
        name: String,
        declared: Vec<String>,
    },
    ReferenceHasNoLocation {
        name: String,
    },
    /// An emulator declared for a platform the project is not.
    PlatformDisagrees {
        name: String,
        project: String,
        emulator: String,
    },
    /// The shared half declares an identity and the local half says nothing
    /// about where the software is.
    SoftwarePathMissing,
    SoftwareMissing {
        path: PathBuf,
    },
    /// §6.6. The file is not the one the configuration was written against.
    SoftwareIdentityMismatch {
        path: PathBuf,
        declared: String,
        found: String,
    },
    /// An anchor gives no bound, or more than one.
    AnchorBound {
        name: String,
        given: Vec<&'static str>,
    },
    /// An anchor's address is not a hexadecimal number.
    AnchorAddress { name: String, given: String },
    /// The anchors do not make sense together — a circle, a parent nobody
    /// declares, or two of one name (§4.7).
    Anchors(AnchorError),
    /// An input log was named and could not be read.
    InputLogUnreadable {
        anchor: String,
        path: PathBuf,
        why: std::io::Error,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unreadable { path, why } => {
                write!(f, "{} could not be read: {why}", path.display())
            }
            Error::Unparsable { path, why } => {
                write!(f, "{} is not valid TOML: {why}", path.display())
            }
            Error::Malformed { why } => write!(
                f,
                "the configuration does not have the shape of one: {why}. A key awaseru does not \
                 understand is refused rather than ignored, including a key for a part of the \
                 tool that does not exist yet"
            ),
            Error::InvariantOverridden { key } => write!(
                f,
                "the machine-local file sets `{key}`, which is an invariant and may not be \
                 overridden there (§6.2). It declares what the software *is*; a local override \
                 would annul the reason it is written down"
            ),
            Error::LocationForUnknownEmulator { name, declared } => write!(
                f,
                "the machine-local file gives a location for an emulator called `{name}`, which \
                 nothing declares. Declared: {}. A location for a name nobody uses is a typo that \
                 would otherwise never be noticed",
                list(declared)
            ),
            Error::DuplicateEmulator { name } => write!(
                f,
                "two emulators are declared as `{name}`, so naming one of them is ambiguous"
            ),
            Error::ReferenceNotDeclared { name, declared } => write!(
                f,
                "the reference is `{name}`, which no emulator declares. Declared: {}",
                list(declared)
            ),
            Error::ReferenceHasNoLocation { name } => write!(
                f,
                "the reference is `{name}` and the machine-local file does not say where it is on \
                 this machine (§6.1)"
            ),
            Error::PlatformDisagrees {
                name,
                project,
                emulator,
            } => write!(
                f,
                "the project is `{project}` and the emulator `{name}` is declared for \
                 `{emulator}`; one of the two is wrong"
            ),
            Error::SoftwarePathMissing => write!(
                f,
                "the machine-local file does not say where the software is (§6.1). The shared file \
                 declares what it is, and only the local one can say where"
            ),
            Error::SoftwareMissing { path } => {
                write!(f, "there is nothing at {}", path.display())
            }
            Error::SoftwareIdentityMismatch {
                path,
                declared,
                found,
            } => write!(
                f,
                "{} hashes to {found} and the configuration declares {declared}. This is a \
                 different build of the software, and every comparison made against it would be \
                 meaningless — which is worse than stopping (§6.6)",
                path.display()
            ),
            Error::AnchorBound { name, given } => write!(
                f,
                "the anchor `{name}` {}. Exactly one of `frames`, `instructions` or `address` \
                 says how far to run from where it starts (§4.2); a default would be a number \
                 nobody chose, and two would make the anchor mean whichever was read first",
                if given.is_empty() {
                    "says how far to run in no way at all".to_string()
                } else {
                    format!(
                        "gives {} ways to say how far to run: {}",
                        given.len(),
                        given.join(", ")
                    )
                }
            ),
            Error::AnchorAddress { name, given } => write!(
                f,
                "the anchor `{name}` gives the address `{given}`, which is not a hexadecimal \
                 number"
            ),
            Error::Anchors(e) => write!(f, "{e}"),
            Error::InputLogUnreadable { anchor, path, why } => write!(
                f,
                "the input log for the anchor `{anchor}` could not be read from {}: {why}",
                path.display()
            ),
        }
    }
}

impl From<AnchorError> for Error {
    fn from(e: AnchorError) -> Self {
        Error::Anchors(e)
    }
}

impl std::error::Error for Error {}

fn list(names: &[String]) -> String {
    if names.is_empty() {
        "nothing".to_string()
    } else {
        names.join(", ")
    }
}

/// Reads both halves, resolves them, and checks what can be checked here.
///
/// What is **not** checked here is anything that needs the backend loaded: the
/// version it reports (§16.1) and the regions it exposes (§6.5). Those belong
/// to whoever opens it, because they cost a library load.
pub fn load(shared_path: &Path, local_path: &Path) -> Result<Loaded, Error> {
    let shared = read_document(shared_path)?;
    let local = read_document(local_path)?;
    resolve_beside(shared, local, shared_path.parent().unwrap_or(Path::new(".")))
}

fn read_document(path: &Path) -> Result<toml::Table, Error> {
    let text = std::fs::read_to_string(path).map_err(|why| Error::Unreadable {
        path: path.to_path_buf(),
        why,
    })?;
    text.parse::<toml::Table>().map_err(|why| Error::Unparsable {
        path: path.to_path_buf(),
        why,
    })
}

/// The resolution rules, separated from the files so that they can be tested
/// without any.
pub fn resolve(shared: toml::Table, local: toml::Table) -> Result<Loaded, Error> {
    resolve_beside(shared, local, Path::new("."))
}

/// The same, saying which directory an anchor's input log is written beside —
/// §6.3's rule that a relative path resolves against the file it is written in
/// and never against the working directory.
pub fn resolve_beside(
    shared: toml::Table,
    mut local: toml::Table,
    beside: &Path,
) -> Result<Loaded, Error> {
    for key in INVARIANTS {
        if dotted(&local, key).is_some() {
            return Err(Error::InvariantOverridden { key });
        }
    }

    // Taken out before the merge: the two halves hold `emulator` in different
    // shapes on purpose, so they are joined by name instead (§6.4).
    let local_emulators = local.remove("emulator");

    let merged = merge(Value::Table(shared), Value::Table(local));
    let configuration: Configuration =
        merged.try_into().map_err(|why| Error::Malformed { why })?;

    let locations: BTreeMap<String, Location> = match local_emulators {
        None => BTreeMap::new(),
        Some(value) => value.try_into().map_err(|why| Error::Malformed { why })?,
    };

    check(&configuration, &locations)?;

    let software = configuration
        .rom
        .path
        .clone()
        .ok_or(Error::SoftwarePathMissing)?;
    if !software.is_file() {
        return Err(Error::SoftwareMissing { path: software });
    }
    let found = digest::of_file(&software).map_err(|why| Error::Unreadable {
        path: software.clone(),
        why,
    })?;
    if found != configuration.rom.sha256 {
        return Err(Error::SoftwareIdentityMismatch {
            path: software,
            declared: configuration.rom.sha256.clone(),
            found,
        });
    }

    let anchors = anchors_from(&configuration.anchor_declarations, beside)?;

    Ok(Loaded {
        configuration,
        locations,
        software,
        anchors,
    })
}

/// Turns the declarations into §4.7's anchors, refusing what cannot be one.
///
/// Every declared anchor's chain is resolved here rather than when it is first
/// used, so a circle or a missing parent is a configuration error found at
/// startup and not something discovered halfway through a replay.
fn anchors_from(declarations: &[AnchorDeclaration], beside: &Path) -> Result<Anchors, Error> {
    let mut anchors = Vec::new();
    for declaration in declarations {
        let mut given = Vec::new();
        if declaration.frames.is_some() {
            given.push("frames");
        }
        if declaration.instructions.is_some() {
            given.push("instructions");
        }
        if declaration.address.is_some() {
            given.push("address");
        }
        if given.len() != 1 {
            return Err(Error::AnchorBound {
                name: declaration.name.clone(),
                given,
            });
        }

        let bound = if let Some(frames) = declaration.frames {
            Bound::Frames(frames)
        } else if let Some(instructions) = declaration.instructions {
            Bound::Instructions(instructions)
        } else {
            let text = declaration.address.as_deref().unwrap_or_default();
            let cleaned = text.trim_start_matches("0x").replace('_', "");
            let address = u64::from_str_radix(&cleaned, 16).map_err(|_| Error::AnchorAddress {
                name: declaration.name.clone(),
                given: text.to_string(),
            })?;
            Bound::Address(address)
        };

        // Read now, so that a log named and missing is a configuration error
        // rather than a surprise at the moment it is needed. The anchor is
        // refused either way (§4.7), and refused with the log in hand is a
        // better refusal than refused because the file was not there.
        let input = match &declaration.input {
            None => None,
            Some(relative) => {
                let path = beside.join(relative);
                let recorded = std::fs::read(&path).map_err(|why| Error::InputLogUnreadable {
                    anchor: declaration.name.clone(),
                    path: path.clone(),
                    why,
                })?;
                Some(InputLog {
                    name: relative.display().to_string(),
                    recorded,
                })
            }
        };

        anchors.push(Anchor {
            name: declaration.name.clone(),
            definition: Definition {
                start: match &declaration.after {
                    None => Start::PowerOn,
                    Some(parent) => Start::Anchor(parent.clone()),
                },
                bound,
                input,
            },
            covers: declaration.covers.clone(),
        });
    }

    let anchors = Anchors::new(anchors)?;
    for name in anchors.names().map(str::to_string).collect::<Vec<_>>() {
        match anchors.chain(&name) {
            Ok(_) => {}
            // An anchor needing input is declared-and-refused rather than
            // malformed: the configuration is right and the tool cannot do it
            // yet, so loading succeeds and asking for *that* anchor refuses.
            Err(AnchorError::InputNotSupported { .. }) => {}
            Err(e) => return Err(Error::Anchors(e)),
        }
    }
    Ok(anchors)
}

/// Everything that can be decided from the two documents alone.
fn check(
    configuration: &Configuration,
    locations: &BTreeMap<String, Location>,
) -> Result<(), Error> {
    let declared: Vec<String> = configuration
        .emulators
        .iter()
        .map(|e| e.name.clone())
        .collect();

    for (i, emulator) in configuration.emulators.iter().enumerate() {
        if configuration.emulators[i + 1..]
            .iter()
            .any(|other| other.name == emulator.name)
        {
            return Err(Error::DuplicateEmulator {
                name: emulator.name.clone(),
            });
        }
        if emulator.platform != configuration.project.platform {
            return Err(Error::PlatformDisagrees {
                name: emulator.name.clone(),
                project: configuration.project.platform.clone(),
                emulator: emulator.platform.clone(),
            });
        }
    }

    for name in locations.keys() {
        if !declared.contains(name) {
            return Err(Error::LocationForUnknownEmulator {
                name: name.clone(),
                declared,
            });
        }
    }

    let wanted = &configuration.reference.uses;
    if !declared.contains(wanted) {
        return Err(Error::ReferenceNotDeclared {
            name: wanted.clone(),
            declared,
        });
    }
    if !locations.contains_key(wanted) {
        return Err(Error::ReferenceHasNoLocation {
            name: wanted.clone(),
        });
    }

    Ok(())
}

/// Looks a dotted key up in a document, so that an invariant can be named the
/// way a person would write it.
fn dotted<'a>(table: &'a toml::Table, key: &str) -> Option<&'a Value> {
    let mut parts = key.split('.');
    let first = parts.next()?;
    let mut current = table.get(first)?;
    for part in parts {
        current = current.as_table()?.get(part)?;
    }
    Some(current)
}

/// `over` wins, per key, recursing into tables.
///
/// Anything that is not a table is replaced whole. An array in particular:
/// merging two lists element by element would mean the shared file's third
/// entry and the local file's third entry are about the same thing, which
/// nothing guarantees.
fn merge(base: Value, over: Value) -> Value {
    match (base, over) {
        (Value::Table(mut base), Value::Table(over)) => {
            for (key, value) in over {
                let merged = match base.remove(&key) {
                    Some(existing) => merge(existing, value),
                    None => value,
                };
                base.insert(key, merged);
            }
            Value::Table(base)
        }
        (_, over) => over,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> toml::Table {
        text.parse().expect("the test's own TOML parses")
    }

    fn shared() -> toml::Table {
        table(
            r#"
            [project]
            platform = "a-platform"

            [rom]
            sha256 = "00"

            [[emulator]]
            name = "ref-a"
            platform = "a-platform"
            backend = "a-backend"
            version = "1.0.0"

            [reference]
            use = "ref-a"
        "#,
        )
    }

    // ---- the merge ----------------------------------------------------

    /// Per key, not per section. A local file that sets one key of a table must
    /// leave the rest of that table alone — a merge that replaced the table
    /// would silently drop the shared half's other keys, which is the mistake
    /// §6.2's wording exists to rule out.
    #[test]
    fn the_local_half_overrides_one_key_and_leaves_its_neighbours() {
        let merged = merge(
            Value::Table(table(
                r#"
                [rom]
                sha256 = "aa"
                note = "kept"
            "#,
            )),
            Value::Table(table(
                r#"
                [rom]
                path = "/somewhere"
            "#,
            )),
        );
        let rom = merged.get("rom").and_then(Value::as_table).expect("a table");
        assert_eq!(rom.get("sha256").and_then(Value::as_str), Some("aa"));
        assert_eq!(rom.get("note").and_then(Value::as_str), Some("kept"));
        assert_eq!(rom.get("path").and_then(Value::as_str), Some("/somewhere"));
    }

    #[test]
    fn the_local_half_wins_where_both_set_the_same_key() {
        let merged = merge(
            Value::Table(table("[reference]\nuse = \"ref-a\"\n")),
            Value::Table(table("[reference]\nuse = \"ref-b\"\n")),
        );
        assert_eq!(
            dotted(merged.as_table().unwrap(), "reference.use").and_then(Value::as_str),
            Some("ref-b"),
            "the last file read wins (§6.2)"
        );
    }

    /// Arrays are replaced, not zipped. Zipping would mean the two files' nth
    /// entries are about the same thing, which nothing makes true.
    #[test]
    fn an_array_is_replaced_whole() {
        let merged = merge(
            Value::Table(table("[reference]\ncrosscheck = [\"x\", \"y\"]\n")),
            Value::Table(table("[reference]\ncrosscheck = [\"z\"]\n")),
        );
        let got = dotted(merged.as_table().unwrap(), "reference.crosscheck")
            .and_then(Value::as_array)
            .expect("an array");
        assert_eq!(got.len(), 1, "replaced, not merged: {got:?}");
    }

    /// Nesting deeper than one level still merges per key.
    #[test]
    fn the_merge_goes_all_the_way_down() {
        let merged = merge(
            Value::Table(table("[a.b.c]\nkept = 1\nbeaten = 1\n")),
            Value::Table(table("[a.b.c]\nbeaten = 2\n")),
        );
        let table = merged.as_table().unwrap();
        assert_eq!(dotted(table, "a.b.c.kept").and_then(Value::as_integer), Some(1));
        assert_eq!(dotted(table, "a.b.c.beaten").and_then(Value::as_integer), Some(2));
    }

    // ---- the invariant ------------------------------------------------

    /// §6.2's exception, and the sharpest rule in this module: the local file
    /// may override anything except this, and the attempt stops the tool
    /// instead of being applied.
    ///
    /// Written as an ordinary merge, this test's configuration would load
    /// happily and every comparison afterwards would be against software the
    /// shared file does not describe.
    #[test]
    fn overriding_the_software_identity_is_refused_and_not_applied() {
        let local = table(
            r#"
            [rom]
            sha256 = "ff"
            path = "/somewhere"
        "#,
        );
        let err = resolve(shared(), local).expect_err("an invariant was overridden");
        match err {
            Error::InvariantOverridden { key } => assert_eq!(key, "rom.sha256"),
            other => panic!("expected the override to be refused, got {other}"),
        }
        assert!(
            err.to_string().contains("§6.2"),
            "and the message must say which rule it is: {err}"
        );
    }

    /// The local half may still set the key *next* to the invariant. A refusal
    /// that caught the whole `[rom]` table would make the one thing the local
    /// file must carry impossible to write.
    #[test]
    fn the_local_half_may_say_where_the_software_is() {
        let local = table(
            r#"
            [rom]
            path = "/no/such/file"

            [emulator.ref-a]
            path = "/lib.so"
        "#,
        );
        let err = resolve(shared(), local).expect_err("there is no file there");
        assert!(
            matches!(err, Error::SoftwareMissing { .. }),
            "it should have got as far as looking for the file, got {err}"
        );
    }

    // ---- shape and names ----------------------------------------------

    /// A key nothing understands is refused. The case that matters is a key for
    /// a part of the tool that does not exist yet: accepting `[mapping]` and
    /// not expanding it would be a configuration that reads as if it worked.
    #[test]
    fn a_key_the_tool_does_not_understand_is_refused() {
        let mut s = shared();
        s.insert(
            "mapping".into(),
            table("files = [\"map/*\"]\n").into(),
        );
        let err = resolve(s, table("[rom]\npath = \"/x\"\n")).expect_err("mapping is not a key yet");
        assert!(matches!(err, Error::Malformed { .. }), "got {err}");
        assert!(
            err.to_string().contains("mapping"),
            "and names the key: {err}"
        );
    }

    #[test]
    fn a_location_for_an_emulator_nothing_declares_is_refused() {
        let local = table(
            r#"
            [rom]
            path = "/x"

            [emulator.ref-typo]
            path = "/lib.so"
        "#,
        );
        let err = resolve(shared(), local).expect_err("no such emulator");
        match err {
            Error::LocationForUnknownEmulator { name, declared } => {
                assert_eq!(name, "ref-typo");
                assert_eq!(declared, ["ref-a"]);
            }
            other => panic!("got {other}"),
        }
    }

    #[test]
    fn a_reference_with_no_location_on_this_machine_is_refused() {
        let err = resolve(shared(), table("[rom]\npath = \"/x\"\n"))
            .expect_err("nothing says where ref-a is");
        assert!(
            matches!(err, Error::ReferenceHasNoLocation { .. }),
            "got {err}"
        );
        assert!(err.to_string().contains("ref-a"));
    }

    #[test]
    fn a_reference_naming_no_declared_emulator_is_refused() {
        let mut s = shared();
        s.insert("reference".into(), table("use = \"ref-z\"\n").into());
        let err = resolve(s, table("[rom]\npath = \"/x\"\n")).expect_err("no ref-z");
        match err {
            Error::ReferenceNotDeclared { name, declared } => {
                assert_eq!(name, "ref-z");
                assert_eq!(declared, ["ref-a"]);
            }
            other => panic!("got {other}"),
        }
    }

    #[test]
    fn two_emulators_with_one_name_are_refused() {
        let s = table(
            r#"
            [project]
            platform = "a-platform"

            [rom]
            sha256 = "00"

            [[emulator]]
            name = "ref-a"
            platform = "a-platform"
            backend = "a-backend"
            version = "1.0.0"

            [[emulator]]
            name = "ref-a"
            platform = "a-platform"
            backend = "other"
            version = "2.0.0"

            [reference]
            use = "ref-a"
        "#,
        );
        let err = resolve(s, table("[rom]\npath = \"/x\"\n")).expect_err("ambiguous");
        assert!(matches!(err, Error::DuplicateEmulator { .. }), "got {err}");
    }

    #[test]
    fn an_emulator_for_another_platform_is_refused() {
        let mut s = shared();
        s.insert(
            "emulator".into(),
            Value::Array(vec![table(
                r#"
                name = "ref-a"
                platform = "another-platform"
                backend = "a-backend"
                version = "1.0.0"
            "#,
            )
            .into()]),
        );
        let err = resolve(s, table("[rom]\npath = \"/x\"\n")).expect_err("wrong platform");
        assert!(matches!(err, Error::PlatformDisagrees { .. }), "got {err}");
    }

    #[test]
    fn a_configuration_with_nowhere_for_the_software_is_refused() {
        let err = resolve(shared(), toml::Table::new()).expect_err("no path anywhere");
        // The reference's location is missing too, and is checked first; both
        // are refusals naming what is absent, which is the point.
        assert!(
            matches!(
                err,
                Error::ReferenceHasNoLocation { .. } | Error::SoftwarePathMissing
            ),
            "got {err}"
        );
    }

    // ---- anchors, §4.7 and §4.9 ----------------------------------------

    /// A shared file with anchors in it, parsed as far as `anchors_from`.
    fn anchors_of(text: &str) -> Result<Anchors, Error> {
        let table = table(text);
        let configuration: Configuration = {
            let mut whole = shared();
            for (k, v) in table {
                whole.insert(k, v);
            }
            Value::Table(whole)
                .try_into()
                .map_err(|why| Error::Malformed { why })?
        };
        anchors_from(&configuration.anchor_declarations, Path::new("."))
    }

    /// Exactly one bound. **None** would mean a default nobody chose, and
    /// **two** would make the anchor mean whichever the code read first.
    #[test]
    fn an_anchor_must_say_how_far_to_run_in_exactly_one_way() {
        let err = anchors_of("[[anchor]]\nname = \"a\"\n").expect_err("no bound");
        match &err {
            Error::AnchorBound { name, given } => {
                assert_eq!(name, "a");
                assert!(given.is_empty());
            }
            other => panic!("got {other}"),
        }
        assert!(err.to_string().contains("no way at all"), "said: {err}");

        let err = anchors_of("[[anchor]]\nname = \"a\"\nframes = 1\ninstructions = 2\n")
            .expect_err("two bounds");
        match &err {
            Error::AnchorBound { given, .. } => {
                assert_eq!(given, &["frames", "instructions"]);
            }
            other => panic!("got {other}"),
        }
        assert!(
            err.to_string().contains("read first"),
            "the message must say why two is worse than none, said: {err}"
        );
    }

    #[test]
    fn each_kind_of_bound_is_understood_and_an_address_is_hexadecimal() {
        let anchors = anchors_of("[[anchor]]\nname = \"a\"\nframes = 600\n").unwrap();
        assert_eq!(
            anchors.get("a").unwrap().definition.bound,
            Bound::Frames(600)
        );

        let anchors = anchors_of("[[anchor]]\nname = \"a\"\ninstructions = 90\n").unwrap();
        assert_eq!(
            anchors.get("a").unwrap().definition.bound,
            Bound::Instructions(90)
        );

        let anchors = anchors_of("[[anchor]]\nname = \"a\"\naddress = \"C40000\"\n").unwrap();
        assert_eq!(
            anchors.get("a").unwrap().definition.bound,
            Bound::Address(0xC4_0000),
            "an address is hexadecimal, because that is how addresses are written"
        );
        let anchors = anchors_of("[[anchor]]\nname = \"a\"\naddress = \"0xC4_0000\"\n").unwrap();
        assert_eq!(
            anchors.get("a").unwrap().definition.bound,
            Bound::Address(0xC4_0000),
            "and the prefix and separators are accepted rather than refused on a technicality"
        );

        let err = anchors_of("[[anchor]]\nname = \"a\"\naddress = \"nowhere\"\n")
            .expect_err("not hexadecimal");
        assert!(matches!(err, Error::AnchorAddress { .. }), "got {err}");
        assert!(err.to_string().contains("nowhere"), "said: {err}");
    }

    /// `after` absent means power-on. A single `from = "power-on"` key would
    /// read two ways the day somebody names an anchor `power-on`.
    #[test]
    fn an_anchor_with_no_after_begins_at_power_on() {
        let anchors = anchors_of(
            "[[anchor]]\nname = \"boot\"\nframes = 10\n\n\
             [[anchor]]\nname = \"ready\"\nafter = \"boot\"\nframes = 5\n",
        )
        .unwrap();
        assert_eq!(anchors.get("boot").unwrap().definition.start, Start::PowerOn);
        assert_eq!(
            anchors.get("ready").unwrap().definition.start,
            Start::Anchor("boot".into())
        );
        let chain: Vec<&str> = anchors
            .chain("ready")
            .unwrap()
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(chain, ["boot", "ready"]);
    }

    /// **A configuration that cannot work is refused at startup**, not halfway
    /// through a replay. A circle only noticed when somebody asks for that
    /// anchor is a configuration that looks fine until it does not.
    #[test]
    fn a_circle_in_the_configuration_is_refused_when_it_is_read() {
        let err = anchors_of(
            "[[anchor]]\nname = \"a\"\nafter = \"b\"\nframes = 1\n\n\
             [[anchor]]\nname = \"b\"\nafter = \"a\"\nframes = 1\n",
        )
        .expect_err("a circle");
        assert!(matches!(err, Error::Anchors(AnchorError::Cycle { .. })), "got {err}");

        let err = anchors_of("[[anchor]]\nname = \"a\"\nafter = \"missing\"\nframes = 1\n")
            .expect_err("no such parent");
        assert!(
            matches!(err, Error::Anchors(AnchorError::Unknown { .. })),
            "got {err}"
        );

        let err = anchors_of(
            "[[anchor]]\nname = \"a\"\nframes = 1\n\n[[anchor]]\nname = \"a\"\nframes = 2\n",
        )
        .expect_err("two of one name");
        assert!(
            matches!(err, Error::Anchors(AnchorError::Duplicate { .. })),
            "got {err}"
        );
    }

    /// §4.9's defaults are the careful ones, and they apply when the table is
    /// absent entirely — not only when it is present and empty.
    #[test]
    fn the_verification_policy_defaults_to_the_careful_numbers() {
        assert_eq!(
            AnchorPolicy::default(),
            AnchorPolicy {
                verify_from_origin: 3,
                reverify_after: 50
            }
        );

        let configuration: Configuration = Value::Table(shared()).try_into().expect("it parses");
        assert_eq!(
            configuration.anchors,
            AnchorPolicy::default(),
            "a configuration with no [anchors] table gets the defaults"
        );

        let mut with = shared();
        with.insert(
            "anchors".into(),
            table("verify_from_origin = 0\n").into(),
        );
        let configuration: Configuration = Value::Table(with).try_into().expect("it parses");
        assert_eq!(
            configuration.anchors.verify_from_origin, 0,
            "zero is permitted — §4.9 says so, and the verdict says it"
        );
        assert_eq!(
            configuration.anchors.reverify_after, 50,
            "and the key not given keeps its default"
        );
    }

    /// The startup keys default to the reproducible ones, and can be turned
    /// off. A default that was convenient rather than reproducible would make
    /// every careless configuration's results quietly worth less.
    #[test]
    fn the_startup_keys_default_to_reproducible_and_can_be_turned_off() {
        let configuration: Configuration = Value::Table(shared()).try_into().expect("it parses");
        assert!(configuration.reference.start_at_power_on);
        assert!(configuration.reference.zero_memory);

        let mut off = shared();
        off.insert(
            "reference".into(),
            table("use = \"ref-a\"\nzero_memory = false\n").into(),
        );
        let configuration: Configuration = Value::Table(off).try_into().expect("it parses");
        assert!(
            configuration.reference.start_at_power_on,
            "the other key keeps its default"
        );
        assert!(!configuration.reference.zero_memory);
    }

    /// An anchor needing input **loads** — the configuration is right and the
    /// tool cannot do it yet — and asking for that anchor is what refuses
    /// (§4.7). Refusing the whole configuration would stop the other anchors
    /// working for a reason that is not about them.
    #[test]
    fn an_anchor_needing_input_loads_and_refuses_only_when_asked_for() {
        let dir = std::env::temp_dir().join("awaseru-config-input-log");
        std::fs::create_dir_all(&dir).expect("a directory");
        std::fs::write(dir.join("press-start.input"), b"whatever").expect("write");

        let table = table(
            "[[anchor]]\nname = \"plain\"\nframes = 1\n\n\
             [[anchor]]\nname = \"needs\"\nframes = 1\ninput = \"press-start.input\"\n",
        );
        let mut whole = shared();
        for (k, v) in table {
            whole.insert(k, v);
        }
        let configuration: Configuration = Value::Table(whole).try_into().expect("it parses");
        let anchors = anchors_from(&configuration.anchor_declarations, &dir).expect("it loads");

        assert!(anchors.chain("plain").is_ok(), "the other anchor still works");
        let err = anchors.chain("needs").expect_err("not supported");
        assert!(
            matches!(err, AnchorError::InputNotSupported { .. }),
            "got {err}"
        );
        assert_eq!(
            anchors
                .get("needs")
                .unwrap()
                .definition
                .input
                .as_ref()
                .map(|l| l.recorded.len()),
            Some(8),
            "the log is read, so the refusal has it in hand"
        );

        // A log named and missing is a configuration error, found now.
        let missing = table_with_missing_log();
        let err = anchors_from(&missing, &dir).expect_err("no such file");
        assert!(matches!(err, Error::InputLogUnreadable { .. }), "got {err}");
    }

    fn table_with_missing_log() -> Vec<AnchorDeclaration> {
        vec![AnchorDeclaration {
            name: "needs".into(),
            after: None,
            frames: Some(1),
            instructions: None,
            address: None,
            covers: vec![],
            input: Some(PathBuf::from("no-such-log.input")),
        }]
    }

    // ---- the identity check -------------------------------------------

    /// §6.6, end to end over a real file: a hash that does not match is
    /// refused, and the refusal names both hashes so that somebody can see
    /// which file they have.
    #[test]
    fn software_that_hashes_to_something_else_is_refused() {
        let dir = std::env::temp_dir().join("awaseru-config-tests");
        std::fs::create_dir_all(&dir).expect("a directory");
        let file = dir.join("software.bin");
        std::fs::write(&file, b"not what the configuration describes").expect("write");

        let mut s = shared();
        s.insert(
            "emulator".into(),
            Value::Array(vec![table(
                r#"
                name = "ref-a"
                platform = "a-platform"
                backend = "a-backend"
                version = "1.0.0"
            "#,
            )
            .into()]),
        );
        let local = format!(
            "[rom]\npath = {:?}\n\n[emulator.ref-a]\npath = \"/lib.so\"\n",
            file.display().to_string()
        );
        let err = resolve(s.clone(), table(&local)).expect_err("the hash is wrong");
        match &err {
            Error::SoftwareIdentityMismatch {
                declared, found, ..
            } => {
                assert_eq!(declared, "00");
                assert_ne!(found, "00");
                assert_eq!(found.len(), 64, "a sha-256 in hexadecimal");
            }
            other => panic!("got {other}"),
        }
        assert!(err.to_string().contains("meaningless"));

        // And the same configuration with the identity it actually has loads.
        let real = digest::of_file(&file).expect("it reads");
        let mut s2 = s;
        s2.insert("rom".into(), table(&format!("sha256 = {real:?}\n")).into());
        let loaded = resolve(s2, table(&local)).expect("now it matches");
        assert_eq!(loaded.software, file);
        assert_eq!(loaded.reference().0.name, "ref-a");
        assert_eq!(loaded.reference().1.path, PathBuf::from("/lib.so"));

        let _ = std::fs::remove_file(&file);
    }
}
