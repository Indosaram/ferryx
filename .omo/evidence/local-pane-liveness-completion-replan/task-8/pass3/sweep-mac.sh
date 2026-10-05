#!/bin/sh
# Definitive missing-field sweep: run all-targets and parse EVERY E0063 site + target.
set -eu
cd /Users/I552267/ferryx-pane-completion/source-21dea3c0
export CARGO_TARGET_DIR=/Users/I552267/ferryx-pane-completion/source-21dea3c0/target
export PATH="$HOME/.cargo/bin:$PATH"
cargo check --manifest-path src-tauri/Cargo.toml --all-targets > /tmp/sweep-all-targets.log 2>&1 || true
echo "=== E0063 sites ==="
grep -A 2 "E0063" /tmp/sweep-all-targets.log | grep -E "^\s+--> " | sort -u
echo "=== failed targets ==="
grep -E "could not compile" /tmp/sweep-all-targets.log | sort -u
echo "=== total errors ==="
grep -cE "^error" /tmp/sweep-all-targets.log || true
