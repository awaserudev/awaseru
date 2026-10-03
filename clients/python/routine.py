"""The fixture's routine, reimplemented in Python — and two ways to get it wrong.

§M4's done-condition needs a reimplementation in a language that is not the
host's. This is it: the same transformation the generated fixture's subroutine
performs, written from its description rather than translated from the Rust.

The transformation is a running total, eight bits and wrapping, exclusive-or'd
with a constant on the way out. Every output byte therefore depends on every
input byte before it, which is what makes two different mistakes show up at two
different offsets.

The wrong ones are wrong on purpose, and each in a way the other is not:

- `without_the_chain` forgets to carry the total forward, so each output depends
  only on its own input. Its **first** byte is right and its second is wrong,
  which is what makes "the first differing offset" a claim worth testing rather
  than a constant.
- `without_the_mask` forgets the exclusive-or, so it is wrong from the first
  byte.

Nothing here reads anything from the tool. These are a client's own code, which
is the whole point: the offsets the tool reports are numbers it worked out from
the reference, not numbers the client sent it.
"""

MASK = 0x5A


def running_total(data: bytes) -> bytes:
    """What the routine does."""
    total = 0
    out = bytearray()
    for byte in data:
        total = (total + byte) & 0xFF
        out.append(total ^ MASK)
    return bytes(out)


def without_the_chain(data: bytes) -> bytes:
    """Wrong: each output depends only on its own input. Differs at offset 1."""
    return bytes((byte ^ MASK) for byte in data)


def without_the_mask(data: bytes) -> bytes:
    """Wrong: no exclusive-or. Differs at offset 0."""
    total = 0
    out = bytearray()
    for byte in data:
        total = (total + byte) & 0xFF
        out.append(total)
    return bytes(out)


def inputs(length: int) -> bytes:
    """The input the tests seed, chosen so that no byte equals its own index.

    A buffer of zeros would be transformed identically by implementations that
    differ, and a buffer of indices would agree with a routine that merely
    copied.
    """
    return bytes(((i * 7) + 3) & 0xFF for i in range(length))
