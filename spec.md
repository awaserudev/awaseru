# awaseru — specification

**Status**: draft. This document is normative: where it and the code disagree, the
code is wrong. Sections marked **OPEN** are decisions not yet taken, with what
would settle them.

`awaseru` (合わせる, Japanese: *to put two things together so that they match*) is a
verification harness for reimplementing console software. The name is the verb,
not a property: the tool does not hold the truth, it places two sides side by
side and says whether they agree.

---

## 1. What this is

### 1.1 The problem

Reimplementing a game natively — moving it from emulation to code — has two bad
options today. Either you write the reimplementation blind, and discover the
routine you did not know about weeks later when something looks wrong; or you
build verification infrastructure first, months before the first line of game
logic.

`awaseru` is the third option: the verification infrastructure, ready, giving an
answer at every step. The developer chooses a unit, writes it, asks whether it
matches, and fixes until it does.

### 1.2 What it does

It drives a reference emulator and compares its state against the state the
developer's reimplementation produces, at points the developer chooses, and
reports where they diverge.

### 1.3 What it is not

- **Not an emulator.** It drives one. The reference must be an existing,
  independently tested emulator; a reference written here would make this
  project's own bugs into "the truth" (§2.5, §7.1).
- **Not a framework.** It does not own the developer's code, its language, its
  build or its process (§8).
- **Not a decompiler.** It makes no claim about what the original code *means*.
- **Not a player.** Running a game for enjoyment is out of scope; where a
  feature serves only that, it is out.

### 1.4 The claim it makes, exactly

> These two agree on the data you asked about, at the point you asked about.

Nothing more. §5 exists to keep that claim honest, because the ways it can be
hollow are more numerous than the ways it can be wrong.

### 1.5 What the user brings

| | who supplies it | why |
|---|---|---|
| the tool | this project | |
| the reference emulator | the user | §11.1 |
| the ROM | the user | §11.1 |
| the mapping of the title | the user, or a community config | §2.1 |
| the reimplementation | the user | §1.3 |

---

## 2. Principles

These are not style. A change that violates one of them is a change to this
section first.

### 2.1 No title knowledge in the tool

No address, name, region layout or behaviour of any particular piece of software
appears in this codebase. That knowledge lives in configuration the user loads
(§6, §9). The tool is to a ROM what a disassembler is to a binary.

### 2.2 No silent agreement

Every comparison reports **how much of the compared data the reference itself
changed** over the interval. A comparison where the reference changed nothing is
reported as *vacuous*: the two sides agree about data neither of them touched,
which is not evidence. This is the most common way a verification tool lies, and
it lies by passing.

### 2.3 Three values, never two

Every verdict is one of:

- **agrees** — compared, and equal.
- **differs** — compared, and not equal, with §5.4's localisation.
- **not determined** — not compared, or compared without meaning. Causes:
  the backend does not expose the region (§3.5); two references disagree with
  each other (§5.5); the run did not reach the point (§4.3); the comparison was
  vacuous (§2.2).

"Not determined" is never collapsed into "agrees". A tool that cannot tell
"they match" from "I did not look" is worse than no tool.

### 2.4 Refusal over guessing

Where the tool cannot establish something, it refuses and says why. Being
blocked is a correct state for a value. No default stands in for a fact.

### 2.5 Determinism

The same inputs produce the same result: in one process, across processes, and
across machines. Where the reference emulator is not deterministic — a
pseudo-random memory fill at power-on is the usual case — the tool either makes
it deterministic or declares the affected state as not determined. It never
compares against a value that varies between runs, and it never reports a
*change set* against a varying baseline: what is deterministic is the final
value at the addresses the software wrote, not the set of addresses that changed.

### 2.6 One path

State is read and written through one mechanism. Where a second door would be
convenient, it is not added, because the second door diverges from the first on
some case and the divergence is found late.

### 2.7 The platform is opaque to the host

The host never names a platform-specific region. It asks the backend what
regions exist and addresses them by the names it is given (§3.1, §7). This is
what makes a second platform an addition rather than a rewrite.

### 2.8 Every claim carries its position

A divergence is reported with where it is, and — where the backend can supply
it — with what produced it. "Byte 3 differs" is a fact; "byte 3 differs, written
at this position" is an answer.

---

## 3. The state model

### 3.1 Regions

A **region** is a named, addressable span of bytes that a backend exposes:

- `name` — a string the backend chooses.
- `size` — in bytes.
- `access` — readable, writable, or both.
- `unit` — the addressable unit, where it is not one byte.

The host obtains the list by asking. It does not know, and must not assume,
which names exist. Fixed-field state models are the mistake this avoids: a model
with fields for one platform's memories cannot gain a second platform without
changing every comparison written against it.

### 3.2 Snapshots

A **snapshot** is:

- the contents of some set of regions,
- the processor state (§3.3),
- the position (§3.4),
- and the identity of the backend and ROM it came from (§6.6).

A snapshot need not be complete. A snapshot that omits a region is not a
snapshot that says the region is empty (§3.5).

### 3.3 The processor state is not optional

Registers, flags, the stack pointer, the program counter and any mode bits are
part of the snapshot and part of seeding. A comparison seeded without them runs
the developer's routine with some other routine's registers, and then reports
the difference as that routine's error. This is not a refinement; without it,
unit-level comparison does not work at all.

### 3.4 Positions

A **position** is where execution stands. A position declares its kind:

- a frame boundary,
- an instruction boundary,
- an address,
- or the end of a bounded run (§4.4).

**A frame boundary is not necessarily an instruction boundary.** A reference
sampled at the end of a frame is frequently part way through an instruction. A
snapshot taken there cannot be seeded into a reimplementation, because there is
no instruction to begin at. The tool therefore records which kind a position is,
and refuses to seed from a non-instruction boundary unless the caller asks for
that explicitly and accepts the result as not determined.

### 3.5 Absent is not equal

A region a backend does not expose is **absent**. Comparisons over absent
regions are *not determined* (§2.3). Optional state must be optional in the
type, so that "they agree" and "they were not compared" cannot be written the
same way.

### 3.6 OPEN — snapshots by value or by handle

A snapshot of a console's full state is of the order of hundreds of kilobytes.
Passing it by value across the API (§8) for every comparison is wasteful; a
handle the tool holds is cheaper but makes the client's state harder to inspect.

*What would settle it*: the first real client, and the measured cost of a
routine-level cycle. Until then the API carries both and the wire format does
not forbid either.

---

## 4. Execution

### 4.1 The primitive

Three operations, and everything else is built from them:

1. **seed** — place a state into the reference.
2. **run** — advance it, bounded (§4.2).
3. **read** — take a snapshot.

### 4.2 Bounds

Every run is bounded, and the bound is part of the request:

- to the next frame boundary,
- to an address,
- for *n* instructions,
- until a condition over state.

There is no unbounded run. "Run until it stops" is not a thing the tool offers,
because a run that does not stop is indistinguishable from a run that has not
finished.

### 4.3 Stop reasons

A run reports where it stopped and why: the bound was reached, an address was
hit, the budget was exhausted, the backend refused, or the backend reached a
state it cannot continue from. The reason is a result, not an error.

### 4.4 Budget

Every run carries a step budget. Exhausting it is a stop reason (§4.3), not a
failure. A count, not a timeout: the same run must stop the same way every time
(§2.5).

### 4.5 A measurement must not run past its subject

A run started to measure one routine must be bounded to that routine. If it is
allowed to continue, the next routine writes over the data being compared and
the comparison silently becomes a reading of something else. This is a rule
about how the tool is *used*, so the tool makes the bound mandatory (§4.2)
rather than trusting the caller to remember.

### 4.6 Reaching a point is the tool's problem, not the client's

Arriving at frame *n* from power-on is expensive and every client needs it. The
tool owns savestate caching and input logs, and a client asks for a position
rather than for a replay. A client that has to invent its own caching will
invent a different one per client.

§4.7 to §4.12 are what owning it requires.

### 4.7 Anchors

An **anchor** is a named position worth returning to. It has three parts:

- a **name**, which appears in configuration and in reports;
- a **definition** of how to reach it — from power-on, or **from another
  anchor** — as a bound (§4.2) plus, where the software needs input before it
  will proceed, a recorded input log;
- a **cached state blob**, backend-opaque (§7.2), taken there.

A client asks for an anchor by name. The tool arrives by loading the blob when
it has a valid one (§4.11) and by replaying the definition when it does not.
Which of the two happened is reported (§4.12) and must never change the result.

**Anchors compose.** An anchor whose definition begins at another anchor pays
the expensive prefix once, and every anchor downstream of it is cheap to add.
This is what makes the mechanism general instead of a special case for one long
opening: *anywhere* a position is reached by replaying something already
replayed, an anchor is the answer.

An anchor is a **blob and not a snapshot** (§3.2), and the difference is not
incidental. A blob restores the whole machine, including whatever the reference
was in the middle of. A structured snapshot cannot be seeded at a position that
is not an instruction boundary, and a frame boundary frequently is not one
(§3.4). The two are different tools: a snapshot is for comparing, and for
seeding a routine; an anchor is for *arriving*.

### 4.8 An anchor is not trusted until it has been shown equivalent

Resuming from a cached blob instead of replaying trades time for a risk, and it
is the worst-shaped risk this tool has: if the cached state is not the state the
replay would have produced, then every comparison downstream of it measures the
wrong machine — **and passes**.

So an anchor carries a demonstration, and until it has one, comparisons made
from it are *not determined* (§2.3) rather than trusted:

1. the definition is replayed from its origin **three times**, and the state at
   the end agrees with itself every time. An anchor whose derivation is not
   deterministic is not an anchor (§2.5);
2. a run started from the cached blob produces the same state as a run that
   arrived by replaying;
3. both are recorded against the identity the blob is keyed by (§4.9).

The demonstration is expensive, and it is run **once per anchor per key** — not
once per comparison. What runs on every load is the cheap check: the position is
the one recorded, and a digest over the regions the anchor declares is the one
recorded. A mismatch throws the blob away and replays.

Worked through, which is also the shape of the test that proves it:

1. replay the definition from its origin, and record the state. Twice more, and
   the three agree — the definition is deterministic;
2. take the blob there and cache it;
3. in a fresh process, resume from the blob. The state agrees with the three;
4. run the same bound onward from each: from a replay and from a resume. They
   arrive at the same place with the same state. **This is the one that matters**
   — agreeing at the anchor is not the same as agreeing after running on from
   it, because a blob can restore the memories an anchor declares and still
   leave something it does not declare in a different state;
5. delete the cache. Everything above still passes, and only the clock changes
   (§4.11).

### 4.9 How much verification, and how often

Both numbers are the user's, because the trade is theirs: replaying from the
origin is the expensive thing the anchor exists to avoid, and how much of it to
keep paying for assurance depends on how much they trust the ground.

```toml
[anchors]
# Replays of the definition before an anchor is trusted (§4.8). Three is the
# default. Zero means never demonstrated — permitted, and every verdict made
# from that anchor says so.
verify_from_origin = 3

# After this many uses, replay from the origin again and check the blob still
# produces the same state. Zero means never.
reverify_after = 50
```

**Why re-verify something whose key has not changed.** The cheap check covers
only what the anchor *declares* it covers. Something outside that set can drift
and never be noticed — and §4.8's whole point is that this failure passes
instead of complaining. Periodic re-derivation is the audit of the cheap check,
and it is the same idea as §16.5 pointing the tool's own comparison machinery at
its own dependency: the cheap thing is trusted because an expensive thing checks
it on a schedule, not because it is believed.

**A verdict records how well verified its anchor was**: the anchor's name, how
many replays it was demonstrated against, and how many uses ago. Turning the
numbers down is allowed; being quiet about having turned them down is not. A
result that reads "agrees" from an anchor nobody ever demonstrated is a result
whose foundation the reader cannot see, which is §2.3's mistake wearing
different clothes.

### 4.10 Using anchors without getting lost

Guidance rather than rules, and written down because the mechanism has a failure
mode that is comfortable to live with for a long time.

- **Anchor at a position you can describe in words.** "The software accepts
  input" is an anchor. "Frame 18 400" is a number that will mean something else
  after any change to anything.
- **Declare the regions your comparisons read.** The cheap check digests what
  the anchor declares (§4.8), so an anchor that declares nothing is checked for
  nothing, and an anchor that declares the regions you actually compare catches
  a stale blob on the load before it can produce a wrong verdict.
- **When a comparison starts failing in a way that makes no sense, delete the
  cache first.** §4.11 guarantees that this costs only time. It is the cheapest
  diagnostic the tool has, and it is cheap precisely because the cache is never
  an input.
- **Build long chains out of short links.** An anchor defined on top of another
  (§4.7) is re-derived only from its parent, so a chain of five cheap anchors
  recovers from an invalidation far faster than one anchor defined from
  power-on.
- **Do not anchor what is already cheap.** An anchor has a cost of its own — a
  demonstration, a key, a cache entry to be wrong about. Positions a few
  thousand instructions from somewhere you already are do not need one.

### 4.11 What an anchor is keyed by, and when it is thrown away

An anchor's blob is keyed by:

- the backend's name and version (§16.1);
- the software's identity (§6.6);
- the anchor's own definition, the input log included, and the anchors it is
  defined on top of.

Any of those changing invalidates the blob. A behaviour change in the reference
is a new reference (§16.5), so a blob produced by one version is not a blob for
another — and a blob is the one artefact where using a stale one is **invisible**
rather than noisy.

**Blobs are a cache, never an input.** Deleting the entire cache must change
nothing except how long a run takes. That property is what makes the cache safe
to be wrong about, and it is worth more than any amount of care in invalidating
it.

### 4.12 Waiting is reported, not hidden

Every run says how it arrived — replayed or resumed — and how long that took.

This is not a convenience, and it is in the specification because of a measured
failure. On the project this tool grew out of, a verification loop ran for about
thirty hours without finishing, and the great majority of that time was spent
replaying an opening sequence that takes minutes before the software will accept
input — once per comparison, hundreds of times. Nothing in the output said so.
The work looked slow rather than wasteful, which is why it went on for thirty
hours.

A tool that hides where its time goes cannot be made faster by the person using
it, because they cannot see what to fix. So: wherever a position can be reached
by resuming rather than by replaying, the tool is expected to do so, and to say
which it did.

---

## 5. Comparison

### 5.1 The verdict

Three values (§2.3), per comparison, over a named set of regions or a span
within one.

### 5.2 Movement

Every comparison reports `moved`: how many of the compared bytes differ between
the **seed** and the **reference's own result**. `moved == 0` makes the
comparison vacuous (§2.2) and the verdict *not determined*.

This number is the difference between a measurement and a decoration, and it is
reported always, not on request.

### 5.3 Controls

The tool offers **perturbation**: run the reference again with a named input
changed, and report whether the comparison noticed. A comparison whose
perturbed form gives the same verdict cannot discriminate, and the tool says so.

A measurement without a control that varies is incomplete. The tool cannot force
the client to run one, but it can record that none was run, and it does.

### 5.4 Localisation

On *differs*, the report carries:

- the first differing offset, and the two values;
- the count of differing bytes;
- where the backend supplies it (§7.3), the **position that last wrote** that
  byte in the reference.

The third item is the one that changes the developer's day. "Your byte is one
too low" sends them reading; "the write at this position did not happen" is the
answer.

### 5.5 Cross-check

Where the configuration names more than one reference for a platform (§6.4),
the tool may run them together. If they disagree with each other, the verdict is
**not determined**, and the report names both references and where they parted.

This is a statement no single emulator can make about itself, and it is the
honest answer when the ground is not solid.

### 5.6 Granularity

**Routine-level comparison is the primary unit.** Seed a state, run one routine,
compare the data that routine touches. Frame-level comparison is a special case
of the same primitive with a frame boundary as the bound.

The order matters because the reverse does not work: a frame runs hundreds of
routines, and a single wrong byte early cascades until the report is a large
number with no information in it. Frame-level agreement is a milestone, not a
daily tool.

---

## 6. Configuration

### 6.1 Two files

| file | committed | contains |
|---|---|---|
| `awaseru.toml` | yes | the platform, the ROM's identity, mapping patterns, which reference to use **by name** |
| `awaseru.local.toml` | no | where each named emulator and the ROM are **on this machine** |

The principle: **the shared file names things and declares invariants; the local
file says where things are.** A configuration meant to travel in a repository
cannot carry absolute paths.

### 6.2 Resolution

The local file overrides the shared one per key; the last file read wins. This
is the `.env` / `.env.local` convention.

With one exception: **overriding an invariant is refused, not applied.** The
ROM's hash (§6.6) is an invariant; overriding it locally would annul the reason
it exists. The tool reports the attempt and stops.

### 6.3 Mapping patterns

```toml
[mapping]
files = [
  "map2/ram.toml",        # one file
  "map/*",                # one level of map/
  "map/events/**",        # map/events/ and below
  "!map/routines.toml",   # excluded
  "!map/draft/*",         # excluded, a set
]
```

1. Paths resolve against **the directory of the file they are written in**,
   never the working directory.
2. `*` does not cross `/`; `**` does. One level is the default, so
   subdirectories are opt-in, per pattern rather than per configuration.
3. A **glob** matches only `*.toml`. An **explicit path**, with no wildcard,
   matches that file whatever its name. A stray `README.md` inside a mapping
   directory is therefore not a candidate, which is different from being
   silently skipped.
4. **Exclusions always win, regardless of order.** There is no re-inclusion.
   Predictability beats power in a file that several people compose.
5. **A pattern that matches nothing is an error.** This is the rule that earns
   its place: a typo in an exclusion, under any other semantics, does nothing
   and is never noticed.
6. An empty final set is an error.
7. Expansion is sorted lexicographically. Directory order is not stable across
   filesystems, and without sorting the same configuration loads differently on
   different machines (§2.5).
8. Nothing resolves outside the configuration's own tree, and symbolic links out
   of it are not followed. A shared configuration must be safe to run.

### 6.4 Emulators

```toml
[[emulator]]
name = "ref-a"
platform = "snes"
backend = "some-backend"

[[emulator]]
name = "ref-b"
platform = "snes"
backend = "other-backend"

[reference]
use = "ref-a"
crosscheck = ["ref-b"]      # optional, §5.5
```

- `name` is distinct from `backend` because two builds of the same backend — a
  patched and an unpatched one, or two versions — must be distinguishable, and
  the name is what appears in reports.
- `use` is explicit rather than positional, so that appending an emulator to a
  shared configuration does not change its meaning.
- The paths live in the local file (§6.1), keyed by `name`.

### 6.5 Startup verification

A backend is a contract, not a label. At startup the tool asks the backend which
regions and capabilities it exposes (§3.1, §7.3) and compares that against what
the configuration and the loaded mapping require. A shortfall is refused with
the list of what is missing.

The alternative — discovering three weeks later that one region was never
actually compared — is the failure this prevents.

### 6.6 ROM identity

The shared file carries the ROM's hash; the local file carries its path. A
mismatch is refused. Pointing the tool at a different revision of the software
makes every comparison meaningless, and meaningless comparisons that pass are
worse than a stopped run.

### 6.7 Anchors

Anchors (§4.7) are declared in the shared file: a name, how to reach it, what it
covers, and the verification policy of §4.9. They are things the project names,
so they travel with it.

What does **not** travel is the cache. The blobs are machine-local, and they are
not configuration at all — they are a derived artefact keyed by §4.11, and
§4.11's rule is that deleting the lot changes nothing but the clock. A
configuration that pointed at a shared cache of blobs would be sharing the one
artefact where a stale copy is invisible.

The keys, as built:

```toml
[anchors]
verify_from_origin = 3      # §4.9; 0 means never demonstrated
reverify_after = 50         # §4.9; 0 means never

[reference]
use = "ref-a"
start_at_power_on = true    # the reproducible position; the default
zero_memory = true          # the default, and a divergence from the hardware

[[anchor]]
name = "accepts-input"
# `after` omitted means power-on. One key rather than a reserved value, so a
# configuration does not read two ways the day somebody names an anchor
# `power-on`.
frames = 600                # or `instructions`, or `address` in hexadecimal —
                            # exactly one, because a default would be a number
                            # nobody chose and two would mean whichever was read
                            # first. An `address` also needs `within = N`: §4.4
                            # says every run carries a budget, and an address is
                            # the first bound that can fail to arrive
covers = ["work-ram"]       # §4.8's cheap check digests these

[[anchor]]
name = "in-the-second-area"
after = "accepts-input"
frames = 300
covers = ["work-ram", "palette-ram"]
```

A configuration that cannot work is refused when it is read, not halfway
through a replay: a circle, a parent nobody declares and two anchors of one
name all stop the load. An anchor naming an input log is the exception — it
loads, because the configuration is right and the tool cannot replay one yet,
and asking for *that* anchor is what refuses (§4.7).

---

## 7. The platform boundary

### 7.1 Shape

Platform support is a Rust trait, implemented by one crate per platform, linked
into the host and **selected by name at runtime**. There is no dynamic loading
of Rust code across an unstable ABI, and no need for one: the user experience of
"load the platform you want" is a registry lookup.

Inside a platform crate, the reference is an existing emulator, driven as a
library or as a child process. The platform crate translates that emulator's
state into §3's model. It does not emulate (§1.3).

### 7.2 The verbs

The trait's surface, in the order it was arrived at:

- enumerate regions (§3.1);
- read a region, or a span of one;
- write a region, or a span of one;
- read the processor state; write the processor state (§3.3);
- run, bounded, returning a stop reason (§4.2, §4.3);
- save and load a backend-opaque state blob (§4.6).

And, as declared capabilities (§7.3):

- callbacks on read, write and execution over an address range;
- execution coverage;
- call and return events;
- register writes with their position within a frame.

### 7.3 Capabilities are declared, not assumed

A backend states what it can do. The host asks before relying on anything beyond
§7.2's mandatory list, and a comparison that needed an absent capability is
*not determined* (§2.3), never silently weaker.

### 7.4 The crate layout

Three crates, and three is the minimum rather than a preference — the dependency
arrows force it:

| crate | kind | holds | depends on |
|---|---|---|---|
| `awaseru-core` | library | the platform trait, the state model, the execution primitive, the differ, the configuration | — |
| `awaseru-<platform>` | library | one backend, implementing the trait | core |
| `awaseru` | **binary** | the host: configuration, backend registry, the external API | core, and each backend it registers |

A backend must see the trait in order to implement it. If the trait lived in the
binary, the backend would depend on the binary — and the binary must depend on the
backend to register it. The third crate is what breaks that cycle.

`awaseru` is the name published and the thing a user installs; the others are its
parts.

### 7.5 Backends from outside

Two things are easily confused, and only the second is closed.

**A third party can write a backend as their own crate, today.** They publish
`awaseru-<platform>` depending on `awaseru-core`, which is an ordinary Rust
dependency and needs nothing from this project beyond `awaseru-core` being
published. Whoever builds the host adds it as a dependency and registers it.

**A pre-built binary cannot load one at runtime.** That would need either a
stable ABI across the boundary, which Rust does not have, or backends as external
processes speaking a protocol — which buys any language and costs a second
protocol, serialization on the hot path of routine-level comparison, and a design
with no users to shape it.

Three things keep the first open and cost nothing today, so they are decided now
rather than designed later:

- `awaseru-core` is published, so a backend can depend on it without vendoring;
- the trait lives in `awaseru-core`, never in the binary (§7.4);
- the backend registry is a table from name to constructor, so adding one is a
  registration rather than surgery.

The language requirement is also milder than it first appears: an adapter mostly
marshals an emulator's own C API into the trait, and the real work of adding a
platform is **instrumenting that emulator**, which is C++ work whatever language
the adapter is in.

*Left open* (Q8): runtime loading into a pre-built binary. *What would settle
it*: someone with an emulator worth having who cannot rebuild the host.

### 7.6 OPEN — the exact trait

The signatures are not fixed here, and deliberately: an abstraction over two
platforms built while only one exists is a guess. Two decisions **are** taken
now, because they are cheap today and a rewrite later:

- regions are named and enumerated, never fixed fields (§3.1);
- no platform name appears in the trait or the protocol (§2.7).

*What would settle the rest*: the second platform, with a real backend behind it.

---

## 8. The external API

### 8.1 Who drives

**The client spawns `awaseru`.** The developer runs their own test suite and
debugger as they normally would, and the tool is a subprocess they control, like
any other fixture. The reverse would make their debugging workflow hostage to
the tool's lifecycle.

### 8.2 Transport

Messages over the child process's standard input and output. No ports, no
listening sockets, no firewall or permission dialog, and identical behaviour on
every platform the host runs on. A socket mode may be added when something needs
several clients at once; it is not the first door.

### 8.3 Framing

A length-prefixed **JSON envelope** carrying the command or result, optionally
followed by a length-prefixed **binary payload** carrying state.

JSON for the control plane because every language reads it and a human can debug
it. Binary for state because a snapshot is of the order of hundreds of kilobytes
and encoding that as hexadecimal inside JSON, thousands of times per session,
makes the arbiter the bottleneck.

The data model is the contract; the encoding of a payload is an implementation
detail that may change behind a version (§8.6).

### 8.4 Bindings

- For a client in the host's own language: a crate, calling in process, no IPC.
- For every other language: the subprocess and the protocol.

Same semantics, two bindings. The fast path exists without closing the open one.

### 8.5 The vocabulary is the configuration's

Region names and symbol names in the API are the names the backend and the
loaded mapping supply (§3.1, §9.1). There is no second naming scheme to learn,
and the API's surface is documented by whatever configuration is loaded.

### 8.6 OPEN — versioning and negotiation

How the protocol version is agreed, and how a client discovers the tool's and
the backend's capabilities, is not specified.

*What would settle it*: the first client written by someone who did not write
the tool.

---

## 9. Mapping and provenance

### 9.1 Symbols

A mapping entry has: a name, a location (a region and an offset, or an address),
zero or more groups, a description, relations to other symbols, and a
**provenance** (§9.2). Groups may nest and may relate to other groups: the
mapping is a graph with tags, not a flat list, so that a concept scattered
across a binary can be navigated as a concept.

The description is read by the API, so that a consumer — human or program —
obtains the context without reconstructing it from raw code.

### 9.2 Provenance is mandatory

Every symbol records how it was established. This serves two purposes, and the
second is not obvious:

- **Epistemic.** A value established by measurement and a value someone
  remembered are not the same value, and a mapping that cannot tell them apart
  decays. Entries with weak provenance are marked as hypotheses and are not
  treated as fact.
- **Distribution.** The provenance field is the audit trail for where the
  mapping's content came from. A mapping is only safely shareable if its
  contributors can say how each entry was established; an entry derived from
  material that may not be redistributable is identifiable rather than mixed in.

Contribution policy, stated once and enforced by review: nothing derived from
leaked source material is accepted.

### 9.3 Validation

- The files parse.
- No two symbols share a name, including across separately loaded files.
- Group references resolve to groups that exist.
- Locations fall inside a region the backend exposes (checked at §6.5).

### 9.4 Deferred

The structured mapping system is a later milestone (§12, M7). What exists from
the start is the **door**: the tool loads mapping from external files the user
supplies, and holds no mapping of its own. A plain-text provenance convention is
the prototype of the structured format, and converts into it.

---

## 10. Reverse-engineering features

These are what make the tool worth using beyond verification, and they are
declared capabilities (§7.3), not assumptions.

- **Write provenance** — for each byte, the position that wrote it. Turns
  "search the disassembly for what writes this address" into a query.
- **Execution coverage** — which bytes of the software executed under this
  input. This is the structural answer to "there is always a routine I did not
  know about": it says what has not been seen yet.
- **Call and return events** — the call tree of an interval.
- **Register writes with position** — most register writes on a console happen
  while the picture is being drawn, so end-of-frame register state does not
  reconstruct a frame. The position is part of the datum.
- **First-divergence localisation** — §5.4, which depends on write provenance.

---

## 11. Distribution and posture

### 11.1 What is never shipped

No ROM, no emulator, no mapping of any particular title. The user brings all
three (§1.5). The tool's position is that of a disassembler: a neutral
development instrument that contains none of the material it is pointed at.

### 11.2 No title is named

No commercial title appears in any file, test, fixture, path, message or commit
message of this project. Public material — documentation, examples, the site —
uses homebrew software only.

### 11.3 The public test suite proves the tool with material it may contain

Expected values from software this project may not redistribute cannot appear in
its tests. Therefore:

- **generated test ROMs** are the primary fixture: small programs assembled
  here, whose behaviour is defined completely, which can also exercise cases a
  real title may never reach — a frame boundary inside an instruction, a
  transfer with a fixed source address, a per-scanline register write;
- **homebrew** provides the end-to-end example and the tutorial;
- "bring your own ROM" tests read a path from the environment and take their
  expectations from configuration, and skip when unset.

### 11.4 OPEN — licence

The intent is a strong copyleft licence for the tool and a permissive one for
mapping configurations, which are data rather than program and should circulate
without friction. Not fixed here: the project is private, and a dependency taken
in the meantime can constrain the choice.

*What would settle it*: the first intent to publish, with the dependency set
known.

### 11.5 Releases are signed

Checksums and signatures, with the key published through more than one channel.
A tool whose premise is verification distributes itself verifiably.

### 11.6 BEFORE ANYTHING IS PUBLISHED, RUN THE LEAK CHECK

> ## ⚠ `tools/leak-check.sh` — every push, every release, every crate
>
> **Not before the first one. Before each one.** §11.2 is a property of
> everything that leaves this machine, and it has already been broken once.

What broke it, so that the next person does not have to find out the same way:
**two compiled Python files were committed, and Python embeds the absolute path
of the machine that compiled it.** The source named nothing. The build artefact
named the software *and* a local user, in a repository whose entire posture is
that neither appears. The check that was supposed to catch it was a `grep -rni`
read from the top — and `grep` prints no matching line for a binary file, so it
reported "clean" for two months of commits it had never looked inside.

A check with a hole in it is worse than no check, because it is produced as
evidence.

So the check is a committed script and these are its rules:

- it searches every **tracked file as binary**, not as text;
- every **commit message**, across every ref;
- and **every blob in history**, which is where the bytecode hid;
- the words it looks for are **not in this repository** — writing them into a
  committed file would be the leak itself — they come from a local
  `.private-words`, and **without that file the script fails** rather than
  passing while checking nothing.

Three things that are true about publishing and are easy to get wrong:

- **A force-push does not remove anything from a hosted repository.** The
  commits stay reachable by their hash until the host's own collection runs, and
  the host's event feed keeps the hashes for months. The only reliable removal
  is deleting the repository and pushing a rewritten history to a fresh one.
- **crates.io is permanent.** A published version cannot be withdrawn, only
  yanked, and a yanked version is still downloadable. What goes in a crate is
  decided *before* `publish`, by `cargo package --list`.
- **Generated files are the dangerous ones.** Bytecode, object files, coverage
  data, build logs, editor caches: each one may embed a path, a user name or a
  hostname. `.gitignore` is the first line and the leak check is the second,
  because the first one is a list somebody has to remember to add to.

---

## 12. Milestones

Each milestone has a done-condition that can be run, not judged.

### M0 — Walking skeleton

The host reads a configuration, selects a platform backend, drives a reference
to a position, and prints one region's bytes.

*Done when*: software supplied through the machine-local configuration runs and
one region's contents come out, with the test asserting the plumbing rather than
anything about the software. No differ, no API.

*How it was met*: by §11.3's third route — a bring-your-own-ROM test that reads
its paths from the environment and takes its expectations from configuration,
and skips when unset. The generated fixture of §11.3's first bullet is still
owed, and M3's done-condition is where it becomes unavoidable: a deliberately
wrong reimplementation has to be wrong about *something defined here*.

### M1 — The state model

Regions enumerated by name (§3.1). Snapshots with processor state and position
(§3.2–§3.4). Absent distinguished from equal (§3.5).

*Done when*: a snapshot round-trips — read, seed, read again, identical bytes —
and a comparison over an unexposed region reports *not determined*.

*How it was met*: both, twice — against the generated fixture of §11.3, where
the assertions are about content because the content is this project's, and
against software supplied through the machine-local configuration, where they
are about plumbing (§11.2). The round trip disturbs the machine between the two
reads, because read-seed-read passes with a seed that does nothing.

What M1 also established, and did not set out to: **determinism from power-on is
not available on the first backend** (§13's Q13), so §4.7's anchors are not an
optimisation but the mechanism by which a run becomes reproducible. M2 inherits
that.

### M2 — The execution primitive

Seed, bounded run, read (§4). Stop reasons. Anchors and their cache
(§4.6–§4.12).

*Done when*: the same run from the same seed produces the same stop reason and
the same state three times, in three separate processes (§2.5) — **and an
anchor's demonstration passes end to end** (§4.8), as a test that runs:

1. the definition replayed from its origin, as many times as `verify_from_origin`
   says, agreeing with itself every time;
2. a run resumed from the cached blob, in a fresh process, agreeing with those;
3. **the same bound run onward from a replay and from a resume, arriving at the
   same position with the same state** — the step that catches a blob which
   restores what the anchor declares and leaves something it does not;
4. the cache deleted, everything above still passing, and only the clock
   different;
5. and `reverify_after` honoured: after that many uses, the tool replays from the
   origin of its own accord and says it did.

*How it was met*: in two tests, because the five steps do not all live in one
process. Steps one, three, four and five run as code the tool itself runs
(`demonstrate`), against the generated fixture of §11.3 where the assertions may
be about content. Step two's "in a fresh process" is a separate test that
**spawns** the host — three children with empty caches replaying, agreeing on a
state digest to the byte, and a fourth resuming a blob one of them wrote and
agreeing with it. Measured through the host against supplied software: 4.380
seconds replayed against 0.010 resumed.

What M2 established that it did not set out to:

- **A reproducible power-on exists** (§13's Q9, now answered). Loading the
  software twice stops at cycle zero at the reset vector, before one
  instruction.
- **Zeroing the seven writable memories there makes runs repeat** (Q13, now
  answered) — including two memories this project does not model as regions,
  one of which was the last thing still differing. The cost is a declared
  divergence from the hardware, which every report states.
- **Input logs are not reachable on this backend** (Q14). It exposes no control
  device for an input to arrive at, so an anchor behind software that waits for
  input is refused rather than reached some other way. The gated fixture that
  would prove otherwise exists and waits.

### M3 — The differ and the honesty rules

Verdicts, movement (§5.2), perturbation (§5.3), localisation (§5.4).

*Done when*: a deliberately wrong reimplementation of a generated ROM's routine
is caught with the first differing offset named; a vacuous comparison is
reported as vacuous; and a perturbation that should be noticed is noticed.

*How it was met*: against the generated fixture of §11.3, in one test, with each
of the three claims paired against the case a vacuous differ would pass.

- **The wrong reimplementation**, twice. One that is right about the output's
  first byte and wrong about the second, so "the *first* differing offset" is a
  claim and not a constant; and one that is wrong from the first byte. Both
  offsets, both values and the count come from the fixture's own Rust versions
  rather than from numbers written down. And the right implementation agrees,
  without which a differ that always differs passes everything.
- **The vacuous comparison**: a span the routine never writes, with a candidate
  that matches it byte for byte. Every compared byte equal, and the verdict *not
  determined*. The mutation that removes the check prints "agrees over 64 bytes,
  of which the reference moved 0", which is §2.2's lie-by-passing in the tool's
  own words.
- **The perturbation**, both ways. Changing the first input byte moves the
  verdict; changing the byte just past the input the routine reads leaves it
  identical, and the tool reports that it cannot discriminate it. Neither
  changes the verdict itself: a control speaks about the comparison's
  sensitivity and does not withdraw a finding.

What M3 established that it did not set out to:

- **A write breakpoint names the writing instruction exactly** (§5.4's third
  item). The break lands *during* the store, before it commits, and the
  instruction's own program counter is the store. So the answer to "what wrote
  this byte" is a position §3.4 forbids seeding from, which the report says in
  those words.
- **§4.5 is a property of the bound, not advice to the caller.** `Bound::Write`
  carries the end of the subject as well as the byte, because a bound that
  waited only for a write runs through the return when the write does not come
  again — and the fixture, whose routine's output is written once inside it and
  once by the instruction after its return, makes the difference between the two
  answers visible rather than theoretical.
- **A debugger write leaves no write record**, so the access counters report
  what the software did. That is what makes them a usable filter for §5.4 and
  what means they cannot be used to check that a seed landed.
- **§2.5 can be checked at the level of a verdict, for free.** §5.3's control
  measures the plain comparison again, so a report holds two readings of one
  measurement; when they disagree, the reference disagrees with itself and
  neither reading is evidence.
- **§5.5's cross-check is blocked, and the blockage is the architecture** (Q10).
  It is the one requirement of §5 that M3 did not build. Two references as two
  child processes is the route M2's process-spawning points at, and it is an
  architecture rather than a patch, so it is recorded rather than half-built.

### M4 — The external API

Framing (§8.3), stdio transport (§8.2), in-process binding (§8.4).

*Done when*: a client written in a language other than the host's drives a full
cycle — seed, run, compare — without linking the host.

*How it was met*: by a Python client in `clients/python/`, standard library only,
which spawns `awaseru serve` itself (§8.1 — the client drives), seeds a routine's
inputs, runs it, compares a reimplementation written in Python against the
reference, reads back what the reference produced, and runs §5.3's control. It
links nothing of this project: no crate, no header, no shared library.

Two numbers in its summary are the tool's and cannot be the client's, which is
what keeps the test from being one a hollow protocol passes. The client is told
where its output span begins and nothing about where it is wrong; the **first
differing offset** comes back from the comparison, and the wrong implementation
used here is right about the output's first byte, so the offset is one past it.
And the **instruction that wrote the reference's value** comes from a replay with
a write breakpoint; the client never sees the program. Both halves are tested,
because either alone is passed by a stub: the right implementation agrees and the
wrong one differs.

What M4 established that it did not set out to:

- **The emulator writes to standard output, which is where §8.2 puts the
  protocol.** So the reference runs in a child process and the server keeps a
  clean pair of streams. The route not taken — `dup2` on the server's own output
  — would have cost no new dependency, because `libc` is already in the lock
  file; it was refused for needing `unsafe` outside the backend's `ffi` (§17.1)
  and because a child that can crash, hang or be killed without taking the
  protocol down is the more robust shape.
- **A tool whose failure mode is "no output" is worse than one that says what it
  waited for.** A mutation that pointed the answers at the emulator's stream did
  not make the tests fail, it made them hang — so the parent reads frames on a
  thread of its own and gives the channel a deadline. §4.2's "no unbounded run"
  is just as true of waiting for somebody else's run.
- **A panic must not be able to look like an answer.** Measured: 213 bytes of
  English in the answer channel without a hook, 13 with one, and the exit status
  still naming the panic either way.
- **The transport is not the cost** (§3.6's Q1, answered): 79.0 ms over the wire
  against 79.7 ms in process for the same cycle, a whole region by value at
  1.4 ms, and framing 128 KiB at 0.10 ms. §5.4's localisation, by contrast,
  doubles a cycle, because it is a second full replay.
- **A difference needs its region on the wire** (Q16, answered). In one process
  it is recoverable while the comparison is in hand; on a wire it is not
  recoverable at all, and taking it from §5.4's optional localisation left a
  client that did not pay for a replay with an offset it could not place.

### M5 — Dogfooding

Reimplement a routine of a homebrew program through the API, as a user of the
tool rather than its author.

*Done when*: the cycle — choose a unit, write it, compare, fix until it agrees,
record provenance, commit — runs end to end without touching the tool's
internals. Anything that forces a change inside the tool is a finding, and is
recorded as one.

### M6 — Reverse-engineering features

§10, as declared capabilities.

*Done when*: a write to an address can be attributed to the position that made
it, and coverage distinguishes executed from unexecuted ROM, on the generated
fixtures.

### M7 — The structured mapping system

§9.1–§9.3.

*Done when*: a mapping split across several files loads as one graph, duplicate
names across files are refused, and the API reports a divergence by symbol name
rather than by address.

---

## 13. Open questions

| id | what is open | what would settle it |
|---|---|---|
| Q1 | **ANSWERED in M4, by measurement: carry both, and the by-value route is affordable.** §3.6's worry was hundreds of kilobytes crossing for every comparison. Measured on one machine in a debug build, five rounds each: a routine-level cycle costs **79.7 ms** in process and **79.0 ms** over the wire — the same number within noise, because both are dominated by the emulator running the routine — while a whole region by value (131 072 bytes) costs **1.4 ms**, about 1.3% of a cycle, and framing that many bytes with no backend at all costs **0.10 ms**. So the transport is not the cost, and a handle would save about one percent of a cycle in exchange for the client not being able to look at the bytes. Both routes stay in the protocol as built: `examine` holds the states and returns only a verdict, `read` brings the bytes. The numbers and the table are in `doc/protocol.md`. One thing worth keeping from taking them: §5.4's localisation **doubles** a cycle, because it is a second full replay — which is why it is asked for and not always done. | settled for this backend and this shape of client. What would move it again: a client doing frame-level work, where a cycle is cheap and a state is the same size, or a backend whose own measurement is fast enough for the transport to matter |
| Q2 | The platform trait's exact signatures (§7.6) | the second platform, with a real backend. **Narrowed by M1**: the processor state is carried opaquely and that works — the first backend's record is exactly 32 bytes, measured, and writing back what was read reproduces the state. So the question is no longer whether an opaque record is enough to *seed* with; it is enough. What it is not enough for is §5.4's localisation, which has to name the register that differs, and that is the thing which will force a shape. Opaque also makes a processor-state comparison nearly useless on its own, since the record begins with a cycle count: `compare` deliberately leaves it out and says why |
| Q3 | **Narrowed by M4, not answered.** What exists now: the handshake carries the protocol version both ways and the tool's own version with it, a mismatch is a refusal naming **both** numbers, and a field this tool does not know is refused rather than ignored — the strict reading, which is the honest one while this is unsettled. §7.3's capabilities are also on the wire in both lists, declared and absent, so a client discovers what may be asked for rather than guessing. What is still open is negotiation: what a client and a tool of different versions should *do* about it, beyond saying so. | still the first client written by someone who did not write the tool. M4's Python client was written against `doc/protocol.md`, which is the next best thing and not the same thing: it never disagreed with the tool, so it never exercised the question |
| Q4 | Licence for the tool and for mapping data (§11.4) | the intent to publish, with the dependency set known |
| Q6 | **Informed by M4, deliberately not answered.** M4 built a reference driven **in a child process** — but as the server's child speaking the protocol, not as a second `Platform` implementation, and the trait stayed library-only on purpose. Two things that informs. First, the cost is not the obstacle: a routine-level cycle measured 79.0 ms over a process boundary against 79.7 ms in process (Q1), so every verb in §7.2 crosses one for nothing measurable. Second, what the trait would have to gain is not a verb but a **lifecycle**: a child can die, be killed, or be alive and silent, and M4 answers those three separately (`doc/protocol.md`) while the trait has no vocabulary for any of them — a library-driven backend cannot do them, so a shared trait would make every host handle cases that only one kind of backend has. | implementing one of each **behind the trait**, which M4 did not do. The next honest attempt is §5.5's cross-check, where two references as two child processes is the architecture (Q10) — that is where a trait spanning both would earn its shape |
| Q7 | The first backend's upstream is a community fork of a project whose original author archived it (§15.4). How much of the risk the narrow C ABI absorbs is untested | a second backend, and one upstream version bump survived |
| Q8 | Whether a pre-built binary should be able to load a backend at runtime, rather than backends being compiled in (§7.5) | someone with an emulator worth having who cannot rebuild the host |
| Q9 | **ANSWERED in M2.** Loading the software twice — with the debugger existing and in a break — stops at cycle 0 at the reset vector, before one instruction, identically across processes. `doc/backend.md` has the measurement. What follows is below, and the original question was: **a starting position that is reproducible.** The first backend begins executing the moment software is loaded, and the earliest stop the transcribed API can ask for lands wherever it had got to by the time the request arrived — so the position a session starts from differs from run to run. §2.5 wants the same run to stop the same way every time, and this is upstream of every comparison. | breaking before the first instruction. The backend does this when a setting of its own says to, and that setting lives in a large configuration record not yet transcribed (§16.1); alternatively, loading a saved state on arrival makes the start a known one and is §4.6's job anyway. **Measured since**: everything downstream of a blob *is* reproducible, exactly and across processes (`doc/backend.md`), so the practical answer is that an anchor's origin is itself a blob, captured once. What stays open is that the first blob's own derivation is not reproducible, so it is the one artefact in the chain whose provenance rests on nothing but having been taken |
| Q10 | **Cross-checking two references of the same backend.** §5.5 compares two references against each other to decide which is wrong. The first backend's emulator is a single object the library owns, reached through functions that take no handle, so two of them in one process are two front ends to one emulator — and the tool refuses the second rather than pretend. **M3 confirmed the blockage is the architecture and not an oversight**: every entry point the host uses — `InitializeEmu`, `LoadRom`, `GetMemoryState`, `SetBreakpoints`, `Step` — addresses that one object, and `Reference` holds a process-wide flag that refuses a second opening, which is why every integration test in this project is one test per file. §5.5 is therefore the one requirement of §5 that M3 did not build; it is recorded here rather than half-built, and the differ's shape does not depend on it: a cross-check produces `Undetermined::ReferencesDisagree`, which exists, is tested in the core, and has no producer. | either two *different* backends, which is what §5.5 is really for, or **two references as two child processes**, which M2 showed is an architecture rather than a patch and is the most likely route: M2's done-condition already spawns the host three times and compares state digests to the byte across processes, so the pieces — a child that comes up reproducibly, a digest that travels, a parent that compares — are built and measured. What is missing is a trait that spans both ways of driving a backend (Q6) and a protocol for the parent to drive the child with, which is M4's work and should not be invented twice. Or loading the same library twice into separate link-map namespaces, which is possible and untested |

| Q11 | **The first backend's version does not identify a build.** Its `GetVersion` returns a constant written into a source file, so every commit between two releases reports the same number and §16.1's check cannot tell them apart. The commit hash is the real identity, and `doc/backend.md` records it. | §16.5's conformance fixtures, which compare behaviour rather than labels — on this backend they are not a refinement of the upgrade policy but the load-bearing part of it. A backend that derived its version from its build would also settle it, and is not ours to change |

| Q12 | **A failed state load is silent.** *Reinforced in M2*: the cheap check earned its place twice over. It caught a blob file that several processes were overwriting between one another's save and read-back — a bug that would otherwise have been a wrong answer rather than a failure — and it caught an injected fault that landed a resume a frame late. What it did **not** catch was an injected fault outside the regions an anchor declares; that is the demonstration's job, and M2 widened the demonstration to every writable region because of it. *Measured since*: a position-and-fingerprint check after the load catches a load that landed elsewhere, and **not** a load that did nothing while the machine had not moved since the save — both checks pass in that case, which was proven by deleting the load call. A test of a load must disturb the machine first. The first backend's `LoadStateFile` returns `void`; given nonsense or a missing file it leaves the machine as it was and says nothing, so success and failure are indistinguishable from the call. §4.8's cheap check is therefore not only the cache's audit but the only detector of a load that did nothing. | a backend that reports whether the load took, or the cheap check being made mandatory on every load rather than expected of it — which is the cheaper of the two and does not need anybody else to change anything |

| Q13 | **ANSWERED in M2**, by writing zeros to all seven writable memory types at Q9's power-on position: three processes then agree on every memory, the processor record, the video record, the cycle count and the position. It cost a declared divergence from the hardware rather than a transcribed configuration record, and every report says which was done. Measured in `doc/backend.md`. The original question was: **the first backend fills work memory pseudo-randomly at power-on, differently in every process.** This is the case §2.5 names by example. Measured with the generated fixture, which does not clear memory: two processes loading the same image see different bytes everywhere the program did not write. It is invisible with software that initialises its own memory, which is why it was not found until there was a fixture that does not. Its consequence is larger than it looks: **determinism from power-on is not attainable on this backend, and determinism from a blob is** — U1 measured that everything downstream of a blob agrees exactly across processes. So §4.7's anchors are not only how a run becomes fast, they are how it becomes reproducible, and M2's done-condition is reachable only through one. | making it deterministic, which this backend can do — it has a power-on memory setting — but only through a configuration record far larger than anything transcribed so far (§16.1), so it is a cost rather than an unknown. Until then, §2.5's other half applies: memory the software has not written is *not determined*, and a comparison must say so rather than compare it |

| Q14 | **§4.7's input logs cannot be replayed on the first backend.** Measured, not assumed: the backend reports **no control device at any of its eight indices**, so setting an input override stores a state nothing reads, and a fixture written to wait for a button stays waiting with one set. The only way to attach a controller is the configuration record passed by value — ten controller configurations, each holding a key-mapping set of its own — which is the record §13's Q13 priced and refused for the same reason: one field wrong silently changes the accuracy of the thing whose job is to be the ground. So an anchor behind software that waits for input is not reachable, and the tool refuses such an anchor rather than arriving somewhere else and calling it that one. **Where that refusal lives changed in M3**: it was a constant in the platform-independent half, which was a fact about the only backend there was rather than about every backend, and it is now §7.3's declaration — the definition says it needs `input-replay`, the reference says whether it has it, and the refusal names both. | a narrow way to attach a controller, which this backend does not export; or transcribing the configuration record, which is a decision about risk rather than an unknown; or driving the backend's movie playback, which replays input deterministically and needs its archive format written — the most promising of the three, because the format is a documented container rather than a C++ struct layout. The gated fixture of §11.3 exists and waits, so whichever route is taken has something to prove itself against |

| Q15 | **§7.2's other three capabilities are routes nobody has taken.** The first backend exports a read flag in its breakpoint record, an execute counter and stamp in its access record, and a flat `GetCallstack` whose record nests only two-field address pairs. Each is enough to implement `stop-on-read`, `execution-coverage` and `call-and-return-events`, and none of them is declared (§7.3), because this crate has not exercised one and a route is not a declaration. Measured in M3 and recorded in `doc/backend.md`. | a comparison that needs one. Each would be transcribed in an afternoon; what is absent is a question the tool is being asked that it cannot answer today, and building against a guess is what §2.4 refuses. Execution coverage is the likeliest first, because "which instructions did this routine run" is the natural companion to §5.4's "which instruction wrote this byte" |
| Q16 | **ANSWERED in M4.** The wire needed the name, so the report carries it. A `Difference` still does not — the comparison's value is unchanged — but `differ::Report` keeps the region of the verdict's difference, taken from the region-by-region comparison that produced it, and the wire form has it on every difference the tool produces. The shape that was *not* chosen: one difference per named region in the report. It was rejected because a report has one verdict (§2.3's fold decides which), so a difference per region would be a list with one real entry and the rest empty. What was wrong with the first attempt is the part worth keeping: the region was taken from §5.4's localisation, which is optional — so a client that did not pay for a replay got an offset with no name, and a test that always asked for localisation never saw it. One place still has an offset without a region, and it is recorded rather than filled: the two verdicts inside §5.3's control, which are summaries of whether the comparison moved and come from a fold across regions. A client that wants a localisable difference from perturbed inputs asks for a measurement with those inputs. Original entry: **a `Difference` does not name the region it is in.** §5.4's first item is an offset, read against the region — and the region's name is not in the value. It is recoverable while the comparison is in hand, and the differ does recover it, by comparing region by region rather than through the folding `compare` and keeping the name of the region whose verdict won. That is enough inside one process and is not enough on a wire: a report crossing §8's boundary carries a difference with an offset and no name, and whoever receives it cannot tell which region `1025` is an offset into. | M4's framing, where the question becomes concrete: either the difference carries the name, or the report carries one difference per named region, and the wire form is what decides which. Not changed now, because both shapes are defensible and the one that is right is the one the protocol needs |

No Q5 was ever issued; the gap is left alone so that the ids already written
down elsewhere keep meaning what they meant.

---

## 14. Conventions

### 14.1 Language

Code, file names, identifiers, comments, documentation and commit messages are
in English.

### 14.2 Refusals are values

A function that cannot establish its answer returns the reason, not a default.
Reasons are specific enough to act on: what was looked for, where, and what was
found instead.

### 14.3 One commit per unit

A unit is a thing that can be stated in one line and verified on its own. Its
commit carries the measurement that verified it.

### 14.4 This document

Changes to §2, §3 and §11 are changes to what the project is, and are made
deliberately rather than in passing. Everything else bends to what is measured.

---

## 15. The first backend — decided

### 15.1 The question that matters is not accuracy

The instinct is to pick the most accurate emulator, because the reference is the
source of truth. That instinct leads nowhere: the most accurate emulators in this
family expose almost nothing programmatically, so choosing one means writing C++
before a line of this tool exists — the trap §1.1 says the project exists to
avoid.

The question that decides it is **which backend is the most instrumentable**,
because accuracy is auditable *later, by this tool*. §5.5's cross-check was
designed for exactly this: add the accuracy reference as a second backend, run
both, and where they disagree the verdict is *not determined*, with both named.
The accuracy worry resolves itself with the thing being built, which is why it
must not block the start.

So: **instrument first, audit second.**

### 15.2 The decision

The first backend is **MesenCE** — the live community fork of Mesen 2 — driven
from Rust over the flat C ABI it already exports.

### 15.3 Why, in the order the evidence arrived

- **The internals already hold everything §7.2 needs.** Its scripting binding
  exposes state read *and write*, memory read and write by type and address,
  memory callbacks for read, write and **execution** — which is a breakpoint by
  address, the primitive §5.6's routine-level comparison depends on — and
  savestates as opaque data rather than files. The existence of that binding is
  the proof: these internals are already shaped for consumption from outside.
- **The C++ side has what the script binding lacks**: stepping by count and
  type, resuming, and savestate files.
- **The boundary already exists.** The project is a C++ core consumed by a
  separate UI written in another language, through a directory whose whole
  purpose is exporting a C API. We are not inventing an interface; we are
  writing another front end against a maintained one.
- **Seven systems in one codebase** — NES, SNES, Game Boy, Game Boy Advance, PC
  Engine, SMS/Game Gear, WonderSwan. One integration amortises across every
  platform this project might add, which is the opposite of a single-system
  emulator where the work never extends.
- **Licence**: GPLv3, matching §11.4's intent.
- **Alive**: thousands of commits, regular releases, automatic development
  builds, under a community organisation rather than one person.

### 15.4 What was ruled out, and definitively

**The scripting binding as the backend transport.** It has no file I/O, no
sockets and no inter-process communication of any kind. A script that cannot talk
to another process cannot serve a tool that lives in another process. This is a
fact about the binding, not a guess about its performance, so there is no
"prototype in the script language first" branch to weigh.

**The accuracy-reference family as the *first* backend.** The capability is not
there to expose: beyond running and reading a few fixed regions, everything
§7.2 requires would have to be added in C++ first. Worse, such a patch is bound
to one version of that core and does not survive an upgrade, and the ones with
the strongest accuracy claim are single-system, so the work never amortises.
They belong in the configuration later — as the reference this tool audits
(§15.1), not as the one it starts on.

### 15.5 One backend at a time

Only one backend is supported until it works end to end. Others — including ones
that require C++ work to expose what §7.2 needs — are added afterwards, against a
trait that by then has a real implementation behind it rather than a guess.

### 15.6 The risk, named

The original upstream was archived and the live line is a community fork. That is
a real dependency risk (Q7). What absorbs it: the licence permits forking; the
surface depended on is a narrow C ABI rather than the whole codebase; and §7.3's
declared capabilities plus a second backend mean the tool is not married to one
emulator. What does not absorb it: nothing, if the fork stalls and no second
backend exists yet — which is an argument for reaching M4 before depending on
this for anything that matters.

---

---

## 16. Backend versions and the dependency policy

### 16.1 A backend is a name *and* a version

The configuration declares both:

```toml
[[emulator]]
name = "ref-a"
platform = "snes"
backend = "mesence"
version = "2.2.1"
```

The tool asks the loaded library which version it is and **refuses on mismatch**.
This is §6.6's ROM-hash rule applied to the other half of the reference: a
configuration that says 2.2.1 and a library that is 2.2.0 produces comparisons
nobody can interpret, and producing them silently is worse than stopping.

The first backend supports this directly: its exported C API returns its own
version and build date, so the check costs one call.

### 16.2 Three legs, because a version is a label

A version string says *which* build, not *what it can do*. All three checks
happen at startup, and they answer different questions:

| check | question it answers | §
|---|---|---|
| **version** | which adaptation code to run | 16.1 |
| **capabilities** | whether this build can do the job at all | 6.5, 7.3 |
| **conformance fixtures** | whether it actually behaves as expected | 16.5 |

A development build between releases may carry an odd version string and the
right capabilities; a release may carry the right version and a regression. Only
the three together are worth anything.

### 16.3 Supported versions are a declared set, never a range

A backend declares the versions it has adaptations for — `["2.2.0", "2.2.1"]` —
and not `>= 2.2.0`. An open range is a promise about builds that do not exist
yet, which nobody can keep: the next release may change behaviour the adaptation
depends on.

An unknown version is refused, naming what is supported. That is §2.4: the tool
does not guess that a newer build behaves like an older one.

### 16.4 Adaptations are additive

Adding support for a new version never removes an older one. When 2.2.0 works and
2.3.0 breaks, the work is to add what 2.3.0 needs — and a user who stays on 2.2.0
deliberately keeps working, with the same results as before.

This is not politeness toward old versions. A reimplementation verified against
2.2.0 was verified against *that reference*; forcing its author forward invalidates
their work for a reason that is the tool's convenience.

### 16.5 A behaviour change in the reference is a new reference

If a version changes what the emulator *does* — an accuracy fix is exactly this —
then the truth changed, not the adaptation. Comparisons made against the old
version are not automatically valid against the new one.

Therefore:

- a snapshot records the backend name and version it came from (§3.2);
- comparing a snapshot taken on one version against a run on another is refused,
  or reported as *not determined* when the caller insists;
- and the generated fixtures (§11.3) serve as a **conformance suite for the
  backend**: on every version bump they say whether the reference still behaves
  the same.

That last item is the one worth noticing. The tool's own comparison machinery,
pointed at its own dependency, turns "we hope the upgrade was safe" into a
measurement. No other kind of tool can audit its dependency this way, and it
costs nothing extra: M0's fixtures and M3's differ are the same parts.

### 16.6 The dependency policy

- **Vendor the backend's source at a pinned commit.** The licence permits it and
  the project's own licence makes it natural. The upstream's fate then does not
  reach us.
- **Pin, never track.** Upgrades are deliberate acts, with §16.5's conformance
  run as the gate.
- **Depend on the narrow surface.** The exported C API, not the codebase. What we
  bind to is what they maintain for their own front end.
- **The real insurance is the second backend** (§15.5), and it is insurance only
  once it exists. Until then, M4 is the point before which nothing important
  should rest on this.

### 16.7 Why an emulator dependency is not an ordinary dependency

Stated once, because the intuition it corrects is strong and comes from elsewhere:
a dependency that stops being maintained usually decays — its API moves, its
runtime advances, its vulnerabilities accumulate. **An emulator does not.** The
console it reproduces will not change, and neither will the software pointed at
it. A frozen emulator is a frozen reference, which is the desirable state for a
reference; what an unmaintained one stops doing is *improving*, not *working*.

The corollary, which is §15.1 again from the other side: the accuracy that a
future version might add is accuracy this tool can measure for itself.

---

## 17. How the code is written

### 17.1 The unsafe line

This tool cannot avoid `unsafe`: loading a shared library and calling a C ABI
require it. So the rule is not abstinence, it is **confinement**.

```
crates/awaseru-core     no unsafe — forbidden, not discouraged
crates/awaseru          no unsafe — forbidden, not discouraged
────────────────── the foreign-function line ──────────────────
crates/awaseru-<platform>::ffi   unsafe permitted, and nowhere else in the crate
```

`unsafe_code = "forbid"` is set in `[workspace.lints]`, inherited by every crate.
A backend crate opts out **for one module**, and its manifest says why.

Three reasons, and none is taste:

- **Undefined behaviour is the deepest non-determinism there is**, and §2.5 asks
  for the same result in one process, across processes and across machines. UB
  may differ by compiler version, optimisation level and target. A verification
  tool reporting its own undefined behaviour as a divergence in the user's work
  is the worst failure this project has available to it.
- **§3.1 requires state to be addressable and enumerable.** `unsafe` is where
  aliasing and hidden state would hide.
- **The audit surface stays Rust**, which is the point of §17.2.

The `ffi` module's job is to be the only place a reviewer has to read carefully:
raw declarations in, a safe API out, and no `unsafe` anywhere above it.

### 17.2 Pure Rust, and as little of it as possible

A crate that builds or links C or C++ is not chosen when a pure-Rust crate of
good reputation does the job. The reference emulator is the deliberate exception
and the only one: it is the product's whole point, it is C++, and §16 is the
policy that contains it.

Every dependency is a decision recorded in `doc/dependencies.md` before it is
used — what it is for, and what the project would do without it. A crate that is
not in that file is not in a `Cargo.toml`, and reaching for one is a halt rather
than a judgement call.

### 17.3 Latest stable, looked up

A crate enters at its latest stable release, checked at the moment it is added —
never a version remembered, and never one copied from another project.
Pre-releases are not stable. The same holds for the toolchain: latest stable
Rust and edition, pinned in `rust-toolchain.toml`. `Cargo.lock` is committed, so
a build is reproducible and the audit reads exactly what ships.

### 17.4 Do not contort to avoid allocation

`Rc`, `Arc`, `clone` and arenas are fine wherever they make the code clearer.
This tool's work is bounded — a few thousand region reads and comparisons, not
millions of operations a frame — so there is no performance case to answer, and
legibility is worth more than a borrow that takes a paragraph to explain.

The place to care about cost is where it is measurable and has been measured:
state transfer across the API (§3.6, Q1), and the per-routine cycle (§5.6).
Everywhere else, write the obvious thing.

### 17.5 Before each commit

`cargo clippy --all-targets --all-features -- -D warnings`, `cargo audit`,
`cargo deny check` and `cargo test`, all green. A commit carries the measurement
that verified its unit, and documentation for whatever it touched.
