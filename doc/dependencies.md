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
| `libloading` 0.9.0 | loading the backend's shared library at a path that comes from the configuration at run time | link it at build time — but then the path stops being configurable and §6.1's split between the shared and the machine-local file collapses |
| `serde` 1.0.229 | deserializing the configuration | hand-write the parsing, which is work with no payoff |
| `toml` 1.1.6 | the configuration format, decided in §6 | change the format, which §6 chose for explicit structure, comments and unambiguous types |

Approved 2026-10-02, for M0. Versions are recorded as each one is actually
taken, at its latest stable release looked up at that moment (§17.3) — never one
remembered or copied from elsewhere.

## Not taken, and why

| crate | why not |
|---|---|
| `bindgen` | there is no C header to read. The backend's exported API is declared only in its `.cpp` files and consumed through declarations maintained on the other side of its own interop boundary, so the binding here is transcribed by hand and reviewed against that source. This is a maintenance cost, and it is the cost §16.1's version check and §16.5's conformance suite exist to contain. |
| an assembler, for generating test ROMs | §11.3's fixtures are assembled byte by byte in Rust. A test fixture whose correctness depends on a tool nobody reads is a worse fixture. |
| a SHA-256 crate | §6.6 needs a hash to say whether a file is the one the configuration was written against. This is an **identity**, not a security primitive: nothing here defends against an adversary choosing the input, and a wrong answer stops the tool rather than letting something through. That makes it the rare case where writing the function out is cheaper than owning a dependency — it has a published specification and published test vectors, so it is checkable to a certainty, which is the only reason this is acceptable. `crates/awaseru/src/digest.rs` carries the standard's vectors, a cross-check against the system's own `sha256sum` at every length around a block boundary, and a test that the digest does not depend on how the bytes arrive — which caught a real bug in the incremental path that the published vectors did not. **If the project ever needs this hash to resist an adversary, that is a different requirement and the crate becomes a case worth making.** |

## The rule

Adding a crate is a halt: the work stops, the case is made here — what it is for,
what it costs, and what the alternative is — and the user decides. "It is only a
small dependency" is not a case.
