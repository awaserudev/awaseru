# The cycle, run against real software

§12's M5 asks for the cycle — choose a unit, write it, compare, fix until it
agrees, record provenance, commit — to run end to end without touching the
tool's internals. This is what that looked like.

The software is supplied by whoever ran it and is not in this repository, nor is
anything that identifies it: no name, no path, no address, no mapping and no
reimplementation (§11.2). What is here is what the **tool** said, which is the
part that is about the tool.

## Getting to a subject

Nothing in the tool helps choose one. It measures a routine you already know,
and there is no verb for "what is happening here". Three things had to be
written from outside, and none of them is exotic:

1. **read a region every frame and report which spans changed.** At the anchor
   this software writes twenty-four bytes a frame — an idle loop, a bad subject.
   Earlier in its boot, one span of some thirteen hundred bytes moves forward
   every frame, which is a buffer being filled and a good one.
2. **bisect to the instruction that wrote a byte.** The backend declares
   `stop-on-write` and `writing-position`; §8's protocol exposes neither, so this
   was rebuilt out of `run` and `read`: twenty replays and eighteen seconds,
   where one write bound would have cost one run.
3. **single-step and record the program counters**, which is how a loop's extent,
   a routine's entry and the address it hands control back to were found.

The third verified itself, and the shape of that is worth keeping: the trace
measures how long each instruction is by subtracting consecutive program
counters, knowing nothing about what they are, while an opcode table says how
long each should be without having seen the machine. When they agree, the read
is genuinely aligned — two sources, neither of them a comment in somebody's
disassembly.

## The wrong turn, and what the tool did with it

The first measurement was bounded by the address the routine hands control back
to. That address is in the caller's main loop and is reached constantly, so the
measurement stopped before its subject had done anything.

The tool did not call that agreement:

```text
not determined — vacuous: the reference changed none of the 12288 bytes
compared, so agreement here is agreement about data neither side wrote
```

§2.2 caught an empty reimplementation that a cruder comparison would have
passed. Bounding the measurement by the routine's own last instruction instead
took it from 4.8 seconds to 333 milliseconds and produced a difference.

What the report could not say is **why** nothing moved: a routine that writes
nothing and a bound that was hit at once produce the same sentence, and the tool
knows how far it ran. That is recorded in `doc/findings.md`.

## The rounds

| round | verdict | first differing byte | differing |
|---|---|---|---|
| 1 — deliberately empty | differs | 65536 | 7093 of 12288 |
| 2 — one step wrong | differs | 65704 | 2130 of 12288 |
| 3 | **agrees** | — | moved 7093 |

Round one is empty on purpose: a comparison that always agrees is passed by a
reimplementation that does nothing, so the cycle has to start by differing. Its
value is the instruction it came back with — §5.4's third item named the store,
and that is what made it possible to read that instruction's own bytes out of
the machine and write round two from them.

Round two is the useful one. The first differing offset **moved forward** and
the count fell from seven thousand to two thousand, which is the differ saying
"the shape is right and one step is wrong". A verdict of pass or fail cannot say
that. Nothing in the request told it where to look: both numbers came back from
the comparison.

Round three agrees, and §5.2's movement comes with it — seven thousand bytes
changed, so this is agreement about data somebody wrote rather than the vacuous
agreement §2.2 refuses.

Every round reported `complete: false`, because no control had run. The tool says
so rather than letting an agreement look finished (§5.3).

## The anchor's bracket

§4.9 brackets a session: the demonstration before, one closing check at the end.
On this software, after the cache-key defect was fixed:

| | |
|---|---|
| the demonstration, from nothing | 42 s — three replays from the origin and two onward runs |
| arriving afterwards | 0.3 s |
| §4.9's closing check | **6.1 s** — one replay, and the blob still produces what replaying produces |

The closing check is 6.9× cheaper than the demonstration, which is what one
replay against three plus two onward runs should look like. It is the number
that says the bracket is affordable: a session pays for assurance twice and not
on every use.

This did not run until the last unit of the milestone, because the two
interfaces keyed the cache differently and the blob one wrote was invisible to
the other. `doc/findings.md` has that as finding 5.

## The control

Every round above reported `complete: false`, because an agreement means nothing
until something has been shown to make it disagree. §5.3's pair, both inside the
input buffer and one byte each:

| | should be | was |
|---|---|---|
| a change the routine distinguishes | noticed | **noticed**, and `complete` became true |
| a change it cannot tell apart | not noticed | **not noticed**, and `complete` stayed false |

The first one changed **exactly one** of the twelve thousand bytes compared. Not
"something moved" — that byte and no other, at an offset the request never
mentioned.

The second is the half worth having, and the tool's wording about it is wrong in
an interesting way, which `doc/findings.md` records.

## What it cost

| | |
|---|---|
| the anchor the measurement starts from, first time | 42 s, which is §4.9's three demonstrations |
| the same anchor afterwards | 0.29 s |
| one round of the cycle | 0.3 s |
| finding the routine | three scripts and an evening |

The last row is the honest one, and it is the finding this milestone exists to
produce: the cycle is cheap and getting to it is not.

## Provenance, and the shape of what is missing

§9 wants a measurement to say what it was measured against, where each address
came from and with what confidence. The full record for this run is in the
workspace, because every line of it is about software this repository does not
name (§11.2). What belongs here is its **shape**, and the gap between the two.

A complete provenance for one measurement, as it had to be written by hand:

```text
software          an identity (a digest), and the declared divergence used to
                  reach it — here, memory zeroed at power-on, which is not what
                  the hardware does
tool              name and version
backend           name, version, AND build date
reference         the configured name
anchor            the definition, and whether the blob was demonstrated
each address      the value, where it came from, and HOW SURE
```

The tool records four of those:

```rust
Provenance { reference, backend, version, software }
```

So the backend's build date — which the command line prints — is not in it; nor
is the declared divergence, which is the most important thing to say about what
machine a measurement was made on; nor is any address at all, nor where one came
from, nor how sure anybody is.

That the addresses are missing is expected: §9.1–§9.3's mapping system is M7.
What is worth noting before then is the **confidence** column. Of the seven
values this measurement rests on, six were read out of the machine twice by two
different means and one was inferred from a single observation. Those two are
not the same kind of fact, and nothing in the tool can tell them apart — which
means a provenance it produced would present the inference exactly as
confidently as the measurement.

A well-formatted guess is the failure §9 exists to prevent, and the field that
would prevent it is one word wide.
