#!/usr/bin/env bash
set -uo pipefail
base="$RUNNER_TEMP/rex-base"
git worktree add --detach "$base" d59508fa9 || exit $?
cleanup() { git worktree remove --force "$base"; git worktree prune; }
trap cleanup EXIT
mkdir -p "$base/src-tauri/resources" "$base/ui"
cp -a src-tauri/resources/ffmpeg "$base/src-tauri/resources/"
cp -a ui/dist "$base/ui/"
git -C "$base" submodule update --init --recursive
export CARGO_TARGET_DIR="$PWD/src-tauri/target"
(cd "$base" && timeout 1800 cargo test --manifest-path src-tauri/Cargo.toml --lib --locked) > "$GATE_DIR/baseline-lib.log" 2>&1; echo "baseline lib EXIT=$?"
(cd "$base" && bun install --cwd ui --frozen-lockfile && bun run --cwd ui test) > "$GATE_DIR/baseline-ui.log" 2>&1; echo "baseline ui EXIT=$?"
(cd "$base" && bun run --cwd ui lint) > "$GATE_DIR/baseline-lint.log" 2>&1; echo "baseline lint EXIT=$?"
(cd "$base" && timeout 1800 cargo build --release --manifest-path src-tauri/Cargo.toml --locked) > "$GATE_DIR/baseline-release.log" 2>&1; echo "baseline release EXIT=$?"
