"""§5.6's cycle, driven by a client that links nothing of the host.

This is §M4's done-condition as a program: seed a routine's inputs, run it,
compare a reimplementation's output against the reference's, and read what the
reference actually produced. Everything here goes over the protocol; the only
thing this process knows about awaseru is the path to its binary.

It is written to be run by hand as well as by the suite, so it asserts its own
expectations and prints a summary of what the tool said. The summary is JSON on
standard output, which is what the suite reads: the numbers in it are the
**tool's** — the offset where the two implementations part, and the position of
the instruction that wrote the reference's value — and this script is told
neither.

Run:

    python3 cycle.py --exe PATH --config PATH --local PATH --home DIR \\
                     --cache DIR --entry N --returns-to N \\
                     --input-at N --output-at N --length N

Addresses and offsets are decimal, and come from whoever owns the software. A
developer doing this for real takes them from their own disassembly; the suite
takes them from the fixture it generated.
"""

import argparse
import json
import sys

import awaseru
import routine


def span(region: str, offset: int, length: int) -> dict:
    return {"region": region, "offset": offset, "length": length}


def examine(routine_description, given, produced, payload, control=None, localise=False):
    command = {
        "command": "examine",
        "routine": routine_description,
        "given": given,
        "produced": produced,
        "localise": localise,
    }
    if control is not None:
        command["control"] = control
    return command, payload


def main() -> int:
    options = parse()
    data = routine.inputs(options.length)
    right = routine.running_total(data)
    wrong = routine.without_the_chain(data)

    described = {
        "name": "running-total",
        "entry": options.entry,
        "returns_to": options.returns_to,
        "within": 20_000,
    }
    given = [span("work-ram", options.input_at, options.length)]
    produced = [span("work-ram", options.output_at, options.length)]

    summary = {}
    with awaseru.Server(
        options.exe, options.config, options.local, options.home, options.cache
    ) as server:
        # ---- the handshake ------------------------------------------------
        hello = server.hello("the python cycle")
        assert awaseru.kind(hello) == "hello", f"the handshake was refused: {hello}"
        assert hello["protocol"] == awaseru.PROTOCOL, hello
        summary["tool"] = hello["tool"]

        # ---- what it can do, asked rather than assumed (§7.3) -------------
        capabilities, _ = server.ask({"command": "capabilities"})
        assert awaseru.kind(capabilities) == "capabilities", capabilities
        summary["declared"] = capabilities["declared"]
        summary["absent"] = capabilities["absent"]

        # ---- what regions exist, by the names the backend gave (§3.1) -----
        regions, _ = server.ask({"command": "regions"})
        assert awaseru.kind(regions) == "regions", regions
        names = [region["name"] for region in regions["regions"]]
        assert "work-ram" in names, f"no region to seed: {names}"
        summary["regions"] = names

        # ---- the right reimplementation agrees ----------------------------
        command, payload = examine(described, given, produced, data + right, localise=True)
        reply, _ = server.ask(command, payload)
        assert awaseru.kind(reply) == "report", f"expected a report: {reply}"
        report = reply["report"]
        summary["right"] = {
            "verdict": awaseru.verdict_of(report),
            "moved": report.get("moved"),
            "localisation": report.get("localisation"),
            "complete": awaseru.complete(report),
        }
        assert awaseru.verdict_of(report) == "agrees", (
            "the implementation this client believes is right must agree with the "
            f"reference: {report}"
        )

        # ---- and the wrong one is caught ----------------------------------
        command, payload = examine(described, given, produced, data + wrong, localise=True)
        reply, _ = server.ask(command, payload)
        assert awaseru.kind(reply) == "report", f"expected a report: {reply}"
        report = reply["report"]
        difference = awaseru.difference_of(report)
        assert difference is not None, f"a wrong reimplementation must be caught: {report}"
        summary["wrong"] = {
            "verdict": awaseru.verdict_of(report),
            "difference": difference,
            "localisation": report.get("localisation"),
        }
        # The offset and the region are the tool's answer, not this client's
        # arithmetic: nothing above says where the two implementations part.
        assert difference["region"] == "work-ram", difference
        assert difference["expected"] != difference["found"], difference
        assert difference["wrote"]["wrote"] == "at", (
            "§5.4's third item: the position that wrote the reference's value "
            f"{difference}"
        )

        # ---- and the bytes the reference itself produced -------------------
        # Read **before** the control, and that is not an accident: a control
        # runs the reference again with an input changed, and leaves the machine
        # where that perturbed run ended. The output span then holds the
        # perturbed answer, which is correct and is not what this comparison was
        # about. Found by this assertion failing with the read at the end.
        reply, bytes_read = server.ask(
            {
                "command": "read",
                "region": "work-ram",
                "offset": options.output_at,
                "length": options.length,
            }
        )
        assert awaseru.kind(reply) == "bytes", reply
        assert bytes_read == right, (
            "the reference's own output must be what this client computed; it is "
            "the whole claim"
        )
        summary["read_matches_our_own"] = True

        # ---- §5.3's control ------------------------------------------------
        changed = bytearray(data)
        changed[0] = (changed[0] + 1) & 0xFF
        command, payload = examine(
            described,
            given,
            produced,
            data + right + bytes(changed),
            control={
                "name": "the first input byte",
                "span": span("work-ram", options.input_at, options.length),
            },
        )
        reply, _ = server.ask(command, payload)
        assert awaseru.kind(reply) == "report", reply
        report = reply["report"]
        summary["control"] = report["control"]
        summary["control_complete"] = awaseru.complete(report)
        assert report["control"]["control"] == "ran", report["control"]
        assert report["control"]["noticed"], (
            "every output byte depends on the first input byte, so changing it must "
            f"move the verdict: {report['control']}"
        )

    json.dump(summary, sys.stdout)
    sys.stdout.write("\n")
    return 0


def parse():
    parser = argparse.ArgumentParser(description="drive one routine-level cycle")
    for path in ("exe", "config", "local", "home", "cache"):
        parser.add_argument(f"--{path}", required=True)
    for number in ("entry", "returns-to", "input-at", "output-at", "length"):
        parser.add_argument(f"--{number}", required=True, type=int)
    return parser.parse_args()


if __name__ == "__main__":
    sys.exit(main())
