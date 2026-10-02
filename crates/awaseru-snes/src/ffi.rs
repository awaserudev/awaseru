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

impl StateBuffer {
    const SIZE: usize = 8192;

    fn new() -> Box<Self> {
        Box::new(StateBuffer([0; Self::SIZE]))
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.0.as_mut_ptr()
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
