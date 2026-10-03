# The mapping files

§9. What you know about the software, in files you write by hand. The tool holds
no mapping of its own (§9.4, §11.1) and loads what it is given.

Everything below is a **template**: the names are invented and the numbers are
arbitrary. A working example arrives with the tutorial program.

## Pointing the configuration at it

In the shared half, because a mapping is the thing a project most wants to share
and §9.2's second purpose is making it shareable. The paths are relative to the
file they are written in (§6.3).

```toml
[mapping]
files = ["mapping/concepts.toml", "mapping/addresses.toml"]
```

Several files, because §M7 is about a mapping **split** across them: what you
understood in one and where it is in another, or one per area. They load as one
graph, and a name used twice is refused with both file names.

## A symbol

```toml
[[symbol]]
name = "tile-buffer"
region = "work-ram"          # a region the backend exposes (§3.1, §8.5)
offset = 1024
length = 64                  # leave out for a point
groups = ["the-decompressor"]
description = "Where the expanded tiles land."
provenance = { how = "measured", note = "the span the memory actually changed" }
```

or, for something the program counter reaches:

```toml
[[symbol]]
name = "tile-expand"
address = 0x8040
groups = ["the-decompressor"]
description = "Entered once per screen."
provenance = { how = "measured", note = "the trace caught the call into it" }

[[symbol.relation]]
kind = "writes"              # whatever word you mean; the tool checks only `to`
to = "tile-buffer"
```

`length` is the one thing §9.1's list does not have and nothing works without: a
location there is a point, and the spans a measurement needs — what to seed,
what to compare — are made of lengths. Giving both a `region`/`offset` and an
`address`, or half of one, is refused rather than resolved (§2.4).

## A group

A group is a concept whose parts are scattered. It has to be declared, or a
mistyped name in a symbol quietly invents a group with one member:

```toml
[[group]]
name = "the-decompressor"
description = "Everything that turns the packed stream into bytes."

[[group]]
name = "its-inner-loop"
description = "The part that runs once per byte."
inside = ["the-decompressor"]
```

Asking for a group gives its symbols **and** those of groups nested inside it. A
group inside itself is refused, however many steps round, and the refusal prints
the way round.

## Provenance, which is not optional (§9.2)

```toml
provenance = { how = "measured", note = "..." }
```

| `how` | means |
|---|---|
| `measured` | observed on the machine |
| `inferred` | derived from something observed, but not observed |
| `assumed` | neither — somebody said so, or it came from elsewhere |

**Anything not `measured` is a hypothesis**, and a report that names such a
symbol says so, so that a reader is never shown a guess as a fact.

The `note` is required even under `measured`, and carries both of §9.2's
purposes. "Measured" does not say measured *how*, and §9.2 also wants an entry
derived from material that may not be redistributable to be identifiable rather
than mixed in — a mandatory note is what makes that findable.

## What the tool checks when it loads them

- the files parse, and a field nobody declared is refused **by name**;
- no two symbols or groups share a name, including across files;
- every group a symbol names, and every group a group is inside, is declared;
- every relation points at a symbol that exists;
- no group is inside itself;
- every `region`/`offset` location falls inside a region the backend exposes,
  checked where the region set is in hand (§6.5) rather than at the first
  measurement that wanted a name.

**A location written as an `address` is not checked**, and that is a gap rather
than a decision: a region set is names and sizes, with nothing about which
addresses reach which region. `doc/findings.md` has it as finding 16.

## What a report does with it

`doc/protocol.md` has the shape. The short version: the name arrives **in
addition** to the number, never instead — a mapping is written by hand and can
be wrong, and the number is what checks it.
