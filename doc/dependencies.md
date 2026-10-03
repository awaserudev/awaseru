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
| `serde` 1.0.229 | deserializing the configuration, and writing the anchor cache's entries (§4.11) | hand-write the parsing, which is work with no payoff. The cache's use of it is safe to couple to a format precisely because §4.11 says a cache may be wrong: a format that changes invalidates a cache, and invalidating a cache costs only time |
| `toml` 1.1.6 | the configuration format, decided in §6 | change the format, which §6 chose for explicit structure, comments and unambiguous types |
| `sha2` 0.11.0 | the hash §6.6 identifies software by, and §4.11's anchor key and §4.8's coverage digests | write it out — which this project did, and `Not taken` below records why that was the wrong call |
| `serde_json` 1.0.151 | §8.3's control plane, which is JSON because every language reads it and a human can debug it | hand-write the encoder and the parser — which is the `sha2` mistake with different details: escapes, surrogate pairs and number formats are exactly the kind of thing that passes a published test vector and fails on a real client. §8.3 names JSON, so the format is not a choice this would be avoiding |

Approved 2026-10-02, for M0. Versions are recorded as each one is actually
taken, at its latest stable release looked up at that moment (§17.3) — never one
remembered or copied from elsewhere.

`sha2` is taken with `default-features = false, features = ["alloc"]`. Its `oid`
default brings `const-oid`, for naming the algorithm in ASN.1, which nothing
here does.

`serde_json` was approved for M4, on 2026-10-02, at its latest stable release
looked up at that moment (§17.3).

What it cost, measured rather than guessed: **30 lock entries became 34**, which
is what was predicted, with one of the four not the crate that was predicted.
`serde_json`, `itoa` (integer formatting, MIT or Apache-2.0) and `memchr`
(substring search, Unlicense or MIT) were expected. The fourth was predicted as
`ryu` and is **`zmij` 1.0.23** (MIT), the same author's newer
double-to-string crate — `serde_json` 1.0.151 has moved on from `ryu`, and the
prediction was made from memory of an older release. The record says the
measured name because the point of measuring is that it corrects you.

The lock file now holds **34** crates, which is what `cargo deny check` and
`cargo audit` run against on every commit.

Also worth writing down, because it decided something: **`libc` is already in
the lock file**, through `cpufeatures` under `sha2`. So the route M4 did *not*
take — protecting the server's standard output with `dup2` — would have cost no
new dependency at all. It was refused for a different reason: it needs `unsafe`
outside the backend's `ffi` module, which §17.1 forbids, and running the
reference in a child process is the more robust shape anyway (`protocol.md`).
The dependency count was not the argument.

What the first four cost, measured rather than guessed: the lock file went from
22 crates to 30. `sha2` accounts for eight of them — `sha2` itself, `digest`,
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
