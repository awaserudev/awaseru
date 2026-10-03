//! One pass of the walking skeleton — M0.
//!
//! Read the configuration, select a backend by name, drive the reference to a
//! position, read one region. This is the host; `main` is argument parsing and
//! printing around it, and the test that is M0's done-condition calls this
//! rather than a second copy of it.

use std::path::{Path, PathBuf};

use awaseru_core::platform::{BackendVersion, Beginning};
use awaseru_core::{Bound, Platform, Position, ReadError, Region, Regions, RunError, Stop};

use crate::arrive::{ArriveError, Arrived, Arriver};
use crate::cache::Cache;
use crate::config::{self, Loaded};
use crate::platform::{self, Request};

/// What to do.
#[derive(Debug, Clone)]
pub struct Plan {
    pub shared: PathBuf,
    pub local: PathBuf,
    /// Where the backend may keep its own files.
    pub home: PathBuf,
    /// How far to run before reading (§4.2). There is no "run until it stops".
    ///
    /// Ignored when `anchor` is given: an anchor carries its own definition.
    pub bound: Bound,
    /// An anchor to arrive at instead of running a bound (§4.7).
    pub anchor: Option<String>,
    /// Where the anchor cache lives. Machine-local (§6.7).
    pub cache: PathBuf,
    /// Which region to read. `None` means the first one the backend reports —
    /// **not** a name chosen here, because §3.1 says the host must not assume
    /// which names exist.
    pub region: Option<String>,
    pub offset: usize,
    pub length: usize,
}

/// What happened.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The emulator's name from the configuration, which is what reports call
    /// it (§6.4).
    pub emulator: String,
    pub backend: String,
    pub version: BackendVersion,
    /// Everything the backend exposes, in its own order (§3.1).
    pub regions: Regions,
    /// Where the reference started, before the run.
    pub started: Position,
    pub stop: Stop,
    /// How the reference came up — §4.12.
    pub beginning: Beginning,
    /// Present when an anchor was asked for: how it was arrived at, and what
    /// the run is worth (§4.8, §4.12).
    pub arrived: Option<Arrived>,
    /// A digest over everything that can have changed: every **writable**
    /// region and the processor record.
    ///
    /// Not the program data, which cannot change and whose two megabytes would
    /// be hashed on every run for nothing.
    ///
    /// This is what §2.5's "the same state" is compared by, across processes.
    /// A comparison of two whole machines byte by byte is not available between
    /// processes without shipping the bytes somewhere, and a digest of what can
    /// change is the same statement for less.
    pub state: String,
    /// The region that was read, and the bytes.
    pub region: Region,
    pub offset: usize,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub enum Error {
    Configuration(config::Error),
    /// The configuration names a platform and backend this build does not
    /// have. Refused with the list, because "unsupported" without the list is
    /// a message nobody can act on.
    NoSuchBackend {
        platform: String,
        backend: String,
        available: String,
    },
    /// The backend crate refused. Its own message is carried through.
    Backend {
        emulator: String,
        why: platform::Failed,
    },
    /// The backend exposes no region of that name (§3.5).
    Read(ReadError),
    Run(RunError),
    /// The run did not get where it was asked to go, so there is nothing to
    /// read that would mean what the caller asked for (§4.3, §2.3).
    DidNotArrive {
        stop: Stop,
    },
    /// The backend came up with nothing to read.
    NoRegions,
    /// An anchor was asked for and could not be reached.
    Arrive(ArriveError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Configuration(e) => write!(f, "{e}"),
            Error::NoSuchBackend {
                platform,
                backend,
                available,
            } => write!(
                f,
                "the configuration asks for the backend `{backend}` on the platform `{platform}`, \
                 and this build has {available}"
            ),
            Error::Backend { emulator, why } => {
                write!(f, "the reference `{emulator}` did not open: {why}")
            }
            Error::Read(e) => write!(f, "{e}"),
            Error::Run(e) => write!(f, "{e}"),
            Error::DidNotArrive { stop } => write!(
                f,
                "the run did not arrive: {stop}. Nothing was read, because what was there would \
                 not be what was asked for (§2.3)"
            ),
            Error::NoRegions => write!(f, "the reference exposes no regions, so there is nothing to read"),
            Error::Arrive(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<config::Error> for Error {
    fn from(e: config::Error) -> Self {
        Error::Configuration(e)
    }
}

impl From<ReadError> for Error {
    fn from(e: ReadError) -> Self {
        Error::Read(e)
    }
}

impl From<RunError> for Error {
    fn from(e: RunError) -> Self {
        Error::Run(e)
    }
}

impl From<ArriveError> for Error {
    fn from(e: ArriveError) -> Self {
        Error::Arrive(e)
    }
}

/// Reads the configuration, opens the reference it names, runs, and reads.
pub fn run(plan: &Plan) -> Result<Outcome, Error> {
    let loaded = config::load(&plan.shared, &plan.local)?;
    let mut reference = open_reference(&loaded, &plan.home)?;
    let (emulator, _) = loaded.reference();

    let regions = reference.regions();
    let region = choose_region(&regions, plan.region.as_deref())?;

    // The position before the run, so that a report can say where it went from
    // as well as where it got to.
    let started = probe_position(&mut *reference)?;
    let beginning = reference.beginning();

    // An anchor carries its own definition, so asking for one replaces the
    // bound rather than adding to it.
    let (stop, arrived) = match &plan.anchor {
        None => {
            let stop = reference.run(plan.bound.clone())?;
            (stop, None)
        }
        Some(name) => {
            std::fs::create_dir_all(&plan.cache).map_err(|e| Error::Configuration(
                config::Error::Unreadable { path: plan.cache.clone(), why: e },
            ))?;
            let cache = Cache::at(&plan.cache);
            let provenance = loaded.provenance(reference.version().reported);
            let mut arriver = Arriver::new(
                &mut *reference,
                &loaded.anchors,
                &cache,
                provenance,
                loaded.configuration.anchors.clone(),
            );
            let arrived = arriver.arrive(name)?;
            let at = arriver.at().clone();
            (
                Stop {
                    reason: awaseru_core::Reason::BoundReached,
                    position: at,
                },
                Some(arrived),
            )
        }
    };
    if !stop.arrived() {
        return Err(Error::DidNotArrive { stop });
    }

    let state = state_digest(&*reference, &regions)?;

    // Clamped to the region rather than refused: asking for more bytes than a
    // region holds is an ordinary thing for a person at a terminal to do, and
    // the outcome says how many came back. A *comparison* asking for a span
    // past the end is a different matter and `read_span` refuses it (§3.1).
    let length = plan.length.min(region.size.saturating_sub(plan.offset));
    let bytes = reference.read_span(&region.name, plan.offset, length)?;

    Ok(Outcome {
        emulator: emulator.name.clone(),
        backend: emulator.backend.clone(),
        version: reference.version(),
        regions,
        started,
        stop,
        beginning,
        arrived,
        state,
        region,
        offset: plan.offset,
        bytes,
    })
}

/// Opens the reference the configuration names — §7.1's registry lookup.
///
/// Public because the reference process (`child::attend`) opens one too, and a
/// second copy of this would be a second place for §16's version checks to
/// live.
pub fn open_reference(loaded: &Loaded, home: &Path) -> Result<Box<dyn Platform>, Error> {
    let (emulator, location) = loaded.reference();
    let entry = platform::find(&emulator.platform, &emulator.backend).ok_or_else(|| {
        Error::NoSuchBackend {
            platform: emulator.platform.clone(),
            backend: emulator.backend.clone(),
            available: platform::describe(),
        }
    })?;

    (entry.open)(Request {
        library: &location.path,
        home,
        software: &loaded.software,
        declared_version: &emulator.version,
    })
    .map_err(|why| Error::Backend {
        emulator: emulator.name.clone(),
        why,
    })
}

/// A digest over every writable region and the processor record.
fn state_digest(platform: &dyn Platform, regions: &Regions) -> Result<String, Error> {
    use sha2::{Digest, Sha256};

    let writable: Vec<String> = regions
        .iter()
        .filter(|r| r.access.writable())
        .map(|r| r.name.clone())
        .collect();
    let coverage = awaseru_core::anchor::Coverage::of(platform, &writable)?;
    let processor = platform.read_processor()?;

    // Region names go in with their digests, so two machines that differ only
    // in *which* regions they have do not hash alike.
    let mut hasher = Sha256::new();
    for (name, digest) in coverage.entries() {
        hasher.update(name.as_bytes());
        hasher.update(digest.as_bytes());
    }
    hasher.update(processor.bytes());
    let out = hasher.finalize();
    Ok(out.iter().fold(String::with_capacity(64), |mut s, byte| {
        use std::fmt::Write;
        let _ = write!(s, "{byte:02x}");
        s
    }))
}

/// The region to read: the one named, or the first the backend reports.
///
/// Defaulting to the *first* rather than to a name is §3.1: a host that reached
/// for a particular name would be assuming which names exist, and the second
/// platform would make that assumption false.
fn choose_region(regions: &Regions, wanted: Option<&str>) -> Result<Region, Error> {
    match wanted {
        Some(name) => regions
            .get(name)
            .cloned()
            .ok_or_else(|| Error::Read(ReadError::Absent {
                region: name.to_string(),
            })),
        None => regions.iter().next().cloned().ok_or(Error::NoRegions),
    }
}

/// Where the reference is, asked for the way any caller would have to: a run of
/// nothing, which arrives immediately and reports the position (§4.2 has no
/// unbounded run, and no "where are you" either).
fn probe_position(reference: &mut dyn Platform) -> Result<Position, Error> {
    Ok(reference.run(Bound::Instructions(0))?.position)
}

/// Bytes, sixteen to a line, the way every tool that has ever printed memory
/// prints it.
///
/// `first` is the offset the first byte is at, so that the addresses down the
/// left are the region's and not the buffer's.
pub fn hexdump(bytes: &[u8], first: usize) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let address = first + row * 16;
        let _ = write!(out, "{address:08x} ");
        for column in 0..16 {
            // A gap in the middle, which is what makes a column countable by
            // eye.
            if column == 8 {
                let _ = write!(out, " ");
            }
            match chunk.get(column) {
                Some(byte) => {
                    let _ = write!(out, " {byte:02x}");
                }
                None => {
                    let _ = write!(out, "   ");
                }
            }
        }
        let _ = write!(out, "  |");
        for &byte in chunk {
            let _ = write!(
                out,
                "{}",
                if byte.is_ascii_graphic() || byte == b' ' {
                    byte as char
                } else {
                    '.'
                }
            );
        }
        let _ = writeln!(out, "|");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use awaseru_core::{Access, Region};

    fn regions() -> Regions {
        Regions::new(vec![
            Region::bytes("first", 16, Access::ReadOnly),
            Region::bytes("second", 32, Access::ReadWrite),
        ])
    }

    /// No name means the first the backend reports — which is the backend's
    /// choice, not the host's. A host with a favourite name would stop working
    /// on the second platform (§3.1, §2.7).
    #[test]
    fn with_no_name_the_backends_first_region_is_read() {
        let chosen = choose_region(&regions(), None).expect("there is one");
        assert_eq!(chosen.name, "first");
    }

    #[test]
    fn a_named_region_is_the_one_read_and_an_unknown_name_is_absence() {
        assert_eq!(
            choose_region(&regions(), Some("second")).unwrap().name,
            "second"
        );
        let err = choose_region(&regions(), Some("third")).expect_err("no such region");
        assert!(
            matches!(err, Error::Read(ReadError::Absent { .. })),
            "an unknown name is absence, not a failure to read (§3.5): {err:?}"
        );
        assert!(err.to_string().contains("third"));
    }

    #[test]
    fn a_backend_with_no_regions_says_so_rather_than_reading_nothing() {
        let err = choose_region(&Regions::default(), None).expect_err("nothing to read");
        assert!(matches!(err, Error::NoRegions), "got {err:?}");
    }

    /// The addresses down the left are the region's, not the buffer's. A dump
    /// that numbered from zero while reading from an offset would have somebody
    /// looking at the wrong address — and that is the kind of mistake this tool
    /// exists to not make.
    #[test]
    fn the_dump_numbers_rows_from_the_offset_it_was_read_at() {
        let dumped = hexdump(&[0u8; 32], 0x1234_5600);
        let lines: Vec<&str> = dumped.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("12345600 "), "got {:?}", lines[0]);
        assert!(lines[1].starts_with("12345610 "), "got {:?}", lines[1]);
    }

    /// A short last row is padded, so the text column stays where it is.
    #[test]
    fn a_partial_last_row_keeps_its_columns() {
        let dumped = hexdump(b"abc", 0);
        let line = dumped.lines().next().expect("one line");
        assert!(line.ends_with("|abc|"), "got {line:?}");
        assert_eq!(
            line.find('|'),
            hexdump(&[0u8; 16], 0).lines().next().unwrap().find('|'),
            "the text column must start in the same place whether the row is full or not"
        );
    }

    /// Bytes that are not text are dots, and the ones that are come through.
    /// Printing a raw byte would put terminal control codes on somebody's
    /// screen.
    #[test]
    fn only_printable_bytes_appear_as_themselves() {
        let dumped = hexdump(&[0x00, 0x1b, b'A', b' ', 0x7f, 0xff], 0);
        let line = dumped.lines().next().expect("one line");
        assert!(line.ends_with("|..A ..|"), "got {line:?}");
        assert!(
            line.contains("1b"),
            "and the byte is still shown in hexadecimal: {line:?}"
        );
    }

    #[test]
    fn nothing_dumps_to_nothing() {
        assert_eq!(hexdump(&[], 0), "");
    }
}
