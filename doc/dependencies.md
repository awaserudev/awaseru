# Dependencies

Every third-party crate is listed here before it is used, with what it is for and
what the project would do without it. A dependency that is not in this file is a
decision for the user, not for whoever is writing the code — so work stops until
it is either added here or avoided.

The point is not purity. It is that a dependency is a piece of the tool someone
else maintains, and §16.7's argument about the reference emulator applies in
miniature to every one of them.

The reference emulator itself is the one dependency not listed here, because it
is not a crate and is not shipped. [`backend.md`](backend.md) is its record:
where it comes from, how it is built, and the commit every measurement so far
was taken against.

## Approved

| crate | for | what we would do without it |
|---|---|---|
| `libloading` 0.9.0 | loading the backend's shared library at a path that comes from the configuration at run time | link it at build time — but then the path stops being configurable and §6.1's split between the shared and the machine-local file collapses |
| `serde` 1.0.229 | deserializing the configuration | hand-write the parsing, which is work with no payoff |
| `toml` 1.1.6 | the configuration format, decided in §6 | change the format, which §6 chose for explicit structure, comments and unambiguous types |
| `sha2` 0.11.0 | the hash §6.6 identifies software by, and §4.11's anchor key and §4.8's coverage digests | write it out — which this project did, and `Not taken` below records why that was the wrong call |

Approved 2026-10-02, for M0. Versions are recorded as each one is actually
taken, at its latest stable release looked up at that moment (§17.3) — never one
remembered or copied from elsewhere.

`sha2` is taken with `default-features = false, features = ["alloc"]`. Its `oid`
default brings `const-oid`, for naming the algorithm in ASN.1, which nothing
here does.

What the four cost, measured rather than guessed: the lock file went from 22
crates to 30. `sha2` accounts for eight of them — `sha2` itself, `digest`,
`block-buffer`, `crypto-common`, `hybrid-array`, `typenum`, `cpufeatures` and
`libc`. `libc` arrives through `cpufeatures`, which is how the hash finds out at
run time whether the processor has instructions for it; it is declarations
rather than a C build, so §17.2's rule about linking C is not in question.

## Not taken, and why

| crate | why not |
|---|---|
| `bindgen` | there is no C header to read. The backend's exported API is declared only in its `.cpp` files and consumed through declarations maintained on the other side of its own interop boundary, so the binding here is transcribed by hand and reviewed against that source. This is a maintenance cost, and it is the cost §16.1's version check and §16.5's conformance suite exist to contain. |
| an assembler, for generating test ROMs | §11.3's fixtures are assembled byte by byte in Rust. A test fixture whose correctness depends on a tool nobody reads is a worse fixture. |
| ~~a SHA-256 crate~~ — **this was wrong, and `sha2` is now approved above** | The case made here was: §6.6 uses the hash as an *identity*, not as a security primitive — nothing defends against an adversary choosing the input, and a wrong answer stops the tool rather than letting something through — and the function has a published specification and published test vectors, so writing it out is checkable to a certainty. Both halves of that are still true, and the conclusion was still wrong. The hand-written one **passed the published vectors and was wrong**: a short incremental update discarded bytes it had just buffered, which a vector arriving in one call cannot reach. What caught it was an extra test somebody thought to write, and "somebody thought to write the right extra test" is not a property a project can rely on. Two hundred and twelve lines of crypto-shaped code that nobody else reads, to avoid a dependency that does not decay (§16.7 does not apply to a frozen hash) and is audited by everybody, was the worse trade. Kept here rather than deleted because the reasoning that produced it is the kind that will come back. |

## The rule

Adding a crate is a halt: the work stops, the case is made here — what it is for,
what it costs, and what the alternative is — and the user decides. "It is only a
small dependency" is not a case.
