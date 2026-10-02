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
  how it is built, and the commit everything so far was measured against. The
  library is supplied, not shipped: you build or obtain your own and name its
  path in your machine-local configuration.

**Status**: the walking skeleton. The host reads a configuration, selects a
backend by name, drives a reference to a bounded position and prints a region's
bytes. No differ and no external API yet. Version 0.0.0 holds the name.
