"""A client for awaseru's protocol, in the standard library and nothing else.

This is §8's other binding (§8.4): a client in a language that is not the
host's, speaking the protocol over a subprocess's standard input and output. It
links nothing, builds nothing and imports nothing that does not ship with
Python — which is the point of §M4's done-condition.

The wire contract is `doc/protocol.md`. This file follows it and does not
extend it: everything here is either framing, transport, or a name for a field
the document already describes.

What it deliberately does NOT offer
-----------------------------------

**No `is_ok()`, and no boolean anywhere near a verdict.** §2.3 has three
values — agrees, differs, not determined — and a client that could ask "did it
pass?" would be a client that reads the third as the first. `verdict_of` gives
the tag back as a string, and a caller has to say what it means.

**No exception for a refusal.** A refusal is a reply (§14.2): it carries what
was looked for and what was found, and a caller that wants to read it should
not have to catch it. The only exceptions here are for a server that stopped
answering, which is not a reply at all.

Blocking reads, on purpose
--------------------------

Nothing here sets a deadline, and that is safe for a reason worth knowing: the
server gives *its* reference process a watchdog and answers a refusal when it
goes silent, so a question always gets an answer or an end of stream. A client
in a language with a convenient timeout is welcome to add one; a client without
one is not left hanging by this design.
"""

import json
import struct
import subprocess

#: The protocol version this client speaks — §8.6, which is open. A mismatch is
#: refused by the tool with both numbers in it.
PROTOCOL = 1

#: Four bytes, big-endian, for each of the two lengths in a frame.
_LENGTH = struct.Struct(">I")

#: What a reader will accept, matching the tool's own limits so that a client
#: and a server disagree about nothing.
ENVELOPE_LIMIT = 1 << 20
PAYLOAD_LIMIT = 64 << 20


class ServerGone(Exception):
    """The server stopped answering.

    Not a refusal and not a verdict: a question whose reference has gone has no
    answer, and inventing one is what §2.3's third value exists to prevent.
    """


class BadFrame(Exception):
    """Something arrived that is not a frame this client can read."""


def encode(envelope: dict, payload: bytes = b"") -> bytes:
    """One frame: the envelope's length, the envelope, the payload's, the payload.

    The payload's prefix is always written, and zero means there is none — which
    is how §8.3's payload is optional.
    """
    text = json.dumps(envelope, separators=(",", ":")).encode("utf-8")
    if not text:
        raise BadFrame("an envelope of no bytes is not a message")
    if len(text) > ENVELOPE_LIMIT:
        raise BadFrame(f"an envelope of {len(text)} bytes, over the limit of {ENVELOPE_LIMIT}")
    if len(payload) > PAYLOAD_LIMIT:
        raise BadFrame(f"a payload of {len(payload)} bytes, over the limit of {PAYLOAD_LIMIT}")
    return b"".join(
        [_LENGTH.pack(len(text)), text, _LENGTH.pack(len(payload)), payload]
    )


def _read_exactly(stream, count: int) -> bytes:
    """Reads `count` bytes, or raises.

    A loop rather than one `read`, because a pipe is allowed to hand over less
    than was asked for and a client that assumed otherwise works until the
    payload gets large.
    """
    chunks = []
    got = 0
    while got < count:
        chunk = stream.read(count - got)
        if not chunk:
            if got == 0:
                raise ServerGone("the stream ended between messages")
            raise BadFrame(f"the stream ended {count - got} byte(s) short of a frame")
        chunks.append(chunk)
        got += len(chunk)
    return b"".join(chunks)


def decode(stream) -> tuple[dict, bytes]:
    """Reads one frame from a stream, returning the envelope and the payload."""
    (envelope_length,) = _LENGTH.unpack(_read_exactly(stream, 4))
    if envelope_length == 0:
        raise BadFrame("an envelope of zero bytes: the sender has lost its place")
    if envelope_length > ENVELOPE_LIMIT:
        raise BadFrame(f"an envelope claiming {envelope_length} bytes")
    envelope = json.loads(_read_exactly(stream, envelope_length).decode("utf-8"))
    (payload_length,) = _LENGTH.unpack(_read_exactly(stream, 4))
    if payload_length > PAYLOAD_LIMIT:
        raise BadFrame(f"a payload claiming {payload_length} bytes")
    payload = _read_exactly(stream, payload_length) if payload_length else b""
    return envelope, payload


class Server:
    """A running `awaseru serve`, asked one command at a time.

    The client spawns the tool (§8.1 — the client drives), which is why this is
    a context manager: leaving the block closes the stream, which is how §8.2's
    conversation ends.
    """

    def __init__(self, exe, config, local, home, cache, log=None):
        arguments = [
            str(exe),
            "serve",
            "--config",
            str(config),
            "--local",
            str(local),
            "--home",
            str(home),
            "--cache",
            str(cache),
        ]
        if log is not None:
            arguments += ["--log", str(log)]
        self.process = subprocess.Popen(
            arguments,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            # Left alone: the tool's own diagnostics are not the protocol, and a
            # client that swallowed them would make a failure harder to read.
            stderr=None,
        )

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
        return False

    def ask(self, command: dict, payload: bytes = b"") -> tuple[dict, bytes]:
        """One command, one answer."""
        if self.process.stdin is None or self.process.stdout is None:
            raise ServerGone("this server's streams are closed")
        try:
            self.process.stdin.write(encode(command, payload))
            self.process.stdin.flush()
        except BrokenPipeError as broken:
            raise ServerGone("the server is gone: the pipe is broken") from broken
        return decode(self.process.stdout)

    def hello(self, client: str) -> dict:
        """The handshake, which comes before anything else."""
        reply, _ = self.ask({"command": "hello", "protocol": PROTOCOL, "client": client})
        return reply

    def close(self):
        """Closes the client's end and waits, which is how a conversation ends."""
        if self.process.stdin is not None:
            self.process.stdin.close()
        return self.process.wait()


# ----------------------------------------------------------- reading replies --


def kind(reply: dict) -> str:
    """Which reply this is: `hello`, `report`, `refused`, and so on."""
    return reply["result"]


def verdict_of(report: dict) -> str:
    """The verdict's tag: `agrees`, `differs` or `not-determined`.

    Three values, as §2.3 requires. There is deliberately no function here that
    turns this into a boolean.
    """
    return report["verdict"]["verdict"]


def difference_of(report: dict) -> dict | None:
    """§5.4's first items, when the verdict differs.

    The region is part of it: an offset is read against a region (§13's Q16),
    and a client that printed the number alone would send its reader to the
    wrong place.
    """
    verdict = report["verdict"]
    if verdict["verdict"] != "differs":
        return None
    return verdict["difference"]


def complete(report: dict) -> bool:
    """§5.3's first sentence: whether a control that varies was run and noticed.

    A boolean is right here and not for a verdict, because this is a question
    about the measurement's method rather than about what it found.
    """
    return bool(report["complete"])


def coverage_of(report: dict) -> dict | None:
    """§10's execution coverage, when the request asked for a span.

    `None` means nobody asked. That is not the same as "nothing ran", and the
    two are kept apart here for the same reason they are on the wire: a client
    that read an absent field as an empty reading would conclude its routine
    never executed.
    """
    return report.get("coverage")


def never_ran(report: dict) -> list[tuple[int, int]]:
    """The stretches that never executed, as [start, end) pairs.

    §10 calls coverage "the structural answer to *there is always a routine I
    did not know about*", so this is the answer and `ran` is the leftover.
    Raises if coverage was not asked for, rather than returning an empty list,
    which would read as "everything ran".
    """
    coverage = coverage_of(report)
    if coverage is None:
        raise ValueError("coverage was not asked for, so there is nothing to say")
    return [(start, end) for start, end in coverage["never_ran"]]
