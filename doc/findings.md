# Findings

What M5 found by *using* the tool rather than writing it.

A finding is anything that made the cycle — choose a unit, write it, compare, fix
until it agrees, record provenance, commit — harder than it should have been. Each
one says whether it **blocked** the cycle, **slowed** it, or merely **annoyed**,
because those want different answers, and each is classified as a fix worth
making, a question for §13, or a note for `doc/`.

This file is completed in M5's ninth unit, where the whole list is classified
together. Entries arrive here as they are found, so that none is lost to the end
of the milestone.

## The list

Thirteen, in the order they were found. **Blocked** means the cycle could not go
on until something was done about it; **slowed** means it cost real time;
**annoyed** means it was wrong and cheap.

| | finding | cost | what it wants |
|---|---|---|---|
| 1 | a bounded run costs about as much as the request around it | slowed | a note for `doc/` — half of it is already fixed |
| 2 | input replay is in the machine and not in the host | **blocked** | **closed when §13's Q14 closed** — two milestones from the measurement to the verb |
| 3 | the emulator's chatter lands on the host's own stdout | annoyed | a fix worth making — **not made**: it needs a file-descriptor redirect, which is `unsafe`, and §17.1 keeps that in the backend's `ffi` module. The fix has a home and is not a cheap one |
| 4 | a frame bound reports a position it then apologises for | annoyed | **fixed in U10** |
| 5 | the two interfaces key the anchor cache differently, and one keys it by a path | **blocked** | **fixed in U10** — one `Loaded::provenance`, built in one place, with a test |
| 6 | a client cannot arrive at an anchor | **blocked** | a design decision: a verb §8 does not have |
| 7 | a bound given with an anchor is discarded in silence | annoyed | **fixed in U10** — refused now, naming both halves |
| 8 | an ancestor's cached blob never shortens the walk | slowed | a question for §13 |
| 9 | the wire has no write bound, so §5.4 had to be rebuilt by hand | slowed | a design decision: §8 gains a bound |
| 10 | nothing in the tool helps you find a routine | slowed | a note for `doc/`, and a question for §13 |
| 11 | a vacuous verdict cannot say why nothing moved | annoyed | a fix worth making — **not made**: the distance is known by the caller and not by `compare`, so saying it means a new field on the wire, which is §8's to decide (§13's Q17) |
| 12 | a control that is not noticed has a third explanation | annoyed | **fixed in U10** |
| 13 | the keyboard that reached no controller | annoyed | a note for `doc/backend.md` |
| 14 | `localise` answers `NothingWrote` on a machine where the routine has not run | annoyed | a fix worth making — found in M6 |
| 15 | forgetting coverage forgets write recency too | **would have blocked** | a note for `doc/backend.md`, and a shape to watch — found in M6 |
| 16 | a symbol given as an address cannot be checked against the machine at all | **slowed**, and it is a gap §9.3 does not know it has | a question for §13 — found in M7 |
| 17 | `measured` hides the distinction its author tabulated: once, or twice by two means | annoyed | a question for §13 — found in M7 |
| 18 | a mapping is only visible where a measurement happens to land | annoyed | a note for `doc/`, and probably a verb one day — found in M7 |
| 19 | the backend says nothing about an input log it could not open | **would have blocked** | **fixed while closing Q14** — the host checks the only observable thing |
| 20 | the watchdog could not tell a stuck backend from a long run | **blocked** §4.9 on a long anchor | **fixed while closing Q14** — it watches progress now, not duration |
| 21 | six more calls the host believes although they cannot report failure | **would block** a verdict, invisibly | **all six fixed by the audit**, plus the half-guarded seventh |
| 22 | a guard against a silent failure cannot be tested end to end | annoyed, and it is a limit rather than a defect | a note for `doc/` — found by the audit, by its own mutation |
| 23 | the parent's deadline on its child is finding 20 again, one level up, and it is live | **will block** the use pass on a cold expensive anchor | a design decision — found by the audit |
| 24 | two processes creating one anchor shared a staging directory | **would corrupt** a cached blob, silently | **fixed** — the staging name carries the process |
| 25 | every run erased the previous run's log | annoyed, and it loses the only record of what went wrong | **fixed** — the name carries the moment |
| 26 | a reference left its scratch state file behind, every time | slowed, and it grows without bound | **fixed** — it goes with the reference |
| 27 | the backend's own folders are shared between processes | **would overwrite** one run's screenshots with another's | a decision: what `--home` is for |

Three blocked the cycle and all three are about the same boundary: what the
**backend** can do, what the **host** can ask for, and what a **client** can ask
the host. The backend is ahead of the host, and the host is ahead of the wire.
Nothing in this list is a measurement that came out wrong.

Four are the tool refusing to guess, and are here as costs rather than faults:
§2.2's vacuous verdict catching an empty reimplementation, §7.3's gate refusing
an anchor it cannot reach, §4.9's closing check refusing a blob that does not
exist, and §5.3's control leaving a measurement incomplete. Each cost an
afternoon and each was right.

## 1. A bounded run costs about as much as the request around it

**Slowed.** Not a defect, and it changes how the tool should be asked things.

Measured twice, in two units, with the same shape:

| asked as | rate |
|---|---|
| one request per frame | 60 frames a second |
| one request for the whole span | 165 a second |
| one request per instruction | 99 instructions a second |
| one request for the whole span | some 160 000 a second |

The emulation is identical either way. What costs is the request, the break and
the wait around it — about 10 ms each — so a span asked for at once is 3.2×
faster for frames and three orders of magnitude faster for instructions.

The host now asks for frame spans in one request, which is where the 165 comes
from. What is **not** fixed is that nothing in the API says this: a caller
writing the obvious loop pays a thousandfold and the tool does not mention it.
`doc/protocol.md` records the numbers; a note in the verb's own documentation
would be cheaper to find.

## 2. Input replay is in the machine and not in the host

**Blocked. CLOSED when §13's Q14 closed.** The verb exists, the capability is
declared, and the anchor behind the recording arrives — 754 seconds replayed,
0.017 resumed, the same state both ways, and the screenshot at it shows what the
person who made the recording said it would.

**What it cost, from first measurement to closed: two milestones.** M5 measured
the machine replaying a log and could not say so; M6 and M7 went by with the
entry below standing; Q14 took about two hours once it was started. The gap was
never about the measurement — it was about there being no verb to ask with, and
the decision that filled it was two sentences long. The lesson is the size of
that gap, not the size of the work: a capability a tool has and cannot be asked
for is invisible, and invisible is indistinguishable from absent.

The original entry follows, unchanged.

**Blocked.** It is the first thing M5 asked for and the tool cannot do it.

`doc/protocol.md` has the measurements: a recorded log replays on this backend,
deterministically, power-cycles when it starts, ends at its own end, and survives
eighty thousand single instructions with the debugger attached. §13's Q14 said
none of that was possible, and was wrong about the reason it had measured.

What stops the capability being declared is above the backend. `Platform` has no
verb for replaying a log, and `Anchors::chain_for` gates an anchor on
`Capability::InputReplay`. So:

- **as it stands**, an anchor carrying an input log is refused, with a refusal
  that now says something false;
- **if the capability were declared** with nothing else changed, the gate would
  pass and the arriver would replay the definition with the log *ignored* —
  reaching a different position, consistently, and caching it as the anchor. A
  wrong answer that repeats is worse than a refusal (§2.3), so the declaration
  is being held back rather than made true by halves.

Two decisions, and both are the user's because both change `awaseru-core`'s
public shape.

### 1a. What an input log holds

Today `InputLog { name, recorded: Vec<u8> }`, with `recorded` declared opaque
"until something can replay it" (§2.4, which was right to wait). Something now
can, and what it takes is a **path to an archive on disk**, not bytes.

| | keeps | costs |
|---|---|---|
| **bytes in the definition** | the definition is self-contained and travels in the project's git | the archive has to be written back out to a file for every replay, and a recording of any length is a large thing to carry in a configuration file |
| **digest in the shared half, path in the local half** | exactly the pattern §6.1 and §6.2 already use for the software: the invariant is stated and refused an override, the location is machine-local | the log does not travel with the project, so a second machine needs the file as well as the configuration — which is already true of the software |

**The recommendation is the second**, because the problem is the one the project
has already solved once. A recorded log is a file the person already has next to
their software, it is identified by what it is rather than where it is, and
§6.2's refusal to let a local file override an invariant is what keeps a replay
from silently becoming a different replay.

### 1b. What the verb is, and whether a log is an origin

A verb on `Platform` taking whatever 1a decides. Two consequences come with it:

- **starting a log returns the machine to its origin** — measured, not assumed.
  So a definition carrying a log has its beginning fixed by the log, and §4.12's
  report of how the machine got to its origin has a second answer to give.
- **`Start::PowerOn` plus a log is consistent; any other `start` plus a log is
  not.** A definition starting from another anchor's position and then replaying
  a log would have the log throw that position away. Either the shape forbids it
  or `Anchors` refuses it, and the shape forbidding it is cheaper.

### What the tool could not help with

Nothing in the tool pointed at any of this. The gate refused the anchor and
named the capability, which read as "this backend cannot press buttons" — the
sentence §13 had written. That the backend *could*, and that the host was the
half missing a verb, took an evening of measuring the machine directly, outside
the tool, against a library the tool wraps. A refusal that names a capability
should also name **which side of the boundary** is missing it; today a host
without a verb and a machine without the ability produce the same sentence.

That is a fix worth making and it is small: §7.3's gate knows the declaration it
read and the verb it has.

## 3. The emulator's chatter lands on the host's own stdout

**Annoyed, and it breaks a documented promise.**

`--state-digest` says it prints "one line a machine can compare, and nothing
else". Running it against real software produced **1.1 MB** of the emulator's own
commentary on stdout — one line per uninitialised read, of which this software
makes a great many at power-on — with the digest somewhere inside it. A caller
doing the obvious thing gets a megabyte and has to filter by eye.

This is the same problem §8.2 already solved once. The reference was put in a
child process precisely because the emulator writes to stdout and the protocol
needs it; the plain command-line host has the same collision and none of the
solution. `--log PATH` exists and is documented as where `awaseru reference`
sends the emulator's output — so the machinery is there, and the one-process
path does not use it.

A fix worth making, and cheap: the same redirection, taken before the backend
comes up.

## 4. A frame bound reports a position it then apologises for

**Annoyed.** Arriving at an anchor bounded by frames prints:

```text
stop=<an address> , of a kind the backend does not say
```

Which is true — this backend reports no reason for a frame-boundary stop — and
reads as a defect rather than as the honest statement it is. The position is
exact and the *kind* is what is unknown. A sentence saying "at a frame boundary,
and the backend does not say what kind of stop that was" would carry the same
information without looking like a bug. A note for `doc/`, or a word in the
printer; not a change to what is measured.


**Fixed in M5's last unit.** It now says the address is exact and that what the
backend does not report is the *kind* of position that is — the same information
the other way round, which reads as a statement rather than as a defect. The
diagnosis above is kept because the way a true sentence can read as a bug is
worth remembering.
## 5. The two interfaces key the anchor cache differently, and one keys it by a path

**Blocked.** It stopped §4.9's closing check from running at all.

The command line arrives at an anchor, demonstrates it and caches the blob.
A client then connects over §8's protocol, points at **the same cache
directory**, and is told there is no blob:

```text
the anchor `<name>` cannot be reached: no blob is cached for it, so nothing
rested on one
```

The refusal is honest about what it found and wrong about the world. §4.11's key
is built from §9's provenance, and the two halves of this project build that
provenance from different things:

| interface | what it calls the software |
|---|---|
| the command line | the configuration's digest |
| the reference process behind the protocol | the software's **path on this machine** |

So the keys differ, the blob is invisible across the boundary, and the
demonstration is paid twice. That is the cheap half of the damage. The expensive
half is that a cache key containing an absolute path is wrong on its own terms:
§6.2 says the location is machine-local and the identity is the invariant, and
keying a cache by location means moving the file invalidates every blob for a
reason that has nothing to do with whether the blob is still right — while two
different files at one path would silently share a key.

A fix worth making, and it is one line: the digest, which is what §9 should be
recording anyway.


**Fixed in M5's last unit**, and it is the fix that unblocked §4.9's closing
check. Both halves now build §9's provenance from one `Loaded::provenance`, with
a test that fails if a path ever reaches it. The closing check ran on real
software for the first time immediately afterwards: 6.1 seconds against 42 for
the demonstration.
## 6. A client cannot arrive at an anchor

**Blocked**, and it is what made finding 5 visible.

§8's protocol has `run`, `read`, `write`, `examine` and `reverify`. It has no
verb for *arrive*. An anchor is reached only as a side effect of `examine`'s
`from`, which needs a routine to measure — so a client that wants §4.9's bracket
cannot open it. `reverify` before any measurement is refused, correctly, because
nothing has rested on a blob that does not exist.

§4.9 describes a session bracketed by a demonstration at the start and one
closing check at the end. A client can ask for the end and not the beginning.

The way round from outside is to arrive with the *other* interface — which is
finding 5's collision, so there is no way round at all today. Recorded rather
than worked around, which is what this milestone is for.

## 7. A bound given with an anchor is discarded in silence

**Annoyed.** `--anchor booted --frames 300` runs the anchor and ignores the
frames, printing the same digest as `--anchor booted` alone and saying nothing.

It is deliberate — an anchor carries its own definition, and the usage text says
"instead of running a bound" — and silence is still the wrong answer. Everything
else in this tool refuses rather than guessing which of two instructions was
meant (§2.4), and this one picks. A refusal naming both would cost a line.

It also means §4.8's fourth step — run the same bound onward from a replay and
from a resume — cannot be asked for from the command line. The tool does it
inside its own demonstration; a user cannot reproduce it.


**Fixed in M5's last unit.** A bound beside an anchor is refused, naming which
bound and why, with each alone still accepted. What is *not* fixed is the second
half of this entry: §4.8's fourth step still cannot be asked for from outside,
because arriving and then running a bound is not a thing the command line can
express. That remains true.
## 8. An ancestor's cached blob never shortens the walk

**Slowed.** With a chained anchor's parent already cached, arriving at the child
replays the whole chain from the origin rather than resuming the parent and
running the child's bound:

| cache holds | arriving at the child | how |
|---|---|---|
| nothing | 51 s | replayed |
| the parent's blob | 51 s | replayed |
| the child's blob | 0.4 s | resumed |

So a chain's cache is all-or-nothing at the leaf. For two anchors it costs a
replay; for a chain of five it costs four. Whether that is worth fixing depends
on how deep chains get, which nothing yet knows — a question for §13 rather than
a fix, and the measurement above is what it needs.

## 9. The wire has no write bound, so §5.4 had to be rebuilt by hand

**Slowed, badly.** It is the finding this milestone exists to produce.

The backend declares `stop-on-write` and `writing-position` (§7.3). The host uses
both: §5.4's localisation is a replay with a write breakpoint, and it reports the
instruction that wrote a byte. §8's protocol exposes **neither**. Its bounds are
frames, instructions and an address, and localisation arrives only bundled inside
`examine` — which needs a routine's entry and return addresses, which is exactly
what somebody asking "what wrote this byte" does not have yet.

So choosing a unit of work meant rebuilding §5.4's third item out of `run` and
`read`: bisection on the instruction count, each probe a fresh session replaying
to the frame before the write, the smallest count at which the byte has changed
being the instruction after the store. It works, and it cost **twenty replays
and eighteen seconds** where one write bound would have cost one run.

Two shapes would fix it and they are different sizes. The small one is a
`Bound::Write` on the wire, which the host already has internally. The larger one
is a verb for "what wrote this", which is §5.4 without a routine around it.

A fix worth making, and the first one is cheap.

## 10. Nothing in the tool helps you find a routine

**Slowed.** The tool measures a routine you already know. There is no verb for
"what is happening here".

Choosing the unit of work needed three scripts written from outside, and none of
them is exotic:

- one that reads a region every frame and reports which spans changed, which is
  how a buffer being filled becomes visible at all;
- one that bisects to a writing instruction, which is finding 9;
- one that steps single instructions and records the program counters, which is
  how a loop's extent and a routine's entry and exit are found.

The third is the interesting one, because it also **verified itself**: the trace
measures how long each instruction is by subtracting consecutive program
counters, with no idea what the instructions are, while the opcode table says how
long each one should be without seeing the machine. When the two agree, the read
is genuinely aligned. That is the shape of evidence this project asks for, and it
was assembled by a user rather than offered by the tool.

A note for `doc/` at least: these three are the first day of using this tool on
software nobody has mapped, and nothing says so.

## 11. A vacuous verdict cannot say why nothing moved

**Annoyed, and it cost a wrong turn.** The behaviour is right and the report is
thin.

The first measurement of the chosen routine was bounded by the address the
routine hands control back to — which turned out to be in the caller's main
loop, reached constantly. So the measurement stopped before its subject had done
anything, and the tool answered:

```text
not-determined / vacuous — the reference changed none of the N bytes compared,
so agreement here is agreement about data neither side wrote
```

That is §2.2 working exactly as it should, and it prevented an empty
reimplementation from passing. What the report cannot say is **which** of two
very different things happened: a routine that genuinely writes nothing, or a
measurement that stopped at once because its bound was hit immediately. The
first is a fact about the software; the second is a mistake in the request.

The tool knows the difference and does not report it. It ran the routine and
knows how far it got — the second case stops after a handful of instructions and
the first after hundreds of thousands. Saying how much was run when the verdict
is vacuous would turn a puzzling answer into an obvious one.

A fix worth making, and small: the vacuous cause carries the distance run.

## 12. A control that is not noticed has a third explanation, and §5.3 offers two

**Merely annoyed**, and it is about wording rather than measurement.

§5.3's pair ran on the real routine. The one that should be noticed was noticed,
and changed **exactly one** of twelve thousand bytes — the single byte it should
have. The one that should not be noticed was not, and the tool said:

```text
... so this comparison cannot discriminate it. Either the comparison is blind
or the routine does not read what was changed
```

Both explanations are wrong here, and that is why the control was worth running.
The routine **did** read the changed byte and legitimately cannot tell the new
value from the old one, because what it asks of that byte is coarser than the
byte. That is the third explanation: the routine read it and is correctly
indifferent to it.

The distinction matters because the two the sentence offers are both faults and
the third is a confirmation. A user running the pair §5.3 asks for — one change
that should be noticed and one that should not — is told that the half which
behaved as intended is a sign something may be wrong.

A note for `doc/`, and a better sentence: the comparison cannot discriminate it,
which is a fault if the routine reads it and depends on it, and the expected
answer if the routine reads it and does not.


**Fixed in M5's last unit.** The sentence now names all three: a fault if the
routine reads what was changed and depends on it, the expected answer if it does
not read it — or reads it and is indifferent to it, which is what a control
chosen to go unnoticed is for.
## 13. The keyboard that reached no controller

**Annoyed, and it cost an evening**, which is a strange pair until you notice
that none of the evening was spent on this project's code.

The input log that answered §13's Q14 had to be recorded by a person pressing
buttons in the emulator's own interface. That interface delivered no input to
the software at all, for hours, with no error and no message. Two gates, both in
its code and neither in any document:

- **the interface swallows every key while its menu bar holds keyboard focus**,
  and a menu opened with the mouse can keep that focus after it has closed. The
  software is running, the window is in front, and nothing arrives;
- **the core discards all input when it believes its window is in the
  background**, which a compositor can report wrongly.

Neither is a fault in this tool and both are facts about the reference it drives,
so they belong in `doc/backend.md` with the rest of what driving this backend
costs. The general lesson is the one worth keeping: **a tool that depends on a
person operating a GUI inherits every bug in that GUI**, including the ones
nobody has written down. This project now depends on a *recording* instead, which
it does not.

---

# Found in M6

The first thirteen are M5's, from using the tool. These two came from building
on it, which is a different kind of evidence and worth keeping apart.

## 14. `localise` answers `NothingWrote` on a machine where nothing has run

**Annoyed**, and it cost a confused twenty minutes in M6's done-condition test.

§5.4's localisation begins with a cheap filter: the backend's write record for
the byte. If the record shows no write, there is nothing to go looking for and
the answer comes back without a replay — which is right, and fast.

What it cannot distinguish is **whose** state that is a fact about. Asked on a
freshly loaded machine, before the routine has been measured, it answers:

```text
NothingWrote
```

which is true of the machine and reads as a statement about the routine: *this
routine does not write that byte*. Those are very different, and §2.3's habit —
never collapse "not determined" into an answer — applies to this pair as much as
to a verdict.

The precondition is real and undocumented: `localise` explains a write that has
happened, so the routine has to have run. A caller who has not run it gets the
answer that means "stop looking".

A fix worth making, and it is a third value rather than a new verb: *the record
shows no write, and this machine has not executed the routine*.

## 15. Forgetting coverage forgets write recency as well

**Would have blocked**, and was caught before it could, by a test that failed
for the wrong reason first.

`forget_coverage` is implemented on the one thing the backend offers:
`ResetMemoryAccessCounts`. That record is not three records. Clearing it clears
the read counts, the write counts and the execute counts together, with their
stamps — so a caller who forgets coverage after measuring and then localises
has thrown away §5.4's cheap filter, and gets finding 14's `NothingWrote`.

Nothing in the verb's name says so. `forget_coverage` sounds like it forgets
coverage.

The two orders are not symmetric, which is what makes this survivable:

| order | what happens |
|---|---|
| forget, run, read coverage, localise | **correct** — the run rewrites the write record |
| run, forget, localise | the write record is gone and the attribution is lost |

M6's own code takes the first order, and the done-condition test is written in
that order on purpose. What is owed is the honest name or the honest refusal:
either the verb says it clears the whole record, or the backend grows a way to
clear one third of it. `doc/backend.md` records the behaviour beside the
measurements.

# Found in M7

## 16. §9.3's location check cannot see an address

**Slowed**, in the sense that half of §9.3's last rule turns out to be
uncheckable — and §9.3 does not know it.

> "Locations fall inside a region the backend exposes."

§9.1 allows a location to be written two ways: a region and an offset, or an
address. The first is checkable and is checked, down to the byte: a symbol
ending exactly at a region's last byte is inside it and one further is not.

**The second cannot be checked at all.** A region set is names, sizes and access
— there is nothing in it about which addresses reach which region, and §3.1 is
explicit that on some machines one byte is reachable through more than one
address. So the host has no address-to-region map to check against, and building
one would be guessing at something that is the backend's to know.

Refusing every address-form symbol instead would make the format useless for
what it is most used for: an entry point is an address, and the real mapping's
entry, return and both buffer bases are all written that way. So they pass
unchecked, and a symbol at `0xDEADBEEF` loads happily against a machine with
32 KiB of cartridge.

That is recorded rather than hidden, and the test asserting it says why. What
would close it is a backend saying which addresses reach which region — which is
a capability (§7.3), not a fix, and nobody has needed it yet (§2.4).

## 17. `measured` hides the distinction its author bothered to tabulate

**Annoyed**, and it is the first thing the real mapping did not fit.

§9.2's three values separate a value established by measurement from one
somebody remembered, and that was the distinction the specification cared about.
Writing the real mapping out showed a second one underneath it.

The table it came from has a confidence column with three entries that say
**high — two sources**, and M5's provenance note is explicit about why: of the
seven values that measurement rested on, six were read twice by two different
means and one was inferred once. The two sources are not decoration — for the
output buffer they are the instruction's operand and the span the memory
actually changed, and the whole argument for trusting that row is that two
unrelated methods agreed.

In the format, all of that lives in `note`, as prose. `how` says `measured` for a
value confirmed twice and `measured` for a value seen once, and nothing can tell
them apart without reading English.

Not fixed, deliberately. A fourth value is additive and can arrive the day
something wants to branch on it, and inventing one now would be inventing for
nobody (§2.4) — but it is worth recording that the first real mapping wanted a
distinction the format flattens, because that is the shape of evidence a later
decision needs.

## 18. A mapping is only visible where a measurement happens to land

**Annoyed**, and it blunts §9.2's first purpose.

The mapping loaded for M7's turn has seven symbols, one of which is a hypothesis
— and that one is in it precisely because its author did not trust it. §9.2's
epistemic purpose is that such an entry is "marked as a hypothesis and not
treated as fact".

It is marked, and a report naming it says so. But a report names only what a
measurement *found*, so the hypothesis is visible exactly when a difference
happens to land on it. In the turn's own measurement no difference did, so the
one entry the mapping is least sure of went unmentioned.

Nothing lets anybody ask the mapping a question of its own — "which of these
are hypotheses", "what covers this address", "what is in this group". The graph
is built, checked and then only ever consulted sideways, by a report that wanted
a name.

A verb would fix it and none exists, which puts this beside finding 10: the tool
measures what you already know and does not help you see what you know. A note
for `doc/` today; a question for whoever decides what comes after §12.

# Found while closing Q14

## 19. The backend says nothing about an input log it could not open

**Would have blocked**, and was caught by a mutation rather than by a thought.

`MoviePlay` returns void. A path that does not exist, an archive that is not
one, a recording made for other software — every one of them comes back looking
like success, and because starting a log power-cycles, the machine is left at
its origin. A caller would then measure from power-on believing it had arrived
somewhere, and every comparison downstream would be about the wrong machine and
would **pass**.

It was found the way this project keeps finding things: by removing the replay
from `arrive` and watching the test still pass. The test was asserting that the
call returned, which the specification's own warning says is not enough — an
arrival is easy to fake, because power-on is a perfectly good-looking position.

The host now checks the one thing it can observe: after starting a log, playback
is live or the log did not start. A recording with no frames is refused by the
same check, which is correct — a log that feeds nothing is not a log.

What would close it properly is a backend that reports the open. That is the
backend's to add, and the check here is honest about being a substitute: the
refusal says the backend is silent and that what can be seen is that nothing is
being replayed.

## 20. The watchdog could not tell a stuck backend from a long run

**Blocked** §4.9's closing check on the anchor Q14 exists for, and was found by
running it.

The watchdog is a wall clock on waiting for a break, and the default is ten
seconds. That is generous for a routine and absurd for a seventeen-thousand-
frame replay, which is two minutes of entirely honest work. So the closing check
on that anchor came back saying:

```text
the backend cannot continue from <an address>: it did not break within 10s
```

which is false, and alarming in the same breath — it reads as the emulator
having wedged.

Raising the number would only move the lie: any fixed duration is a guess about
how long legitimate work takes, and a bigger guess is still a guess. What tells
a stuck backend from a working one is whether the machine is **doing** anything,
and the processor's cycle count answers that — it only goes up, and a wedged
backend stops moving it.

The deadline now resets whenever the count has changed. A run of any length is
fine as long as it is still running, and a backend that has genuinely stopped is
caught in the same ten seconds as before. The comment on the constant had the
reasoning in it all along — "a run that ended on one would not be reproducible"
— and the implementation was measuring the wrong thing.

# Found by the audit before the first use pass

## 21. Six more calls the host believes, and the pattern they form

Findings 19 and 20 and §13's Q12 were three sightings of one animal, found
months apart and by accident. The audit went looking for the rest of the herd:
all thirty-one imported symbols, asked what each reports when it fails.

**Twenty-one of thirty-one return `void`.** They cannot report a failure at all.
Eight of those sit in a path where a wrong answer still looks like an answer,
and two of the eight are already guarded — `LoadStateFile`, because Q12 hurt,
and `MoviePlay`, because finding 19 hurt. That leaves six.

| the call | what a silent failure produces | what the host could observe instead |
|---|---|---|
| seeding a span | the routine runs on whatever was already there, and §5's comparison is about input nobody chose | read the span back and compare it with the bytes just written |
| writing a region | the same, and §5.3's perturbation goes through here | the same |
| writing the processor | the processor stays as it was | read it back |
| reading the video record | zeros, so `dot = 0` and `line = 0` — and `frame_position` reads exactly that as a frame boundary | the frame counter does not advance across two readings with a run between them |
| reading the processor | thirty-two bytes of filler, and the filler is already there | if the whole head is still filler, the call wrote nothing |
| reading the access record | zeros, which read as "nothing was read, written or executed" — §10's coverage and §5.4's cheap filter | a byte the run certainly touched has a count |

A seventh is half-guarded and worth saying separately. **Saving a state** reads
the file back, so a save that wrote nothing is caught — but a stale file from the
same process is not, because the path is per-process and fixed. The blob is then
wrong, and the error arrives much later from `load_state`, blaming the load for
something the save did.

### The one that is most worth looking at

`GetPpuState`, because the code around it was written **carefully**. It does not
assume the backend is at a frame boundary; it reads the video record and checks
that the line and the dot are both zero, and the comment says *checked rather
than assumed*. A read that fails silently returns zeros — which is exactly the
condition being checked for. **The care inverts into a false positive**, and
every position reported from a frame-bounded run would be a frame boundary at
frame zero.

### What was done

All six, and the seventh:

| | what the host checks now |
|---|---|
| seeding a span, writing a region | the span is read back and compared, and the refusal says **which byte** parted and what each side holds |
| writing the processor | the record is read back and compared |
| reading the video record | the buffer is handed over **filled with a sentinel** rather than zeroed, and a buffer that comes back still full of it is a call that wrote nothing. The position then says it does not know, which is what `Unclassified` is for |
| reading the processor | the same sentinel, looked at in the middle rather than only past the end |
| reading the access record | the same, with a sentinel record |
| saving a state | the file is removed before the save, so a save that did nothing cannot be mistaken for the previous one |

The cost is a read the size of each write — about 0.4 ms for a twelve-kilobyte
seed, against 79.7 ms for a cycle. That is the price of not trusting a `void`.

### The pattern, which is the point of having hunted

It is not that the backend is poor. It is that **the host checks where it has
already been burned and does not check where it has not.** `load_state` carries
two checks because Q12 hurt; `MoviePlay` carries one because finding 19 hurt.
The six above are in exactly the state those two were in beforehand, and nothing
distinguishes them except that nobody has been burned by them yet.

That is not a thing reading finds. It is a thing counting finds, and the count
only means something with all thirty-one in one table.

## 22. A guard against a silent failure cannot be tested end to end

**Annoyed**, and it is a limit rather than a defect — but it is worth writing
down because the audit caught itself doing the wrong thing.

The six guards above fire only when a `void`-returning call has silently done
nothing. **No test can make a working backend do that.** The first attempt at
testing them asserted that the happy path still works, and all three mutations
passed — removing a guard broke nothing, which is precisely what the loop
driving this audit had warned against.

What is testable is the guard's **decision**, so the decisions were pulled out
as free functions and tested with both answers: a buffer that came back exactly
as it was handed over, and one that did not; two readings that agree, and two
that part at a named byte. Both fail under mutation.

The integration tests are kept and their claim was corrected. They do not prove
a guard fires. They prove it does **not** fire when the call worked, which is
the other way for a guard to be wrong and is not nothing: a check too strict
would refuse every honest write.

What would close it properly is a backend that can be told to misbehave — a
fake implementing the same symbols, returning nothing. That is a real option and
a day's work, and nobody has needed it enough yet (§2.4).

## 23. The parent's deadline on its child is finding 20 one level up

**Will block the first use pass**, on the first `examine` from a cold anchor
behind the recording. Found by asking of every fixed number what finding 20
asked of one.

`child.rs` gives the parent 120 seconds to receive one answer. Its comment says
the number is *generous, because an `examine` replays a routine several times
and a cold anchor replays from the origin* — which is the author knowing the
risk and answering it with a bigger guess. **A bigger guess is still a guess,
and this project now has the measurement that exceeds it: a cold arrival at the
anchor behind the recording takes 754 seconds.**

`Child::set_watchdog` exists and **nothing in the host ever calls it**, so the
120 is not a default anybody can move.

### What it actually guards, which is less than it looks

A **dead** child is already noticed at once: the channel disconnects and the
parent reports a death rather than waiting. And since finding 20, a **hung
backend** is caught by the child itself, in about ten seconds of no progress,
and comes back as a refusal.

So the parent's deadline guards one case: a child hung in its own code, with the
backend fine. That is the rarest of the three, and the deadline's price is every
legitimate run longer than two minutes.

**The deadline is in the wrong place.** The thing that can hang is guarded
better one level down.

### The three routes, none of them taken

- **a heartbeat on the wire.** The child says "still working" while it works,
  and the parent's deadline becomes what it should be — silence, not duration,
  exactly as finding 20's fix. It is a protocol change (§8) and a new message
  kind, which is not additive (§8.6) and is the user's to decide;
- **watching the child's processor time.** A working child burns it and a hung
  one does not, which is observable without the child's cooperation. It is
  `/proc` on Linux, and the host is the platform-independent half of this
  project — putting an operating system's file layout in it is a worse trade
  than it first looks;
- **letting the caller set it.** `set_watchdog` is already there; what is
  missing is a way to reach it from outside, which means a name on the command
  line or on the wire, frozen forever.

Not guessed. The first is the right shape and the most expensive; the third is
the cheapest and the least honest, since it asks the user to predict what they
cannot measure yet.

# Found by asking how two of these run at once

## 24. Two processes creating one anchor shared a staging directory

**Would corrupt a cached blob, silently**, and it was found by a question rather
than by a hunt: *if a developer wants three measurements at once, is this
multithreaded or does he run it three times?*

He runs it three times. One emulator per process is this project's shape and is
deliberate — the backend is a C library with one emulator, one debugger and one
loaded image in global state, so `Reference` refuses a second in one process.
**Several processes is therefore how anything is done in parallel.**

And what they share is the cache. `Cache::put` staged its files in a directory
named after the key's digest **and nothing else**, then cleared it, filled it and
renamed it into place. Two processes creating the same anchor at the same time
shared that one directory, and the clearing of one deleted the other's files
mid-write.

It is not hypothetical. Three measurements of different parts of one piece of
software usually share a prefix anchor — the thing that gets them past the
opening — and on a cold cache all three race to create it. §13's Q12 already
recorded a sighting of this: *a blob file that several processes were
overwriting between one another's save and read-back*, caught then by §4.8's
cheap check.

**Fixed**: the staging name carries the process id, as the state file already
did. The entry it renames into is still the key's own, which is what makes the
blob appear atomically.

What is **not** fixed is the race itself: two processes may still both do the
work and one rename over the other. That is wasted effort rather than a wrong
answer — they computed the same blob from the same definition — so it is a cost
and not a defect. A lock would turn it into a wait, and nobody has measured
whether the wait is cheaper than the work.

## 25. Every run erased the previous run's log

**Annoyed**, and it quietly lost the only record of what the emulator said.

A reference wrote its emulator's output to `reference.log` — one fixed name,
beside the backend's home. Every run overwrote it.

The output is the only record of what the emulator was complaining about while
something went wrong, and the usual way to discover something went wrong is to
look afterwards — by which time a second run has already happened.

**Fixed**: the default is `awaseru-YYYYMMDD-HHMMSS-mmm.log`. A fixed prefix to
find and sweep them by, and the moment so that two cannot be confused. Kept
rather than rotated, because these are for a person — to read, to filter, to send
to somebody who might recognise what the emulator was saying, and to compare
against the same run on a later version. The civil-date arithmetic is written
out rather than taken as a sixth dependency for twenty lines.

**What is still true**: in the one-process command line the emulator's output
still goes to the host's own stdout, which is finding 3 and needs a
file-descriptor redirect. And in the server path, **stderr is the transport** —
the protocol's answers travel on it — so there is no log of stderr to keep
there. The emulator writes 130 lines to stdout and none to stderr, measured in
M4, so what is worth keeping is kept.

## 26. A reference left its scratch state file behind, every time

**Slowed, and it grew without bound.** Found by asking whether each process
should have a directory of its own.

A blob's bytes pass through a file on their way to and from the backend, which
only speaks in filenames. The file is named per-process — `awaseru-state-<pid>`
— which is right, and it was never removed. One afternoon on a small image had
left **thirty-four of them, five megabytes**. A long session on a large image
would be measured in gigabytes, and nothing would ever say so.

**Fixed**: the reference takes it when it goes. A failure to remove it is
ignored on purpose — a scratch file that will not delete is a tidying problem,
and refusing to finish because of one would make it the caller's.

## 27. The backend's own folders are shared between processes

**Would overwrite one run's screenshots with another's.** The same question
found it, and it is a decision rather than a fix.

`--home` is described as *where the backend may keep its own files*, and two
quite different kinds of thing live there:

| | kind |
|---|---|
| the scratch state file | one run's working file — already per-process, now removed with it |
| the log | one run's record — carries the moment since finding 25 |
| `Screenshots/`, `SaveStates/`, `Saves/`, `RecentGames`, `Debugger` | **the backend's own**, named by the backend, with no idea another process exists |

The backend names a screenshot after the software and a counter, so two
processes with one home write the same filename. Several processes is how
anything is done in parallel here (finding 24), so this is reachable the moment
anybody measures two things at once and looks at a picture afterwards.

**This entry drew the wrong conclusion about the cache and the correction is the
part worth keeping.** It said the cache was the opposite case and right as it
was, because a blob is expensive and reusable, so sharing it was the whole
point. That reasoning looked only at the cost of replaying and never at what
sharing a derived artefact between two pieces of work means. The cache had the
same defect as the home and a worse one: §6.7 already refuses a shared cache of
blobs, because a blob is the single artefact where a stale copy from somebody
else's run is **invisible** rather than noisy. The default obeyed §6.7's letter,
since nobody had configured anything, and broke its reason.

The decision that settled it was the user's, and it is a rule rather than a
preference: *nothing is shared by default; the person turns reuse on where they
decide it belongs; the tool does not infer it.* Slow because somebody has not
discovered a feature is a better failure than fast because the tool deduced
something and deduced it wrong — a wrong deduction costs the same hours and
also cannot be trusted afterwards.

Both directories are answered by the same thing, which is why neither needed its
own policy: a **session** (finding 28), one named directory holding one piece of
work, with the backend's home and the anchor cache inside it. Two pieces of work
are two directories, so the collision stops being guarded against and stops
being possible. What crosses from one session to another does so because
somebody asked for it, never because the tool went looking.

## 28. Two fixed paths that every invocation on the machine shared

**The cause behind findings 24, 26 and 27, which were each a symptom of it.**
Found by answering a question about handing one piece of work to somebody else,
and the question is what made it visible: nothing in the code reads wrongly.

`--home` and `--cache` each had a default, and each default was a single fixed
path:

```rust
let mut home  = std::env::temp_dir().join("awaseru-backend-home");
let mut cache = std::env::temp_dir().join("awaseru-anchor-cache");
```

So **every** run on the machine wrote into those two directories, whatever
software, anchor or piece of work it belonged to. Two runs against two different
pieces of software shared one anchor cache and one backend home. Nobody chose
that; it arrived with the walking skeleton, when there was one piece of work and
the default was obviously fine, and it was never revisited while three separate
findings were written about its consequences.

This is finding 20's shape again, which is the part worth noticing. The watchdog
had been wrong since M2, the code read correctly, the comment stated the right
principle, and it surfaced only when a legitimate run first exceeded ten
seconds. Here the code read correctly too, and §6.7's prose stated the right
principle in so many words — *"what does not travel is the cache"* — while the
implementation shipped the opposite as a default. **A principle written in the
specification is not a principle the code has.** Neither of these was found by
reading.

**Fixed.** There is no default any more: a run names a session with `--session
PATH`, which supplies both, or gives both paths. With neither, it refuses and
says both ways out. Taking a default away is normally the one thing not done
here — a parameter that shipped keeps working, and `--home` and `--cache` both
do, including as overrides inside a session — but a default that was wrong is
the one kind worth removing.

Two things fell out of it that are worth recording separately:

- An anchor's name was never validated. It did not matter while a blob lived in
  a directory named after a digest; it matters now that the directory is named
  after the anchor, because a name is then a path component and `../..` would
  decide where a write lands. Refused at the configuration, with the reason,
  rather than cleaned up at the store — silently rewriting somebody's name is
  how a name stops meaning what they wrote.
- The cache now holds **at most one blob per anchor name**. Under a digest it
  could hold several for one anchor — the same definition against a different
  reference or backend version — and that was free rather than owed: §4.11
  already says changing either invalidates the blob, and an invalidated blob is
  one that gets replayed. A session pins one reference and one version, which is
  why one slot is the right number. Written down as a test, so that widening it
  again has to be a decision.

## 29. A received blob would have cost the full replay, so a box was worth nothing

**Found by building the thing it breaks, and it is a decision rather than a
defect.** §4.9 demonstrates an anchor nobody has demonstrated before it is used,
of the tool's own accord, and the test for that is `demonstrated_with == 0`. A
blob that arrives in a box has a `demonstrated_with` that belongs to whoever
packed it, and a session that counted it as its own would be reporting evidence
it does not have — so it has to be zero here.

Zero means the demonstration runs. The demonstration replays from the origin.
**So taking a box in would have cost the 754 seconds the box existed to save**,
and the whole feature would have been a slower way of doing nothing.

The way out is not to relax §4.8 and not to force the replay. Both are recorded
and they are different facts:

- `demonstrated_with` is zero, because this session has demonstrated nothing;
- `demonstrated_elsewhere` says who did, and the arrival says so in words;
- the verdict carries §4.8's caveat, so every comparison from the blob is **not
  determined** until this session establishes it;
- and nothing demonstrates it automatically, because that would spend exactly
  what the box saved on work nobody asked for.

So a received blob resumes in milliseconds and is honestly labelled. The person
gets the speed and is told what they do not have, which is the three-valued
verdict of §2.3 doing the job it was put there for.

**§8.6 prevented the obvious mistake.** The first attempt was a new
`Undetermined` reason — "demonstrated elsewhere". But `Undetermined` maps onto
`Cause`, which is the vocabulary a verdict travels by, and §8.6 says a new
variant in a reply is not additive. §4.8's existing caveat is already the truth
about this session; who it was belongs in the report, not in the vocabulary. The
specification stopped a breaking change that looked like an improvement.

## 30. Two tests could not fail on what a reader sees

**Both found by running the tool and looking at the output, not by reading.**

A Rust string literal's `\` at the end of a line swallows the newline and the
next line's indentation. Written through a tool that eats the backslash first,
it does not — and what shipped was a sentence with twenty spaces in the middle
of it:

```
…so it is theirs: nothing                  replayed it to check…
```

Twice, in two different report lines. Both times a test asserted a **fragment**
of the sentence, and a fragment matched either way, so the test passed on
output no person would accept. Both now require the whole sentence and that it
contains no double space.

The lesson is narrower than "test the output" and more useful: **an assertion on
a substring of a sentence cannot fail on the sentence's shape.** Where the thing
under test is prose a human reads, the test has to look at it the way a human
does.

## 31. A mutation passed, and the mutation was wrong

**Worth recording because the conclusion was nearly the opposite one.** A test
said the name a person gives their emulator must not decide whether a blob
applies. Mutating the comparison to include that name left the test passing,
which looks exactly like a test that cannot fail.

It was not. The mutation assigned the name into the field the next line
overwrote — `reference` is written before `backend` in a key, so reading one
into the other's place changed nothing. Done properly, with a field of its own,
the test failed as it should.

**A mutation that passes is evidence about the mutation first and the test
second.** The audit's U3 found three real cases of tests that could not fail,
which makes the reflex to believe the mutation a strong one; this is the case
where believing it would have led to rewriting a test that was already right.

## 32. The leak check looks for names, and §11.2 forbids more than names

**Found by nearly committing one.** The frente's closing proof was two lines
pasted out of a real run, and they carried `stop=0x……` — a program counter
reached inside the supplied software. §11.2 says no **title, path, address,
mapping or reimplementation** of supplied software reaches the repository.
`tools/leak-check.sh` ran and said `clean`.

It was right about what it checks. Its own header says it: *"nothing that
reaches a remote may **name** it"*, and it works by searching every tracked file
for the words in a local, gitignored list. A word list cannot catch an address,
because an address is not a word anybody can list in advance — the whole point
is that it is discovered by running the tool.

So the check covers one of §11.2's five kinds and the other four rest on
somebody noticing. The addresses already in the repository are invented
fixtures (`0xC400CF`, `0xDEADBE`), which is why nothing had gone wrong yet; the
first real one arrived the moment a document quoted a run.

**Not fixed, and the reason is that the obvious fix is wrong.** Refusing every
`0x` followed by six hexadecimal digits would refuse the fixtures, which are
there on purpose and are not addresses of anything. Telling a real position from
an invented one needs to know which software is meant, which is exactly the
knowledge §11.2 keeps out of the repository. What might work is narrower: refuse
a hexadecimal literal in `doc/` and `spec.md` — prose has no need of one, while
test fixtures do — and say so rather than guessing. That is a decision about the
check and belongs with whoever makes §11.4's.

The pattern is the one this whole frente kept finding, in its sharpest form yet:
**a check that passes is a statement about what it checks, and reading its
output is not the same as reading its header.**

## 33. A control reported an offset with no region to place it against

**Found by using the tool from outside, in the first measurement that asked for
a control.** `Difference`'s `region` documents itself as *always present on a
difference this tool produced*, and §13's Q16 records that being answered. A
control's difference is one the tool produced, and it was absent.

The cause is the one the differ's own comment already names. Comparing through
`compare` folds several regions into one verdict: the fold keeps the difference
and loses which region it came from, which is exactly why `by_region` exists
for the main verdict. The control was still going through the fold.

So a client reading a control to find out **where** the perturbation was noticed
got an offset and nothing to place it against. With one span that is guessable;
with more than one — and the shape allows a list — there is nothing to guess
with, and the offset is then a number that looks precise and is not.

**Fixed.** The control compares region by region like the main verdict, and the
two verdicts it reports each carry their own region rather than the report's.
That last part matters: a control can be noticed in a region the main comparison
agreed about, and naming the main one would have been worse than naming none.

Nothing on the wire changed. `region` was already optional and already
documented as present; this made the documentation true.

The shape of the defect is worth more than the defect. **A field that documents
itself as always present, with one code path that does not set it, reads as
correct in every test that uses the other path.** The tests were right about
what they covered, and nobody had asked a control about a real region until
somebody outside the project did.

## 34. A client cannot ask to arrive at an anchor

**Found in the first hour of the first use from outside, by wanting to do the
most ordinary thing there is.** The vocabulary has eight commands — `Hello`,
`Capabilities`, `Regions`, `Read`, `Write`, `Run`, `Reverify`, `Examine` — and
none of them puts the reference at an anchor.

So a client **cannot resume a cached blob.** The only two ways to an anchor are
`Examine`, which arrives as part of a measurement the client may not want to
make, and `Reverify`, which replays the definition and therefore costs the thing
the cache exists to avoid.

Which means the whole of §4.7 to §4.12 — the anchors, the cache, the
demonstration, 686 seconds becoming 0.014 — **is unavailable over the protocol
except as a side effect.** A client that only wants to look at what the
reference holds at a position has to replay from power-on with `Run`, every
time. That is §10's half of the work, the reverse-engineering half, and it is
the first thing anybody does before they have a reimplementation to compare.

**Not fixed: it is a new verb, which is a decision rather than a repair** (§13's
Q26).

Worth saying what the absence is *not*. It is not an oversight in the sense of
somebody forgetting: §5.6 makes routine comparison the primary unit, and a
client doing only that never needs to arrive separately, because `Examine`
arrives for it. The vocabulary is exactly large enough for the thing the
specification calls primary. What the first outside use found is that **a person
does other things first**, and the smallest of them has no verb.

## 35. `moved` is absent in the one case where the count is zero

Small, and worth writing down because of where it falls. §5.2's movement is
reported as `Option<usize>`, documented *absent rather than zero where there is
no count* — and in a vacuous comparison it comes back absent.

But there **is** a count there: it is zero, and zero is the reason for the
verdict. The sentence beside the verdict says so in words — *the reference
changed none of the N bytes compared* — so the information is not lost, only the
machine-readable half of it.

A client that branches on `moved.is_some()` to decide whether a measurement was
real therefore concludes that nothing was counted, in exactly the case where
counting happened and produced the number that decided everything. One that
branches on the value is fine. The two readings disagree only here.

**Not fixed, because it is a shape question rather than a bug**: `None` and
`Some(0)` mean different things to a client and the right answer is to decide
which this is, not to change it quietly. Recorded so that the decision is one
somebody makes.

## 36. One budget serves two runs that have nothing in common

Found by reading, and recorded with that said plainly, because this project's
record is that reading does not find defects — so this is a structural
observation that has not yet cost anything.

A routine carries one `within`, and §5.6's measurement spends it twice: once
running from the anchor **to the entry**, and once running from the entry **to
the return**. Those two are not the same kind of number. Reaching a routine
depends on how far away the anchor is and can be millions of instructions;
running one is as long as the routine, and §4.5's whole point is that the second
bound must be tight.

So a budget large enough to reach a distant routine is a budget that no longer
bounds the measurement, and a budget tight enough to bound it cannot reach. One
field cannot be both.

It has not bitten: the subject of the first use is reached within two thousand
instructions of its anchor, which is also a tight bound on the routine itself.
That is the lucky case rather than the general one, and the general one is a
routine somebody has to run a frame of software to reach.

## 37. The supplied software's name was in this repository, and the check could not see it

**The worst finding in this file, and it is mine.** The subject's title, in its
common abbreviation, was written into this repository in six places — three test
doc comments, two source files using it as an example session name, one
measurements document — and pushed. `tools/leak-check.sh` said `clean` every
time, before every one of those pushes.

It was right about what it checks. It searches tracked files for the words in a
local list, and the abbreviation was not one of them. Finding 32 recorded that
the check covers one of §11.2's five kinds; this is the same gap with a real
leak behind it instead of a near miss, and in the kind the check **does** cover.

### Why the word was not in the list, which is the part worth knowing

Three letters and a digit. Put in a list that `grep -i` applies to every tracked
file, **binaries included**, it also matches a hexadecimal fragment — and this
repository is full of digests, addresses and fixtures that contain it by
coincidence. A list entry that fires inside a digest makes the check cry wolf, and
a check that cries wolf is one somebody turns off.

So the abbreviation is a word a word-list cannot hold. That is not an argument
for leaving it out; it is an argument that **the list is the wrong instrument for
a short name**, and nobody had noticed because nobody had tried to add one.

### What was done and what was not

The working tree is clean: all six occurrences are redacted, and the check now
finds nothing in any tracked file.

**The history is not, and cannot be from here.** Twenty-eight blobs name it, in
commits that are already on the remote. Removing them means rewriting published
history and forcing it, which is not a repair — it is a decision about a
published repository, and it belongs to whoever owns it.

### The shape, which is general

A private-word list is a **denylist**, and a denylist protects against what
somebody remembered. Every leak in this file got through one: the compiled
Python files that embedded a path (finding in §11.2's own header), the addresses
in finding 32, and now a title short enough to be mistaken for a number. The
alternative is an allowlist — a check that refuses anything in `doc/` and
`spec.md` outside a vocabulary — and it is more work and more false refusals and
it fails the other way, which is the way that is safe.

## 38. The parent's deadline makes establishing an anchor impossible over the protocol

**§13's Q21 bit, for the first time, and it bit the verb written to answer
Q23.** The twenty-third entry and Q21 have said since the audit that the
parent's two-minute deadline guards the rarest of three failures and charges
every legitimate run over two minutes for it. Nothing had cost anything yet.

Then `Demonstrate` arrived. §4.8's five steps replay the definition
`verify_from_origin` times and once more for the fourth step, so establishing an
anchor behind a long recording is several replays — minutes, not seconds. Asked
for over §8's protocol, the reference is a child process, and the deadline
refused it:

```
refused: looking for an answer from the reference, found the reference
process is still running and has not answered in 120s, so this has no
answer rather than a wrong one
```

**The refusal is right and that is the point.** Finding 20's fix is working: it
can tell a child that is still running from one that has died, and it says which.
It simply cannot wait. So a verb whose whole purpose is to turn a position that
resumes in milliseconds into evidence **cannot be used by a client at all**, and
the only way to establish an anchor today is in process, through the command
line, which is why that verb was added in the same unit.

What this changes about Q21 is not the argument — the argument was already
written in `child.rs`'s own comment, including the word *generous* being called
out as a bigger guess — but the **cost**. Q21 was a defect with no consequence;
it is now the thing standing between a client and §4.8.

**Not fixed, and raising the number would be the mistake the comment warns
about.** 120 against a demonstration's several hundred seconds is not a number
that is slightly wrong; the two have no relation, because one is a guess about
hanging and the other is however long a recording is. Finding 23's three routes
out are still the three routes out, and all of them cost a decision.

## 39. A fix for a state the code could not be in

**Found by looking at a cache entry after the thing I had just written ran.**

§8.5b's demonstration records itself against the blob. Finding 29 keeps a
packer's demonstration beside this session's rather than erasing it, because a
demonstration belongs to the run that performed it and where a blob came from is
provenance. So a blob that arrived in a box and has since been established here
should carry **both** facts.

The arrival's reply reads those two in a particular order, and the order is the
whole correctness of that function: this session's demonstration decides whether
a comparison is evidence, the packer's is provenance, and reading the packer's
first reports somebody else's work after this session has done its own. That was
written, tested with the both-true state made to happen, and mutated against.

**And the state could not occur.** §4.8's second step stores the blob through
the same path a replay uses, which writes a *fresh* entry — so the packer's name
was dropped there, before the demonstration got as far as recording anything.
The ordering fix was correct and unreachable, and the test that proved it
constructed a state the tool would never produce.

Worse, the specification said otherwise. §8.5b had *"both are kept"* in it, in
the present tense, an hour after being written. That is the rot the audit's fifth
unit hunts, committed by the same hand that wrote the hunt.

**Fixed**: the provenance is read before the store and put back after it, and
the both-established state is now a state the tool can be in.

The shape is worth more than the fix. **A test that constructs its own
precondition cannot tell you the precondition happens.** Making the state
happen is what the audit asks for, and it was done — in the test. What nobody
did was ask whether anything else could make it, and the answer was no.

### And then the same pair was read wrongly in a second place

Proved by running it. With the state finally reachable, an arrival said:

> *This blob was demonstrated by … and **not here**, so it is theirs … and
> establishing it in this session is what would make a verdict from it
> evidence*

about an anchor this session had just established, **beside a verdict carrying
no caveat at all** — the two halves of one line contradicting each other.

The protocol's reply had been fixed. The arrival's own §4.12 line had not,
because nobody looked for a **second reader of the same fact**. Fixed, with a
test that makes the both-true state happen and a mutation that fails it.

So the finding is really two: a fix for a state that could not occur, and then
the discovery that fixing one of two readers is not fixing the thing. **Where a
pair of fields has an order that matters, the question is not whether the order
is right — it is how many places read the pair.** Here it was two, and only one
was known about.
