# The two configuration files

§6.1 splits the configuration in half, and the split is the point: one file holds
**what must be true** and travels in the project's git, the other holds **where
things are on this machine** and never travels. §6.2 refuses an invariant written
in the local half, so there is no way to point the tool at different software by
editing a file that nobody reviews.

Everything below is a template. The values are placeholders, and deliberately so:
this document describes the shape, and a working example arrives with the tutorial
program, which is software that can be shipped.

## The shared half — `awaseru.toml`

```toml
[project]
platform = "snes"

[rom]
# The invariant, not the location. §6.2 refuses an override of this in the local
# half: pointing the tool at another revision makes every comparison meaningless,
# and meaningless comparisons that pass are worse than a stopped run.
sha256 = "<the software's digest>"

[[emulator]]
name = "ref-a"
platform = "snes"
backend = "<a backend this build knows>"
version = "<a version the backend crate lists>"

[reference]
use = "ref-a"
```

### §4.9's verification policy

```toml
[anchors]
# How many times a session demonstrates an anchor from its origin before
# trusting a cached blob, at the start of the session.
verify_from_origin = 3
# And whether it re-verifies once at the end, which catches a blob that drifted
# while the session was using it.
reverify_at_end = true
```

Bracketed rather than counted, and the arithmetic is why: re-verifying every
fiftieth use over ten thousand comparisons is about fifty hours of replaying, for
a property that either holds for the session or does not.

### The anchors — §4.7

An anchor is a **definition** plus, separately, a cached blob. The definition is
what is written here; the blob is derived from it and is never an input (§4.11).

```toml
[[anchor]]
name = "<a name of your own>"
# Exactly one bound, because §4.2 says every run carries one:
frames = 1200            # run to the end of this many frames
# instructions = 90      # or run this many instructions
# address = "<hex>"      # or run until the program counter reaches here,
# within = 100000        #   which needs a budget (§4.4) — an address bound is
#                        #   the first that can fail to arrive
# What §4.8's cheap check digests on every load. §4.10: declare the regions the
# comparisons read.
covers = ["work-ram", "palette-ram"]
```

Anchors chain. An anchor with no `after` begins at **power-on**; one with `after`
begins where that anchor ends:

```toml
[[anchor]]
name = "<a later one>"
after = "<the earlier one>"
frames = 600
covers = ["work-ram", "palette-ram"]
```

There is no `from = "power-on"` key, because a configuration that reads two ways
the day somebody names an anchor `power-on` is worse than one with two keys.

### An anchor that needs input

Where the software will not proceed without a button:

```toml
[[anchor]]
name = "<the one behind the input>"
input = "<a path relative to THIS file>"
frames = 17767
covers = ["work-ram"]
```

The path resolves against the file it is written in and never against the working
directory (§6.3). The log is read when the configuration loads, so a log named and
missing is a configuration error rather than a surprise at the moment it is needed.

**Today this anchor is refused**, and the refusal says so in full:

```text
the anchor `<name>` needs the input log `<path>` to be reached, which takes the
capability `input-replay` — replay a recorded input log — and this reference does
not declare it (§7.3). Reaching the anchor without it would arrive somewhere else
and call it this anchor, so it is refused instead (§2.4)
```

The first backend **can** replay a log — `doc/protocol.md` has the measurements —
and the capability is undeclared because `Platform` has no verb for asking.
`doc/findings.md` has what that needs. An anchor carrying a log takes no `after`:
a log returns the machine to its origin, so it begins at power-on and nowhere else.

## The machine-local half — `awaseru.local.toml`

```toml
[rom]
path = "<where the software is on this machine>"

[emulator.ref-a]
path = "<where that emulator's library is on this machine>"
```

Note the shape change: the shared half declares emulators as a list of tables with
a `name`, and the local half addresses them by that name (§6.4). The two halves
hold `emulator` in different shapes on purpose, so they are joined by name rather
than merged position by position.

## What is not in either file

The **cache** (§6.7) and the backend's **home** are machine-local and are given on
the command line, because they are scratch space rather than configuration:

```sh
awaseru --config awaseru.toml --local awaseru.local.toml \
        --home <a directory the backend may write in> \
        --cache <where the anchor blobs live> \
        --anchor <name> --state-digest
```

Deleting the cache costs time and nothing else (§4.11).
