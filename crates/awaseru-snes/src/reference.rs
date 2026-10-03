//! A reference this crate can drive, behind the trait in `awaseru-core`.
//!
//! # How a bounded run is made out of what the backend offers
//!
//! The backend has no synchronous "advance and return". It has a request — step
//! this far, then break — honoured by its own emulation thread, and a
//! notification when the break happens. So every run here is: note how many
//! breaks have been reported, lodge the request, wait for the count to rise.
//! `ffi::Backend::listen_for_breaks` says why the obvious alternative does not
//! work.
//!
//! # What was measured, and what it decided
//!
//! Both of the bounds this reference honours were run against a real backend
//! before being claimed, and what came back decided how each position is
//! reported:
//!
//! - **Counting instructions.** Successive single steps moved the program
//!   counter by two to four bytes each and never backwards within a straight
//!   run, which is what stepping between instructions looks like. So a run
//!   bounded by instructions reports an **instruction boundary**.
//!
//! - **Counting frames.** Asking the backend to run to the first line of the
//!   display landed on exactly line 0, dot 0, every time, and advanced the
//!   frame counter by exactly one. So a run bounded by frames reports a
//!   **frame boundary** — and the implementation checks the line and the dot
//!   before saying so, rather than assuming it (§2.4).
//!
//! - **And the two are not the same place.** The first instruction step taken
//!   out of a frame boundary consumed a single cycle and arrived at the target
//!   of a branch: the frame boundary had fallen *inside* an instruction, and
//!   that one cycle was what remained of it. This is §3.4 — a frame boundary is
//!   not an instruction boundary — measured on a real reference rather than
//!   argued from a manual, and it is why the two bounds report different kinds
//!   of position instead of one convenient kind.
//!
//! # Why there is only one of these per process
//!
//! The backend's emulator is a single object owned by the library, reached
//! through functions that take no handle, and the notification callback it
//! accepts carries no user data. Two of these in one process would be two
//! front ends to one emulator. So the second one refuses (§2.4 again: refusing
//! is better than half working), and §13 carries the question of what to do
//! about cross-checking two references when they are the same backend.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use awaseru_core::{
    Blob, Bound, Capabilities, Capability, Platform, Position, ReadError, Reason, Region, Regions,
    Recency, RunError, StateError, Stop, WriteError, platform::BackendVersion, check_read,
    check_write,
};
use awaseru_core::snapshot::Processor;

use crate::ffi::{Backend, Breakpoint, LoadError, PROCESSOR_STATE_BYTES, StepKind};
use crate::memory::{MAPPINGS, ZEROED_AT_POWER_ON};

/// Taken for as long as a reference exists, because the backend's emulator is
/// one global object (see this module's header).
static IN_USE: AtomicBool = AtomicBool::new(false);

/// How long the backend may make **no progress at all** before it is given up
/// on.
///
/// **This is not the budget of §4.4.** A budget is a count, so that the same
/// run stops the same way every time (§2.5); this is a wall clock, and a run
/// that ended on one would not be reproducible. That is exactly why hitting it
/// is reported as the backend being unable to continue rather than as a budget
/// running out: the two mean different things to whoever reads the result.
///
/// # Why it watches progress and not duration
///
/// It used to be a deadline on the whole wait, and that could not tell a stuck
/// backend from a long run. Ten seconds is generous for a routine and absurd
/// for a seventeen-thousand-frame replay, which takes two minutes of honest
/// work — so §4.9's closing check on such an anchor came back saying the
/// backend could not continue, which was false and alarming in the same breath.
///
/// Raising the number would only move the lie. What tells the two apart is
/// whether the machine is *doing* anything, and the processor's cycle count
/// answers that: it only goes up, and a backend that is wedged stops moving it.
/// So the deadline is reset whenever the count changes, and a run of any length
/// is fine as long as it is still running.
const DEFAULT_WATCHDOG: Duration = Duration::from_secs(10);

/// How long to sleep between looks at the break count. Short enough not to
/// dominate a step, long enough not to spin a core.
const POLL: Duration = Duration::from_micros(200);

/// Why a reference could not be opened.
#[derive(Debug)]
pub enum OpenError {
    /// One already exists in this process.
    AlreadyInUse,
    /// The library did not load, or is not this backend.
    Load(LoadError),
    /// The backend declined the software it was given.
    SoftwareRefused,
    /// The backend's debugger did not come up, and without it nothing can be
    /// bounded (§4.2), so there is no usable reference.
    DebuggerAbsent,
    /// The backend never reported a break, so there is no position to start
    /// from and no way to know what state a read would be reading.
    NeverStopped { waited: Duration },
    /// The second load did not stop by itself, so the reproducible power-on
    /// this depends on is not there any more.
    NoReproduciblePowerOn { waited: Duration },
    /// The backend came up but exposes none of the memories this crate maps.
    NoRegions,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::AlreadyInUse => write!(
                f,
                "a reference is already open in this process, and this backend has room for one"
            ),
            OpenError::Load(e) => write!(f, "{e}"),
            OpenError::SoftwareRefused => write!(
                f,
                "the backend would not load the software it was given — either the path is not \
                 what the configuration says it is, or this backend does not recognise it"
            ),
            OpenError::DebuggerAbsent => write!(
                f,
                "the backend's debugger did not start, and without it a run cannot be bounded"
            ),
            OpenError::NeverStopped { waited } => write!(
                f,
                "the backend never reported stopping, after {waited:?} — nothing can be read from \
                 it, because there is no telling what it is in the middle of"
            ),
            OpenError::NoReproduciblePowerOn { waited } => write!(
                f,
                "loading the software a second time did not stop at power-on within {waited:?}. \
                 That stop is what makes a run reproducible, and it happens because this backend \
                 breaks for one instruction on loading when its debugger exists and was paused. \
                 One of those has stopped being true, so there is no reproducible position to \
                 start from — and starting somewhere else while reporting otherwise is worse \
                 than stopping"
            ),
            OpenError::NoRegions => write!(
                f,
                "the backend reports a size of zero for every memory this crate maps, so there is \
                 nothing to compare"
            ),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<LoadError> for OpenError {
    fn from(e: LoadError) -> Self {
        OpenError::Load(e)
    }
}

/// How a reference should come up.
///
/// The defaults are the reproducible ones, and that is a position rather than a
/// convenience: a reference that does not start the same way twice cannot be
/// the ground for anything (§2.5), and §4.7's anchors hang from a position that
/// repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Startup {
    /// Come up at the reproducible power-on — cycle zero, at the reset vector,
    /// before one instruction (`doc/backend.md`) — rather than wherever the
    /// backend had got to by the time it could be stopped.
    ///
    /// `false` is the state M0 and M1 worked in, and it is **not**
    /// reproducible: where the first stop lands depends on how long the backend
    /// ran before the request arrived.
    pub at_power_on: bool,
    /// Write zeros over every writable memory at that position
    /// (`ZEROED_AT_POWER_ON`).
    ///
    /// Without this, the memories are filled pseudo-randomly and differently in
    /// every process, so nothing downstream repeats. With it, three processes
    /// agree on everything — measured.
    ///
    /// It is a **divergence from the hardware**: a real console has rubbish in
    /// its memory at power-on, and software that reads it behaves differently.
    /// That is why it is a declared option that reports itself rather than
    /// something done quietly.
    pub zero_memory: bool,
}

impl Default for Startup {
    fn default() -> Self {
        Startup {
            at_power_on: true,
            zero_memory: true,
        }
    }
}

impl Startup {
    /// As M0 and M1 had it: wherever the backend was when it could first be
    /// stopped, with whatever the power-on fill left. Kept so that the
    /// difference can be demonstrated rather than asserted.
    pub fn as_found() -> Self {
        Startup {
            at_power_on: false,
            zero_memory: false,
        }
    }
}

/// What a reference actually did on the way up — §4.12.
///
/// Carried so that a report can say it. A run whose memory was left random, or
/// which began somewhere unreproducible, is a run whose result means less, and
/// the result should say so next to itself rather than in a footnote somebody
/// has to find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub at_power_on: bool,
    /// The memories that were zeroed, by the names `ZEROED_AT_POWER_ON` gives
    /// them. Empty when none were.
    pub memory_zeroed: Vec<&'static str>,
    /// The input log that put the machine here, if one did — §4.7.
    ///
    /// **This supersedes the two above rather than joining them.** Starting a
    /// log power-cycles and applies the settings recorded with it, including how
    /// memory comes up, so after a replay the reproducibility is the log's doing
    /// and not this crate's declared divergence. A report that still credited
    /// `Startup::zero_memory` would be crediting the wrong thing, and §4.12
    /// exists so that what a result rests on sits next to the result.
    pub input_log: Option<String>,
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(log) = &self.input_log {
            return write!(
                f,
                "by replaying the input log `{log}`, which power-cycled the machine and applied \
                 the settings recorded with it — so what repeats here is the recording's doing \
                 and not this tool's"
            );
        }
        match (self.at_power_on, self.memory_zeroed.is_empty()) {
            (true, false) => write!(
                f,
                "from power-on with {} memories zeroed — reproducible, and not what the \
                 hardware does",
                self.memory_zeroed.len()
            ),
            (true, true) => write!(
                f,
                "from power-on with memory left as the backend filled it — NOT reproducible \
                 between processes"
            ),
            (false, false) => write!(
                f,
                "from wherever the backend had got to, with {} memories zeroed — the position \
                 is NOT reproducible",
                self.memory_zeroed.len()
            ),
            (false, true) => write!(
                f,
                "from wherever the backend had got to, with memory left as it was — NOTHING \
                 here is reproducible"
            ),
        }
    }
}

/// A region, and which of the backend's memories it is.
#[derive(Debug, Clone)]
struct Mapped {
    region: Region,
    memory_type: u32,
}

/// An emulator this crate drives, as the host sees it.
pub struct Reference {
    backend: Backend,
    /// The software, kept so that the reference can be returned to its origin —
    /// which on this backend means loading it again (`doc/backend.md`).
    software: PathBuf,
    /// Where the backend may keep its own files. The opaque blob goes through
    /// one, because that is the only way this backend offers it.
    home: PathBuf,
    mapped: Vec<Mapped>,
    regions: Regions,
    /// Whether the backend is sitting in a break.
    ///
    /// Reads are only coherent while it is: the backend's memory is being
    /// written by its own thread otherwise, and what came back would be a torn
    /// read that compares unequal for a reason nothing records. So this is
    /// checked rather than hoped for.
    stopped: bool,
    /// The last position known to be true, which is what a failed run reports
    /// rather than inventing one.
    last_position: Position,
    watchdog: Duration,
    /// What it did on the way up, for a report to say (§4.12).
    origin: Origin,
}

impl std::fmt::Debug for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Reference({:?}, {} region(s), stopped: {}, at {}, {})",
            self.backend,
            self.regions.len(),
            self.stopped,
            self.last_position,
            self.origin
        )
    }
}

impl Reference {
    /// Opens the backend at `library`, brings it up headless, loads `software`,
    /// and brings it to a stop it can be read from.
    ///
    /// `home` is a directory the backend keeps its own files in. It is handed in
    /// rather than chosen here, because where a tool puts files on somebody's
    /// machine is the machine-local configuration's business (§6.1).
    pub fn open(library: &Path, home: &Path, software: &Path) -> Result<Self, OpenError> {
        Self::open_with(library, home, software, Startup::default())
    }

    /// The same, saying how it should come up.
    pub fn open_with(
        library: &Path,
        home: &Path,
        software: &Path,
        startup: Startup,
    ) -> Result<Self, OpenError> {
        Self::claim(|| Ok(Backend::open(library)?), home, software, startup)
    }

    /// The same, from a library somebody has already opened — and, usually,
    /// already asked what version it is (§16.1).
    ///
    /// This exists because the version check belongs to whoever reads the
    /// configuration, and it must happen **before** software is loaded: a
    /// refusal that costs a ROM load and a debugger is a refusal that arrives
    /// late. So the host opens the library, checks it, and hands it here.
    pub fn adopt(backend: Backend, home: &Path, software: &Path) -> Result<Self, OpenError> {
        Self::adopt_with(backend, home, software, Startup::default())
    }

    /// The same, saying how it should come up.
    pub fn adopt_with(
        backend: Backend,
        home: &Path,
        software: &Path,
        startup: Startup,
    ) -> Result<Self, OpenError> {
        Self::claim(|| Ok(backend), home, software, startup)
    }

    fn claim(
        backend: impl FnOnce() -> Result<Backend, OpenError>,
        home: &Path,
        software: &Path,
        startup: Startup,
    ) -> Result<Self, OpenError> {
        if IN_USE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(OpenError::AlreadyInUse);
        }

        match backend().and_then(|backend| Self::bring_up(backend, home, software, startup)) {
            Ok(reference) => Ok(reference),
            Err(e) => {
                // The slot is only held by a reference that exists. Leaving it
                // taken after a failure would turn one bad path into a process
                // that can never open a reference again.
                IN_USE.store(false, Ordering::SeqCst);
                Err(e)
            }
        }
    }

    fn bring_up(
        backend: Backend,
        home: &Path,
        software: &Path,
        startup: Startup,
    ) -> Result<Self, OpenError> {
        backend.init();
        backend.initialize_headless(home)?;

        // The throttle comes off before anything runs. The backend paces itself
        // to the console's real time by default, which is what somebody
        // watching wants and the opposite of what a replay wants: measured on
        // supplied software, 1800 frames took 31.6 s paced and the marginal
        // rate was 60 frames a second — exactly real time, which is the
        // signature of a frame delay rather than of a limit.
        //
        // Nothing about the emulated machine depends on it: the delay is a
        // sleep between frames, and §2.5's determinism tests are what say so
        // rather than this comment.
        backend.set_emulation_config(crate::ffi::EmulationConfig::unthrottled());

        // Before the software is loaded, so that nothing it does is missed.
        backend.listen_for_breaks();

        if !backend.load_rom(software)? {
            return Err(OpenError::SoftwareRefused);
        }

        backend.initialize_debugger();
        if !backend.is_debugger_running() {
            return Err(OpenError::DebuggerAbsent);
        }

        // The backend runs freely from the moment the software loads, so the
        // first thing to do is stop it. One instruction is the shortest request
        // that has a defined stopping place.
        //
        // Where *this* stop lands is not reproducible — the backend had been
        // running for an unmeasured time when the request arrived. It is only
        // the first of two steps when `at_power_on` is set, and the whole of it
        // otherwise.
        let watchdog = DEFAULT_WATCHDOG;
        let before = backend.breaks();
        backend.step(1, StepKind::Instruction);
        if !wait_for_break(&backend, before, watchdog) {
            return Err(OpenError::NeverStopped { waited: watchdog });
        }

        if startup.at_power_on {
            // **Loading the software again** lands at cycle zero, at the reset
            // vector, before one instruction has run — and does so identically
            // in every process (`doc/backend.md`). It works because the backend
            // breaks for one instruction on loading when its debugger both
            // exists and was paused, and after the step above both are true.
            // The first load exists only to make them true.
            let before = backend.breaks();
            if !backend.load_rom(software)? {
                return Err(OpenError::SoftwareRefused);
            }
            if !wait_for_break(&backend, before, watchdog) {
                // The second load did not stop by itself, which means the
                // conditions this depends on have changed. Refusing is the
                // honest answer: carrying on would leave the reference
                // somewhere unreproducible while the report said otherwise.
                return Err(OpenError::NoReproduciblePowerOn { waited: watchdog });
            }
        }

        let mapped: Vec<Mapped> = MAPPINGS
            .iter()
            .filter_map(|m| {
                // A memory the backend reports as empty is **absent**, not a
                // region of no bytes (§3.5). This is not hypothetical: whether
                // there is battery-backed memory at all is a property of the
                // cartridge, so for some software that region is simply not
                // there, and a comparison over it must come out as not
                // determined rather than as agreement over nothing.
                let size = backend.memory_size(m.memory_type) as usize;
                (size > 0).then(|| Mapped {
                    region: Region {
                        name: m.region.to_string(),
                        size,
                        access: m.access,
                        // The backend hands these out as flat arrays of bytes,
                        // indexed by the byte, whatever the hardware addresses
                        // them by. The unit is a property of how the backend
                        // exposes the memory, not of the hardware.
                        unit: 1,
                    },
                    memory_type: m.memory_type,
                })
            })
            .collect();

        if mapped.is_empty() {
            return Err(OpenError::NoRegions);
        }

        let regions = Regions::new(mapped.iter().map(|m| m.region.clone()).collect());

        // Zeroing does not move the machine — measured: the position and both
        // state records are identical before and after — so it happens here,
        // after the position is settled and before anything is read.
        let mut memory_zeroed = Vec::new();
        if startup.zero_memory {
            for (memory_type, name) in ZEROED_AT_POWER_ON {
                let size = backend.memory_size(*memory_type) as usize;
                if size == 0 {
                    // Absent for this software — a cartridge without battery
                    // memory is the ordinary case. Nothing to settle.
                    continue;
                }
                backend
                    .write_memory(*memory_type, &vec![0u8; size])
                    .map_err(OpenError::Load)?;
                memory_zeroed.push(*name);
            }
        }

        let last_position = if startup.at_power_on {
            // Not called an instruction boundary even though nothing has
            // executed: the backend reports a dot part way along the first
            // line, so this is a position it can name and not one this crate
            // will classify (§2.4).
            Position::Unclassified {
                pc: u64::from(backend.cpu_snapshot().pc),
            }
        } else {
            Position::InstructionBoundary {
                pc: u64::from(backend.cpu_snapshot().pc),
            }
        };

        Ok(Reference {
            backend,
            software: software.to_path_buf(),
            home: home.to_path_buf(),
            mapped,
            regions,
            stopped: true,
            last_position,
            watchdog,
            origin: Origin {
                at_power_on: startup.at_power_on,
                memory_zeroed,
                input_log: None,
            },
        })
    }

    /// What this reference did on the way up — §4.12. A report says it.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// How long a run waits for the backend to break before reporting that it
    /// cannot continue. See `DEFAULT_WATCHDOG` for why this is not a budget.
    pub fn set_watchdog(&mut self, watchdog: Duration) {
        self.watchdog = watchdog;
    }

    /// Where the reference stands, as far as anything here can say.
    pub fn position(&self) -> Position {
        self.last_position.clone()
    }

    /// Whether the backend is in a break, which is the only state a read of it
    /// means anything in.
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// The processor's own cycle count, which only goes up while it executes.
    ///
    /// `None` while the reference is not stopped, for the same reason a read is
    /// refused there: the number would be read out from under the thread
    /// writing it.
    ///
    /// It says that something ran, and nothing about what. That makes it the one
    /// measure of progress available that does not depend on what the software
    /// happens to be doing.
    pub fn cycles(&self) -> Option<u64> {
        self.stopped.then(|| self.backend.cpu_snapshot().cycles)
    }

    /// What the backend calls the memory behind a region, for a report that has
    /// to be checked against the backend's own source.
    pub fn backend_name_for(&self, region: &str) -> Option<&'static str> {
        let memory_type = self.mapped.iter().find(|m| m.region.name == region)?.memory_type;
        MAPPINGS
            .iter()
            .find(|m| m.memory_type == memory_type)
            .map(|m| m.backend_calls_it)
    }

    fn memory_type(&self, region: &str) -> Option<u32> {
        self.mapped
            .iter()
            .find(|m| m.region.name == region)
            .map(|m| m.memory_type)
    }

    /// Refuses a read taken while the reference is running, because what came
    /// back would be a torn read of memory its own thread is writing.
    fn require_stopped(&self) -> Result<(), ReadError> {
        if self.stopped {
            return Ok(());
        }
        Err(ReadError::Backend {
            why: "the reference is not stopped, so a read of it would be a read of memory being \
                  written underneath — the last run did not reach a break"
                .to_string(),
        })
    }

    /// Refuses a write taken while the reference is running, for the same
    /// reason a read is refused: the backend's own thread is writing the same
    /// memory, so what lands is neither what was there nor what was asked for.
    fn require_stopped_to_write(&self) -> Result<(), WriteError> {
        if self.stopped {
            return Ok(());
        }
        Err(WriteError::NotStopped)
    }

    /// Where the opaque blob goes. The blob's bytes are carried in memory and
    /// this is only the hatch the backend insists on.
    ///
    /// **Named for this process**, which it was not at first. Several processes
    /// sharing a home directory — which is the ordinary case, since the home
    /// has a default — would write the same file between one another's save and
    /// read-back, and each would get the other's machine. It surfaced as an
    /// intermittent failure the moment the test suite ran four of them at once,
    /// and it surfaced as a *failure* rather than a wrong answer only because
    /// §4.8's cheap check was there to catch it.
    fn state_file(&self) -> PathBuf {
        self.home
            .join(format!("awaseru-state-{}.tmp", std::process::id()))
    }

    /// Something cheap to tell a resumed machine from an untouched one.
    ///
    /// The processor record, which begins with a cycle count that only goes up.
    /// Two machines at the same position with different cycle counts are not
    /// the same machine, and this is what notices.
    fn fingerprint(&self) -> Vec<u8> {
        self.backend.processor_state().bytes
    }

    /// Where the machine is after a save or a load, classified only as far as
    /// it can be.
    ///
    /// Not claimed to be an instruction boundary even though it probably is —
    /// the break that ends a save comes from completing whatever instruction
    /// was in progress. Probably is not measured, and §2.4 would rather say
    /// nothing (§3.4's kinds exist so that nothing has to be rounded).
    fn position_now(&self) -> Position {
        // `None` is the backend having written nothing into the record, and the
        // honest answer to that is the same as the answer to "it is not at a
        // frame boundary": this does not know where it is. Saying so is what
        // `Unclassified` is for, and it is what keeps a failed read from
        // reporting a frame boundary at frame zero.
        let video = self.backend.video_snapshot();
        if video.is_some_and(|v| v.line == 0 && v.dot == 0) {
            Position::FrameBoundary {
                frame: u64::from(video.expect("just checked").frames),
            }
        } else {
            Position::Unclassified {
                pc: u64::from(self.backend.cpu_snapshot().pc),
            }
        }
    }

    /// One step of a bounded run: lodge the request, wait for the break.
    ///
    /// `Ok(())` means it stopped. `Err(Stop)` is the whole result, already
    /// filled in, for a run that did not.
    fn step_and_wait(&mut self, count: u32, kind: StepKind) -> Result<(), Stop> {
        let before = self.backend.breaks();
        self.backend.step(count, kind);
        if wait_for_break(&self.backend, before, self.watchdog) {
            return Ok(());
        }
        // Nothing here knows where the backend is any more, so nothing claims
        // to: the position reported is the last one that was true, and it says
        // so.
        self.stopped = false;
        Err(Stop {
            reason: Reason::CannotContinue {
                why: format!(
                    "it did not break within {:?}; the position given is the last one known, not \
                     where it is now",
                    self.watchdog
                ),
            },
            position: self.last_position.clone(),
        })
    }

    /// Checks that a write arrived — and it is not optional politeness.
    ///
    /// The call that performs it returns `void`: a memory type the backend
    /// declines, a span it clips, a build where it does nothing at all, every
    /// one of them comes back looking like success. A seed that silently did
    /// not land leaves the routine running on whatever was already there, and
    /// §5's comparison is then about input nobody chose — a wrong answer that
    /// looks exactly like an answer, in the path every verdict rests on.
    ///
    /// Reading the span back costs a read of the same size: about 0.4 ms for a
    /// twelve-kilobyte seed, against 79.7 ms for a cycle. That is the price of
    /// not having to trust a `void`.
    fn landed(&self, region: &str, offset: usize, bytes: &[u8]) -> Result<(), WriteError> {
        let back = self
            .read_span(region, offset, bytes.len())
            .map_err(|e| WriteError::Backend {
                why: format!("the write could not be read back to check it: {e}"),
            })?;
        if let Some(first) = mismatch(&back, bytes) {
            return Err(WriteError::Backend {
                why: format!(
                    "the backend accepted {} byte(s) at {offset} of `{region}` and did not \
                     store them: byte {first} reads {:#04X} and was written as {:#04X}. This \
                     call reports nothing when it fails, so what is checked is the memory",
                    bytes.len(),
                    back.get(first).copied().unwrap_or(0),
                    bytes.get(first).copied().unwrap_or(0),
                ),
            });
        }
        Ok(())
    }

    /// The backend's access record for a span, **exactly as it keeps it**.
    ///
    /// Raw on purpose. §2.4 says not to guess a shape before something has been
    /// measured, and nothing in this project has ever read the execute half of
    /// this record — §13's Q15 called it a route nobody had taken. So this
    /// returns the record and interprets none of it: what a platform-independent
    /// verb should say about execution is decided by what reading this shows,
    /// not the other way round.
    ///
    /// `write_recency` is the one piece already interpreted, and it is built on
    /// the same call.
    pub fn access_record(
        &self,
        region: &str,
        offset: usize,
        length: usize,
    ) -> Result<Vec<crate::ffi::AccessCounts>, ReadError> {
        self.require_stopped()?;
        check_read(&self.regions, region, Some((offset, length)))?;
        let memory_type = self.memory_type(region).ok_or_else(|| ReadError::Absent {
            region: region.to_string(),
        })?;
        let at = u32::try_from(offset).map_err(|_| ReadError::Backend {
            why: format!("this backend counts addresses in 32 bits, and {offset} does not fit"),
        })?;
        let span = u32::try_from(length).map_err(|_| ReadError::Backend {
            why: format!("this backend counts lengths in 32 bits, and {length} does not fit"),
        })?;
        self.backend
            .access_counts(memory_type, at, span)
            .ok_or_else(|| ReadError::Backend {
                why: format!(
                    "the backend wrote no access record for {span} byte(s) from {offset} of \
                     `{region}`. This call reports nothing when it fails, so what is checked is \
                     whether the record was written at all — an unwritten one reads as `nothing \
                     was ever read, written or executed here`, which is a perfectly ordinary \
                     answer and would be believed"
                ),
            })
    }

    /// Throws away every access count the backend holds.
    ///
    /// Whether this is how "what executed in THIS run" is asked for, or whether
    /// the stamps make it a subtraction instead, is what M6's gate measures.
    pub fn forget_access_counts(&self) {
        self.backend.reset_access_counts();
    }

    /// Asks the backend for a picture of the frame, into its own screenshot
    /// folder under this reference's home.
    ///
    /// Here because a claim about *where* a machine is should be checkable by
    /// looking rather than by believing a frame count — and on real software
    /// that is the difference between "it ran 17 767 frames" and "the player
    /// has control".
    pub fn screenshot(&self) {
        self.backend.take_screenshot();
    }

    /// Replays a recorded input log from power-on — §4.7's input log, at last.
    ///
    /// Not a verb of the `Platform` trait, and deliberately not yet: this is a
    /// measurement of what the backend can do, and §7.6 says not to guess a
    /// signature before a second platform exists to answer with. What it does
    /// is what the backend does — apply the settings the log carries, which is
    /// what creates a control device, then power-cycle and feed the recorded
    /// frames.
    pub fn play_input_log(&mut self, path: &Path) -> Result<(), LoadError> {
        self.backend.play_movie(path)?;
        self.last_position = self.position_now();
        Ok(())
    }

    /// Whether a replayed input log is still feeding frames.
    pub fn input_log_playing(&self) -> bool {
        self.backend.movie_playing()
    }

    /// The position after a run bounded by frames — **if** the backend really
    /// is at the start of a frame.
    ///
    /// Checked rather than assumed. The backend lands on line 0, dot 0 every
    /// time it has been measured; if it ever does not, this says it does not
    /// know where it is instead of calling something a frame boundary because
    /// that is what was asked for.
    fn frame_position(&self) -> Position {
        // `None` is the backend having written nothing into the record, and the
        // honest answer to that is the same as the answer to "it is not at a
        // frame boundary": this does not know where it is. Saying so is what
        // `Unclassified` is for, and it is what keeps a failed read from
        // reporting a frame boundary at frame zero.
        let video = self.backend.video_snapshot();
        if video.is_some_and(|v| v.line == 0 && v.dot == 0) {
            Position::FrameBoundary {
                frame: u64::from(video.expect("just checked").frames),
            }
        } else {
            Position::Unclassified {
                pc: u64::from(self.backend.cpu_snapshot().pc),
            }
        }
    }

    fn instruction_position(&self) -> Position {
        Position::InstructionBoundary {
            pc: u64::from(self.backend.cpu_snapshot().pc),
        }
    }

    /// A bound of nothing: the reference is already where it was asked to get
    /// to, and nothing is asked of the backend.
    fn already_there(&self) -> Stop {
        Stop {
            reason: Reason::BoundReached,
            position: self.last_position.clone(),
        }
    }

    fn arrive(&mut self, position: Position) -> Stop {
        self.last_position = position.clone();
        Stop {
            reason: Reason::BoundReached,
            position,
        }
    }
}

/// Waits for the backend's break count to pass `before`.
///
/// Free of the race described in `ffi::Backend::listen_for_breaks`, because
/// `before` is read before the request is lodged and the count only rises.
/// Where two readings of the same span first differ, if they do.
///
/// Pulled out so that it can be tested without a backend. The guard it serves
/// can only fire when a `void`-returning call has silently done nothing, and no
/// test can make a working backend do that — so what is tested is the decision,
/// and the integration tests beside it check that the decision does not fire
/// when the write did land.
fn mismatch(back: &[u8], wanted: &[u8]) -> Option<usize> {
    if back.len() != wanted.len() {
        return Some(back.len().min(wanted.len()));
    }
    back.iter().zip(wanted).position(|(a, b)| a != b)
}

fn wait_for_break(backend: &Backend, before: u64, watchdog: Duration) -> bool {
    // The cycle count only goes up, and a wedged backend stops moving it. So
    // the deadline is on *silence*, not on duration: it resets every time the
    // machine has got anywhere, and a two-minute replay is as acceptable as a
    // two-millisecond one as long as it is still working.
    let mut deadline = Instant::now() + watchdog;
    let mut cycles = backend.cpu_snapshot().cycles;
    while backend.breaks() == before {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
        let now = backend.cpu_snapshot().cycles;
        if now != cycles {
            cycles = now;
            deadline = Instant::now() + watchdog;
        }
    }
    true
}

impl Platform for Reference {
    fn version(&self) -> BackendVersion {
        BackendVersion {
            reported: self.backend.version().to_string(),
            built: Some(self.backend.build_date()),
        }
    }

    fn beginning(&self) -> awaseru_core::Beginning {
        awaseru_core::Beginning {
            reproducible: self.origin.at_power_on,
            settled: self.origin.memory_zeroed.iter().map(|n| (*n).to_string()).collect(),
            by_input_log: self.origin.input_log.clone(),
        }
    }

    /// §7.3, for this backend.
    ///
    /// What is declared is what has been **measured on this backend**, with
    /// the numbers in `doc/backend.md` — a shorter list than what the library
    /// exports, and a longer one than what this crate has a verb for today:
    ///
    /// - `stop-on-execution` — an execution breakpoint stops exactly at the
    ///   target. Asked for one address it stopped there; asked for another, at
    ///   that one. `Bound::Address` is built on it.
    /// - `stop-on-write` — a write breakpoint stops *during* the write, before
    ///   it commits: the byte still holds its old value and one further step
    ///   lands the new one.
    /// - `writing-position` — at that break, asked for the *instruction's*
    ///   program counter rather than the processor's, the backend returns the
    ///   store itself. §5.4's third item, exact rather than approximate.
    /// - `write-recency` — the access counters give a per-address write stamp
    ///   in a clock of the backend's own, which is comparable with other
    ///   stamps and with nothing else.
    /// - `execution-coverage` — the same record counts **executions** per byte,
    ///   and M6 measured that it means what it says: byte-exact about what ran
    ///   and what did not, not polluted by the host's own reads, clearable, and
    ///   kept for memory that is not the cartridge as well. `doc/backend.md`
    ///   has the measurements. This was the longest-standing of §13's Q15's
    ///   untaken routes, and taking it is what turned it into a declaration.
    ///
    /// All four are reachable through a verb of this crate: `Bound::Address`
    /// for the first, `Bound::Write` for the second and third — one bound
    /// carrying both a byte and the end of its subject, §4.5 — and
    /// `write_recency` for the fourth. The declaration came before the verbs,
    /// which is the right order: a host cannot be written against a capability
    /// nobody has declared.
    ///
    /// What is deliberately **not** declared, and why each is a different kind
    /// of absence:
    ///
    /// - `input-replay` — **this backend can do it, and the capability is
    ///   still not declared.** M5 measured a recorded log replaying
    ///   deterministically and surviving everything the host does to the
    ///   machine while it plays; `play_input_log` is the verb, and
    ///   `doc/protocol.md` holds the five measurements. What is missing is
    ///   above this crate: `Platform` has no verb for replaying a log, so a
    ///   declaration here would pass §7.3's gate and let an anchor carrying an
    ///   input log be replayed with the log *ignored* — arriving somewhere
    ///   else, consistently, and caching it as the anchor. §7.3's rule is that
    ///   a capability is declared when it has been measured; the reason this
    ///   one waits is that a declaration nothing can act on is not merely
    ///   useless but unsafe. `doc/findings.md` has the two decisions it needs.
    /// - `stop-on-read`, `call-and-return-events` — a route exists for each and
    ///   nothing here has taken it: a read flag in the breakpoint record and an
    ///   exported call-stack reader. `doc/backend.md` lists them under what was
    ///   *not* exercised. A route is not a declaration (§7.3), because a host
    ///   relying on one would be relying on this crate's reading of a header.
    /// - `register-writes` — needs the event viewer, which needs the nested
    ///   configuration record this project has twice refused to transcribe.
    fn capabilities(&self) -> Capabilities {
        Capabilities::of([
            Capability::StopOnExecution,
            Capability::StopOnWrite,
            Capability::WritingPosition,
            Capability::WriteRecency,
            Capability::ExecutionCoverage,
            Capability::InputReplay,
        ])
    }

    fn regions(&self) -> Regions {
        self.regions.clone()
    }

    fn read(&self, region: &str) -> Result<Vec<u8>, ReadError> {
        self.require_stopped()?;
        check_read(&self.regions, region, None)?;
        let memory_type = self.memory_type(region).ok_or_else(|| ReadError::Absent {
            region: region.to_string(),
        })?;
        Ok(self.backend.memory(memory_type))
    }

    /// A span, by reading the region and taking a part of it.
    ///
    /// The backend has no partial read — it writes a whole memory into a buffer
    /// — so a span costs a whole region either way. Doing the arithmetic to
    /// pretend otherwise would buy nothing (§17.4).
    fn read_span(&self, region: &str, offset: usize, len: usize) -> Result<Vec<u8>, ReadError> {
        self.require_stopped()?;
        check_read(&self.regions, region, Some((offset, len)))?;
        let whole = self.read(region)?;
        Ok(whole[offset..offset + len].to_vec())
    }

    /// §5.4's cheap filter, from the access counters.
    ///
    /// The stamp is the backend's own clock and not the processor's: at
    /// processor cycle 32 the stamp read 426 (`doc/backend.md`). So it is
    /// comparable with other stamps from this backend and with nothing else,
    /// which is what `Recency` says of it.
    ///
    /// A write counter of zero is `NeverWritten` rather than a stamp of zero:
    /// a stamp of zero is a real reading at the very beginning of the clock,
    /// and conflating the two would make the first write of a run look like no
    /// write at all.
    fn write_recency(&self, region: &str, offset: usize) -> Result<Recency, ReadError> {
        self.require_stopped()?;
        check_read(&self.regions, region, Some((offset, 1)))?;
        let memory_type = self.memory_type(region).ok_or_else(|| ReadError::Absent {
            region: region.to_string(),
        })?;
        let at = u32::try_from(offset).map_err(|_| ReadError::Backend {
            why: format!("this backend counts addresses in 32 bits, and {offset} does not fit"),
        })?;
        let counts = self
            .backend
            .access_counts(memory_type, at, 1)
            .ok_or_else(|| ReadError::Backend {
                why: "the backend wrote no access record for one address".to_string(),
            })?;
        let Some(count) = counts.first() else {
            return Err(ReadError::Backend {
                why: "the backend returned no access record for one address".to_string(),
            });
        };
        Ok(if count.writes == 0 {
            Recency::NeverWritten
        } else {
            Recency::Stamp(count.write_stamp)
        })
    }

    fn replay_input_log(
        &mut self,
        log: &awaseru_core::anchor::InputLog,
    ) -> Result<(), awaseru_core::RunError> {
        // The backend opens the archive itself, which is why `InputLog` carries
        // the path: the bytes are §4.11's business and this call's business is a
        // filename.
        self.play_input_log(&log.path)
            .map_err(|why| awaseru_core::RunError::Backend {
                why: format!(
                    "the input log `{}` could not be started from {}: {why}",
                    log.name,
                    log.path.display()
                ),
            })?;

        // **The backend reports nothing about whether it opened the file.**
        // `MoviePlay` returns void: a path that does not exist, an archive that
        // is not one, a recording for other software — all of them come back
        // looking like success, and the machine is left at the origin the call
        // power-cycled it to. A caller would then measure from power-on
        // believing it had arrived somewhere, which is the silent wrong answer
        // this project refuses everywhere.
        //
        // So the host checks the one thing it can observe: playback is live, or
        // the log did not start. A recording with no frames in it is refused by
        // the same check, which is correct — a log that feeds nothing is not a
        // log.
        if !self.input_log_playing() {
            return Err(awaseru_core::RunError::Backend {
                why: format!(
                    "the input log `{}` was started from {} and is not playing. This backend \
                     says nothing about a log it could not open, so what can be seen is that \
                     nothing is being replayed — check that the file is a recording for this \
                     software",
                    log.name,
                    log.path.display()
                ),
            });
        }

        // Starting a log power-cycles (`doc/protocol.md`), so the machine is at
        // its origin and the origin this reference reports has a second way of
        // having been reached. §4.12 wants that said rather than inferred.
        self.origin = Origin {
            at_power_on: true,
            memory_zeroed: Vec::new(),
            input_log: Some(log.name.clone()),
        };
        Ok(())
    }

    fn coverage(
        &self,
        region: &str,
        offset: usize,
        length: usize,
    ) -> Result<awaseru_core::ExecutionCoverage, ReadError> {
        // The same shape as `write_recency` and for the same reason: the record
        // is only coherent while the backend is in a break, because its own
        // thread is writing memory otherwise.
        let record = self.access_record(region, offset, length)?;
        Ok(awaseru_core::ExecutionCoverage {
            region: region.to_string(),
            offset,
            executions: record.iter().map(|c| c.executions).collect(),
        })
    }

    fn forget_coverage(&mut self) -> Result<(), ReadError> {
        // Not gated on being stopped: clearing a counter does not read memory,
        // and a caller asking to forget while the machine runs gets what it
        // asked for rather than a refusal about coherence it did not need.
        self.forget_access_counts();
        Ok(())
    }

    fn write(&mut self, region: &str, bytes: &[u8]) -> Result<(), WriteError> {
        self.require_stopped_to_write()?;
        let declared = check_write(&self.regions, region, None)?;
        if bytes.len() != declared.size {
            // A partial write presented as a whole-region write would leave the
            // rest of the region holding whatever was there, which is a machine
            // state nobody asked for and nothing records.
            return Err(WriteError::Span(
                declared
                    .span(0, bytes.len())
                    .err()
                    .unwrap_or(awaseru_core::SpanError::PastTheEnd {
                        region: region.to_string(),
                        offset: 0,
                        len: bytes.len(),
                        size: declared.size,
                    }),
            ));
        }
        let memory_type = self.memory_type(region).ok_or_else(|| WriteError::Absent {
            region: region.to_string(),
        })?;
        self.backend
            .write_memory(memory_type, bytes)
            .map_err(|e| WriteError::Backend { why: e.to_string() })?;
        self.landed(region, 0, bytes)
    }

    fn write_span(
        &mut self,
        region: &str,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), WriteError> {
        self.require_stopped_to_write()?;
        check_write(&self.regions, region, Some((offset, bytes.len())))?;
        let memory_type = self.memory_type(region).ok_or_else(|| WriteError::Absent {
            region: region.to_string(),
        })?;
        let address = u32::try_from(offset).map_err(|_| WriteError::Backend {
            why: "this backend addresses a span with a 32-bit number".to_string(),
        })?;
        self.backend
            .write_memory_span(memory_type, address, bytes)
            .map_err(|e| WriteError::Backend { why: e.to_string() })?;
        self.landed(region, offset, bytes)
    }


    fn read_processor(&self) -> Result<Processor, ReadError> {
        self.require_stopped()?;
        let read = self.backend.processor_state();
        if read.wrote_nothing {
            // The record came back entirely as the filler it was handed. The
            // filler was already here to catch a record that GREW; nobody was
            // looking at the middle for one that never arrived. This call
            // reports nothing when it fails, so the record itself is what says
            // whether it ran.
            return Err(ReadError::Backend {
                why: format!(
                    "the backend wrote nothing into the {PROCESSOR_STATE_BYTES} bytes of its \
                     processor record. This call cannot report a failure, so what is checked \
                     is whether anything was written at all"
                ),
            });
        }
        if read.wrote_beyond {
            // The backend's record is bigger than this binding reads, so what
            // came back is a truncation. Refusing is the only honest answer:
            // the missing part is registers, and §3.3 says a comparison seeded
            // without them is a comparison of something else.
            return Err(ReadError::Backend {
                why: format!(
                    "the backend wrote more than the {PROCESSOR_STATE_BYTES} bytes this binding                      reads, so its processor record has grown and the transcription is stale"
                ),
            });
        }
        Ok(Processor::opaque(read.bytes))
    }

    fn write_processor(&mut self, processor: &Processor) -> Result<(), WriteError> {
        self.require_stopped_to_write()?;
        self.backend
            .write_processor_state(processor.bytes())
            .map_err(|e| WriteError::Backend { why: e.to_string() })?;
        // The same reason as `landed`: the call reports nothing, so what is
        // checked is the record itself. Thirty-two bytes, so the check is free.
        let back = self.read_processor().map_err(|e| WriteError::Backend {
            why: format!("the processor write could not be read back to check it: {e}"),
        })?;
        if back.bytes() != processor.bytes() {
            return Err(WriteError::Backend {
                why: "the backend accepted a processor record and did not store it. This call \
                      reports nothing when it fails, so what is checked is the record"
                    .to_string(),
            });
        }
        Ok(())
    }

    fn return_to_origin(&mut self) -> Result<(), RunError> {
        if !self.origin.at_power_on {
            // There is no origin to return to: this reference came up wherever
            // the backend had got to, and that place is not reachable again.
            // §2.5 is why this refuses instead of returning somewhere near it.
            return Err(RunError::Backend {
                why: "this reference did not come up at the reproducible power-on, so there is \
                      no position to return to. Open it with the default startup if a definition \
                      has to be replayed"
                    .to_string(),
            });
        }

        // The same two-step as coming up: the debugger is already in a break,
        // so loading the software again stops at cycle zero before one
        // instruction has run.
        let before = self.backend.breaks();
        let accepted = self
            .backend
            .load_rom(&self.software)
            .map_err(|e| RunError::Backend { why: e.to_string() })?;
        if !accepted {
            return Err(RunError::Backend {
                why: "the backend would not load the software again".to_string(),
            });
        }
        if !wait_for_break(&self.backend, before, self.watchdog) {
            self.stopped = false;
            return Err(RunError::Backend {
                why: format!(
                    "loading the software again did not stop at power-on within {:?}",
                    self.watchdog
                ),
            });
        }
        self.stopped = true;

        // And the same memories, so the origin is the origin and not merely
        // the same position with different contents.
        for name in &self.origin.memory_zeroed {
            let Some((memory_type, _)) = ZEROED_AT_POWER_ON.iter().find(|(_, n)| n == name) else {
                continue;
            };
            let size = self.backend.memory_size(*memory_type) as usize;
            if size == 0 {
                continue;
            }
            self.backend
                .write_memory(*memory_type, &vec![0u8; size])
                .map_err(|e| RunError::Backend { why: e.to_string() })?;
        }

        self.last_position = self.position_now();
        Ok(())
    }

    fn save_state(&mut self) -> Result<Blob, StateError> {
        if !self.stopped {
            return Err(StateError::NotStopped);
        }
        let path = self.state_file();
        // Taken away first, because `SaveStateFile` reports nothing. Without
        // this, a save that silently did nothing leaves the PREVIOUS save of
        // this process on disk, the read below succeeds, and the blob is of a
        // moment that has passed — caught much later by `load_state`'s position
        // check, which then blames the load for what the save did.
        let _ = std::fs::remove_file(&path);
        self.backend
            .save_state_to(&path)
            .map_err(|e| StateError::Backend { why: e.to_string() })?;
        let bytes = std::fs::read(&path).map_err(|e| StateError::Backend {
            why: format!("the state the backend wrote to {} could not be read: {e}", path.display()),
        })?;
        // After the save, never before — saving advances the machine
        // (`doc/backend.md`), and a blob whose position was read first is a
        // blob that appears to be off by one.
        let position = self.position_now();
        self.last_position = position.clone();
        Ok(Blob::new(bytes, position, self.fingerprint()))
    }

    fn load_state(&mut self, blob: &Blob) -> Result<(), StateError> {
        if !self.stopped {
            return Err(StateError::NotStopped);
        }
        let path = self.state_file();
        std::fs::write(&path, blob.bytes()).map_err(|e| StateError::Backend {
            why: format!("the state could not be written to {}: {e}", path.display()),
        })?;
        self.backend
            .load_state_from(&path)
            .map_err(|e| StateError::Backend { why: e.to_string() })?;

        // The backend says nothing about a load that failed (§13's Q12), so
        // these two checks are the whole of knowing whether it did anything.
        let found = self.position_now();
        if found != *blob.position() {
            return Err(StateError::LandedElsewhere {
                expected: blob.position().clone(),
                found,
            });
        }
        if self.fingerprint() != blob.fingerprint() {
            return Err(StateError::FingerprintDiffers);
        }
        self.last_position = found;
        Ok(())
    }

    fn run(&mut self, bound: Bound) -> Result<Stop, RunError> {
        if !self.stopped {
            return Err(RunError::Backend {
                why: "the reference is not stopped; the last run never reached a break, so there \
                      is nothing to run on from"
                    .to_string(),
            });
        }

        match bound {
            Bound::Instructions(0) | Bound::Frames(0) => Ok(self.already_there()),

            Bound::Instructions(n) => {
                let Ok(count) = u32::try_from(n) else {
                    // The backend's step count is a 32-bit number. Splitting a
                    // larger one into several steps would be a different run —
                    // each step is a break, and a break is an observation — so
                    // this refuses instead (§2.4).
                    return Ok(Stop {
                        reason: Reason::Refused {
                            why: format!(
                                "this backend counts instructions in a 32-bit number, and {n} does \
                                 not fit; ask for several runs rather than one"
                            ),
                        },
                        position: self.last_position.clone(),
                    });
                };
                match self.step_and_wait(count, StepKind::Instruction) {
                    Ok(()) => {
                        let at = self.instruction_position();
                        Ok(self.arrive(at))
                    }
                    Err(stop) => Ok(stop),
                }
            }

            Bound::Frames(n) => {
                // All but the last frame in **one** request, then one more to
                // land on a boundary.
                //
                // The obvious shape is one request per frame, because what the
                // backend offers for a boundary is "run to the first line of
                // the display" and that is a frame boundary exactly once per
                // frame. It is also three times slower, and on real software
                // that is the difference between five minutes and ninety
                // seconds: measured, 600 frames took 10.36 s one at a time and
                // 3.24 s in one request, because each request is a break and a
                // resume through the debugger rather than emulation.
                //
                // Running them in bulk lands wherever the last frame ends,
                // which is not a boundary — so the final frame is asked for the
                // slow way, and the position reported is a real boundary. The
                // two routes were measured to reach **the same machine**: the
                // digest over every writable region and the processor record
                // was identical, and both reported frame boundary 600.
                let mut remaining = n;
                while remaining > 1 {
                    let bulk = u32::try_from(remaining - 1).unwrap_or(u32::MAX);
                    if let Err(stop) = self.step_and_wait(bulk, StepKind::Frames) {
                        return Ok(stop);
                    }
                    remaining -= u64::from(bulk);
                }
                if let Err(stop) = self.step_and_wait(0, StepKind::ToLine) {
                    return Ok(stop);
                }
                let at = self.frame_position();
                Ok(self.arrive(at))
            }

            Bound::Address { address, within } => {
                let Ok(budget) = u32::try_from(within) else {
                    return Ok(Stop {
                        reason: Reason::Refused {
                            why: format!(
                                "this backend counts instructions in a 32-bit number, and a \
                                 budget of {within} does not fit"
                            ),
                        },
                        position: self.last_position.clone(),
                    });
                };
                if budget == 0 {
                    // A budget of nothing cannot arrive anywhere, and a run of
                    // no instructions that claims to be looking for an address
                    // would be a run that always exhausts.
                    return Ok(Stop {
                        reason: Reason::Refused {
                            why: "a budget of no instructions cannot reach an address".to_string(),
                        },
                        position: self.last_position.clone(),
                    });
                }

                let target = u32::try_from(address).map_err(|_| RunError::Backend {
                    why: format!("this backend addresses in 24 bits, and {address:#X} is wider"),
                })?;

                // §4.4's budget and the address are **one mechanism** here: a
                // step request of `within` instructions with a breakpoint
                // armed stops at whichever comes first (`doc/backend.md`).
                // Nothing else is needed, and nothing else would be a count.
                self.backend
                    .set_breakpoints(&[Breakpoint::execute_at(target)])
                    .map_err(|e| RunError::Backend { why: e.to_string() })?;
                let stepped = self.step_and_wait(budget, StepKind::Instruction);
                self.backend.clear_breakpoints();

                if let Err(stop) = stepped {
                    return Ok(stop);
                }

                // Which of the two happened is told apart by looking: the
                // instruction's own counter, not the processor's, because at a
                // breakpoint the processor's has not necessarily moved and
                // §5.4's measurements use the same call.
                let reached = self.backend.instruction_pc();
                let at = self.instruction_position();
                self.last_position = at.clone();
                Ok(Stop {
                    reason: if reached == target {
                        Reason::AddressHit {
                            address: u64::from(reached),
                        }
                    } else {
                        // §4.3: a reason is a result. The budget ran out, which
                        // is not a failure — the caller asked for a bounded
                        // look and got one.
                        Reason::BudgetExhausted
                    },
                    position: at,
                })
            }

            // §5.4's exact answer, and §4.5 built into the bound: two
            // breakpoints armed at once, one on the byte and one on the end of
            // the subject, and whichever happens first is what is reported.
            Bound::Write {
                region,
                offset,
                until,
                within,
            } => {
                let Ok(budget) = u32::try_from(within) else {
                    return Ok(Stop {
                        reason: Reason::Refused {
                            why: format!(
                                "this backend counts instructions in a 32-bit number, and a                                  budget of {within} does not fit"
                            ),
                        },
                        position: self.last_position.clone(),
                    });
                };
                if budget == 0 {
                    return Ok(Stop {
                        reason: Reason::Refused {
                            why: "a budget of no instructions cannot reach a write".to_string(),
                        },
                        position: self.last_position.clone(),
                    });
                }

                // The byte is named by region and offset (§3.1), so the region
                // has to exist and the offset has to be inside it. A bound
                // naming a byte that is not there is refused rather than
                // widened to the nearest one.
                let within_region = check_read(&self.regions, &region, Some((offset, 1)));
                let memory_type = match within_region.and_then(|_| {
                    self.memory_type(&region).ok_or_else(|| ReadError::Absent {
                        region: region.clone(),
                    })
                }) {
                    Ok(memory_type) => memory_type,
                    Err(e) => {
                        return Ok(Stop {
                            reason: Reason::Refused { why: e.to_string() },
                            position: self.last_position.clone(),
                        });
                    }
                };
                let Ok(byte) = u32::try_from(offset) else {
                    return Ok(Stop {
                        reason: Reason::Refused {
                            why: format!("{offset} is wider than this backend's addresses"),
                        },
                        position: self.last_position.clone(),
                    });
                };
                let target = u32::try_from(until).map_err(|_| RunError::Backend {
                    why: format!("this backend addresses in 24 bits, and {until:#X} is wider"),
                })?;

                self.backend
                    .set_breakpoints(&[
                        Breakpoint::write_within(memory_type, byte, byte).with_id(1),
                        Breakpoint::execute_at(target).with_id(2),
                    ])
                    .map_err(|e| RunError::Backend { why: e.to_string() })?;
                let stepped = self.step_and_wait(budget, StepKind::Instruction);
                self.backend.clear_breakpoints();

                if let Err(stop) = stepped {
                    return Ok(stop);
                }

                // Which of the two broke is told apart by the instruction's own
                // program counter, the same call §5.4's measurement used: at a
                // write it names the store, and at the end of the subject it
                // names the instruction asked for.
                let reached = self.backend.instruction_pc();
                if reached == target {
                    let at = self.instruction_position();
                    self.last_position = at.clone();
                    return Ok(Stop {
                        reason: Reason::AddressHit {
                            address: u64::from(reached),
                        },
                        position: at,
                    });
                }

                // A write, caught **during** the instruction that performs it:
                // the byte still holds its old value and the write lands on the
                // next step (`doc/backend.md`). So the position is
                // mid-instruction, which is both the exact answer §5.4 wants
                // and a place §3.4 forbids seeding from — and the machine
                // really is there, so nothing is committed here to tidy it up.
                let at = Position::MidInstruction {
                    pc: u64::from(reached),
                };
                self.last_position = at.clone();
                Ok(Stop {
                    reason: Reason::WriteHit { region, offset },
                    position: at,
                })
            }
        }
    }
}

impl Drop for Reference {
    fn drop(&mut self) {
        self.backend.release_debugger();
        self.backend.stop();
        self.backend.release();
        IN_USE.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The decision behind the write guard, with both answers.
    ///
    /// Same reason as `untouched`: a write that silently did not land cannot be
    /// produced by a working backend, so what is tested is where the guard says
    /// the two readings part.
    #[test]
    fn a_write_is_checked_by_where_the_readings_part() {
        assert_eq!(
            mismatch(&[1, 2, 3, 4], &[1, 2, 3, 4]),
            None,
            "a write that landed has nothing to report"
        );
        assert_eq!(
            mismatch(&[1, 2, 9, 4], &[1, 2, 3, 4]),
            Some(2),
            "and one that did not says WHERE, because that is what somebody has to go and look at"
        );
        assert_eq!(
            mismatch(&[0, 0, 0, 0], &[1, 2, 3, 4]),
            Some(0),
            "a span the backend left untouched parts at its first byte"
        );

        // A short read is a different failure and is still a failure.
        assert_eq!(mismatch(&[1, 2], &[1, 2, 3, 4]), Some(2));
        assert_eq!(mismatch(&[], &[1]), Some(0));
        assert_eq!(mismatch(&[], &[]), None, "nothing written, nothing to check");
    }

    /// The watchdog's deadline is on **silence**, not on duration — a run that
    /// is still working is never given up on, however long it takes.
    ///
    /// Tested on the loop's own shape rather than against a backend, because
    /// what changed is the rule: a deadline that resets on progress cannot be
    /// reached while progress is being made, and one that does not reset is
    /// reached in a fixed time no matter what the machine is doing.
    #[test]
    fn a_deadline_that_resets_on_progress_is_never_reached_while_there_is_progress() {
        let watchdog = Duration::from_millis(40);

        // Progress every tick: the deadline keeps moving and the loop would run
        // for as long as the work does.
        let mut deadline = Instant::now() + watchdog;
        let mut cycles = 0u64;
        let mut ticks = 0;
        for step in 1..=12u64 {
            std::thread::sleep(Duration::from_millis(10));
            if Instant::now() >= deadline {
                break;
            }
            if step != cycles {
                cycles = step;
                deadline = Instant::now() + watchdog;
            }
            ticks += 1;
        }
        assert_eq!(
            ticks, 12,
            "120ms of work under a 40ms watchdog, and it is never given up on because \
             something was happening the whole time"
        );

        // No progress at all: the same loop, the same watchdog, and it stops.
        let mut deadline = Instant::now() + watchdog;
        let mut cycles = 7u64;
        let mut ticks = 0;
        for _ in 0..12 {
            std::thread::sleep(Duration::from_millis(10));
            if Instant::now() >= deadline {
                break;
            }
            let now = 7u64;
            if now != cycles {
                cycles = now;
                deadline = Instant::now() + watchdog;
            }
            ticks += 1;
        }
        assert!(
            ticks < 12,
            "a backend that moves nothing is still caught, and in the same time as before"
        );
    }
    use awaseru_core::Access;

    /// Every error says something a person can act on, and the three that mean
    /// different things do not read the same.
    ///
    /// **What this does not cover**: that each of these is raised on the path it
    /// describes. Four of the five need a backend that fails in a particular
    /// way, which is what §16.5's conformance fixtures are for.
    #[test]
    fn opening_failures_explain_themselves() {
        let said = OpenError::AlreadyInUse.to_string();
        assert!(said.contains("already open"), "said: {said}");

        let said = OpenError::NeverStopped {
            waited: Duration::from_secs(3),
        }
        .to_string();
        assert!(said.contains("never reported stopping"), "said: {said}");
        assert!(
            said.contains("3s"),
            "and says how long it waited, said: {said}"
        );

        assert_ne!(
            OpenError::DebuggerAbsent.to_string(),
            OpenError::NoRegions.to_string(),
            "a backend without a debugger and a backend without memories are different problems"
        );
    }

    /// Each of the four ways a reference can come up says something different,
    /// and the three that are not reproducible say so.
    ///
    /// §4.12 wants a run to report how it arrived. A report that read the same
    /// whether or not the memory was settled would be worse than none, because
    /// somebody would trust it.
    #[test]
    fn every_way_of_coming_up_reports_itself_and_the_weak_ones_admit_it() {
        let origin = |at_power_on, zeroed: &[&'static str]| Origin {
            at_power_on,
            memory_zeroed: zeroed.to_vec(),
            input_log: None,
        };
        let good = origin(true, &["work-ram"]).to_string();
        let no_zero = origin(true, &[]).to_string();
        let no_power_on = origin(false, &["work-ram"]).to_string();
        let neither = origin(false, &[]).to_string();

        assert!(good.contains("reproducible"), "said: {good}");
        assert!(
            good.contains("not what the hardware does"),
            "the good case must still admit the divergence, said: {good}"
        );

        for (what, said) in [
            ("memory left random", &no_zero),
            ("position not reproducible", &no_power_on),
            ("neither", &neither),
        ] {
            assert!(
                said.contains("NOT") || said.contains("NOTHING"),
                "the `{what}` case must say plainly that it is not reproducible, said: {said}"
            );
        }

        let all = [&good, &no_zero, &no_power_on, &neither];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b, "the four cases must not read alike");
            }
        }
    }

    /// The defaults are the reproducible ones. A default that was convenient
    /// rather than reproducible would make every careless caller's results
    /// quietly worth less.
    #[test]
    fn the_default_startup_is_the_reproducible_one() {
        assert_eq!(
            Startup::default(),
            Startup {
                at_power_on: true,
                zero_memory: true
            }
        );
        assert_eq!(
            Startup::as_found(),
            Startup {
                at_power_on: false,
                zero_memory: false
            }
        );
        assert_ne!(Startup::default(), Startup::as_found());
    }

    /// The access the mapping declares is the access the region gets. A region
    /// built with the wrong access would refuse reads that should work, or
    /// accept writes that should not.
    #[test]
    fn the_mapping_decides_a_regions_access() {
        for mapping in MAPPINGS {
            let region = Region {
                name: mapping.region.to_string(),
                size: 16,
                access: mapping.access,
                unit: 1,
            };
            assert!(region.access.readable(), "`{}`", mapping.region);
            assert_eq!(
                region.access == Access::ReadOnly,
                mapping.region == "program-rom"
            );
        }
    }
}
