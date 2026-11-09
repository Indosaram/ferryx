#!/usr/bin/env python3
"""Run the relay end-to-end suites and refuse any run that did not exercise them fully.

A relay E2E target counts as passed only when cargo exits 0, every named target reports a
libtest summary, at least one test ran in each, and nothing was ignored, filtered or skipped.
An ignored or missing suite is a failure, never a quiet green.
"""
import argparse
import re
import subprocess
import sys

TARGETS = (
    "attach_handshake_hides_sentinel_from_relay",
    "relay_pairing_generation_regression",

)
RUNNING = re.compile(r"^\s*Running tests[/\\](?P<name>[A-Za-z0-9_]+)\.rs\b")
RESULT = re.compile(
    r"^test result: (?P<status>\w+)\. (?P<passed>\d+) passed; (?P<failed>\d+) failed; "
    r"(?P<ignored>\d+) ignored; (?P<measured>\d+) measured; (?P<filtered>\d+) filtered out"
)


def command(targets):
    args = ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "--no-fail-fast"]
    for target in targets:
        args += ["--test", target]
    return args + ["--", "--test-threads=1"]


def summaries(output):
    """Maps each integration target to its libtest summary, in the order cargo ran them."""
    found = {}
    current = None
    for line in output.splitlines():
        running = RUNNING.match(line)
        if running:
            current = running.group("name")
            continue
        result = RESULT.match(line)
        if result and current is not None:
            found[current] = {key: (value if key == "status" else int(value))
                              for key, value in result.groupdict().items()}
            current = None
    return found


def verdict(returncode, output, targets):
    """Every reason this run must not count as a pass; empty means it passed."""
    problems = []
    if returncode != 0:
        problems.append("cargo test exited {}".format(returncode))
    found = summaries(output)
    for target in targets:
        summary = found.get(target)
        if summary is None:
            problems.append("{}: no test summary (target missing or not run)".format(target))
            continue
        if summary["status"] != "ok" or summary["failed"]:
            problems.append("{}: {} failed".format(target, summary["failed"]))
        if summary["passed"] == 0:
            problems.append("{}: zero tests ran".format(target))
        if summary["ignored"]:
            problems.append("{}: {} ignored".format(target, summary["ignored"]))
        if summary["filtered"]:
            problems.append("{}: {} filtered out".format(target, summary["filtered"]))
    return problems


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", action="append", dest="targets",
                        help="override the relay E2E target list (tests only)")
    args = parser.parse_args(argv)
    targets = tuple(args.targets or TARGETS)
    # cargo output is UTF-8; a narrower console encoding (cp949, cp1252) must not crash the echo.
    sys.stdout.reconfigure(errors="replace")
    process = subprocess.Popen(command(targets), stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, text=True, encoding="utf-8",
                               errors="replace")
    captured = []
    for line in process.stdout:
        sys.stdout.write(line)
        captured.append(line)
    returncode = process.wait()
    problems = verdict(returncode, "".join(captured), targets)
    for problem in problems:
        print("relay-e2e-gate: FAIL {}".format(problem), file=sys.stderr)
    if problems:
        return 1
    print("relay-e2e-gate: PASS {} targets, none ignored or skipped".format(len(targets)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
