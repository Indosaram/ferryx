#!/usr/bin/env python3
"""Compile a complete libtest inventory in bounded shards, without editing source.

Opt-in local tooling. Default Cargo builds and production sources are unchanged.
The disposable worktree contains only mechanically inserted test-function cfg gates.
Success requires exact compiler-derived inventory equality, not a sum of test counts.
By default this command only lists tests. --run-tests executes the full runtime
gate on a permitted machine; neither mode waives runtime requirements.
"""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid

from test_shard_inventory import parse_compiled_list, parse_expanded, parse_expanded_locations, verify_source_coverage, verify_self_dispatch_cohorts, verify_disjoint_union


def output(command, cwd):
    return subprocess.check_output(command, cwd=cwd, text=True).strip()


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()



def verify_runtime_fonts(target, env):
    """Fail before compilation rather than silently accepting missing Linux glyphs."""
    if "linux" not in target:
        return []
    records = []
    for family in ("MesloLGS NF", "DejaVu Sans", "DejaVu Sans Mono", "Noto Color Emoji"):
        try:
            matched = subprocess.check_output(
                ["fc-match", "--format=%{family}\t%{file}\n", family],
                env=env, text=True, stderr=subprocess.PIPE, timeout=15,
            ).strip()
        except (OSError, subprocess.SubprocessError) as error:
            raise ValueError("Linux runtime font preflight requires working fontconfig fc-match") from error
        fields = matched.split("\t")
        if len(fields) != 2 or family not in [name.strip() for name in fields[0].split(",")]:
            raise ValueError(f"required runtime font {family!r} is missing; fontconfig returned {matched!r}")
        path = Path(fields[1])
        if not path.is_file():
            raise ValueError(f"fontconfig resolved a missing font file for {family!r}: {path}")
        records.append({"family": family, "path": str(path), "sha256": sha(path)})
    return records


def snapshot(source, target, env=None):
    env = os.environ if env is None else env
    return {
        "commit": output(["git", "rev-parse", "HEAD"], source),
        "tree": output(["git", "rev-parse", "HEAD^{tree}"], source),
        "lock_sha256": sha(source / "src-tauri/Cargo.lock"),
        "rustc": output(["rustc", "-Vv"], source),
        "target": target,
        "features": "default",
        "profile": {key: value for key, value in sorted(env.items()) if key.startswith("CARGO_PROFILE_") or key in ("CARGO_BUILD_JOBS", "CARGO_INCREMENTAL")},
    }


def compile_contract(args):
    """Normalize split/joined semantic flags, excluding artifact paths/hashes."""
    result = {"test": "--test" in args, "cfg": [], "codegen": [], "extern": []}
    scalar = ("--crate-name", "--edition", "--target", "--crate-type")
    valued = (*scalar, "--cfg", "-C", "--codegen", "--extern")
    codegen_values = {}
    i = 0
    while i < len(args):
        arg = args[i]
        key = value = None
        if arg in valued:
            if i + 1 >= len(args) or args[i + 1].startswith("-"):
                raise ValueError(f"missing value for {arg}")
            key, value = arg, args[i + 1]
            i += 2
        else:
            for option in valued:
                if arg.startswith(option + "="):
                    key, value = option, arg[len(option) + 1:]
                    break
            if key is None and arg.startswith("-C") and len(arg) > 2:
                key, value = "-C", arg[2:]
            i += 1
        if key is None:
            continue
        if not value:
            raise ValueError(f"empty value for {key}")
        if key in scalar:
            result[key] = value
        elif key == "--cfg":
            if value.split("=", 1)[0].strip() not in ("ferryx_test_sharding", "ferryx_test_shard"):
                result["cfg"].append(value)
        elif key in ("-C", "--codegen"):
            name = value.split("=", 1)[0]
            if name not in ("metadata", "extra-filename"):
                # Fail closed rather than hide order-sensitive overrides by sorting.
                if name in codegen_values and codegen_values[name] != value:
                    raise ValueError(f"conflicting repeated codegen option: {name}")
                codegen_values[name] = value
                result["codegen"].append(value)
        elif key == "--extern":
            result["extern"].append(value.split("=", 1)[0])
    for key in ("cfg", "codegen", "extern"):
        result[key].sort()
    if not result["test"] or result.get("--crate-name") != "ferryx_lib":
        raise ValueError("not a library-test compiler invocation")
    return result


def reject_external_config(source, env):
    roots = [source, *source.parents]
    candidates = [root / ".cargo" / name for root in roots for name in ("config", "config.toml")]
    home = Path(env.get("CARGO_HOME", str(Path.home() / ".cargo")))
    candidates += [home / name for name in ("config", "config.toml")]
    if any(path.exists() for path in candidates):
        raise ValueError("external Cargo config is not supported; use explicit equivalent environment settings")
    if any(value for key, value in env.items() if key.endswith("RUSTFLAGS") or key in ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTC_BOOTSTRAP")):
        raise ValueError("external Rust flags/wrappers/bootstrap are not supported")


def write_wrapper(path, config):
    """Unique workspace-wrapper paths give each shard a separate Cargo fingerprint."""
    code = '''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
CONFIG = %s
rustc, args = sys.argv[1], sys.argv[2:]
if '--test' in args and '--crate-name' in args and args[args.index('--crate-name')+1] == 'ferryx_lib':
    original = list(args)
    if any('ferryx_test_shard' in arg for arg in args):
        raise SystemExit('refusing preexisting shard flags')
    if CONFIG['mode'] == 'inventory':
        kept=[]; i=0
        while i<len(args):
            if args[i]=='--emit': i+=2; continue
            if args[i].startswith('--emit='): i+=1; continue
            kept.append(args[i]); i+=1
        args=kept+['-Zunpretty=expanded']
        # This diagnostic-only opt-in is scoped to this crate, not dependency builds.
        # Ordinary shard compilation below does not set RUSTC_BOOTSTRAP.
        os.environ['RUSTC_BOOTSTRAP']='ferryx_lib'
        fd=os.open(CONFIG['expanded'],os.O_CREAT|os.O_TRUNC|os.O_WRONLY,0o600)
        os.dup2(fd,1); os.close(fd)
    else:
        args += ['--check-cfg', 'cfg(ferryx_test_sharding)', '--check-cfg',
                 'cfg(ferryx_test_shard, values('+','.join(json.dumps(str(i)) for i in range(CONFIG['count']))+'))',
                 '--cfg', 'ferryx_test_sharding', '--cfg', 'ferryx_test_shard='+json.dumps(str(CONFIG['index']))]
    Path(CONFIG['record']).write_text(json.dumps({'original':original,'actual':args},indent=2))
os.execv(rustc,[rustc,*args])
''' % repr(config)
    path.write_text(code)
    path.chmod(0o700)


def run_stage(label, command, cwd, env, evidence, bounded, timeout):
    log = evidence / (label + ".log")
    print(f"START {label}", flush=True)
    started = time.monotonic()
    with (evidence / (label + ".console.log")).open("w") as console:
        result = subprocess.run(
            [sys.executable, str(bounded), "--timeout-seconds", str(timeout),
             "--heartbeat-seconds", "5", "--log", str(log), "--", *command],
            cwd=cwd, env=env, stdout=console, stderr=subprocess.STDOUT,
        )
    print(f"END {label} exit={result.returncode} seconds={time.monotonic()-started:.1f}", flush=True)
    if result.returncode:
        raise RuntimeError(f"{label} failed ({result.returncode}); see {log}")
    return log


def executable_from_log(log):
    executables = set()
    for line in log.read_text().splitlines():
        if not line.startswith("{"):
            continue
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (item.get("reason") == "compiler-artifact"
                and item.get("target", {}).get("name") == "ferryx_lib"
                and item.get("profile", {}).get("test") and item.get("executable")):
            executables.add(item["executable"])
    if len(executables) != 1:
        raise ValueError("expected exactly one library-test executable")
    executable = Path(executables.pop())
    if not executable.is_file():
        raise ValueError("reported test executable is missing")
    return executable


def verify_runtime_target(build_snapshot):
    hosts = re.findall(r"^host: (\S+)$", build_snapshot["rustc"], re.M)
    if hosts != [build_snapshot["target"]]:
        raise ValueError("--run-tests requires the native rustc host target; cross-target binaries cannot be executed by this runner")


def prepare_runtime_helper(target, generated, env, evidence, bounded, timeout):
    """Build this checkout's helper and stage the compile-time fixture path.

    Cargo's JSON artifact path remains authoritative when CARGO_TARGET_DIR or an
    explicit target changes the output layout. Never reuse an arbitrary helper
    from the source checkout or assume the shared target directory's layout.
    """
    command = ["cargo", "build", "--manifest-path", "remote-helper/Cargo.toml",
               "--bin", "ferryx-remote-helper", "--target", target,
               "--locked", "--offline", "-j1", "--message-format=json"]
    log = run_stage("runtime-helper-build", command, generated, env, evidence, bounded, timeout)
    executables = set()
    for line in log.read_text().splitlines():
        if not line.startswith("{"):
            continue
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        artifact_target = item.get("target", {})
        if (item.get("reason") == "compiler-artifact"
                and artifact_target.get("name") == "ferryx-remote-helper"
                and artifact_target.get("kind") == ["bin"]
                and item.get("profile", {}).get("test") is False
                and item.get("executable")):
            executables.add(item["executable"])
    if len(executables) != 1:
        raise ValueError("expected exactly one remote-helper binary artifact")
    executable = Path(executables.pop())
    if not executable.is_absolute():
        executable = generated / executable
    if not executable.is_file():
        raise ValueError("reported remote-helper executable is missing")
    suffix = ".exe" if "windows" in target else ""
    staged = generated / "remote-helper/target/debug" / ("ferryx-remote-helper" + suffix)
    staged.parent.mkdir(parents=True, exist_ok=True)
    if executable.resolve() != staged.resolve():
        shutil.copy2(executable, staged)
    digest = sha(executable)
    if sha(staged) != digest:
        raise ValueError("staged remote-helper differs from Cargo artifact")
    return {"target": target, "artifact": str(executable), "staged": str(staged),
            "sha256": digest, "lock_sha256": sha(generated / "remote-helper/Cargo.lock")}


def run_full_runtime(state, generated, env, evidence, bounded, timeout):
    """Run every compiled test with normal libtest ignored semantics, no filters."""
    state["runtime"] = "INCOMPLETE"
    helper = state.get("runtime_helper")
    if not helper or not Path(helper["staged"]).is_file() or sha(helper["staged"]) != helper["sha256"]:
        raise ValueError("verified runtime helper is missing or changed")
    for shard in state["shards"]:
        log = run_stage(f"shard-{shard['index']}-runtime",
                        [shard["executable"], "--test-threads=1"], generated / "src-tauri",
                        env, evidence, bounded, timeout)
        summaries = re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;', log.read_text(), re.M)
        ignored = sum(test["ignored"] for test in shard["tests"])
        expected = (len(shard["tests"]) - ignored, 0, ignored, 0, 0)
        if len(summaries) != 1 or tuple(map(int, summaries[0])) != expected:
            raise ValueError("runtime totals differ from exact compiled inventory")
    state["runtime"] = "FULL_RUNTIME_PASS"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--worktree-root", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--shards", type=int, default=8)
    parser.add_argument("--stage-timeout", type=int, default=1800)
    parser.add_argument("--run-tests", action="store_true", help="Execute all library tests on a permitted machine; no filters/skips")
    parser.add_argument("--baseline", type=Path, help="Verified baseline JSON from the identical snapshot")
    args = parser.parse_args()
    if not 2 <= args.shards <= 32 or args.stage_timeout <= 0:
        parser.error("shards must be 2..32 and timeout positive")
    source = args.source_root.resolve(strict=True)
    worktree_root = args.worktree_root.resolve(strict=True)
    evidence = args.evidence.resolve()
    # Never accept stale invocation records or overwrite another invocation's evidence.
    evidence.mkdir(parents=True, exist_ok=False)
    subprocess.run(["git", "diff", "--exit-code", "HEAD", "--", "src-tauri/src", "src-tauri/tests/support", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock", "src-tauri/build.rs"], cwd=source, check=True)
    if not (source / "ui/dist/index.html").is_file():
        parser.error("build the frontend first; ui/dist is required")
    bounded = source / "scripts/ci-run-bounded.py"
    env = os.environ.copy()
    env.update(CARGO_BUILD_JOBS="1", CARGO_INCREMENTAL="0", CARGO_NET_OFFLINE="true")
    reject_external_config(source, env)
    reject_external_config(worktree_root, env)
    if args.run_tests:
        (evidence / "runtime-font-prerequisites.json").write_text(
            json.dumps(verify_runtime_fonts(args.target, env), indent=2))
    state = {"snapshot": snapshot(source, args.target, env), "shards": [], "status": "INCOMPLETE", "runtime": "NOT_RUN"}
    (evidence / "snapshot.json").write_text(json.dumps(state["snapshot"], indent=2))
    base = ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "--target", args.target,
            "--lib", "--no-run", "--locked", "--offline", "-j1", "--message-format=json"]
    generated = None
    owned_worktree = False
    canonical = Path(output(["git", "rev-parse", "--path-format=absolute", "--git-common-dir"], source)).parent
    try:
        if args.run_tests:
            verify_runtime_target(state["snapshot"])
        generated = worktree_root / ("test-shards-" + time.strftime("%Y%m%d-%H%M%S", time.gmtime()) + "-" + uuid.uuid4().hex[:12])
        if generated.exists():
            raise ValueError("refusing a preexisting generated-worktree path")
        subprocess.run(["git", "worktree", "add", "--detach", str(generated), state["snapshot"]["commit"]], cwd=canonical, check=True)
        owned_worktree = True
        # Clone only the already verified local submodule; never silently fetch a new revision.
        expected_ghostty = output(["git", "rev-parse", "HEAD:src-tauri/vendor/ghostty"], source)
        if output(["git", "rev-parse", "HEAD"], source / "src-tauri/vendor/ghostty") != expected_ghostty:
            raise ValueError("source Ghostty is not at its pinned commit")
        subprocess.run(["git", "diff", "--exit-code", "HEAD"], cwd=source / "src-tauri/vendor/ghostty", check=True)
        subprocess.run(["git", "-c", "protocol.allow=never", "-c", "protocol.file.allow=always", "-c",
                        "submodule.src-tauri/vendor/ghostty.url=" + str(source / "src-tauri/vendor/ghostty"),
                        "submodule", "update", "--init", "--recursive", "--no-fetch"], cwd=generated,
                       env=env | {"GIT_ALLOW_PROTOCOL": "file"}, check=True)
        if output(["git", "rev-parse", "HEAD"], generated / "src-tauri/vendor/ghostty") != expected_ghostty:
            raise ValueError("generated Ghostty revision differs")
        state["ghostty_commit"] = expected_ghostty
        shutil.copytree(source / "ui/dist", generated / "ui/dist")
        if args.run_tests:
            state["runtime_helper"] = prepare_runtime_helper(
                args.target, generated, env, evidence, bounded, args.stage_timeout)
        if args.baseline:
            baseline = json.loads(args.baseline.read_text())
            if baseline["snapshot"] != state["snapshot"]:
                raise ValueError("baseline snapshot differs: refusing stale inventory")
            full_inventory = baseline["tests"]
            baseline_contract = baseline["compile_contract"]
            source_locations = baseline["source_locations"]
        else:
            wrapper = evidence / "inventory-rustc.py"
            expanded = evidence / "unsharded-expanded.rs"
            write_wrapper(wrapper, {"mode": "inventory", "expanded": str(expanded), "record": str(evidence / "inventory-rustc.json")})
            inventory_env = env | {"RUSTC_WRAPPER": str(wrapper)}
            run_stage("inventory", base, source, inventory_env, evidence, bounded, args.stage_timeout)
            if not (evidence / "inventory-rustc.json").is_file():
                raise ValueError("inventory compiler wrapper was not invoked")
            full_inventory = parse_expanded(expanded.read_text())
            source_locations = parse_expanded_locations(expanded.read_text())
            baseline_contract = compile_contract(json.loads((evidence / "inventory-rustc.json").read_text())["original"])
        (evidence / "baseline.json").write_text(json.dumps({"snapshot": state["snapshot"], "compile_contract": baseline_contract, "tests": full_inventory, "source_locations": source_locations}, indent=2))
        state["full_inventory_count"] = len(full_inventory)

        instrument = ["cargo", "run", "--offline", "--locked", "--manifest-path",
                      str(source / "tools/test-shard-instrumenter/Cargo.toml"), "--", "apply",
                      "--root", str(generated), "--shards", str(args.shards)]
        manifest_text = subprocess.check_output(instrument, cwd=source, env=env, text=True)
        manifest = json.loads(manifest_text)
        manifest["self_dispatch_reviews"] = json.loads((source / "tools/test-shard-instrumenter/self-dispatch-reviews.json").read_text())
        verify_source_coverage(source_locations, manifest, source)
        cohort_edges = verify_self_dispatch_cohorts(source_locations, manifest, source)
        (evidence / "self-dispatch-cohorts.json").write_text(json.dumps(cohort_edges, indent=2))
        (evidence / "instrumentation.json").write_text(json.dumps(manifest, indent=2))
        for index in range(args.shards):
            wrapper = evidence / f"shard-{index}-rustc.py"
            record = evidence / f"shard-{index}-rustc.json"
            write_wrapper(wrapper, {"mode": "shard", "count": args.shards, "index": index, "record": str(record)})
            shard_env = env | {"RUSTC_WORKSPACE_WRAPPER": str(wrapper)}
            log = run_stage(f"shard-{index}-compile", base, generated, shard_env, evidence, bounded, args.stage_timeout)
            if not record.is_file():
                raise ValueError("shard compiler wrapper was not invoked")
            actual_contract = compile_contract(json.loads(record.read_text())["actual"])
            if actual_contract != baseline_contract:
                raise ValueError("shard compiler features/profile/extern set differs from baseline")
            executable = executable_from_log(log)
            all_text = subprocess.check_output([str(executable), "--list"], env=env, cwd=generated, text=True, timeout=60)
            ignored_text = subprocess.check_output([str(executable), "--list", "--ignored"], env=env, cwd=generated, text=True, timeout=60)
            (evidence / f"shard-{index}.list").write_text(all_text)
            (evidence / f"shard-{index}.ignored.list").write_text(ignored_text)
            names = parse_compiled_list(all_text)
            ignored = set(parse_compiled_list(ignored_text))
            if not ignored.issubset(names):
                raise ValueError("ignored tests absent from complete list")
            rows = [{"name": name, "ignored": name in ignored} for name in names]
            state["shards"].append({"index": index, "executable": str(executable), "sha256": sha(executable), "tests": rows})
            (evidence / "result.json").write_text(json.dumps(state, indent=2))
        verify_disjoint_union(full_inventory, [shard["tests"] for shard in state["shards"]])
        state["status"] = "COMPILE_AND_EXACT_INVENTORY_PASS"
        print(f"PASS: {len(full_inventory)} distinct compiler-resolved tests across {args.shards} shards", flush=True)
        if args.run_tests:
            run_full_runtime(state, generated, env, evidence, bounded, args.stage_timeout)
        print("Runtime: " + state["runtime"], flush=True)
    except Exception as error:
        state["status"] = "FAILED"
        state["error"] = str(error)
        raise
    finally:
        (evidence / "result.json").write_text(json.dumps(state, indent=2))
        if owned_worktree and generated is not None and generated.exists():
            # This unique worktree is owned solely by this invocation; preserve its diff before cleanup.
            with (evidence / "generated-test-gates.patch").open("w") as patch:
                subprocess.run(["git", "diff", "--binary"], cwd=generated, stdout=patch, check=True)
            subprocess.run(["git", "worktree", "remove", "--force", str(generated)], cwd=canonical, check=True)


if __name__ == "__main__":
    main()
