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

### Breakpoints, and what they make possible

Transcribed from two sources that agree field for field — `Breakpoint.h` and
the interop struct its front end marshals — and `SetBreakpoints` takes a
**pointer and a length**, not a struct by value. That is what makes it
tractable where the configuration record was not.

| offset | field | |
|---|---|---|
| 0 | id | 32-bit |
| 4 | processor | one byte, then three of padding |
| 8 | memory | 32-bit |
| 12 | kind | 32-bit flags: read 1, write 2, execute 4, forbid 8 |
| 16, 20 | first and last address | 32-bit each |
| 24, 25, 26 | enabled, mark, ignore-dummy | one byte each |
| 27 | condition | a thousand bytes of text |
| | **total** | **1028 bytes**, aligned to four |

Three things were measured with it, and all three are what M3 was planned
around.

**An execution breakpoint stops exactly at the target.** Asked for `$8010` it
stopped at `$8010`; asked for `$8023`, at `$8023`. So a bound by address is
available, and M0's refusal of one can go.

**§4.4's budget needs no new mechanism.** Stepping *n* instructions with a
breakpoint active stops at whichever comes first: a budget of five with a
target twenty instructions away stopped five instructions in, and a budget of a
million reached the target. So an address bound is a step request plus a
breakpoint, and telling "arrived" from "ran out" is comparing the program
counter against the address asked for. It is a count and not a clock, which is
what §2.5 wants of it.

**A write breakpoint names the instruction that is writing.** This is §5.4's
third item, the one the specification says changes the developer's day, and it
is exact rather than approximate:

- the break happens **during** the write, before it commits: the byte still
  holds its old value, and one further instruction step is what lands the new
  one;
- the processor's own program counter has already moved past the instruction —
  `$8014`, where the store is a four-byte instruction at `$8010`;
- and `GetProgramCounter` asked for the *instruction's* counter returns
  **`$8010`**: the store itself.

So localisation is a write breakpoint and a replay, and the replay is what
anchors were built for.

### What the access counters give, and what they do not

`GetMemoryAccessCounts` fills a flat 40-byte record per address: read, write and
execute stamps, and a counter for each. For a byte the fixture had just written
once, the write counter was one and the stamp non-zero while its neighbours —
not yet written — were zero.

The stamp is in the backend's own clock, which is **not** the processor's cycle
count: at processor cycle 32 the stamp read 426. So stamps are comparable with
each other and not with anything else, which makes them a cheap way to ask
*when* a byte was last written and no way at all to ask *where* from. For where,
see the breakpoint above.

### What the write breakpoint and the counters gave when built on

M3's seventh unit built §5.4's localisation on the three measurements above, and
two more things came out of doing it rather than measuring it.

**Two breakpoints can be armed at once**, with distinct ids: a write breakpoint
on one memory and an execution breakpoint on the processor bus, in one
`SetBreakpoints` call. Which of them broke is told apart by the instruction's
own program counter — at the execution breakpoint it is the address asked for,
at the write it is the store. That is what lets §4.5 be part of the bound: a
localisation waits for a write *or* for the end of its subject, and whichever
comes first is what it reports. Measured on the fixture's routine, whose output
is written once inside the routine and once again by the instruction after the
return: bounded to the return, the answer is the routine's store; bounded six
bytes later, the answer is the clobber and the count is two.

**A debugger write leaves no write record.** Seeding a span through
`SetMemoryValues` does not move the access counters: a byte seeded that way and
never written by the software reads back `NeverWritten`. So the counters record
what the *software* did, which is what makes them a usable filter — a byte with
no record cannot have been written by the routine, and that answer costs no
replay. It also means the counters cannot be used to check that a seed landed;
reading the bytes back is what does that.

### One emulator per process, and what that costs

Every entry point this project uses takes no handle: `InitializeEmu`,
`LoadRom`, `GetMemoryState`, `SetMemoryValues`, `SetBreakpoints`, `Step`,
`GetCpuState`, `SaveStateFile`. They address the one emulator the library owns.
So two references in one process are two front ends to one machine, and
`Reference` holds a process-wide flag that refuses the second opening rather
than hand back something that looks like a second machine.

Three consequences, all of them live in this repository:

- **every integration test that opens a reference is one test in a file of its
  own**, because `cargo test` runs test binaries in parallel but the tests
  inside one binary share a process. Two such tests in one file fail with the
  refusal, which is how this was found in M3 rather than by reading;
- **§5.5's cross-check cannot be built on this backend** (§13's Q10). It needs
  two references that can disagree with each other;
- **"in a fresh process" is spelled by spawning the host**, which is what M2's
  done-condition does and what the route to §5.5 would build on.

### What this backend declares, and on what evidence

§7.3 says a backend states what it can do and the host asks before relying on
anything beyond §7.2's mandatory verbs. This is that statement for this backend,
with the evidence next to each entry so that the claim can be checked rather
than taken.

| capability | declared | evidence |
|---|---|---|
| `stop-on-execution` | yes | the measurement above, and `Bound::Address` is built on it — exercised on every run of the routine tests |
| `stop-on-write` | yes | the measurement above: the break lands during the write, before it commits |
| `writing-position` | yes | the measurement above: the instruction's own program counter at that break is the store |
| `write-recency` | yes | the measurement above: a per-address write stamp, in the backend's clock — and a debugger write leaves no record, so it reports the software's writes |
| `input-replay` | **no** | measured absent — no control device at any of the eight indices (§13's Q14) |
| `stop-on-read` | **no** | a route, not taken: the breakpoint record has a read flag and nothing here has used it |
| `execution-coverage` | **no** | a route, not taken: the access record has an execute counter and a stamp, measured only for writes |
| `call-and-return-events` | **no** | a route, not taken: `GetCallstack` is exported, and its `StackFrameInfo` is flat enough to transcribe — two address words, three address records of two fields each, and a flag word. Nothing here has read one |
| `register-writes` | **no** | needs the event viewer, which needs the nested configuration record this project has refused twice |

The distinction between the last four and `input-replay` is the one worth
keeping: `input-replay` is absent **in the machine**, and the other four are
absent **in this crate**. A route that exists and has not been taken is not a
declaration, because a host relying on one would be relying on this crate's
reading of a header rather than on anything that has run.

Three of the four now have a verb behind them: `Bound::Address` for
`stop-on-execution`, `Bound::Write` and `Platform::write_recency` for the three
§5.4's localisation is built from. The sentence below is what it replaced, and
is kept because the order it describes is the point: the declaration came first
and the verbs were written against it. The three declared without
one are what §5.4's localisation is built from, and declaring them is what lets
that be written at all — a host cannot ask for a capability nobody declares.

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

### What it puts in memory before the software does

**Work memory at power-on is filled pseudo-randomly, and not the same way
twice.** Two processes loading one image see different bytes everywhere the
program has not written.

This went unnoticed through all of M0 because software that initialises its own
memory hides it — the readings agreed across processes because the software had
overwritten everything by the time anything looked. It surfaced the moment there
was a fixture that does *not* initialise memory, which is one reason §11.3 wants
a fixture of our own.

Two things follow:

- **Nothing may be asserted about memory the software has not written.** The
  fixture writes sentinels around its own patterns for exactly this reason, so
  a test can check where a write landed without reading a byte the program left
  alone.
- **Determinism from power-on is not available here; determinism from a blob
  is.** That is not a small distinction. §4.7's anchors stop being only a way to
  make a run fast and become the way a run is made reproducible at all.

Recorded as §13's Q13, with what it would cost to turn off.

### A power-on position that is reproducible

There is one, and it is reached by loading the software **twice**:

1. load it, bring up the debugger, and step once so the debugger is in a break;
2. load the same file again.

The second load stops by itself at **cycle 0, at the reset vector, before one
instruction has run**. Measured on two different pieces of software and in
three separate processes: the cycle count, the program counter, the frame, the
line and the dot are identical every time.

Why it works, from the backend's own source: on loading, it breaks for one
instruction when the debugger both exists *and* was paused — and after the
first load and a step, both are true. The first load exists only to create
those conditions.

This is the root every anchor hangs from (§4.7), and it is what answers §13's
Q9.

### What is still random there, and what it takes to stop being

At that position the memories are filled pseudo-randomly, and the fill differs
between processes. Seven of the memory types the backend exposes can be written
and are not read-only, and **writing zeros to all seven at cycle 0 makes two
processes — and three — agree on everything afterwards**:

| | |
|---|---|
| zeroed | work memory, battery memory, video memory, the sprite table, the palette, the sound processor's memory, the sound processor's registers |
| after sixty frames, across three processes | every memory agrees; so do the processor record, the video record, the cycle count and the position |

Two details worth having:

- **Without zeroing, the timing was already deterministic.** The cycle count,
  the position, the processor record and the video record agreed across
  processes even with a random fill; only memory *contents* differed. So the
  non-determinism was the initial fill and nothing else — which is why zeroing
  is enough rather than merely helpful.
- **The sound processor's registers do zero.** Writing zeros to them leaves
  them zero, which was checked rather than assumed: the digest afterwards is
  the digest of 128 zero bytes.

This answers §13's Q13 — at a price. A real console has rubbish in its memory
at power-on, and software that reads it behaves differently. The divergence is
deliberate, it is declared in the shared configuration, it can be turned off,
and every report says which it was.

### What it will not let us do: press a button

**No control device exists.** The backend's own enquiry — which of its eight
input indices has a device — answers no to all eight, so an input override is a
state stored where nothing reads it. A fixture written to wait for a button
stays waiting with one set, which is how this was established rather than
inferred.

The default for each port is no controller, and the only way to change it is the
configuration record passed by value: ten controller configurations, each
holding a key-mapping set of thirty-one members. That is the same record whose
transcription was priced and refused for the power-on memory setting, for the
same reason — one field wrong silently changes the accuracy of the thing whose
job is to be the ground.

Recorded as §13's Q14, with the three routes that might settle it. The most
promising is the backend's movie playback, which replays input deterministically
and needs an archive format written rather than a C++ struct layout guessed at.

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

**5. The blob goes through a file, so the file's name has to be this process's.**
Found by the test suite running four processes at once against the same home
directory, which is the ordinary case because the home has a default: each was
writing the same temporary file between another's save and read-back, and each
got the other's machine. It appeared as an intermittent failure rather than a
wrong answer only because §4.8's cheap check caught it, which is the clearest
argument for that check there is.

**6. The position check cannot catch a load that did nothing when nothing has
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
