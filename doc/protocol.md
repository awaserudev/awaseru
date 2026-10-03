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
| `reverify` | §4.9's closing check: replay the anchor once and see the cached blob still produces what replaying produces | — |

### Replies

`hello`, `capabilities`, `regions`, `bytes`, `written`, `stopped`, `report`,
`refused`. A refusal carries **both** what was looked for and what was found:
one that said "invalid request" would make a client's author guess, and §2.4
refuses guessing on this side of the line too.

### Every message, field by field

A second client is written from this. Each example is a complete envelope; the
payload, where there is one, is described beside it.

**`hello`** — first, always. Nothing else is answered before it.

```json
{"command":"hello","protocol":1,"client":"a name for the logs"}
{"result":"hello","protocol":1,"tool":"0.0.0"}
```

A protocol the tool does not speak is refused with **both** numbers in it.

**`capabilities`** — §7.3, and both lists.

```json
{"command":"capabilities"}
{"result":"capabilities",
 "declared":["stop-on-execution","stop-on-write","write-recency","writing-position"],
 "absent":["stop-on-read","execution-coverage","call-and-return-events","register-writes","input-replay"]}
```

**`regions`** — §3.1, by the backend's own names (§8.5).

```json
{"command":"regions"}
{"result":"regions","regions":[{"name":"work-ram","size":131072,"readable":true,"writable":true}]}
```

**`read`** — the bytes come back in the payload, and `length` is what was
actually read.

```json
{"command":"read","region":"work-ram","offset":1024,"length":64}
{"result":"bytes","region":"work-ram","offset":1024,"length":64}
```

**`write`** — the bytes go out in the payload. Not a way to seed a machine: a
console is not its memories, and §5.3's perturbation is what this is for.

```json
{"command":"write","region":"work-ram","offset":768}
{"result":"written","region":"work-ram","offset":768,"length":64}
```

**`run`** — bounded, always (§4.2). Four bounds:

```json
{"command":"run","bound":{"bound":"frames","count":2}}
{"command":"run","bound":{"bound":"instructions","count":100}}
{"command":"run","bound":{"bound":"address","address":32800,"within":20000}}
{"command":"run","bound":{"bound":"write","region":"work-ram","offset":1024,"until":32783,"within":20000}}

{"result":"stopped","stop":{
  "reason":{"reason":"address-hit","address":32800},
  "position":{"position":"instruction-boundary","pc":32800},
  "arrived":true,
  "says":"stopped at 0x8020, which is instruction boundary at 0x8020"}}
```

`reason` is one of `bound-reached`, `address-hit` (with `address`),
`budget-exhausted`, `write-hit` (with `region` and `offset`), `refused` (with
`why`), `cannot-continue` (with `why`). `position` is one of `frame-boundary`
(with `frame`), `instruction-boundary`, `mid-instruction` or `unclassified`
(each with `pc`). `arrived` is carried rather than left to be derived, because
every client would otherwise write that match itself and the one that gets it
wrong compares a state from the wrong place.

**`examine`** — §5.6's cycle and all of §5's answers.

```json
{"command":"examine",
 "routine":{"name":"running-total","entry":32800,"returns_to":32783,"within":20000},
 "given":[{"region":"work-ram","offset":768,"length":64}],
 "produced":[{"region":"work-ram","offset":1024,"length":64}],
 "control":{"name":"the first input byte","span":{"region":"work-ram","offset":768,"length":64}},
 "localise":true}
```

`routine.from` names an anchor to begin from (§4.7) and may be left out, which
means wherever the reference already is. `control` may be left out, and its
absence is **recorded in the report** rather than skipped (§5.3). `localise`
costs a replay — about as much again as the measurement itself — so it is asked
for.

The reply is a report, and this is all of it:

```json
{"result":"report","report":{
  "routine":"running-total",
  "verdict":{"verdict":"differs","difference":{
      "region":"work-ram","first":1025,"expected":87,"found":80,
      "differing":63,"compared":64,
      "wrote":{"wrote":"at","position":{"position":"mid-instruction","pc":32812},"writes":1}}},
  "as_compared":null,
  "moved":null,
  "localisation":{"region":"work-ram","offset":1025,
                  "wrote":{"wrote":"at","position":{"position":"mid-instruction","pc":32812},"writes":1},
                  "replayed":true,"from":null},
  "control":{"control":"not-run","says":"no control was run, …"},
  "complete":false,
  "beginning":{"repeats":true,"settled":["work-ram","…"],"says":"began at a position it can return to, …"},
  "took_ms":162}}
```

- **`verdict`** is the field to read: everything that bears on it is already
  applied — §4.8's caveat for an anchor nobody demonstrated, §4.12's beginning
  that does not repeat, and §2.5's two readings of one measurement disagreeing.
- **`as_compared`** is present only when the comparison alone said something
  different, so a client that ignores it is never misled by it.
- **`moved`** is §5.2, present for agreement and absent otherwise — a report
  that said `0` for a difference would be stating something it does not know.
- **`complete`** is §5.3's first sentence, and false both when no control was run
  and when one was run and went unnoticed.
- **`took_ms`** is the only field that is not reproducible, and no comparison
  uses it.

**`reverify`** — the closing half of a session (§4.9).

```json
{"command":"reverify","anchor":"after-the-opening"}
{"result":"reverified","anchor":"after-the-opening","uses":9,"took_ms":163}
```

A client brackets its session with this: the tool demonstrates an anchor before
using one nobody has demonstrated, and the client asks for this at the end. If
it is **refused**, every comparison made from that anchor in that session is
void — which is why a disagreement is a refusal and not a field in a result.

Nothing is re-verified in between, and that is deliberate: the policy used to
replay from the origin every N uses, and a count cannot know what it is
spending. On a definition that replays in five minutes, ten thousand comparisons
would have spent fifty hours verifying and seventeen minutes comparing (§4.9).

**`refused`** — the shape every failure takes, and the only one.

```json
{"result":"refused",
 "looking_for":"a region named `nowhere`",
 "found":"this backend exposes no region named `nowhere`"}
```

Both halves are always there. A refusal that said "invalid request" would make a
client's author guess, and §2.4 refuses guessing on this side of the line too.

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

## What a routine-level cycle costs

§3.6 asks whether snapshots should cross the API by value or by handle, and says
the first real client and the measured cost of a routine-level cycle are what
would settle it. The client exists, so here are the costs. Both routes are in
the protocol as built: `examine` is the handle route — the tool holds both states,
compares them, and only a verdict crosses — and `read` is the by-value route,
where the bytes themselves come back and the client can look at them.

Five rounds each, on one machine, in a **debug build**. The absolute numbers are
therefore pessimistic; the ratios are the point.

| | per cycle |
|---|---|
| in process, verdict only | 79.7 ms |
| in process, with §5.4's localisation | 152.2 ms |
| over the wire, verdict only | 79.0 ms |
| over the wire, plus the output span read back | 75.0 ms |
| over the wire, a whole region by value (131 072 bytes) | 1.4 ms |
| framing 128 KiB, no backend at all | 0.10 ms |

What that says:

- **the transport is not the cost.** In process and over the wire are the same
  number to within the noise of five rounds, because both are dominated by the
  emulator running a routine. A protocol that cost nothing measurable is a
  protocol nobody has to design around.
- **a whole region by value costs about 1.3% of a cycle.** §3.6's worry was
  "hundreds of kilobytes for every comparison"; measured, that is a millisecond
  against eighty. Framing it is a tenth of that again.
- **§5.4's localisation doubles a cycle**, because it is a second full replay
  (`localise.rs` says so, and this is the number). Which is why it is asked for
  rather than always done — and the record of that is this measurement rather
  than an assumption.

The first version of this measurement asked for localisation over a candidate
that **agreed**, where there is nothing to localise, and came out *faster* than
the plain cycle — which is what gave it away. It now measures over a candidate
that differs, which is the only case where the replay happens.

## A client in another language

`clients/python/` holds one, in the standard library and nothing else:
`awaseru.py` is the framing and the transport, `routine.py` is the fixture's
routine reimplemented from its description plus two ways of getting it wrong,
and `selftest.py` checks both without needing a backend.

Two things the client deliberately does not offer, and they are the same two
rules the tool follows:

- **no `is_ok()` and no boolean near a verdict.** §2.3 has three values, and a
  client that could ask "did it pass?" is a client that reads the third as the
  first. `verdict_of` hands back the tag as a string and makes the caller say
  what it means.
- **no exception for a refusal.** A refusal is a reply carrying what was looked
  for and what was found (§14.2), and a caller that wants to read one should not
  have to catch it. The only exceptions are for a server that stopped answering,
  which is not a reply at all.

Its reads block, and that is safe for a reason worth writing down: the server
gives its reference process a watchdog and answers a refusal when that process
goes quiet, so a question always gets an answer or an end of stream. A client in
a language with a convenient deadline may add one; a client without one is not
left hanging by this design.

### Two things a client learns here rather than from the shapes

**A control leaves the machine perturbed.** §5.3's control runs the reference
again with an input changed, and the reference is left where *that* run ended —
so a `read` after a control returns the perturbed answer, which is correct and is
not what the comparison was about. Found by the Python client's own assertion
failing with its read at the end of the cycle; the read now happens before the
control, and the client says why.

**A control's verdicts carry no region.** The report's own difference names its
region (§13's Q16), and the two verdicts inside a control do not: they are
summaries of *whether the comparison moved*, which is what `noticed` is computed
from, and they come from a comparison folded across regions rather than one kept
per region. A client that wants a localisable difference from perturbed inputs
sends an `examine` with those inputs instead. Recorded rather than filled with a
wrong name, which is what taking the plain difference's region would have been.

### A conversation, end to end

What `cycle.py` does, which is §M4's done-condition:

1. spawn `awaseru serve --config … --local … --home … --cache …`;
2. `hello`, and check the protocol number that comes back;
3. `capabilities`, to see what may be asked for (§7.3) — `absent` included;
4. `regions`, to learn the names (§3.1) rather than assume them;
5. `examine` with the implementation you believe is right, and expect `agrees`;
6. `examine` with one you know is wrong, with `localise`, and read the first
   differing offset, its region, and the instruction that wrote the reference's
   value;
7. `read` the output span and compare it against your own, **before** any
   control;
8. `examine` with a `control`, and check that it was `noticed`;
9. close the stream, which is how the conversation ends.

Step 7 is before step 8 on purpose: a control runs the reference again with an
input changed and leaves the machine where **that** run ended, so a read after it
returns the perturbed answer.

### What a second client would need from this document

Everything above: the frame's four fields and their widths, the two limits, every
command and reply field by field, where the bytes are, the three shapes of a
verdict, and the order of a conversation. The Python client was written against
this document, and its self-test asserts the same frame vector the tool's own
test does — which is how a drift between the two is caught by whichever runs
first. Proved by mutation: writing the lengths little-endian fails the client's
own check before any conversation is attempted.

What this document deliberately does not promise: **a version other than 1**.
§8.6's negotiation is open (§13's Q3), and what exists instead is a refusal with
both numbers in it. A client built against this document should expect that
refusal rather than a fallback, and a second version of this protocol will be
written when there is a client whose author did not write the tool.

## Replaying a recorded input log

§4.7's input log, which §13's Q14 recorded as unreachable on this backend. It is
reachable, and the record was wrong about **why** it was not.

### What Q14 got wrong

Q14 said the backend "reports no control device at any of its eight indices, so
setting an input override stores a state nothing reads". Both halves were
measured and both were true. The conclusion drawn from them was not: that the
backend cannot accept input.

It cannot accept input **because nothing had ever told it a controller exists**.
A control device is created from the emulator's configuration, and this project
has never set that configuration — the record carrying it is the nested one §13's
Q13 refused to transcribe twice. So the console came up with no controller and an
input override had nowhere to land. The backend was not missing an ability; the
host was missing a sentence.

The lesson is narrower than "measure things", because the measurement was right.
It is that **an absence is not a cause.** Eight empty device slots were read off
the machine correctly and then explained by a guess, and the guess went into §13
wearing the measurement's authority.

### What a recorded log carries, and why that is the way in

A log is a plain zip of two text files.

**`Input.txt`** — one line per frame, one field per device:

```text
|..|............
 ^^ ^^^^^^^^^^^^
 |  the controller: twelve characters in the order ABXYLRSTUDLR,
 |  `.` for not pressed, any other character for pressed
 the console's own buttons
```

**`GameSettings.txt`** — lines of `name value`, lower case with dots, which are
the emulator's setting names and not the field names of any C++ struct:

```text
MesenVersion 2.2.1
MovieFormatVersion 3
SHA1 <the software's hash>
emu.consoleType Snes
snes.ramPowerOnState AllZeros
snes.enableRandomPowerOnState false
snes.region Auto
snes.port1.type SnesController      ← this line is the whole answer to Q14
snes.port2.type None
```

Playback applies those settings and then power-cycles. So the controller is
created by the act of replaying, from one line of a text file, without the host
touching the configuration record it twice refused to transcribe. The archive is
a documented container rather than a struct layout, which is why §13 called this
the most promising of Q14's three routes.

### What was measured

Against supplied software, with a log of 17 767 frames recorded through the
emulator's own interface by the person who owns the software. Thirty-five of
those frames have a button pressed; the rest is the software's opening playing
itself.

| question | answer | how |
|---|---|---|
| does `MoviePlay` take a path and nothing else? | yes | one string, accepted headless |
| does starting a log power-cycle? | **yes** | five hundred frames run first, then the log started: the state after one frame of playback is byte-identical to the same thing done on a freshly loaded machine. A log is a beginning, not an addition |
| does the recorded input arrive? | **yes** | checked by looking. The screenshot at the log's end shows the software exactly where the person who recorded it said it would be, with the player in control. A frame counter cannot say that and a screenshot can |
| does playback survive the debugger? | **yes** | eighty thousand single instructions stepped in the middle of playback, and playback still reporting itself live afterwards — with the screen having advanced past the point the recorded presses are at, so the input arrived *while* the host was stepping |
| do replays agree? | **yes** | three replays of three thousand frames reached one state, byte for byte, over every writable region and the processor |
| where does a log end? | at its own end | the full 17 767 frames stop exactly at frame boundary 17 767 and playback reports itself finished there, so the log's length is the bound and nothing has to guess |

### What a replay costs

The number that multiplies everything built on top of this.

| route | rate | the full log |
|---|---|---|
| one request per frame | 60 a second | 4 m 56 s |
| one request for the whole run | **165 a second** | **1 m 48 s** |
| with no debugger attached at all | 365 a second | 49 s |

**One request for many frames is 3.2× one request per frame**, and reaches a
byte-identical state at the same frame boundary — so the host asks for the whole
span at once. The cost removed is the host's, not the emulator's: the same frames
are emulated either way.

**Detaching the debugger is a further 2.2×, and is unusable.** With nothing
attached there is no break, so the end of the log can only be *noticed*, by
polling, and the machine runs on while it is being noticed. Three detached
replays of the same log were asked where they had stopped: three different
states, at three different positions. A route that is twice as fast and lands
somewhere else each time cannot reach an anchor, because an anchor is a position
that can be returned to (§4.7). It is recorded here so that nobody measures it
again.

So the debugger costs a replay 2.2× and buys the only thing that makes a replay
worth doing.

**The same lesson, much louder, for instructions.** A run bounded at one
instruction costs about 10 ms — 99 a second, measured in the test — while eighty
thousand instructions asked for as a single bound run in about half a second, or
some 160 000 a second. Three orders of magnitude, and none of it is emulation:
it is the request, the break and the wait around it. Anything in §5 that walks
instructions should ask for the span and not the step. Where a step at a time is
unavoidable, 10 ms is the unit of the bill.

### What is still missing, which is above this backend

The capability is **not declared** (§7.3), and the backend's own documentation
says why: `Platform` has no verb for replaying a log. Declaring it on the
strength of these measurements would pass §7.3's gate and let an anchor carrying
an input log be replayed with the log ignored — arriving somewhere else,
consistently, and caching it as the anchor. `doc/findings.md` holds the two
decisions that would change that.

### What it cost to find out, which is a finding of its own

Nothing above required a human to press a button, and that matters, because on
the machine this was measured on **a human could not**. The emulator's interface
delivered no keyboard input to the software for an evening. Two gates, both in
its code and neither in any document:

- the interface swallows every key while its menu bar holds keyboard focus, and
  a menu opened with the mouse can keep that focus after it has closed;
- the core discards all input when it believes its window is in the background,
  which a compositor can report wrongly.

The log was eventually recorded through that interface once the focus was taken
away from the menu bar. The lesson for this project is that a tool depending on a
person operating a GUI inherits every bug in that GUI, and that this one is now
depending on a *recording* instead, which it does not.

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

- **A child that is alive with its answer channel closed.** Still not
  arrangeable with this child. What *was* recorded here before is now wrong and
  the correction is worth more than the note: a failed write used to report a
  death of its own, which meant it reported one **over the top of an answer the
  child had already sent**. A child that cannot open its reference answers a
  framed refusal and exits, so the refusal can be waiting in the channel while
  the write hits a closed pipe — and the parent said "the reference is gone"
  where the child had said why, about one run in ten. A failed write now falls
  through to the read, so the channel's contents win and a disconnected channel
  is the only path to a death. There is a test that makes that deterministic by
  waiting for the child to leave before asking.
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

---

## A note on what this directory may not contain

§11.2 keeps the software this project is developed against private, and the
Python client is where that rule was broken once. The *source* never named it;
the **compiled bytecode** did, because Python embeds the absolute path of the
machine that compiled it, and this machine's path names the software. Two `.pyc`
files were committed before anyone noticed.

So: `__pycache__/` is ignored, and `tools/leak-check.sh` searches every tracked
file **as binary**, every commit message, and every blob in history. The words it
looks for are not in this repository — writing them here would be the leak —
they come from a local `.private-words`, and without that file the check fails
rather than passing while checking nothing.
