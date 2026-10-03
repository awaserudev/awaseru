# What a report carries, and the two switches

**Provisional.** This is the smallest decision that lets M6 proceed, not the
settled answer. The tool has no users and almost no use, so the evidence that
would settle it does not exist yet: which fields are always present and never
read, and which are needed so often that having to ask for them is a tax.
Deciding now would be guessing with ceremony. See §13's Q17 for what stays open.

## The rule that is settled

> A field is a **switch** only if asking for it costs a run of the machine.
> Everything else is always sent.

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
| `coverage` | **false** | arrives with M6; instrumenting execution costs, by how much is not yet measured |

That is the whole list today. §5.3's control needs no switch: a measurement runs
one when a perturbation is sent and not otherwise.

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

