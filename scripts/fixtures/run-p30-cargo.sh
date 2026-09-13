#!/usr/bin/env bash
set -euo pipefail
# The shared Darwin Cargo target is exclusively scheduled by the lead.
if [[ "${P30_CARGO_SLOT:-}" != "lead-authorized" ]]; then
  echo 'P30 requires a lead-issued exclusive Cargo slot.' >&2
  exit 2
fi
case "${1:-}" in
  red|green) phase="$1" ;;
  *) echo 'usage: run-p30-cargo.sh red|green' >&2; exit 2 ;;
esac
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
log="docs/evidence/windows-review-20260913/p30/${phase}.log"
if [[ -e "$log" ]]; then
  echo "Refusing to overwrite evidence: $log" >&2
  exit 2
fi
{
  date -u '+%Y-%m-%dT%H:%M:%SZ'
  git rev-parse HEAD
  shasum -a 256 src-tauri/src/native_terminal/images.rs src-tauri/tests/native_terminal_images.rs
  cargo test --offline --manifest-path src-tauri/Cargo.toml --features native-terminal --test native_terminal_images kitty_rgb_expansion_limit_does_not_block_text_frames -- --exact --nocapture --test-threads=1
} 2>&1 | tee "$log"
# A disabled feature/zero-case invocation must not be counted as GREEN.
grep -Eq '^test result: ok\. 1 passed; 0 failed;' "$log"
