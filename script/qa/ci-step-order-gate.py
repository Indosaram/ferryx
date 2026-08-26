#!/usr/bin/env python3
"""CI step-order gate: proves zig setup + submodule init precede any cargo/tauri invocation.

Machine-consumed output: JSON per (workflow, job) with boolean gates. Exit 0 only when
every job that invokes cargo/tauri has BOTH a zig step and a submodule step at a lower index.
"""
import json, re, sys, pathlib

WORKFLOWS = [".github/workflows/build-test.yml", ".github/workflows/release.yml"]
ZIG = re.compile(r"setup-zig|zig[-_ ]?version|mlugg/setup-zig|ziglang", re.I)
SUBMODULE = re.compile(r"submodules?\s*:\s*(true|recursive)|git submodule update", re.I)
BUILDCMD = re.compile(r"\bcargo\s+(check|build|test|clippy)\b|tauri-apps/cli\s+build|cargo\s+tauri", re.I)


def load_jobs(path):
    """Parse the workflow with PyYAML when available, else a minimal step scanner."""
    text = pathlib.Path(path).read_text()
    try:
        import yaml
        doc = yaml.safe_load(text)
        out = {}
        for job_name, job in (doc.get("jobs") or {}).items():
            steps = []
            for step in (job.get("steps") or []):
                blob = json.dumps(step)
                steps.append(blob)
            out[job_name] = steps
        return out
    except ImportError:
        # Fallback: split on top-level job keys, then on "- name:"/"- uses:" step markers.
        out = {}
        job = None
        buf = []
        for line in text.splitlines():
            m = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
            if m and not line.startswith("    "):
                if job:
                    out[job] = buf
                job, buf = m.group(1), []
                continue
            if job is not None:
                if re.match(r"^\s+- (name|uses):", line):
                    buf.append(line)
                elif buf:
                    buf[-1] += "\n" + line
        if job:
            out[job] = buf
        return out


def gate(path):
    results = {}
    for job_name, steps in load_jobs(path).items():
        zig_at = next((i for i, s in enumerate(steps) if ZIG.search(s)), None)
        sub_at = next((i for i, s in enumerate(steps) if SUBMODULE.search(s)), None)
        build_at = next((i for i, s in enumerate(steps) if BUILDCMD.search(s)), None)
        if build_at is None:
            results[job_name] = {"builds": False, "ok": True}
            continue
        results[job_name] = {
            "builds": True,
            "build_step_index": build_at,
            "zig_step_index": zig_at,
            "submodule_step_index": sub_at,
            "zig_before_build": zig_at is not None and zig_at < build_at,
            "submodule_before_build": sub_at is not None and sub_at < build_at,
            "ok": zig_at is not None and sub_at is not None and zig_at < build_at and sub_at < build_at,
        }
    return results


def main():
    report = {w: gate(w) for w in WORKFLOWS}
    print(json.dumps(report, indent=2, sort_keys=True))
    failures = [
        f"{w}:{j}" for w, jobs in report.items() for j, r in jobs.items() if not r["ok"]
    ]
    if failures:
        print("FAIL: jobs missing zig/submodule before build: " + ", ".join(sorted(failures)))
        return 1
    print("PASS: every building job has zig + submodule init before its build step")
    return 0


if __name__ == "__main__":
    sys.exit(main())
