# awaseru

Verified reimplementation harness for console software.

`awaseru` (合わせる — *to put two things together so that they match*) runs a
reference emulator alongside a reimplementation, compares their state at points
you choose, and reports where they diverge — so a program can be moved from
emulation to native code with evidence that nothing broke.

It contains no knowledge of any particular title: the emulator, the ROM and the
mapping are yours.

The specification is in [`spec.md`](spec.md) and is normative: where it and the
code disagree, the code is wrong.

Alongside it:

- [`doc/dependencies.md`](doc/dependencies.md) — every third-party crate, what
  it is for, and what the project would do without it. A crate that is not in
  that file is not in a `Cargo.toml`.
- [`doc/backend.md`](doc/backend.md) — where the reference emulator comes from,
  how it is built, the commit everything so far was measured against, and what
  was measured of its behaviour. The library is supplied, not shipped: you build
  or obtain your own and name its path in your machine-local configuration.

The test material is a program this project assembles itself —
`crates/awaseru-snes/src/fixture.rs`, a few dozen bytes of machine code whose
behaviour is defined where it is written. Expected values taken from software
that cannot be redistributed cannot appear in a public test suite, so they
don't: tests that need somebody's own ROM read its path from the environment
and skip when it is unset.

**Status**: the execution primitive. The host reads a configuration, brings a
reference up at a position it can return to, and arrives at **anchors** — named
positions it caches so that getting there is paid for once. Snapshots are read
off it and written back, compared with a verdict that distinguishes *agrees*
from *not determined*, and the same run gives the same state in three separate
processes. No differ and no external API yet. Version 0.0.0 holds the name.

Anchors are the part worth knowing about if you are reading this to decide
whether the tool is for you. Reaching a position by replaying from power-on is
what makes a verification loop spend its day waiting; an anchor turns that into
a cached state, and §4.8 of the specification is the machinery that proves
resuming the cache is the same as replaying — because if it is not, every
comparison below it measures the wrong machine and *passes*.
