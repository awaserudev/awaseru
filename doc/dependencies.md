# Dependencies

Every third-party crate is listed here before it is used, with what it is for and
what the project would do without it. A dependency that is not in this file is a
decision for the user, not for whoever is writing the code — so work stops until
it is either added here or avoided.

The point is not purity. It is that a dependency is a piece of the tool someone
else maintains, and §16.7's argument about the reference emulator applies in
miniature to every one of them.

## Approved

| crate | for | what we would do without it |
|---|---|---|
| `libloading` | loading the backend's shared library at a path that comes from the configuration at run time | link it at build time — but then the path stops being configurable and §6.1's split between the shared and the machine-local file collapses |
| `serde` | deserializing the configuration | hand-write the parsing, which is work with no payoff |
| `toml` | the configuration format, decided in §6 | change the format, which §6 chose for explicit structure, comments and unambiguous types |

Approved 2026-10-02, for M0.

## Not taken, and why

| crate | why not |
|---|---|
| `bindgen` | there is no C header to read. The backend's exported API is declared only in its `.cpp` files and consumed through declarations maintained on the other side of its own interop boundary, so the binding here is transcribed by hand and reviewed against that source. This is a maintenance cost, and it is the cost §16.1's version check and §16.5's conformance suite exist to contain. |
| an assembler, for generating test ROMs | §11.3's fixtures are assembled byte by byte in Rust. A test fixture whose correctness depends on a tool nobody reads is a worse fixture. |

## The rule

Adding a crate is a halt: the work stops, the case is made here — what it is for,
what it costs, and what the alternative is — and the user decides. "It is only a
small dependency" is not a case.
