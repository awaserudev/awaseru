# What a report carries, and the two switches

**Provisional.** This is the smallest decision that lets M6 proceed, not the
settled answer. The tool has no users and almost no use, so the evidence that
would settle it does not exist yet: which fields are always present and never
read, and which are needed so often that having to ask for them is a tax.
Deciding now would be guessing with ceremony. See §13's Q17 for what stays open.

## The rule that is settled

> A field is a **switch** if asking for it costs a run of the machine, **or
> costs time proportional to the size of a region.** Everything else is always
> sent.

The second clause was added in M6, by measurement, and the first version of this
rule did not anticipate it — see the log at the end.

That is the whole criterion, and it is the one part worth fixing early, because
it decides future cases without another conversation: M6's coverage is a switch
because instrumenting costs; a new cause for `not determined` is not, because it
is already in hand when the answer is computed.

Two consequences follow from it, and both are about **not** adding things:

- **a free field never becomes a parameter.** Hiding something already computed
  saves nothing measurable — a whole region by value costs 1.4 ms and framing
  costs 0.10, against 79.7 ms for a cycle — and a client that does not want a
  field ignores it. What it costs instead is a frozen name and a way for two
  clients to disagree about what a report contains;
- **as few names are frozen now as possible**, because a parameter's name cannot
  change once it ships. Every switch that does not exist is a name that is still
  free.

## The switches

| | default | why |
|---|---|---|
| `localise` | **false** | §5.4 is a second full replay: it doubles a cycle (§13's Q1). Already on the wire as `examine.localise`, so its name and position are fixed |
| `coverage` | **false**, and it is a span rather than a flag | §10's execution coverage, landed in M6. Asking is naming a span on the `examine`; absent asks for none, which is the `false` this table promised. A flag would have meant "the whole region", and reading one costs 34 µs per kilobyte — about 70 ms for a cartridge, against 79.7 ms for a cycle. **A config default was considered and not built**: there is nothing sensible to default a span to, and nobody has asked for a project-wide one (§2.4). If use shows otherwise it goes in the log below |

That is the whole list today. §5.3's control needs no switch: a measurement runs
one when a perturbation is sent and not otherwise.

**§9's symbol names are not a switch either, and that is said here rather than
left to be noticed.** M7 made a report say what the mapping calls the byte that
differs and the instruction that wrote it. Naming costs a lookup over symbols
already in memory — no run, no region-sized read — so by the rule above it is
sent always. A project with no mapping files gets exactly the report it got
before §9 existed, because an empty mapping names nothing and the fields are
absent rather than empty.

Everything else a report carries — the verdict, all eleven causes for *not
determined*, §5.2's movement, `complete`, §4.12's beginning, the sentences, the
time taken — is always sent and is not switchable.

## Why new switches start at `false`

Turning a switch from `false` to `true` later is additive: clients that did not
ask begin receiving something extra, and one that ignores what it does not know
is unaffected. Turning one from `true` to `false` takes away what people already
had.

So the two directions are not equally reversible, and a switch whose right
default is unknown starts at `false` — which is the direction that can still be
changed. **Nothing is ever removed from the default**; that is the rule this
whole document is subordinate to.

### What makes that true, and does not exist yet

Additive is only free if a client tolerates fields it does not know, and the
replies in this protocol currently carry `deny_unknown_fields` — including where
the host's own parent process reads its child. So today a new field in a reply
breaks an older reader, and every addition would be a protocol version bump.

The fix is asymmetric and is the one piece of this that should land before M6
adds anything:

| | today | needed |
|---|---|---|
| **commands**, client to tool | strict | **strict** — a misspelled field must be refused and not ignored (§2.4) |
| **replies**, tool to client | strict | **tolerant** — a client ignores what it does not know |

## The log

When a switch had to be turned on, and why. This is the evidence Q17 is waiting
for: a switch that everybody turns on immediately was the wrong default, and a
field nobody ever reads was the wrong thing to send.

| when | switch | who turned it on, and what for |
|---|---|---|
| M5 | `localise` | every round of the cycle. The first measurement's value was the instruction it named — that is what made it possible to go and read those bytes and write the next round |
| M6 | `coverage` | once, in the client that drives the whole cycle, and deliberately in only one of its measurements — so that the other reports show the field **absent**, which is what "nobody asked" has to look like. Too early to read anything into: one use by the test that put it there is not evidence about defaults |

## The rule's first correction, and what forced it

M6's gate measured what reading execution coverage costs, and it is the first
answer in this project where **costing a run and costing bytes come apart**.

Coverage needs no extra run at all: the backend counts as it goes, so the
measurement is already done when the comparison ends. Under the rule as first
written — a switch only if it costs a run — coverage would therefore be sent
always.

But reading it costs **34 µs per kilobyte**, measured: 1.1 ms for a 32 KiB
region, and a 2 MiB cartridge is 72 MiB of record crossing the boundary at about
70 ms. A routine-level cycle is 79.7 ms (§13's Q1). So sending coverage with
every report would roughly **double** the cost of a measurement, for an answer
most measurements do not look at.

The rule now has a second clause. What it keeps from the first is the shape of
the question — *what does asking cost* — rather than the one it could have
drifted into, which is *how important is this field*. Importance is an argument;
cost is a number.

