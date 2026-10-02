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

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use awaseru_core::{
    Bound, Platform, Position, ReadError, Reason, Region, Regions, RunError, Stop,
    platform::BackendVersion, check_read,
};

use crate::ffi::{Backend, LoadError, StepKind};
use crate::memory::MAPPINGS;

/// Taken for as long as a reference exists, because the backend's emulator is
/// one global object (see this module's header).
static IN_USE: AtomicBool = AtomicBool::new(false);

/// How long to wait for a break before giving up on one.
///
/// **This is not the budget of §4.4.** A budget is a count, so that the same
/// run stops the same way every time (§2.5); this is a wall clock, and a run
/// that ends on it is not reproducible. That is exactly why hitting it is
/// reported as the backend being unable to continue rather than as a budget
/// running out: the two mean different things to whoever reads the result.
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

/// A region, and which of the backend's memories it is.
#[derive(Debug, Clone)]
struct Mapped {
    region: Region,
    memory_type: u32,
}

/// An emulator this crate drives, as the host sees it.
pub struct Reference {
    backend: Backend,
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
}

impl std::fmt::Debug for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Reference({:?}, {} region(s), stopped: {}, at {})",
            self.backend,
            self.regions.len(),
            self.stopped,
            self.last_position
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
        Self::claim(|| Ok(Backend::open(library)?), home, software)
    }

    /// The same, from a library somebody has already opened — and, usually,
    /// already asked what version it is (§16.1).
    ///
    /// This exists because the version check belongs to whoever reads the
    /// configuration, and it must happen **before** software is loaded: a
    /// refusal that costs a ROM load and a debugger is a refusal that arrives
    /// late. So the host opens the library, checks it, and hands it here.
    pub fn adopt(backend: Backend, home: &Path, software: &Path) -> Result<Self, OpenError> {
        Self::claim(|| Ok(backend), home, software)
    }

    fn claim(
        backend: impl FnOnce() -> Result<Backend, OpenError>,
        home: &Path,
        software: &Path,
    ) -> Result<Self, OpenError> {
        if IN_USE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(OpenError::AlreadyInUse);
        }

        match backend().and_then(|backend| Self::bring_up(backend, home, software)) {
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

    fn bring_up(backend: Backend, home: &Path, software: &Path) -> Result<Self, OpenError> {
        backend.init();
        backend.initialize_headless(home)?;
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
        // first thing to do is stop it. One instruction is the shortest
        // request that has a defined stopping place.
        //
        // **Where that stop lands is not reproducible**, and it is not pretended
        // to be: the backend had already been running for an unmeasured time
        // when the request arrived. §13 carries what it would take to start from
        // a position that *is* reproducible.
        let watchdog = DEFAULT_WATCHDOG;
        let before = backend.breaks();
        backend.step(1, StepKind::Instruction);
        if !wait_for_break(&backend, before, watchdog) {
            return Err(OpenError::NeverStopped { waited: watchdog });
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
        let last_position = Position::InstructionBoundary {
            pc: u64::from(backend.cpu_snapshot().pc),
        };

        Ok(Reference {
            backend,
            mapped,
            regions,
            stopped: true,
            last_position,
            watchdog,
        })
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

    /// The position after a run bounded by frames — **if** the backend really
    /// is at the start of a frame.
    ///
    /// Checked rather than assumed. The backend lands on line 0, dot 0 every
    /// time it has been measured; if it ever does not, this says it does not
    /// know where it is instead of calling something a frame boundary because
    /// that is what was asked for.
    fn frame_position(&self) -> Position {
        let video = self.backend.video_snapshot();
        if video.line == 0 && video.dot == 0 {
            Position::FrameBoundary {
                frame: u64::from(video.frames),
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
fn wait_for_break(backend: &Backend, before: u64, watchdog: Duration) -> bool {
    let deadline = Instant::now() + watchdog;
    while backend.breaks() == before {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
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
                // One request per frame, because what the backend offers is
                // "run to the first line of the display" and that is a frame
                // boundary exactly once per frame. Asking it for n frames'
                // worth of video cycles in one request also works, but it lands
                // wherever in the frame it started, which is not a boundary and
                // would have to be reported as a position nothing can seed from.
                for _ in 0..n {
                    if let Err(stop) = self.step_and_wait(0, StepKind::ToLine) {
                        return Ok(stop);
                    }
                }
                let at = self.frame_position();
                Ok(self.arrive(at))
            }

            Bound::Address(address) => Ok(Stop {
                reason: Reason::Refused {
                    why: format!(
                        "this binding cannot stop on an address yet. The backend can — it takes a \
                         list of breakpoints — but the shape of that list is not among the \
                         declarations transcribed here, and {address:#X} is not worth guessing at"
                    ),
                },
                position: self.last_position.clone(),
            }),
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
