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

## The vocabulary

One pair of enums, `Command` and `Reply`, in `crates/awaseru/src/protocol.rs`.
Both bindings of §8.4 speak them: the subprocess binding encodes them into the
frames above, the in-process binding passes them as values. That is what makes
"same semantics, two bindings" structural rather than a promise — there is one
vocabulary and two ways of carrying it.

The wire types are **written out rather than derived from the tool's internal
ones**, because §8.3 says the data model is the contract. Deriving it would make
every internal field name a promise nobody wrote down, and renaming one for
clarity would break a client. Each conversion from an internal type is an
exhaustive match, so a variant added inside the tool stops the file compiling
rather than quietly serializing as something else.

### Commands

| command | asks for | payload |
|---|---|---|
| `hello` | the handshake; says which protocol the client speaks | — |
| `capabilities` | what the backend declares, **and what it does not** (§7.3) | — |
| `regions` | every region by name, with its size and access (§3.1) | — |
| `read` | a span of one region | the bytes come back in the reply's payload |
| `write` | a span of one region, written | the bytes go out with the command |
| `run` | a bounded advance (§4.2) — frames, instructions, an address with its budget (§4.4), or a byte with the end of its subject (§4.5) | — |
| `examine` | §5.6's cycle and all of §5's answers | the given spans' bytes, then the produced spans' bytes |

### Replies

`hello`, `capabilities`, `regions`, `bytes`, `written`, `stopped`, `report`,
`refused`. A refusal carries **both** what was looked for and what was found:
one that said "invalid request" would make a client's author guess, and §2.4
refuses guessing on this side of the line too.

### Where the bytes are

A command or reply that carries state does not put it in the JSON. The envelope
says how to cut the payload: each span in the command has a length, and the
payload is those spans' bytes **concatenated in the order the spans are
listed**. A client that can count can cut it, nothing is base64, and nothing is
doubled in size — which is §8.3's whole reason for having a binary payload.

### §2.3 on the wire

A verdict has **three** shapes, tagged `agrees`, `differs` and
`not-determined`, and the third carries a `cause` a client can branch on
(`vacuous`, `capability-absent`, `not-repeatable`, …) beside the sentence a
client can print. There is no boolean anywhere in a verdict, and no shape that
a two-valued client could mistake for one.

§5.4's third item has four shapes for the same reason: `not-looked`,
`not-available` with the capability named, `nothing-wrote`, and `at` with the
position and how many times the byte was written. "Nobody asked" is not the same
answer as "nothing wrote it", and neither is "the backend cannot say".

§5.3's absence is a shape too — `{"control":"not-run"}` with its sentence —
because a field reading `null` would be read as "no problem".

### The version

`hello` carries the protocol version both ways, and a mismatch is a refusal
naming both numbers. Negotiation is **not** invented here: §8.6 and §13's Q3 say
the first client written by someone who did not write the tool is what settles
how it should work.

## The two bindings, and why there is one implementation

§8.4 asks for a crate for clients in the host's language and the subprocess for
every other. Both go through **one** `apply`: a `Command` and the payload in, a
`Reply` and a payload out. The server decodes a frame, calls it, and encodes the
answer; it decides nothing.

`apply` has **no error type**. Every failure is a `refused` reply carrying both
halves (§14.2). A binding that returned a `Result` would make the server decide
how to turn an error into a reply, which is a second place for the protocol's
behaviour to live and the place the two bindings would drift apart.

What that buys, measured by deleting it: with the payload-length check removed,
a client whose arithmetic is off does not get a refusal — **it panics the
process**, inside a slice. The check is what turns a client's mistake into a
sentence naming the number needed and the number sent.

### Where the bytes are, exactly

For `examine`, the payload is:

```text
payload := given[0].length … given[n].length
           produced[0].length … produced[m].length
           control.span.length          (only when a control is named)
```

Nothing else, and nothing less: a payload that is not exactly that long is
refused with both numbers. A client whose spans and bytes disagree would
otherwise seed a routine with bytes nobody chose, and the comparison would be of
something else entirely while looking fine.

### What the vocabulary's tests do NOT cover

- **A client that sends nonsense that parses.** Every command round-trips and
  every refusal carries both halves, but nothing here checks what the *tool*
  does with a region name that does not exist or an offset past the end of one.
  That is the binding's to check, and it is checked where the binding is.
- **Field-level compatibility across versions.** Unknown fields are currently
  refused by the parser rather than ignored, which is the strict reading and the
  right one until §8.6 is settled: a client sending a field this tool does not
  know is a client expecting something this tool does not do.

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

## The transport

The client spawns `awaseru serve` (§8.1 — the client drives), writes framed
commands to its standard input, and reads framed replies from its standard
output. No ports, no sockets, nothing to allow through a firewall.

```text
client ──stdin/stdout, framed──▶ awaseru serve ──stdin/stderr, framed──▶ awaseru reference
                                                                         │
                                       the emulator's voice ─────────────┘──▶ a log file
```

**The server decides nothing.** It decodes a frame, hands the command to the
reference process, and encodes the answer. The handshake, the version check,
every refusal about a region or a span, and every verdict belong to the binding
of §8.4 — the same code the in-process binding runs. Proved by mutation: making
the server answer `hello` itself fails both of its tests, because the handshake
it is supposed to enforce is the child's.

What the server does own is the transport's own failures, and each is a reply
rather than a silence: a frame it cannot read, a reference process that will not
start, one that dies, one that goes quiet.

### One reference per server, never replaced

§13's Q10 gives a server one reference, and the vocabulary has no command that
asks for a second — structural, rather than a rule being enforced.

A reference that dies is **not** respawned. A fresh process is a fresh machine
at a fresh position, and handing that to a client mid-conversation would be
handing it a different machine wearing the same name. So the death is remembered
and every later command gets the same refusal, until the client closes the stream
and starts again deliberately.

That assertion needed two attempts. The first version of its test stopped one
command too early: a mutation that forgot the death still passed, because the
command right after a death is refused either way — the respawn only shows on the
command after *that*. The test now sends it, and compares the two refusals
word for word.

### Seeding and comparing across it

The payload's three segments in order — the given spans, the produced spans, the
control's span — are what a client fills to run §5.6's cycle over the wire. All
three are exercised: a right reimplementation agreeing, a wrong one caught at an
offset only the reference knows, a control that is noticed, inputs cut out of the
payload in two pieces rather than one, and a payload one byte short refused with
both numbers.

Two things that a careless version of this would have got wrong, both caught by
mutation:

- **the order of the segments.** Taking the control's bytes before the produced
  spans' makes the right implementation stop agreeing, because the routine is
  then seeded with the candidate's bytes. The order is the order the spans are
  listed, and the test that proves it is the one that seeds in two pieces.
- **where a difference's region comes from** (§13's Q16, answered). It is the
  report's, taken from the comparison that produced the verdict — **not** the
  localisation's, which is optional. Taken from the localisation, a client that
  did not pay for a replay got an offset with no region to read it against, and
  a test that always asked for localisation never noticed.

### What the transport's tests do NOT cover

- **A client that sends a frame while an answer is still coming.** The
  conversation is strictly one command, one answer. Nothing enforces that from
  the server's side and nothing needs to yet; a client that pipelines would read
  the answers in order and could not tell which belonged to what.
- **A server with no client.** Closing the client's end ends the conversation
  with a success status, which is tested; a client that vanishes mid-command is
  not, because a write to a dead client is the same `BrokenPipe` the child's
  side already exercises.

## The reference in a child process

| | |
|---|---|
| the child | `awaseru reference`, a subcommand of the same binary — a client installs one program, and `current_exe` is a path that always exists |
| commands | the child's standard input, framed |
| answers | the child's standard error, framed |
| the emulator's voice | the child's standard output, redirected to a log file |
| a panic | a hook that appends it to the log, so it cannot look like an answer |
| a refusal before the reference opens | **also framed**, then the child leaves |

Measured on the fixture: **324 lines** of the emulator's own output landed in
the log across one conversation, and every frame the parent read was a frame.

### Three ways a question goes unanswered, and none is a verdict

| | what the parent reports |
|---|---|
| the child died | `Died`, with what `wait` said: an exit status, `101` named as a panic, or a signal |
| the child is alive and not answering | `Silent`, with how long it waited |
| the child answered something unparseable | `NotOurs`, with what it said |

A measurement whose reference stopped existing has **no** verdict, and saying it
agreed or differed would be inventing one. §2.3's third value is for this.

`Silent` exists because of a mutation. With the answers written to the stream the
emulator owns, the parent did not fail — **it hung**. A tool whose failure mode
is "no output" is worse than one that says what it waited for, so the parent
reads frames on a thread of its own and gives the channel a deadline: a read on
a pipe cannot be given one, and §4.2's "no unbounded run" is just as true of
waiting for somebody else's.

### What the child's tests do NOT cover

- **A child that is alive with its answer channel closed.** Measured by
  mutation: the branch that would report it is never reached, because the kernel
  closes the read end of the command pipe when a process dies, so the *write*
  fails first with `BrokenPipe`. The branch is kept — a future child, or a
  library that starts writing to standard error, produces exactly that — and it
  is recorded as untested rather than counted as covered.
- **Two children.** One child is one reference, which is all §13's Q10 allows.
  Two is what §5.5's cross-check would need.
- **The client's streams.** Nothing here reads the client's standard input or
  writes its standard output; the server is the next unit.

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
