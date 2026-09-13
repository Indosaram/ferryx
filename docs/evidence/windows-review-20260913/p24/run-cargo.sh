#!/usr/bin/env bash
set -euo pipefail
cd /Users/indo/code/project/orca-lite
phase=${1:?expected red or green}
case "$phase" in red|green) ;; *) echo 'phase must be red or green' >&2; exit 2;; esac
receipt=docs/evidence/windows-review-20260913/p24-slot-grant.txt
if [[ ! -f "$receipt" ]] || ! grep -q 'st_01a09a0a' "$receipt"; then
  echo 'Blocked: lead-issued exclusive Cargo slot receipt naming st_01a09a0a required.' >&2
  exit 3
fi
out="docs/evidence/windows-review-20260913/p24/$phase"
mkdir -p "$out"
if [[ -e "$out/test.log" ]]; then
  echo 'Refusing to overwrite existing execution evidence.' >&2
  exit 4
fi
{
  date -u '+%Y-%m-%dT%H:%M:%SZ'
  rustc -Vv
  cargo -V
  shasum -a 256 src-tauri/src/dag/watcher.rs src-tauri/Cargo.lock
  printf 'CARGO_TARGET_DIR=%s\n' "${CARGO_TARGET_DIR:-src-tauri/target}"
  printf '%s\n' 'cargo test --manifest-path src-tauri/Cargo.toml --lib dag::watcher::tests:: -- --nocapture --test-threads=1'
} > "$out/provenance.log"
set +e
cargo test --manifest-path src-tauri/Cargo.toml --lib dag::watcher::tests:: -- --nocapture --test-threads=1 > "$out/test.log" 2>&1
status=$?
set -e
printf '%s\n' "$status" > "$out/exit-code.txt"
# Cargo prints the exact executable path in the test log; retain its content hash.
python3 - "$out/test.log" > "$out/binary-sha256.log" <<'PY'
import hashlib, pathlib, re, sys
text = pathlib.Path(sys.argv[1]).read_text()
for name in re.findall(r'Running unittests .*? \(([^\n)]+)\)', text):
    path = pathlib.Path(name)
    if path.is_file():
        print(hashlib.sha256(path.read_bytes()).hexdigest(), path)
PY
printf 'P24 %s exit=%s; receipts=%s\n' "$phase" "$status" "$out"
exit "$status"
