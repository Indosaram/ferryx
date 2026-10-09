#!/usr/bin/env python3
import argparse
import ctypes
import hashlib
import os
import shutil
import sys
import tarfile
import tempfile
import time
import urllib.request
import uuid

AT_FDCWD = -100
RENAME_EXCHANGE = 1 << 1


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Fail-closed atomic swap of verified mobile UI dist assets."
    )
    parser.add_argument(
        "--artifact-tar",
        required=True,
        help="Path to verified UI dist tar.gz archive",
    )
    parser.add_argument(
        "--expected-sha256",
        required=True,
        help="Expected SHA-256 hex checksum of the artifact tar archive",
    )
    parser.add_argument(
        "--dist-dir",
        required=True,
        help="Live static UI dist directory",
    )
    parser.add_argument(
        "--baseline-pids",
        nargs="+",
        type=int,
        required=True,
        help="Baseline PIDs that must be alive and remain alive",
    )
    parser.add_argument(
        "--health-url",
        default="http://127.0.0.1:8787/api/account/v1/health",
        help="HTTP health URL to verify post-swap",
    )
    return parser.parse_args()


def compute_sha256(file_path: str) -> str:
    h = hashlib.sha256()
    with open(file_path, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()


def compute_bytes_sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def check_pids(pids: list[int], stage: str) -> None:
    for pid in pids:
        try:
            os.kill(pid, 0)
        except OSError as e:
            print(f"FATAL [{stage}]: Required baseline PID {pid} is not alive: {e}", file=sys.stderr)
            sys.exit(1)
        proc_exe = f"/proc/{pid}/exe"
        if os.path.exists(proc_exe):
            try:
                target = os.readlink(proc_exe)
                print(f"OK [{stage}]: PID {pid} verified alive (exe: {target})")
            except OSError as e:
                print(f"FATAL [{stage}]: Cannot inspect /proc/{pid}/exe: {e}", file=sys.stderr)
                sys.exit(1)
        else:
            print(f"OK [{stage}]: PID {pid} verified alive.")


def safe_inspect_and_extract_tar(tar_path: str, target_dir: str) -> str:
    target_abs = os.path.abspath(target_dir)
    index_bytes = None

    with tarfile.open(tar_path, "r:*") as tar:
        for member in tar.getmembers():
            if not (member.isreg() or member.isdir()):
                print(
                    f"FATAL: Archive member '{member.name}' is type {member.type} (not regular file or dir). Special nodes forbidden.",
                    file=sys.stderr,
                )
                sys.exit(1)

            norm_name = os.path.normpath(member.name)
            if norm_name.startswith("/") or norm_name.startswith("\\") or norm_name.startswith("../") or norm_name == "..":
                print(f"FATAL: Archive member '{member.name}' attempts absolute or parent traversal.", file=sys.stderr)
                sys.exit(1)

            dest_path = os.path.abspath(os.path.join(target_dir, norm_name))
            if not (dest_path == target_abs or dest_path.startswith(target_abs + os.sep)):
                print(f"FATAL: Archive member '{member.name}' resolves outside target directory.", file=sys.stderr)
                sys.exit(1)

            clean_rel = norm_name.lstrip("./")
            if clean_rel == "index.html" and member.isreg():
                f = tar.extractfile(member)
                if f is not None:
                    index_bytes = f.read()

        if index_bytes is None:
            print("FATAL: Archive does not contain root index.html regular file.", file=sys.stderr)
            sys.exit(1)

        tar.extractall(target_dir)

    return compute_bytes_sha256(index_bytes)


def atomic_exchange_dirs(dir_a: str, dir_b: str) -> None:
    if not sys.platform.startswith("linux"):
        print("FATAL: Atomic deployment requires Linux renameat2(RENAME_EXCHANGE). Refusing on non-Linux.", file=sys.stderr)
        sys.exit(1)

    libc = ctypes.CDLL("libc.so.6", use_errno=True)
    if not hasattr(libc, "renameat2"):
        print("FATAL: libc.renameat2 symbol not found in libc.so.6. Refusing non-atomic fallback.", file=sys.stderr)
        sys.exit(1)

    renameat2_func = libc.renameat2
    renameat2_func.argtypes = [
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_uint,
    ]
    renameat2_func.restype = ctypes.c_int

    ret = renameat2_func(
        AT_FDCWD,
        os.path.abspath(dir_a).encode("utf-8"),
        AT_FDCWD,
        os.path.abspath(dir_b).encode("utf-8"),
        ctypes.c_uint(RENAME_EXCHANGE),
    )
    if ret != 0:
        errno = ctypes.get_errno()
        raise OSError(errno, f"libc.renameat2 RENAME_EXCHANGE failed: {os.strerror(errno)}")


def verify_health_endpoint(url: str) -> None:
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Ferryx-Deploy-Verifier/1.0"})
        with urllib.request.urlopen(req, timeout=5.0) as resp:
            if resp.status == 200:
                print(f"OK [post-swap]: Health endpoint {url} returned 200 OK.")
                return
            print(f"FATAL [post-swap]: Health endpoint {url} returned status {resp.status}.", file=sys.stderr)
            sys.exit(1)
    except Exception as e:
        print(f"FATAL [post-swap]: Health endpoint probe failed: {e}", file=sys.stderr)
        sys.exit(1)


def main() -> None:
    args = parse_args()

    check_pids(args.baseline_pids, "pre-swap")

    if not os.path.isfile(args.artifact_tar):
        print(f"FATAL: Artifact tar file not found: {args.artifact_tar}", file=sys.stderr)
        sys.exit(1)

    actual_sha = compute_sha256(args.artifact_tar)
    if actual_sha.lower() != args.expected_sha256.lower():
        print(
            f"FATAL: Artifact SHA-256 mismatch!\n  Expected: {args.expected_sha256}\n  Actual:   {actual_sha}",
            file=sys.stderr,
        )
        sys.exit(1)
    print(f"OK: Verified artifact SHA-256: {actual_sha}")

    dist_dir = os.path.abspath(args.dist_dir)
    if os.path.islink(dist_dir):
        print(f"FATAL: Target dist directory '{dist_dir}' is a symlink. Symlinked live dist forbidden.", file=sys.stderr)
        sys.exit(1)

    if not os.path.isdir(dist_dir):
        print(f"FATAL: Target dist directory does not exist or is not a directory: {dist_dir}", file=sys.stderr)
        sys.exit(1)

    parent_dir = os.path.dirname(dist_dir)
    stage_dir = tempfile.mkdtemp(prefix=f".{os.path.basename(dist_dir)}_stage_", dir=parent_dir)

    print(f"Staging sibling copy from {dist_dir} to {stage_dir} to retain existing hashed assets...")
    for item in os.listdir(dist_dir):
        s = os.path.join(dist_dir, item)
        d = os.path.join(stage_dir, item)
        if os.path.islink(s):
            print(f"FATAL: Existing dist contains symlink '{s}'. Forbidden.", file=sys.stderr)
            shutil.rmtree(stage_dir)
            sys.exit(1)
        if os.path.isdir(s):
            shutil.copytree(s, d, symlinks=False)
        else:
            shutil.copy2(s, d)

    print("Validating tar members and overlaying verified tar assets...")
    expected_index_sha = safe_inspect_and_extract_tar(args.artifact_tar, stage_dir)

    staged_index = os.path.join(stage_dir, "index.html")
    if not os.path.isfile(staged_index):
        print("FATAL: Staged dist is missing index.html after extraction.", file=sys.stderr)
        shutil.rmtree(stage_dir)
        sys.exit(1)

    staged_index_sha = compute_sha256(staged_index)
    if staged_index_sha.lower() != expected_index_sha.lower():
        print(f"FATAL: Staged index.html SHA mismatch with tar root index.html!", file=sys.stderr)
        shutil.rmtree(stage_dir)
        sys.exit(1)

    print(f"Performing atomic swap via libc.renameat2 RENAME_EXCHANGE between {dist_dir} and {stage_dir}...")
    atomic_exchange_dirs(dist_dir, stage_dir)

    timestamp_suffix = f"{int(time.time())}_{uuid.uuid4().hex[:8]}"
    backup_dir = os.path.join(parent_dir, f"{os.path.basename(dist_dir)}_backup_{timestamp_suffix}")
    os.rename(stage_dir, backup_dir)
    print(f"OK: Previous live dist preserved at unique backup: {backup_dir}")

    live_index = os.path.join(dist_dir, "index.html")
    if not os.path.isfile(live_index):
        print("FATAL: Live dist missing index.html after swap!", file=sys.stderr)
        sys.exit(1)

    live_index_sha = compute_sha256(live_index)
    if live_index_sha.lower() != expected_index_sha.lower():
        print(f"FATAL: Live index.html SHA does not match expected artifact index SHA!", file=sys.stderr)
        sys.exit(1)

    check_pids(args.baseline_pids, "post-swap")
    verify_health_endpoint(args.health_url)

    print("SUCCESS: Atomic frontend deployment verified cleanly with zero downtime, zero process kills, and preserved backups.")


if __name__ == "__main__":
    main()
