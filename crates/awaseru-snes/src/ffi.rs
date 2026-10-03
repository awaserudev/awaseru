//! The raw binding to the backend's shared library.
//!
//! This is the **only** module in this crate that is allowed `unsafe` (§17.1).
//! Its job is to be the one place a reviewer has to read carefully: raw symbol
//! declarations in, a safe API out, and nothing unsafe above it.
//!
//! # Where these declarations come from
//!
//! The backend exports a flat C ABI but ships **no C header** — the functions
//! are declared only in its `.cpp` files, with a macro that sets symbol
//! visibility. So there is nothing for a binding generator to read, and these
//! declarations are transcribed by hand from two sources that must agree:
//!
//! 1. the exported definitions in its `InteropDLL/*.cpp`, which are what the
//!    symbol actually is;
//! 2. the declarations its own front end uses to call them, which are
//!    maintained in step with the implementation because its interface breaks
//!    otherwise.
//!
//! **Where the two disagree, the implementation wins.** They do disagree once
//! already: the size of a memory region is declared as a signed 32-bit integer
//! on the consumer side and defined as an unsigned one in the implementation.
//! The two have the same ABI representation, so nothing breaks either way, and
//! the unsigned reading is the true one.
//!
//! This hand transcription is a maintenance cost, and it is the cost that
//! §16.1's version check and §16.5's conformance fixtures exist to contain: a
//! backend whose signatures moved under us is caught by the version refusing to
//! match, not by a corrupted read.

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// What can go wrong before the backend is usable.
#[derive(Debug)]
pub enum LoadError {
    /// The library could not be opened at all.
    Library { path: String, why: String },
    /// The library opened but a symbol this binding needs is not in it.
    Symbol { name: &'static str, why: String },
    /// A path that cannot be handed to a C API.
    Path { path: String, why: &'static str },
    /// An argument this backend cannot be given.
    Argument { why: &'static str },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Library { path, why } => {
                write!(f, "the backend library at {path} could not be loaded: {why}")
            }
            LoadError::Symbol { name, why } => write!(
                f,
                "the backend library does not export `{name}`, which this binding needs: {why}. \
                 Either it is not the backend this configuration names, or it was built without \
                 the interop layer"
            ),
            LoadError::Path { path, why } => write!(f, "the path {path} cannot be used: {why}"),
            LoadError::Argument { why } => {
                write!(f, "this backend cannot be given {why}")
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// The symbols this binding uses, resolved once at load.
///
/// Every field is a function pointer obtained from the loaded library. They are
/// resolved eagerly so that a missing symbol is a load failure naming the
/// symbol, rather than a surprise in the middle of a run.
#[allow(non_snake_case)]
struct Symbols {
    GetMesenVersion: unsafe extern "C" fn() -> u32,
    GetMesenBuildDate: unsafe extern "C" fn() -> *const c_char,
    InitDll: unsafe extern "C" fn(),
    #[allow(clippy::type_complexity)]
    InitializeEmu: unsafe extern "C" fn(
        homeFolder: *const c_char,
        windowHandle: *mut c_void,
        viewerHandle: *mut c_void,
        softwareRenderer: bool,
        noAudio: bool,
        noVideo: bool,
        noInput: bool,
    ),
    Release: unsafe extern "C" fn(),
    LoadRom: unsafe extern "C" fn(path: *const c_char, patch: *const c_char) -> bool,
    IsRunning: unsafe extern "C" fn() -> bool,
    Stop: unsafe extern "C" fn(),
    GetMemorySize: unsafe extern "C" fn(memory_type: u32) -> u32,
    GetMemoryState: unsafe extern "C" fn(memory_type: u32, buffer: *mut u8),
    RegisterNotificationCallback: unsafe extern "C" fn(callback: NotificationCallback) -> *mut c_void,
    InitializeDebugger: unsafe extern "C" fn(),
    ReleaseDebugger: unsafe extern "C" fn(),
    IsDebuggerRunning: unsafe extern "C" fn() -> bool,
    IsExecutionStopped: unsafe extern "C" fn() -> bool,
    Step: unsafe extern "C" fn(cpu_type: u8, count: u32, step_type: i32),
    GetCpuState: unsafe extern "C" fn(state: *mut u8, cpu_type: u8),
    GetPpuState: unsafe extern "C" fn(state: *mut u8, cpu_type: u8),
    SetCpuState: unsafe extern "C" fn(state: *const u8, cpu_type: u8),
    SetMemoryState: unsafe extern "C" fn(memory_type: u32, buffer: *const u8, length: i32),
    /// Four 32-bit numbers by value — see `EmulationConfig`.
    SetEmulationConfig: unsafe extern "C" fn(EmulationConfig),
    SetMemoryValues:
        unsafe extern "C" fn(memory_type: u32, address: u32, data: *const u8, length: i32),
    SaveStateFile: unsafe extern "C" fn(path: *const c_char),
    LoadStateFile: unsafe extern "C" fn(path: *const c_char),
    SetBreakpoints: unsafe extern "C" fn(breakpoints: *const Breakpoint, length: u32),
    GetProgramCounter: unsafe extern "C" fn(cpu_type: u8, of_the_instruction: bool) -> u32,
    GetMemoryAccessCounts: unsafe extern "C" fn(
        offset: u32,
        length: u32,
        memory_type: u32,
        counts: *mut AccessCounts,
    ),
    ResetMemoryAccessCounts: unsafe extern "C" fn(),
}

/// An opened backend library.
///
/// Holding this keeps the library loaded; dropping it unloads it. The symbol
/// pointers above borrow from it, which is why they live in the same struct and
/// why nothing hands them out.
///
/// `Debug` says what it is and not what it holds: printing resolved function
/// pointers would be noise, and the library handle has nothing legible in it.
pub struct Backend {
    symbols: Symbols,
    /// Last, so it outlives nothing — but it must not be dropped before the
    /// symbols stop being callable, which is why it is kept here at all.
    _library: libloading::Library,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Backend({} built {})", self.version(), self.build_date())
    }
}

impl Backend {
    /// Opens the library at `path` and resolves every symbol this binding uses.
    pub fn open(path: &Path) -> Result<Self, LoadError> {
        // SAFETY: opening a library runs its initialisers, which is arbitrary
        // code from outside this program. That is inherent to having a backend
        // at all (§15), and the path comes from the user's own machine-local
        // configuration (§6.1) rather than from anything this tool invents.
        let library = unsafe { libloading::Library::new(path) }.map_err(|e| LoadError::Library {
            path: path.display().to_string(),
            why: e.to_string(),
        })?;

        macro_rules! symbol {
            ($name:literal) => {{
                // SAFETY: the signature is transcribed from the backend's own
                // exported definition; see this module's header for the two
                // sources and which one wins. A name that is absent is an error
                // here rather than a call into nothing later.
                let found = unsafe { library.get(concat!($name, "\0").as_bytes()) }.map_err(
                    |e: libloading::Error| LoadError::Symbol {
                        name: $name,
                        why: e.to_string(),
                    },
                )?;
                // SAFETY: `Symbol::into_raw` then a transmute would be the
                // alternative; dereferencing the symbol yields the function
                // pointer, whose lifetime is tied to `library`, which this
                // struct keeps alive.
                *found
            }};
        }

        let symbols = Symbols {
            GetMesenVersion: symbol!("GetMesenVersion"),
            GetMesenBuildDate: symbol!("GetMesenBuildDate"),
            InitDll: symbol!("InitDll"),
            InitializeEmu: symbol!("InitializeEmu"),
            Release: symbol!("Release"),
            LoadRom: symbol!("LoadRom"),
            IsRunning: symbol!("IsRunning"),
            Stop: symbol!("Stop"),
            GetMemorySize: symbol!("GetMemorySize"),
            GetMemoryState: symbol!("GetMemoryState"),
            RegisterNotificationCallback: symbol!("RegisterNotificationCallback"),
            InitializeDebugger: symbol!("InitializeDebugger"),
            ReleaseDebugger: symbol!("ReleaseDebugger"),
            IsDebuggerRunning: symbol!("IsDebuggerRunning"),
            IsExecutionStopped: symbol!("IsExecutionStopped"),
            Step: symbol!("Step"),
            GetCpuState: symbol!("GetCpuState"),
            GetPpuState: symbol!("GetPpuState"),
            SetCpuState: symbol!("SetCpuState"),
            SetMemoryState: symbol!("SetMemoryState"),
            SetEmulationConfig: symbol!("SetEmulationConfig"),
            SetMemoryValues: symbol!("SetMemoryValues"),
            SaveStateFile: symbol!("SaveStateFile"),
            LoadStateFile: symbol!("LoadStateFile"),
            SetBreakpoints: symbol!("SetBreakpoints"),
            GetProgramCounter: symbol!("GetProgramCounter"),
            GetMemoryAccessCounts: symbol!("GetMemoryAccessCounts"),
            ResetMemoryAccessCounts: symbol!("ResetMemoryAccessCounts"),
        };

        Ok(Backend {
            symbols,
            _library: library,
        })
    }

    /// The library's own version, as it reports it.
    ///
    /// The packing — major, minor and revision in the three low bytes of a
    /// 32-bit word — was read off a built library rather than from any
    /// document, and the backend's own source was afterwards found to agree
    /// exactly. `doc/backend.md` quotes it, along with the one fidelity note:
    /// the major is declared wider there than it is read here, which parts
    /// company only above major 255.
    ///
    /// What this number does **not** do on this backend is identify a build —
    /// it is a constant in a source file, not derived from one — which is
    /// §13's Q11 and the reason `doc/backend.md` leads with a commit hash.
    pub fn version(&self) -> Version {
        // SAFETY: no arguments, returns a scalar.
        let raw = unsafe { (self.symbols.GetMesenVersion)() };
        Version::from_raw(raw)
    }

    /// When the library was built, as it reports it.
    pub fn build_date(&self) -> String {
        // SAFETY: the backend returns a pointer to a static string it owns; it
        // is read immediately and copied, and never freed here.
        let ptr = unsafe { (self.symbols.GetMesenBuildDate)() };
        if ptr.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    /// Prepares the library. Must be called before anything else touches it.
    pub fn init(&self) {
        // SAFETY: no arguments, no return.
        unsafe { (self.symbols.InitDll)() }
    }

    /// Brings up the emulator **headless**: no audio, no video, no input.
    ///
    /// The backend takes those three as arguments of its own, so a reference
    /// that draws nothing and makes no sound needs no trickery here.
    pub fn initialize_headless(&self, home: &Path) -> Result<(), LoadError> {
        let home = CString::new(home.as_os_str().as_encoded_bytes()).map_err(|_| {
            LoadError::Path {
                path: home.display().to_string(),
                why: "it contains a zero byte, which a C string cannot carry",
            }
        })?;
        // SAFETY: `home` outlives the call. The two handles are window handles
        // the backend only uses when it has video, which is switched off here.
        unsafe {
            (self.symbols.InitializeEmu)(
                home.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                true,  // software renderer
                true,  // no audio
                true,  // no video
                true,  // no input
            )
        }
        Ok(())
    }

    /// Tears the emulator down.
    pub fn release(&self) {
        // SAFETY: no arguments, no return.
        unsafe { (self.symbols.Release)() }
    }

    /// Loads a ROM. `false` means the backend refused it.
    pub fn load_rom(&self, path: &Path) -> Result<bool, LoadError> {
        let path_c =
            CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| LoadError::Path {
                path: path.display().to_string(),
                why: "it contains a zero byte, which a C string cannot carry",
            })?;
        // SAFETY: `path_c` outlives the call; a null patch means none.
        Ok(unsafe { (self.symbols.LoadRom)(path_c.as_ptr(), std::ptr::null()) })
    }

    /// Whether a ROM is loaded and running.
    pub fn is_running(&self) -> bool {
        // SAFETY: no arguments, returns a scalar.
        unsafe { (self.symbols.IsRunning)() }
    }

    /// Stops the running ROM.
    pub fn stop(&self) {
        // SAFETY: no arguments, no return.
        unsafe { (self.symbols.Stop)() }
    }

    /// How many bytes the backend says a memory type holds.
    pub fn memory_size(&self, memory_type: u32) -> u32 {
        // SAFETY: a scalar in, a scalar out.
        unsafe { (self.symbols.GetMemorySize)(memory_type) }
    }

    /// Reads a whole memory type into a fresh buffer.
    ///
    /// The buffer is sized by asking first, because the backend writes without
    /// being told how much room there is — so the size and the read must come
    /// from the same source, and that source is the backend.
    pub fn memory(&self, memory_type: u32) -> Vec<u8> {
        let size = self.memory_size(memory_type) as usize;
        let mut buffer = vec![0u8; size];
        if size > 0 {
            // SAFETY: the buffer is exactly the length the backend just said it
            // would write. A size of zero is not passed on, because a pointer
            // into an empty `Vec` is not one the backend should be handed.
            unsafe { (self.symbols.GetMemoryState)(memory_type, buffer.as_mut_ptr()) }
        }
        buffer
    }

    /// Asks the backend to tell us when its debugger breaks.
    ///
    /// There is no synchronous "step and return" in this backend: `step` sets a
    /// request and the emulation thread honours it whenever it gets there. The
    /// obvious alternative — step, then poll `is_execution_stopped` — **does not
    /// work**, and that was measured rather than reasoned: the poll sees the
    /// previous break, returns immediately, and the caller reads state from
    /// before the step. Counting breaks removes the race, because the count is
    /// read before the step is asked for and only ever goes up.
    pub fn listen_for_breaks(&self) {
        // SAFETY: the callback is a plain function pointer with no captured
        // state and no lifetime, so it stays valid for the life of the process.
        // The returned listener handle is the backend's; it is not freed here,
        // which leaks one small object per process and is the price of not
        // having to prove the backend has stopped calling it.
        unsafe { (self.symbols.RegisterNotificationCallback)(on_notification) };
    }

    /// How many times the backend has broken into its debugger since the
    /// library was loaded.
    ///
    /// Process-global, because the callback the backend takes carries no user
    /// data and the backend's emulator is itself a single global object. That is
    /// the backend's design, not a choice made here, and it is what §13's
    /// question about running two references in one process is about.
    pub fn breaks(&self) -> u64 {
        BREAKS.load(Ordering::SeqCst)
    }

    /// Brings up the debugger, which is what makes bounded running possible.
    pub fn initialize_debugger(&self) {
        // SAFETY: no arguments, no return.
        unsafe { (self.symbols.InitializeDebugger)() }
    }

    /// Takes it down again.
    pub fn release_debugger(&self) {
        // SAFETY: no arguments, no return.
        unsafe { (self.symbols.ReleaseDebugger)() }
    }

    pub fn is_debugger_running(&self) -> bool {
        // SAFETY: no arguments, returns a scalar.
        unsafe { (self.symbols.IsDebuggerRunning)() }
    }

    /// Whether the backend is sitting in a break.
    ///
    /// Useful to assert, useless to wait on — see `listen_for_breaks`.
    pub fn is_execution_stopped(&self) -> bool {
        // SAFETY: no arguments, returns a scalar.
        unsafe { (self.symbols.IsExecutionStopped)() }
    }

    /// Asks the backend to advance and then break.
    ///
    /// Returns as soon as the request is lodged, not when the break happens.
    /// `breaks` is how the caller learns it happened.
    pub fn step(&self, count: u32, kind: StepKind) {
        // SAFETY: three scalars, no return. `MAIN_CPU` and `kind` are values of
        // the backend's own enumerations; see their declarations for the two
        // sources they were transcribed from.
        unsafe { (self.symbols.Step)(MAIN_CPU, count, kind as i32) }
    }

    /// The processor's position, as far as this binding transcribes it.
    pub fn cpu_snapshot(&self) -> CpuSnapshot {
        let mut buffer = StateBuffer::new();
        // SAFETY: the backend writes its own state structure into the buffer.
        // The buffer is far larger than that structure and correctly aligned —
        // see `StateBuffer` for why its size is deliberately not a transcribed
        // number.
        unsafe { (self.symbols.GetCpuState)(buffer.as_mut_ptr(), MAIN_CPU) };
        CpuSnapshot {
            cycles: buffer.u64_at(0),
            // The program counter is sixteen bits and the bank it sits in is a
            // separate byte; a comparison wants the whole address, so they are
            // joined here rather than at every call site.
            pc: (u32::from(buffer.u8_at(20)) << 16) | u32::from(buffer.u16_at(18)),
        }
    }

    /// Writes a whole memory.
    ///
    /// The length is handed over as the backend declares it — a signed 32-bit
    /// number on both sides of its interop boundary — so a buffer longer than
    /// that is refused here rather than silently truncated to a negative.
    pub fn write_memory(&self, memory_type: u32, bytes: &[u8]) -> Result<(), LoadError> {
        let length = i32::try_from(bytes.len()).map_err(|_| LoadError::Argument {
            why: "a memory longer than a signed 32-bit length can describe",
        })?;
        // SAFETY: the pointer is to `bytes`, which outlives the call, and the
        // length is exactly its length.
        unsafe { (self.symbols.SetMemoryState)(memory_type, bytes.as_ptr(), length) }
        Ok(())
    }

    /// Writes part of a memory, at an address within it.
    pub fn write_memory_span(
        &self,
        memory_type: u32,
        address: u32,
        bytes: &[u8],
    ) -> Result<(), LoadError> {
        let length = i32::try_from(bytes.len()).map_err(|_| LoadError::Argument {
            why: "a span longer than a signed 32-bit length can describe",
        })?;
        // SAFETY: as `write_memory`. The backend is responsible for the range,
        // and the caller has already checked it against the region (§3.1).
        unsafe { (self.symbols.SetMemoryValues)(memory_type, address, bytes.as_ptr(), length) }
        Ok(())
    }

    /// Reads the processor state, and says whether the backend wrote more of
    /// the buffer than this binding reads.
    ///
    /// # How the length was arrived at
    ///
    /// By measurement, and the measurement is sound rather than approximate.
    /// The buffer was filled with `0xFF`, the state read, and the last changed
    /// byte noted; then the same with `0x00`. Bytes past 31 held the filler in
    /// **both** cases — had the backend written them they would have come back
    /// equal under both fillers, and they did not. So the backend writes
    /// exactly 32 bytes, and this reads exactly those.
    ///
    /// `wrote_beyond` is how a backend whose record has grown is caught at run
    /// time rather than by a truncated read nobody notices. §16.1's version
    /// check would catch a version bump; this catches a rebuild that kept the
    /// version and moved the struct.
    pub fn processor_state(&self) -> ProcessorRead {
        const FILLER: u8 = 0xFF;
        let mut buffer = StateBuffer::filled(FILLER);
        // SAFETY: the backend writes its own record into the buffer, which is
        // far larger than the record and correctly aligned.
        unsafe { (self.symbols.GetCpuState)(buffer.as_mut_ptr(), MAIN_CPU) };
        ProcessorRead {
            bytes: buffer.head(PROCESSOR_STATE_BYTES).to_vec(),
            wrote_beyond: buffer
                .tail(PROCESSOR_STATE_BYTES)
                .iter()
                .any(|&b| b != FILLER),
        }
    }

    /// Writes the processor state back.
    ///
    /// The backend copies its own record's worth out of the buffer, so the
    /// buffer is made the full size and the given bytes placed at the front.
    /// Fewer bytes than it reads would leave the rest of the record filled with
    /// whatever this buffer happened to hold, which is why a short state is
    /// refused rather than padded.
    pub fn write_processor_state(&self, bytes: &[u8]) -> Result<(), LoadError> {
        if bytes.len() != PROCESSOR_STATE_BYTES {
            return Err(LoadError::Argument {
                why: "a processor state that is not the length this backend's record is",
            });
        }
        let mut buffer = StateBuffer::filled(0);
        buffer.put(bytes);
        // SAFETY: the backend copies its record's worth from the front of the
        // buffer; the buffer is larger than that and the front is the state.
        unsafe { (self.symbols.SetCpuState)(buffer.as_ptr(), MAIN_CPU) }
        Ok(())
    }

    /// Asks the backend to write its whole machine state to a file.
    ///
    /// **This advances the machine** to the next place its debugger can break
    /// (`doc/backend.md`), so the position of what was saved is the position
    /// read *after* this returns.
    pub fn save_state_to(&self, path: &Path) -> Result<(), LoadError> {
        let path = c_path(path)?;
        // SAFETY: the string outlives the call.
        unsafe { (self.symbols.SaveStateFile)(path.as_ptr()) }
        Ok(())
    }

    /// Asks the backend to read a machine state back from a file.
    ///
    /// **Reports nothing.** Given a file of nonsense, or no file at all, the
    /// backend leaves the machine as it was and says so to nobody (§13's Q12).
    /// Whether it worked is decided by looking at where the machine is
    /// afterwards, which is why a blob carries its position.
    pub fn load_state_from(&self, path: &Path) -> Result<(), LoadError> {
        let path = c_path(path)?;
        // SAFETY: the string outlives the call.
        unsafe { (self.symbols.LoadStateFile)(path.as_ptr()) }
        Ok(())
    }

    /// Arms a set of breakpoints, replacing whatever was armed before.
    ///
    /// An empty slice disarms everything, which is what the call wants anyway:
    /// the backend takes the whole set each time, so there is no "remove one".
    pub fn set_breakpoints(&self, breakpoints: &[Breakpoint]) -> Result<(), LoadError> {
        let length = u32::try_from(breakpoints.len()).map_err(|_| LoadError::Argument {
            why: "more breakpoints than a 32-bit count can describe",
        })?;
        // SAFETY: a pointer into the slice, and exactly its length. The backend
        // copies what it needs during the call — see `Debugger::SetBreakpoints`,
        // which assigns into its own vector — so the slice need not outlive it.
        // An empty slice's pointer is passed as null rather than dangling.
        let pointer = if breakpoints.is_empty() {
            std::ptr::null()
        } else {
            breakpoints.as_ptr()
        };
        unsafe { (self.symbols.SetBreakpoints)(pointer, length) }
        Ok(())
    }

    /// Disarms every breakpoint.
    pub fn clear_breakpoints(&self) {
        // SAFETY: a null pointer with a length of zero, which is what the
        // backend's own front end sends to clear them.
        unsafe { (self.symbols.SetBreakpoints)(std::ptr::null(), 0) }
    }

    /// Where the **instruction being executed** begins.
    ///
    /// Different from the processor's own program counter, and the difference is
    /// the whole point of this call existing. At a write breakpoint the
    /// processor's counter has already moved past the store — measured at
    /// `$8014` for a four-byte store beginning at `$8010` — and this returns
    /// `$8010`: the instruction doing the write (`doc/backend.md`).
    ///
    /// That is §5.4's third item, and it is why localisation can name an
    /// instruction rather than a cycle.
    pub fn instruction_pc(&self) -> u32 {
        // SAFETY: two scalars in, a scalar out.
        unsafe { (self.symbols.GetProgramCounter)(MAIN_CPU, true) }
    }

    /// The processor's own program counter, which after a write breakpoint is
    /// the instruction *after* the one writing.
    pub fn next_pc(&self) -> u32 {
        // SAFETY: as `instruction_pc`.
        unsafe { (self.symbols.GetProgramCounter)(MAIN_CPU, false) }
    }

    /// How often, and how recently, each byte of a span was touched.
    ///
    /// The buffer is sized here and the backend fills it, so a caller cannot
    /// ask for more than it has room for.
    pub fn access_counts(&self, memory_type: u32, offset: u32, length: u32) -> Vec<AccessCounts> {
        let mut counts = vec![AccessCounts::default(); length as usize];
        if length > 0 {
            // SAFETY: the buffer is exactly `length` records long, which is
            // what the backend is told to fill.
            unsafe {
                (self.symbols.GetMemoryAccessCounts)(
                    offset,
                    length,
                    memory_type,
                    counts.as_mut_ptr(),
                )
            }
        }
        counts
    }

    /// Sets the emulation settings — used to take the throttle off.
    pub fn set_emulation_config(&self, config: EmulationConfig) {
        // SAFETY: a flat record of four 32-bit numbers, passed by value, as
        // both the header and the front end's own declaration describe it.
        unsafe { (self.symbols.SetEmulationConfig)(config) }
    }

    /// Forgets every access count, so that the next measurement is of one
    /// interval rather than of all history.
    pub fn reset_access_counts(&self) {
        // SAFETY: no arguments, no return.
        unsafe { (self.symbols.ResetMemoryAccessCounts)() }
    }

    /// Where the video hardware stands.
    pub fn video_snapshot(&self) -> VideoSnapshot {
        let mut buffer = StateBuffer::new();
        // SAFETY: as `cpu_snapshot`.
        unsafe { (self.symbols.GetPpuState)(buffer.as_mut_ptr(), MAIN_CPU) };
        VideoSnapshot {
            dot: buffer.u16_at(0),
            line: buffer.u16_at(2),
            frames: buffer.u32_at(8),
        }
    }
}

/// The shape of the callback the backend hands notifications to.
///
/// `__stdcall` in its declaration, which is nothing on the platforms this is
/// built for, so the C calling convention is the whole of it.
type NotificationCallback = extern "C" fn(kind: i32, parameter: *mut c_void);

/// The notification that means the debugger has broken.
///
/// The number is this value's position in the backend's notification
/// enumeration, which both of the sources named in this module's header agree
/// on, and it was confirmed by counting: one notification of this kind arrives
/// per step requested, and none arrive while nothing is stepping.
const BREAK_NOTIFICATION: i32 = 5;

/// How many breaks the backend has reported.
///
/// A counter rather than a flag, so that a break which happens between asking
/// and looking cannot be missed.
static BREAKS: AtomicU64 = AtomicU64::new(0);

/// Runs on the backend's emulation thread. It does one thing, because anything
/// it did would be happening inside the reference's own execution.
extern "C" fn on_notification(kind: i32, _parameter: *mut c_void) {
    if kind == BREAK_NOTIFICATION {
        BREAKS.fetch_add(1, Ordering::SeqCst);
    }
}

/// Which processor the step and state calls are about.
///
/// The backend's enumeration of processors puts the main one first, in both
/// sources. This backend exposes several — a console's sound processor and its
/// coprocessors are in the same list — and nothing here addresses them yet.
const MAIN_CPU: u8 = 0;

/// The kinds of step this binding asks for.
///
/// Only two of the backend's are transcribed, and on purpose: these two are the
/// ones whose stopping behaviour has been *measured* (see `crate::reference`).
/// Declaring the rest would make them callable without that, which is the
/// guessing §2.4 is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum StepKind {
    /// Count whole instructions. Breaks between instructions.
    Instruction = 0,
    /// Run until the video hardware reaches the line given as the count.
    ToLine = 7,
}

/// A buffer for a state structure the backend fills in.
///
/// The backend writes a structure whose **size is not part of what this binding
/// transcribes**: it is a large, frequently-changing record and there is no
/// header to take its layout from (see this module's header). So the buffer is
/// made far bigger than any plausible version of it, aligned for the widest
/// field it can contain, and only the leading fields are read — the ones at the
/// front of the record in both sources, which are also the ones whose values
/// were checked against measurements.
///
/// Over-allocating is the honest option here. Guessing the size and being wrong
/// by a few bytes is a write past the end of the buffer.
#[repr(C, align(8))]
struct StateBuffer([u8; StateBuffer::SIZE]);

/// How many bytes the backend's processor record is, measured — see
/// `Backend::processor_state` for how, and why the measurement is exact rather
/// than a lower bound.
pub const PROCESSOR_STATE_BYTES: usize = 32;

impl StateBuffer {
    const SIZE: usize = 8192;

    fn new() -> Box<Self> {
        Self::filled(0)
    }

    fn filled(byte: u8) -> Box<Self> {
        Box::new(StateBuffer([byte; Self::SIZE]))
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.0.as_mut_ptr()
    }

    fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr()
    }

    fn head(&self, len: usize) -> &[u8] {
        &self.0[..len]
    }

    fn tail(&self, from: usize) -> &[u8] {
        &self.0[from..]
    }

    fn put(&mut self, bytes: &[u8]) {
        self.0[..bytes.len()].copy_from_slice(bytes);
    }

    fn u8_at(&self, offset: usize) -> u8 {
        self.0[offset]
    }

    fn u16_at(&self, offset: usize) -> u16 {
        u16::from_ne_bytes([self.0[offset], self.0[offset + 1]])
    }

    fn u32_at(&self, offset: usize) -> u32 {
        u32::from_ne_bytes([
            self.0[offset],
            self.0[offset + 1],
            self.0[offset + 2],
            self.0[offset + 3],
        ])
    }

    fn u64_at(&self, offset: usize) -> u64 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.0[offset..offset + 8]);
        u64::from_ne_bytes(bytes)
    }
}

/// What a breakpoint may stop on. The backend's own flags, which combine.
///
/// Transcribed from two sources that agree: the enumeration in its debugger's
/// types, and the one its front end marshals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum StopOn {
    Read = 1,
    Write = 2,
    Execute = 4,
}

/// One of the backend's breakpoints.
///
/// # Why this one could be transcribed when a configuration record could not
///
/// It is **flat** — ten fields and a fixed array, no nested records — and
/// `SetBreakpoints` takes a **pointer and a length** rather than a struct by
/// value, so nothing here depends on guessing how a large record is passed in
/// registers. Two sources agree on it field for field, and a test checks that
/// this transcription is 1028 bytes with the offsets they describe: a
/// transcription error that moved a field would change one or the other.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Breakpoint {
    id: i32,
    cpu_type: u8,
    memory_type: i32,
    stop_on: i32,
    first_address: i32,
    last_address: i32,
    enabled: bool,
    mark_event: bool,
    ignore_dummy_operations: bool,
    /// An expression language this project has not looked at and does not need.
    /// Always empty, and a test keeps it that way: a condition nobody wrote
    /// should not be a condition nobody noticed.
    condition: [u8; Self::CONDITION_BYTES],
}

impl std::fmt::Debug for Breakpoint {
    /// Says what it stops on and where, and not the thousand bytes of
    /// condition it is not using.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Breakpoint(stop_on={:#x} memory={} {:#x}..={:#x}{})",
            self.stop_on,
            self.memory_type,
            self.first_address,
            self.last_address,
            if self.enabled { "" } else { ", disarmed" }
        )
    }
}

impl Breakpoint {
    const CONDITION_BYTES: usize = 1000;

    /// Stops when the processor reaches `address` on its own bus.
    ///
    /// The bus rather than a memory, because an address is where the program
    /// counter will be and the program counter addresses the bus.
    pub fn execute_at(address: u32) -> Self {
        Self::new(StopOn::Execute, PROCESSOR_BUS, address, address)
    }

    /// Stops when anything writes within a span of one memory.
    ///
    /// A memory rather than the bus, because a byte of work memory can be
    /// written through more than one address and a comparison cares about the
    /// byte.
    pub fn write_within(memory_type: u32, first: u32, last: u32) -> Self {
        Self::new(StopOn::Write, memory_type, first, last)
    }

    /// A distinct identifier, for arming more than one at a time.
    ///
    /// The backend keeps them in a vector and reports which one broke by id;
    /// two sharing an id would be two the report cannot tell apart, which is
    /// exactly the question §5.4's localisation asks of a pair.
    pub fn with_id(mut self, id: i32) -> Self {
        self.id = id;
        self
    }

    fn new(stop_on: StopOn, memory_type: u32, first: u32, last: u32) -> Self {
        Breakpoint {
            id: 1,
            cpu_type: MAIN_CPU,
            memory_type: memory_type as i32,
            stop_on: stop_on as i32,
            first_address: first as i32,
            last_address: last as i32,
            enabled: true,
            mark_event: false,
            // Dummy operations are the backend's name for the reads a processor
            // makes while deciding what to do. A comparison is about writes the
            // software meant, so they are ignored.
            ignore_dummy_operations: true,
            condition: [0; Self::CONDITION_BYTES],
        }
    }
}

/// The backend's own address space, as a memory type. Used for execution
/// breakpoints, because a program counter addresses the bus.
const PROCESSOR_BUS: u32 = 0;

/// How often and how recently one byte was touched.
///
/// The stamps are in the **backend's own clock**, which is not the processor's
/// cycle count — at processor cycle 32 a write stamp read 426
/// (`doc/backend.md`). So a stamp compares with another stamp and with nothing
/// else: it answers *when, relative to other accesses*, and never *where from*.
/// The backend's emulation settings — four 32-bit numbers and nothing else.
///
/// Transcribed where the configuration record was twice refused, and the
/// difference is the whole justification: this is a flat 16 bytes with no
/// nested record, no string and no array, so there is no layout to guess at.
///
/// `speed` is a percentage, and **zero means no limit**: the backend's frame
/// delay is computed as zero and it runs as fast as it can
/// (`Emulator::GetFrameDelay`). Left at its default of 100, every replay is
/// paced to the console's real time, which is what a person watching wants and
/// the opposite of what a replay wants.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmulationConfig {
    pub speed: u32,
    pub turbo_speed: u32,
    pub rewind_speed: u32,
    pub run_ahead_frames: u32,
}

impl EmulationConfig {
    /// As fast as the machine can, which is what a measurement wants.
    ///
    /// `run_ahead_frames` is zero deliberately: run-ahead speculatively
    /// executes and rewinds, which is a feature for a player and an extra
    /// source of divergence for a comparison.
    pub fn unthrottled() -> Self {
        EmulationConfig {
            speed: 0,
            turbo_speed: 300,
            rewind_speed: 100,
            run_ahead_frames: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccessCounts {
    pub read_stamp: u64,
    pub write_stamp: u64,
    pub execute_stamp: u64,
    pub reads: u32,
    pub writes: u32,
    pub executions: u32,
}

/// A read of the processor's record, with the one thing a caller cannot see for
/// itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessorRead {
    /// Exactly the record, as measured.
    pub bytes: Vec<u8>,
    /// Whether the backend wrote past what this binding reads — which would
    /// mean its record has grown and this transcription has gone stale.
    pub wrote_beyond: bool,
}

/// A path as a C string, refused rather than mangled when it cannot be one.
fn c_path(path: &Path) -> Result<CString, LoadError> {
    CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| LoadError::Path {
        path: path.display().to_string(),
        why: "it contains a zero byte, which a C string cannot carry",
    })
}

/// As much of the processor's position as this binding reads.
///
/// Not the processor *state* of §3.3 — the registers and flags are not here,
/// because nothing compares them yet and §7.6 leaves their shape open. This is
/// what a position needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuSnapshot {
    /// The full address being executed, bank included.
    pub pc: u32,
    /// The processor's own cycle count, which only goes up. Useful for saying
    /// "something ran" without claiming to know what.
    pub cycles: u64,
}

/// Where the video hardware stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoSnapshot {
    /// Completed frames.
    pub frames: u32,
    /// The line being drawn.
    pub line: u16,
    /// How far along that line.
    pub dot: u16,
}

/// A backend version, decomposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
    pub revision: u8,
    /// What the library actually returned, kept so that a comparison can be
    /// made on the number rather than on a reconstruction of it.
    pub raw: u32,
}

impl Version {
    /// Splits the packed word the backend returns.
    pub fn from_raw(raw: u32) -> Self {
        Version {
            major: ((raw >> 16) & 0xFF) as u8,
            minor: ((raw >> 8) & 0xFF) as u8,
            revision: (raw & 0xFF) as u8,
            raw,
        }
    }

    /// The packing, in reverse. Exists so that a decode can be checked against
    /// the number it came from rather than believed.
    pub fn to_raw(self) -> u32 {
        (self.major as u32) << 16 | (self.minor as u32) << 8 | self.revision as u32
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The decomposition is reversible for every byte triple. This covers the
    /// arithmetic and nothing else: it says the three fields are read from the
    /// three bytes the `to_raw` side writes them to, not that those are the
    /// bytes the **backend** puts them in.
    #[test]
    fn the_version_packing_round_trips() {
        for major in [0u8, 1, 2, 42, 255] {
            for minor in [0u8, 1, 2, 99, 255] {
                for revision in [0u8, 1, 7, 255] {
                    let raw =
                        (major as u32) << 16 | (minor as u32) << 8 | revision as u32;
                    let v = Version::from_raw(raw);
                    assert_eq!((v.major, v.minor, v.revision), (major, minor, revision));
                    assert_eq!(v.to_raw(), raw, "re-packing must give the number back");
                }
            }
        }
    }

    /// **The test the transcription rests on.** Two sources agree that this
    /// record is 1028 bytes with these offsets; if this transcription is not,
    /// then one of its fields is somewhere the backend does not look, and every
    /// breakpoint would be armed on the wrong address or the wrong kind.
    ///
    /// Offsets are checked by construction rather than by `offset_of`, which
    /// would need a nightly feature: a value is built, its bytes are read, and
    /// each field is found where it is expected.
    #[test]
    fn the_breakpoint_record_is_the_shape_both_sources_describe() {
        assert_eq!(
            std::mem::size_of::<Breakpoint>(),
            1028,
            "a 1028-byte record is what both sources describe: 27 bytes of fields, a thousand \
             of condition, and one of padding to a four-byte boundary"
        );
        assert_eq!(std::mem::align_of::<Breakpoint>(), 4);

        let b = Breakpoint::write_within(15, 0x0100, 0x0103);
        let bytes: [u8; 1028] = unsafe { std::mem::transmute(b) };

        assert_eq!(&bytes[0..4], &1i32.to_ne_bytes(), "the id is first");
        assert_eq!(bytes[4], MAIN_CPU, "then the processor, in one byte");
        assert_eq!(
            &bytes[8..12],
            &15i32.to_ne_bytes(),
            "then the memory, after three bytes of padding"
        );
        assert_eq!(
            &bytes[12..16],
            &(StopOn::Write as i32).to_ne_bytes(),
            "then what it stops on"
        );
        assert_eq!(&bytes[16..20], &0x0100i32.to_ne_bytes(), "then the first address");
        assert_eq!(&bytes[20..24], &0x0103i32.to_ne_bytes(), "and the last");
        assert_eq!(bytes[24], 1, "enabled");
        assert_eq!(bytes[25], 0, "not marking an event");
        assert_eq!(bytes[26], 1, "and ignoring the processor's own dummy reads");
        assert!(
            bytes[27..1027].iter().all(|&b| b == 0),
            "the condition is empty, and a condition nobody wrote must not be one nobody noticed"
        );
    }

    /// The two kinds differ in what they stop on and in which memory they
    /// watch, and both of those matter. An execution breakpoint watches the
    /// bus, because that is what a program counter addresses; a write
    /// breakpoint watches a memory, because a byte can be written through more
    /// than one address.
    #[test]
    fn the_two_kinds_of_breakpoint_watch_different_things() {
        let execute = Breakpoint::execute_at(0x8010);
        let write = Breakpoint::write_within(15, 0x100, 0x100);

        assert_eq!(execute.stop_on, StopOn::Execute as i32);
        assert_eq!(execute.memory_type, PROCESSOR_BUS as i32);
        assert_eq!(execute.first_address, 0x8010);
        assert_eq!(execute.last_address, 0x8010, "one address, not a span");

        assert_eq!(write.stop_on, StopOn::Write as i32);
        assert_eq!(write.memory_type, 15);
        assert_ne!(
            write.memory_type, execute.memory_type,
            "a write watches a memory and an execution watches the bus; the same memory for \
             both would arm one of them on the wrong thing"
        );

        // And the flags are the backend's, which combine — so they must be
        // distinct powers of two or two kinds would be one.
        for (a, b) in [
            (StopOn::Read, StopOn::Write),
            (StopOn::Write, StopOn::Execute),
            (StopOn::Read, StopOn::Execute),
        ] {
            assert_eq!(
                (a as i32) & (b as i32),
                0,
                "{a:?} and {b:?} overlap, so arming one would arm the other"
            );
        }
    }

    /// Bits above the three bytes are not part of the version and must not leak
    /// into it. This is the case that would catch a decode written with the
    /// shifts right and the masks missing.
    #[test]
    fn the_high_byte_is_not_part_of_the_version() {
        let v = Version::from_raw(0xFF_02_02_01);
        assert_eq!((v.major, v.minor, v.revision), (2, 2, 1));
        assert_eq!(v.raw, 0xFF_02_02_01, "the raw word is kept whole");
        assert_ne!(v.to_raw(), v.raw, "and re-packing drops what is not version");
    }
}
