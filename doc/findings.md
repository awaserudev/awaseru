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
