# Sessions: what is kept, what travels, what is refused

A person doing real work has several pieces of work open at once, against more
than one piece of software, and wants to hand one of them to somebody else
without handing over the rest. This document is how that works and, more
importantly, what it refuses to do.

## The governing rule

**Nothing is shared by default. The person turns reuse on where they decide it
belongs. The tool does not infer it.**

Slow because a person has not discovered a feature is a better failure than fast
because the tool deduced something and deduced it wrong. §4.12 exists because
thirty hours were once spent replaying an opening sequence; a tool that spent
thirty hours on a wrong deduction would have cost the same and also be
untrustworthy, which is worse.

This has a boundary with §4.12, which requires the tool to resume rather than
replay wherever it can. Both hold, with the line drawn here:

| | |
|---|---|
| within one session | resume always. This is §4.12, and it is not sharing |
| across sessions | never by inference. Only by an explicit act |

§4.12 settles the rest by itself: it already requires a run to say where its
time went. Replaying is honest as long as the report says a kept box would have
made it instant. **Saying is not deciding** — it is how the feature becomes
discoverable without the tool inferring anything and without coupling one
session to another.

## A session

A session is named by the person. The name is mandatory; there is no default
derived from a process id, because a name is what makes a session addressable
tomorrow, resumable after a crash, and shippable at all.

A session pins **one** piece of software and **one** backend version. That is
what makes it a unit rather than a pile, and it is why its contents can be named
after the things in them instead of after digests.

```
<name>/
  session.toml        the software's identity, the backend and its version, when this began
  anchors/<name>/     key, entry.toml, blob — one directory per anchor, named after the anchor
  runs/               what was asked, when, and what came back
  home/               the backend's own scratch. Does not travel
  logs/               does not travel
```

Nothing leaves that directory. Two sessions are two directories, so a collision
between them stops being something guarded against and stops being possible.

What is still read from outside is **input**: the shared configuration half, the
mapping, an input log. Reading a file is not shared state.

### Why the digest stopped being the directory name

A digest is correct and unreadable, and a person choosing what to hand over has
to be able to see what they have. The digest does not disappear: it stays inside
`key`, and `key` is still what validates. It merely stops being the name.

This costs something. Under `{digest}/`, two different definitions could not
land in one directory. Under `<anchor name>/`, they can — a definition edited on
one side arrives from the other with the same name and a different key. That is
what the refusal below is for.

### How it is spelled

```
awaseru --session work/a-name --anchor settled
```

One option carries both the name and the place: the name is the path's last
component. There is no root anywhere that sessions collect in, so nothing has to
be looked up to find them and a person who can see the directory can compress
it.

`--home` and `--cache` are unchanged and still override what a session would
have supplied. What went away is their default, which was a single fixed path
per machine that every piece of work shared — with neither a session nor both
paths, a run now refuses and says both ways out. Removing a default is normally
the one thing not done here; a default that was wrong is the exception.

A session is open while a process holds it, which a `lock` file says. A second
process is refused and told who holds it. **Whether that process is still alive
is not guessed at**: a machine that stopped never got to say so, there is no
portable way to ask, and a timeout would be a guess wearing a constant. The
refusal names the file to remove, and the person decides — the same reason
nothing else here infers anything.

An anchor's name is refused at the configuration unless a directory can carry
it, because the name is now a path component.

## The record

Nothing in this tool used to write a verdict to disk. A run was a `Plan` in, an
`Outcome` out, printed, gone. A session's `runs/` is where that stops: what was
asked, when, against which software identity and backend version, and what came
back — with §2.3's three values intact on disk and not only on a screen.

A received run is read back through a verb, not by reading the directory.
Reading the directory would bind every consumer to the layout forever; a verb
keeps the layout an implementation detail and the protocol the contract, under
§8.6 (a new field is additive, a new variant is not).

## A box

What travels is not the cache. The cache is a warehouse; a box is drawn from it.

A box is selected by anchor name and closes over `Anchors::chain(name)` — that
anchor's **ancestors**, and the definitions, input logs and blobs they need.
Siblings do not travel. Two pieces of work that branch from a common trunk share
the trunk, so handing over one of them hands over the trunk and not the other
branch.

The box carries the closure rather than only the leaf blob, because §4.11 says a
blob is a cache and never an input, and a cache nobody can re-derive is a cache
that can only be believed.

**A box carries the software's identity, never the software.** A receiver whose
copy differs is refused, and that is a consequence of how the key works rather
than a rule anybody had to be told.

## Restoring refuses

Restore rebuilds the key under the receiver's own names — the reference's name
is a wording choice in a configuration file and must not decide whether a state
applies — and then compares part by part.

| | |
|---|---|
| the key matches | nothing to do, and it **says** nothing was done |
| the name matches and the key does not | **refused**, naming the part that differs |
| nothing is there | written |

Restore never replaces. Work already on the receiver's disk cannot be damaged by
a box, because replacing is not a thing restore is able to do.

A box of several anchors where one does not apply does not report one answer for
the set. It reports per anchor, in §2.3's three values; a set has no single
verdict.

### Evidence does not merge

Identical state under an identical key is the same state. A demonstration is
not. §4.8 and §4.9 make a demonstration the property of the run and the machine
that performed it, so a sender's demonstration arrives recorded as the sender's
and never in place of the receiver's.

## What is not claimed

A verdict compares the reference against a reimplementation. A box contains no
reimplementation, so a box cannot carry a verdict — only positions, readings and
the record of what was asked. Where a verdict does travel, it travels pinned to
the build that produced it, because a verdict about a build that no longer
exists is worse than no verdict.
