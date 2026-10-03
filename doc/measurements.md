# The numbers, and when they were last taken

Every measurement in `doc/` is a claim with a date on it. This says which were
re-taken by the audit before the first use pass, which were not, and why.

**Nothing moved enough to change a decision**, which is the only question that
matters here and was asked of each one.

## Re-taken, and what they say now

| what | as written | re-taken | moved |
|---|---|---|---|
| a routine-level cycle, in process | 79.7 ms | **73.6 ms** | −8% |
| the same over the wire | 79.0 ms | **73.2 ms** | −7% |
| with §5.4's localisation | about twice a cycle | **151.6 ms**, 2.06× | unchanged |
| a whole region by value (128 KiB) | 1.4 ms | **1.09 ms** | −22% |
| framing 128 KiB with no backend | 0.10 ms | **0.105 ms** | unchanged |
| the access record, 32 KiB | 1.1 ms, 34 µs per KiB | **0.77 ms, 24 µs per KiB** | −30% |
| one single-instruction run | 10 ms, 99 a second | **10.2 ms, 98 a second** | unchanged |
| a recorded log replaying | 165 frames a second | **192 a second** | +16% |

### What the movement does and does not mean

The absolute numbers drift with the machine: the same code, a differently loaded
computer, and eight to thirty per cent either way. **The ratios did not drift**,
and every decision in this project rests on a ratio rather than on a number:

- **the transport is not the cost** (§13's Q1): 73.6 ms in process against 73.2
  over the wire, the same within noise, as it was at 79.7 and 79.0;
- **a whole region by value is affordable**: 1.09 against 73.6 is 1.5%, where
  the decision was taken at 1.3%;
- **§5.4's localisation doubles a cycle**, which is why it is asked for rather
  than always done: 2.06× now, 2× then;
- **coverage over a cartridge is too expensive to send always**
  (`doc/report-options.md`'s second clause): at 24 µs per KiB a 2 MiB cartridge
  is about 49 ms against 73.6 for a cycle. The number fell and the conclusion
  did not — sending it with every report would still add two thirds.

That is the useful thing this re-taking established, and it is worth more than
the fresher numbers: **a decision resting on a ratio survives a machine; one
resting on a number would not have.**

## Not re-taken, and why

| what | why |
|---|---|
| ~~a cold arrival at the anchor behind the recording — 754 s~~ | **re-taken after all, by the session frente, and it turned out to be a composite.** See below |
| detaching the debugger — 2.2× faster, and useless | it needs three detached replays of a 17 767-frame log, and what it established is a **shape** — three replays landing in three different places — rather than a number |
| asking for a whole span of frames at once — 3.2× | the same: the conclusion is that one request for many frames reaches a byte-identical state, which is not a number that drifts |
| the bring-up measurements in `doc/backend.md` | taken once, on a machine coming up. They are facts about what the backend does, not about how fast it does it |

## How to re-take them

- the cycle, the region and the framing: `cargo test -p awaseru --test the_cost`
  with `AWASERU_TEST_BACKEND` set. It needs no software and no recording;
- the single-instruction rate and the replay rate: `the_input_log` with
  `AWASERU_TEST_SOFTWARE` and `AWASERU_TEST_INPUT_LOG` as well (§11.3);
- the access record: it has no timed test, and the audit measured it with a
  throwaway. **That is a small gap** — a number four documents rest on, with
  nothing that re-takes it.

---

## Re-taken by the session frente, 2026-10-03

### The cold arrival was a composite, and nobody had said so

The 754 s figure is the one four documents rest on and the one the audit
deliberately did not re-take, on the grounds that the ratio against 0.017 s was
too large to be worth 754 s of re-measuring. Re-taking it for this frente — a
fresh session, an empty cache, the same recording — gave **686.6 s**, nine per
cent under the 754 s, which is ordinary machine drift and says nothing
interesting. What it did say is what the number is **made of**, and that is the
useful part.

| what | taken |
|---|---|
| **one replay** of the definition, which is what produces the blob | **99 s** |
| then §4.8's demonstration: three more for step 1, one for step 4 | four replays |
| **the whole cold arrival, which is what the 754 s line reported** | **686.6 s** |

So a cold arrival is **five replays and not one**: 99 s of it produces the blob
and the other 587 s establish it. Five replays at 137 s on average against the
one measured at 99 s is the demonstration's own cost — each pass also witnesses
every writable region, which the replay alone does not. `Arrived.took` is measured
from the top of `arrive` and the value is built *after* the demonstration, and
`demonstrate` replays `verify_from_origin` times to show the definition is
deterministic (step 1) and once more to show that running onward from a resumed
blob agrees with running onward from a replayed one (step 4). The decomposition
was always there to be read in the code; the number went into four documents
without it.

**What this changes and what it does not.** Nothing resting on the ratio moves:
resuming is still four orders of magnitude cheaper than arriving cold, which is
the only thing the number was ever used for. What it changes is what the number
is *called*. A line that says "a cold arrival costs 754 s" and a line that says
"a replay costs 99 s and the policy asks for three more" lead to different
decisions — the second one says **the policy is four fifths of the bill**, and
that is a thing a person can change. `verify_from_origin` is the number doing
it, and nothing in `doc/` had ever said that it multiplies the cost of first
arriving at an anchor by five.

### The box, and what it carries

| what | taken |
|---|---|
| the whole box: blob, key, entry, definitions, a 17 767-frame recording | **168 KB** |
| the recording inside it, against the original | byte-identical, 1 362 bytes |
| a receiver's arrival from a restored box | **0.014 s**, which is **49 000×** under the 686.6 |
| the same, in the session that packed it | 0.011 s |

The receiver was built from the travelling half alone, with its machine-local
configuration written from scratch, no session, no backend home and no cache.
Both sides then answered the same question by digest and agreed exactly — the
same stop position and the same state — differing only in `evidence`, which the
receiver does not have and does not claim.

### How to re-take these

- the box and both arrivals: `save` into a directory, copy it beside a
  configuration whose local half you write yourself, `restore`, and ask for the
  anchor with `--state-digest` on both sides;
- the cold arrival: a session with an empty cache and `--anchor`. It costs what
  it costs, and the table above is now the reason to be careful about which part
  of it you are quoting.

---

## Re-taken by the verbs frente, 2026-10-03

### What establishing an anchor costs, which is what a box is worth

The open question about handing a box to somebody asked for one number and
nobody had it: a blob that arrives in a box resumes in milliseconds and no
comparison from it is evidence, so **what does changing that cost?**

| what | taken |
|---|---|
| arriving at a cached anchor, over the protocol | **9 ms** |
| one replay of the definition | 99 s |
| **establishing the anchor here** — §4.8's five steps, the blob already present | **582.5 s** |
| a cold arrival: one replay, then the demonstration | 686.6 s |

### The answer, and it has two halves that must not be mixed

**A box is worth 76 000× when what you want is a position, and 15% when what
you want is evidence.** Both are true and they answer different questions.

Arriving is 9 ms against 686.6 s, which is the ratio that gets quoted. But
arriving establishes nothing, and a comparison from an unestablished anchor is
*not determined* (§4.8). To get a verdict a receiver has to demonstrate, and
that is **582.5 s** — against the 686.6 s a cold arrival costs, which is the
same demonstration with one replay in front of it.

So the arithmetic that matters:

| | |
|---|---|
| what a box saves a receiver who needs evidence | 686.6 − 582.5 = **104 s**, about **15%** |
| what establishing costs against one replay | 582.5 / 99 = **5.9×** |
| what it costs against the whole cold arrival | 582.5 / 686.6 = **85%** |

**The demonstration is 85% of the bill.** Which makes the decision not to
demonstrate on arrival clearly right rather than merely defensible: a client
asking where the reference is would otherwise have been charged almost the whole
cold price for a look.

And it makes the box's value honest rather than inflated. "686 seconds becoming
9 milliseconds" is a true sentence about **looking**, and it is the wrong
sentence about **verifying** — where the box buys 15%. Anybody quoting the first
number about the second thing is wrong by a factor of five thousand.

The number `demonstrate` reports is three, which is §4.9's `verify_from_origin`
and counts step 1's replays only. The work is four replays — three for step 1
and one for step 4 — plus witnessing every writable region after each, which is
why 582.5 over four is 146 s against a bare replay's 99.

### How to re-take it

`awaseru demonstrate <anchor> --session PATH`, in process. **Not over the
protocol**, where the parent's two-minute deadline refuses it before it finishes
(`doc/findings.md`'s thirty-eighth entry) — which is itself the measurement's
most useful by-product.
