# The numbers, and when they were last taken

Every measurement in `doc/` is a claim with a date on it. This says which were
re-taken by the audit before the FF5 use pass, which were not, and why.

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
| a cold arrival at the anchor behind the recording — 754 s, and the 44 000× it is worth | the cold half costs 754 s to re-take and the warm half is free. The ratio is so far outside the noise that a re-taking could only confirm it |
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
