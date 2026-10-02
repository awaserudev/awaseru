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
        }
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
    resolve(shared, local)
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
pub fn resolve(shared: toml::Table, mut local: toml::Table) -> Result<Loaded, Error> {
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

    Ok(Loaded {
        configuration,
        locations,
        software,
    })
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
