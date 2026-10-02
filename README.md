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

**Status**: specification, no implementation yet. Version 0.0.0 holds the name.
