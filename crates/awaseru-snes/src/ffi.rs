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
    /// document. Whether this interpretation is the right one is not settled
    /// here; §16.1's check, where the configuration declares a version and a
    /// mismatch refuses, is what settles it.
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
