# The external API: the wire, and what was measured before it was built

§8 of `spec.md` is normative. This file is the contract a second client could be
written from, and the record of the measurements the contract rests on.

At this point it holds the measurements only. The framing, the vocabulary and
the handshake are the units that follow, and each one adds its section here.

---

## The problem this had to solve first

§8.2 puts the protocol on the child process's standard input and output. The
backend library writes to standard **output**, from C++, beneath Rust's
capture — 130 lines of it in a single routine-level run. A length-prefixed
stream does not survive text injected into the middle of it, so the transport
could not be built until it was settled where the emulator's voice goes.

The decision taken before this milestone started: **the reference runs in a
child process, and the server owns a clean standard input and output.** The
emulator's noise is the child's problem, and the public contract of §8.2 is
untouched — the client still speaks to `awaseru` over its stdin and stdout.

What remained to be measured is everything about how the parent and that child
talk, because that part is internal and had no evidence behind it.

---

## What was measured of the streams

Five probes, run against the built library and the generated fixture of §11.3.
No production code was written in this unit; the probes were deleted and these
numbers are what they left behind.

### The library is silent until the processor runs

Opening the library, asking its version, and initialising it headless produce
**nothing at all** — not one byte, on either stream, with a 300 ms wait after
initialisation to be sure. The first output appears when the software starts
executing.

**And nothing is built on that.** A future version of the library with a
startup banner would corrupt a handshake that relied on the silence, and would
do it silently. The framing refuses what it cannot parse (§14.2) precisely so
that this class of surprise is a refusal rather than a wrong answer.

### What the chatter is, and where it goes

| | |
|---|---|
| volume | 130 lines in one routine-level cycle: 65 while the reference comes up, 65 over two frames, **0** during an address-bounded run |
| stream | all 130 on fd 1 (standard output); **zero** on fd 2 |
| content | one single class, `[CPU] Uninitialized memory read: $ADDR`, and nothing else — no errors, no warnings, no load failures |

So the child's standard output carries one diagnostic this project already has
a better answer for: §5.4's localisation and the access counters say which
instruction wrote a byte and when, which is more than "something read a byte
nobody wrote".

**It goes to a log file in the backend's home directory, not to a sink.** The
chatter is worth nothing today; a library that one day says something else on
that stream is worth having, and a log costs nothing. Discarding it would mean
discovering the next thing it says by not discovering it.

### Nothing reads standard input

The parent wrote 28 bytes to the child's standard input and closed it. After
the reference came up, ran two frames and ran to an address, **all 28 bytes
were still unread**. The library does not touch stdin, so the child's stdin is
free to carry commands.

### The internal channel: commands on stdin, answers on stderr

Chosen because the emulator never writes to fd 2 — measured above, twice — and
because it needs no `libc`, no `unsafe` (§17.1), no extra file descriptor and no
socket. §8.2's public contract is unaffected: this is the parent talking to its
own child.

The one hazard is ours, not the library's, and it was measured:

| the child | bytes on the answer channel | exit | what the parent sees |
|---|---|---|---|
| answers and exits | 10 — the answer | code 0 | the answer, then end of file |
| **panics** | **213 — the answer, then `thread '…' panicked at …`** | code 101 | a panic that looks like an answer |
| panics, with a hook writing to a log | **13 — the answer only** | code 101 | the answer, then end of file |
| exits without answering | 0 | code 3 | end of file and nothing else |
| is killed mid-command | the answer it had already sent | **signal 9**, no code | end of file, and `wait` names the signal |

So the child installs a **panic hook that writes to the log file** instead of to
standard error. Without it, a panic is 213 bytes of English in the middle of a
binary protocol. With it, the answer channel carries answers, and the panic is
still visible twice over: in the log, and in the exit code.

### A dead child cannot be mistaken for a slow one

Three independent signals, all measured:

- the answer channel returns **end of file** — `read` gives `Ok(0)`, not an
  error;
- `wait` gives either an exit **code** (0 for a clean exit, 101 for a panic,
  anything else for a refusal the child chose) or a **signal** (9 when killed),
  never both;
- writing to a corpse's standard input returns **`BrokenPipe`**, and the parent
  **survives** it. Rust's runtime ignores `SIGPIPE`, so a write to a dead child
  is an error value and not the parent's death.

One practical trap, found by the probe failing: `Child::wait` drops the child's
standard input handle, so a parent that wants to write to a dead child has to be
holding that pipe already. Taking the handle before waiting is the difference
between `BrokenPipe` and a panic about a handle that is gone.

---

## The framing

```text
frame := envelope_len:u32be  envelope:bytes[envelope_len]
         payload_len:u32be   payload:bytes[payload_len]
```

A second client is written against these numbers, so they are here and not only
in the code.

| | |
|---|---|
| prefix width | **four bytes**, **big-endian** (network order) — `struct.pack(">I", n)` in Python, which is the second client §M4 asks for |
| envelope | JSON text, UTF-8. Validated as text by the framing and as JSON by the layer above it |
| envelope limit | **1 MiB**. The control plane is one command or one result; the largest this project can imagine is a region list, which is kilobytes |
| payload | arbitrary bytes, carrying state (§8.3) |
| payload limit | **64 MiB**. §8.3 puts a snapshot at hundreds of kilobytes and a console's whole memory at a couple of megabytes; this leaves room for a batch and not for an accident |
| no payload | `payload_len` of **zero**. The prefix is always present |
| zero-length envelope | **refused** |

Four decisions, and the reasons rather than the rules:

**The payload's prefix is always there.** §8.3 makes the payload optional and
this is how: zero length. A reader that had to consult the JSON to know whether
more bytes followed could not find the end of a message it could not parse — and
finding the end of a message you do not understand is exactly what a version
refusal needs to do (§8.6). Four bytes of zero per message is the price.

**An empty payload and no payload are the same thing**, which costs nothing: a
payload carries state, and zero bytes of state is not a state.

**A zero-length envelope is refused**, because the shortest JSON value is two
bytes. A sender that offers none has lost its place in the stream, and saying so
is more useful than handing an empty string to a parser.

**Every length is checked against its limit before anything is allocated.**
Nothing a client sends is trusted, a length least of all: four bytes can ask for
four gigabytes. Reading the body then uses `take`, so a length that passed the
limit but exceeds what is actually coming costs what arrives rather than what
was claimed — the limit is the policy and the `take` is the floor under it.

**A clean end of stream is `Ok(None)`, and a stream that stops mid-frame is a
refusal.** That distinction is the whole reason this is not `read_exact` with an
error: §M4's child processes are read with "the sender finished" and "the sender
died" as different answers, and U1 measured that a dead child gives end of file
while a live one gives bytes.

### What the framing's tests do NOT cover

- **Concurrency.** One frame is written with one `write_all`, so two writers on
  one stream cannot interleave halves of a message — but nothing here tests two
  writers, because nothing in M4 has two.
- **A reader that must not block.** Everything here is blocking. A server that
  wants to do something else while waiting for a frame needs a different shape,
  and nothing needs one yet.
- **JSON.** The framing validates UTF-8 and stops. A valid frame can carry
  nonsense, and refusing that is the vocabulary's job.

---

## What these measurements do NOT cover

- **Another platform.** All of it was measured on Linux. The public transport of
  §8.2 is stdin and stdout and is portable; the *internal* choice of standard
  error as the answer channel is portable in the same way, but the measurements
  behind it — that this library writes to fd 1 and never to fd 2 — are this
  library's on this platform.
- **A library that writes to standard error.** If one ever does, the answer
  channel is contaminated and the route out is a Unix domain socket in the
  backend's home: `std` has it, it needs no port and no firewall dialogue, and
  it is immune to both streams. Recorded here so the next person does not have
  to rediscover the option; not built, because nothing needs it.
- **Several children.** One child is one reference, which is all §13's Q10
  allows this backend. Two children is what §5.5's cross-check would need, and
  this unit measured nothing about two.
