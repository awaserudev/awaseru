"""Checks the client against the contract, without needing a backend.

Two things are worth checking before a conversation is attempted, because each
would otherwise fail inside a protocol exchange where the cause is hard to see:

1. **the framing matches `doc/protocol.md` byte for byte.** A client that is one
   byte out produces a refusal from the tool and a puzzled afternoon for whoever
   wrote it. The vector here is the same one the tool's own test asserts.
2. **the two wrong reimplementations are wrong where they claim to be.** The
   done-condition rests on the offsets being 1 and 0; if the Python versions
   drifted, the test would still pass and would be testing nothing.

Run: `python3 selftest.py`. It prints nothing and exits 0 when all is well.
"""

import io
import sys

import awaseru
import routine


def the_frame_is_the_documented_bytes():
    frame = awaseru.encode({}, b"\xAB\xCD")
    expected = bytes(
        [
            0, 0, 0, 2,  # the envelope's length, big-endian
            0x7B, 0x7D,  # {}
            0, 0, 0, 2,  # the payload's length, big-endian
            0xAB, 0xCD,
        ]
    )
    assert frame == expected, f"the framing is not the contract's: {frame!r}"

    # And with no payload the prefix is still there, holding zero.
    assert awaseru.encode({}) == bytes([0, 0, 0, 2, 0x7B, 0x7D, 0, 0, 0, 0])


def a_frame_round_trips():
    out = awaseru.encode({"command": "read", "region": "work-ram"}, b"abc")
    envelope, payload = awaseru.decode(io.BytesIO(out))
    assert envelope == {"command": "read", "region": "work-ram"}, envelope
    assert payload == b"abc", payload

    # Two frames back to back are two frames, with nothing of the first left in
    # the second.
    stream = io.BytesIO(awaseru.encode({"n": 1}, b"x") + awaseru.encode({"n": 2}))
    first, bytes_of_first = awaseru.decode(stream)
    second, bytes_of_second = awaseru.decode(stream)
    assert (first, bytes_of_first) == ({"n": 1}, b"x"), (first, bytes_of_first)
    assert (second, bytes_of_second) == ({"n": 2}, b""), (second, bytes_of_second)


def an_ended_stream_and_a_broken_one_are_different():
    # Nothing at all: the sender finished.
    try:
        awaseru.decode(io.BytesIO(b""))
    except awaseru.ServerGone:
        pass
    else:
        raise AssertionError("a stream that ends between messages is not a bad frame")

    # Half a length: the sender died mid-message.
    try:
        awaseru.decode(io.BytesIO(b"\x00\x00"))
    except awaseru.BadFrame:
        pass
    else:
        raise AssertionError("half a length must not pass for a frame")


def a_reader_survives_a_dribbling_stream():
    """One byte at a time, which is what a pipe is allowed to do."""

    class Dribble(io.RawIOBase):
        def __init__(self, data):
            self.data = data
            self.at = 0

        def read(self, count=-1):
            if self.at >= len(self.data):
                return b""
            byte = self.data[self.at : self.at + 1]
            self.at += 1
            return byte

    frame = awaseru.encode({"command": "regions"}, bytes(300))
    envelope, payload = awaseru.decode(Dribble(frame))
    assert envelope == {"command": "regions"}, envelope
    assert len(payload) == 300


def the_wrong_implementations_are_wrong_where_they_claim():
    data = routine.inputs(0x40)
    right = routine.running_total(data)
    assert len(right) == len(data)

    chain = routine.without_the_chain(data)
    assert chain[0] == right[0], "this one is right about the first byte"
    assert chain[1] != right[1], "and wrong about the second"
    assert _first_difference(right, chain) == 1

    mask = routine.without_the_mask(data)
    assert _first_difference(right, mask) == 0, "this one is wrong from the start"

    # And the right one is right about everything, or the comparison below has
    # nothing to find.
    assert _first_difference(right, routine.running_total(data)) is None


def _first_difference(a: bytes, b: bytes):
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            return i
    return None


def main():
    checks = [
        the_frame_is_the_documented_bytes,
        a_frame_round_trips,
        an_ended_stream_and_a_broken_one_are_different,
        a_reader_survives_a_dribbling_stream,
        the_wrong_implementations_are_wrong_where_they_claim,
    ]
    for check in checks:
        check()
    return 0


if __name__ == "__main__":
    sys.exit(main())
