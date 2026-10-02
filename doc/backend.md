# The first backend: how it is obtained, built and pinned

§16.6 of `spec.md` is the policy. This file is the record that makes it
actionable: everything measured in `crates/awaseru-snes` was measured against
one library, built from one commit, by one compiler, and this says which.

Nothing here is required to *use* awaseru. The library is supplied, not shipped
(§1.5, §11.1) — the user builds or obtains their own and names its path in their
machine-local configuration. This file exists so that somebody who wants to
check a measurement can stand where it was taken.

## What it is

| | |
|---|---|
| upstream | `https://github.com/nesdev-org/MesenCE.git` |
| branch | `master` |
| **pinned commit** | `a60e79feb4d6dcced5922d636f9211837d01e381` |
| that commit's date | 2026-09-26 11:52:50 +0900 |
| that commit's subject | *Audio: WASAPI — Improve audio quality+latency when fast forwarding (#283)* |
| licence | GNU GPL v3 (`LICENSE` at its root) |
| what awaseru calls it | `mesence`, in the configuration's `backend` field |

It is a community fork of a project whose original author archived it, which is
§15.4's risk and §13's Q7.

## Obtaining it

```sh
git clone https://github.com/nesdev-org/MesenCE.git
cd MesenCE
git checkout a60e79feb4d6dcced5922d636f9211837d01e381
```

**Pin, never track** (§16.6). An upgrade is a deliberate act with §16.5's
conformance run as the gate, and on this backend the gate matters more than
usual — see *What the version does not tell you*, below.

## Building it

```sh
make core USE_GCC=true -j8
```

That produces `InteropDLL/obj.linux-x64/MesenCore.so`, which is the file the
machine-local configuration points at.

Three things worth knowing, because the project's own `COMPILING.md` describes
building the whole emulator and that is not what this is:

- **`core` is not the default target.** `make` with no target builds `ui`, which
  is the front end. `core` builds `InteropDLL/$(OBJFOLDER)/$(SHAREDLIB)` and
  stops — the shared library with the flat C ABI that `crates/awaseru-snes/src/ffi.rs`
  binds to, and nothing else.
- **`core` needs neither SDL2 nor the .NET SDK**, although `COMPILING.md` asks
  for both. Those are the front end's. The built library's whole dynamic
  dependency set, measured with `ldd`, is:

  ```
  linux-vdso.so.1
  libm.so.6
  libc.so.6
  /lib64/ld-linux-x86-64.so.2
  ```

  Nothing else. That narrow surface is why driving an emulator as a library is
  tolerable at all (§15, §16.6's "depend on the narrow surface").
- **`USE_GCC=true` selects g++ over the makefile's default of clang++.** It is
  recorded because it is what was used, not because clang would be wrong. The
  compiler used for every measurement in this repository so far:

  ```
  g++ (Debian 14.2.0-19) 14.2.0
  ```

### This recipe was run, not just written down

A detached worktree at the pinned commit, `make core USE_GCC=true -j8` with the
g++ above, on an eight-core machine: **2 minutes 26 seconds**, exit 0, no
errors. All eighteen symbols `ffi.rs` resolves are exported from the result, and
its dynamic dependencies are the four above and nothing else.

Against the library that every measurement in this repository was taken from,
the result is the same size to the byte and differs in exactly **25 bytes**, in
two places:

| bytes | what they are |
|---|---|
| 641–660 | the GNU build-id note, which is a hash over the build and so cannot match |
| 10723470–10723476 | the build timestamp the backend embeds and reports through `GetMesenBuildDate` — `Oct  2 2026, 15:31:03` against `Oct  2 2026, 16:46:21` |

Nothing else differs. The whole test suite, including the run that drives a
reference and reads a region, passes against the rebuilt library as it does
against the original.

So the recipe above is not a description of how the library was probably made.
It reproduces it.

## Pointing awaseru at it

The path goes in the machine-local half of the configuration, keyed by the
emulator's name (§6.1, §6.4) — never in the shared half, which travels in a
repository:

```toml
# awaseru.local.toml
[emulator.ref-a]
path = "/somewhere/MesenCE/InteropDLL/obj.linux-x64/MesenCore.so"
```

and the shared half declares what it is, including the version, which awaseru
checks against what the loaded library reports (§16.1):

```toml
# awaseru.toml
[[emulator]]
name = "ref-a"
platform = "snes"
backend = "mesence"
version = "2.2.1"
```

## What the version does tell you

`crates/awaseru-snes/src/ffi.rs` reads the version out of a packed 32-bit word
as `major << 16 | minor << 8 | revision`. That packing was **measured** first —
read off a built library before any source was consulted — and the source
confirms it exactly, at `Core/Shared/EmuSettings.cpp`:

```cpp
uint32_t EmuSettings::GetVersion()
{
    //Version 2.2.1
    uint16_t major = 2;
    uint8_t minor = 2;
    uint8_t revision = 1;
    return (major << 16) | (minor << 8) | revision;
}
```

One fidelity note: `major` is declared sixteen bits wide there and the binding
reads eight. The two agree for every version that exists and would part company
only above major 255, which is why it is recorded here rather than guarded
against.

## What the version does **not** tell you

**The version is a constant in a source file.** It is not derived from the
build, the commit, or the tag. Every commit between two releases reports the
same number — so the library built from the commit pinned above and a library
built from any other commit on the way to 2.2.2 are indistinguishable to §16.1's
check.

This does not make the check useless: it still catches the configuration that
says 2.2.1 against a library that is 2.3.0, which is the common mistake. But it
means:

- **the commit hash is the real identity of a backend build**, not the version
  string, and that is why this file leads with the hash;
- §16.2's point — a version is a label, not a behaviour — is sharper here than
  it reads: on this backend the label does not even identify the build;
- §16.5's conformance fixtures are the only mechanism that can actually tell two
  builds apart, and they are therefore not a refinement but the load-bearing
  part of the upgrade policy.

Recorded as `spec.md` §13, Q11.

## What §16.6 asks for and this does not yet do

§16.6's first bullet is **vendor the source at a pinned commit**, so that the
upstream's fate does not reach us. This project has not done that: the backend
lives in a clone beside the repository, and what is pinned is a hash written
down here.

That is a smaller guarantee than vendoring, and the gap is deliberate for now
rather than overlooked. Vendoring a C++ emulator into this repository touches
the licence question that §11.4 leaves open, and it is a decision better taken
once that one is. Until it is taken, the risk is the one §16.7 describes: an
emulator that stops being maintained stops *improving*, not working, and the
console it reproduces is not going to change.

## What was measured of its behaviour

Taken against the pinned build above, with a cartridge loaded, the emulator
stopped in its debugger. These are the facts the M1 implementation is built on;
anything here that stops being true should break a test rather than a comparison.

### Writing

| | |
|---|---|
| a whole region written and read back | identical; 128 KiB in 0.1 ms |
| a span written | the span arrives and the bytes either side of it are untouched |
| the processor state written and read back | identical, byte for byte, through an opaque buffer |

The processor state round-trip is the one that decided a design: handing back the
buffer the read filled reproduces the state exactly, so M1 carries it **opaquely**
and this project transcribes no register layout. Changing one byte of the buffer
and reading it back returns the change, so the round-trip is not a no-op. Of the
8 KiB buffer, 13 bytes were non-zero at the position measured — the record is
small and mostly zero, which is why over-allocating costs nothing.

### How big its state records are

Not documented anywhere, and not guessed. Measured by filling the buffer with
`0xFF`, reading the record, and noting the last changed byte; then again with
`0x00`. Bytes past the end held the filler in **both** cases — had the backend
written them they would have come back equal under both fillers, and they did
not. So these are exact, not lower bounds:

| record | bytes |
|---|---|
| the processor | **32** |
| the video hardware | 202 |

Only the processor's is used: it is what `read_processor` returns, opaquely. The
binding still hands the backend a buffer far larger than that and checks whether
anything past byte 31 was touched, so a backend whose record has grown is caught
at run time rather than by a truncated read nobody notices. §16.1's version
check would catch a version bump; that check catches a rebuild that kept the
version and moved the struct.

### Saving and loading the opaque blob

| | |
|---|---|
| save | 21 ms, a file of 210–280 KB |
| load | 25–60 ms |
| 600 frames replayed instead | **12.16 s** — the blob is about **200× faster** |

Four rules came out of it, and three of them are the kind that would otherwise be
found much later as a comparison nobody can explain.

**1. Saving advances the machine, and so does loading.** Both run on to the next
point the debugger can break at, which completes whatever instruction was in
progress. At a frame boundary there usually is one (§3.4). So:

- the position of a blob is the reading taken **after** the save, never before;
- read that way, a load reproduces it **exactly** — the same cycle count, the
  same program counter, three times over and in a different process;
- and a blob of a position is therefore one break-point *ahead* of the position
  itself. Replaying five frames landed on cycle 372 847; the blob saved there
  reads 372 848, with identical memory. Comparing "the state at the anchor"
  against "the state the replay reached" has to account for that, or compare
  memory only.

**2. The machine must be stopped before a blob is loaded.** Loaded while it is
running, the load happens and execution immediately overtakes it, so every
reading afterwards is of somewhere else. This is not a subtle failure in the
output — it looks like the blob did not work — but nothing says so.

**3. A load that fails is silent.** `LoadStateFile` returns `void`. Given a file
of nonsense, or a path with no file at it, the machine is left exactly as it was
and the call reports nothing. Success and failure are indistinguishable from the
call site, which is why §4.8's cheap check — position and a digest of the
declared regions — is not only an audit of the cache but the **only** way to know
a load did anything. Recorded as §13's Q12.

**4. Runs rooted in a blob are deterministic across processes.** A blob loaded in
a fresh process gave the same state as in the process that made it, and the same
bound run onward gave the same state again. `resume + 4 frames` and
`origin + 9 frames` arrived at the same cycle with the same memory. That is §2.5
satisfied for anything downstream of a blob — and it is what makes §4.7's anchors
sound.

**5. The position check cannot catch a load that did nothing when nothing has
moved.** Proven rather than suspected: deleting the call to the backend's load
from this project's own implementation leaves the position and the fingerprint
exactly as the blob recorded them — because the machine had not advanced since
the save — and both checks pass. What caught it was a test that disturbs the
machine first and then shows the disturbance gone. So the checks detect a load
that landed *elsewhere*, and a test of a load has to put the machine somewhere
it demonstrably is not before loading. §4.11's digest over an anchor's declared
regions is the general form of that.

**What this does not cover**: a blob belonging to *different software*. Testing
that needs a second ROM, which is what §11.3's generated fixture will provide.

## Reproducing the measurements

With a built library and software of your own:

```sh
AWASERU_TEST_BACKEND=/path/to/MesenCore.so \
AWASERU_TEST_SOFTWARE=/path/to/your.rom \
AWASERU_TEST_CONFIG_DIR=/path/to/a/directory/with/both/config/files \
cargo test
```

Without those, every test that needs a backend prints `SKIPPED` and the suite
still passes — §11.3's third route, which is how this project tests against
software it may not redistribute.
