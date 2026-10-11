#!/bin/bash
# ci.sh - ferryx macOS precheck (lint gates + Rust unit/integration suites) on a
# rented XenoCI Mac, driven by XenoCast (https://github.com/Xeno-CI/xenocast).
#
#   xenocast build --repo Indosaram/ferryx --ref <branch> --script ci.sh
#
# Use --repo mode: a local-folder upload (`xenocast build --script ./ci.sh`)
# carries the working tree without .git or dotfiles, and the fixture suites
# re-clone the checkout and resolve HEAD~1. Expect ~15-20 minutes on a lite
# rental; the exit code is the build's exit code.
#
# Mirrors the "precheck" job in .github/workflows/build-test.yml. Toolchain
# (Rust stable, Bun, Zig 0.16.0) is installed into $HOME when missing; the
# XenoCI build VM ships with Xcode, Homebrew and Node only.
set -euo pipefail
cd "$(dirname "$0")"

# Keep test compilation within the VM's disk budget, as in CI. Assertions stay on.
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.bun/bin:$PATH"

echo '== toolchain =='
if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
rustup target add aarch64-apple-darwin 2>/dev/null || true
if ! command -v bun >/dev/null 2>&1; then
  npm install -g bun || curl -fsSL https://bun.sh/install | bash
fi
if ! command -v zig >/dev/null 2>&1; then
  mkdir -p "$HOME/.local/zig" "$HOME/.local/bin"
  curl -fsSL https://ziglang.org/download/0.16.0/zig-aarch64-macos-0.16.0.tar.xz \
    | tar -xJ -C "$HOME/.local/zig" --strip-components 1
  ln -sf "$HOME/.local/zig/zig" "$HOME/.local/bin/zig"
fi
cargo --version && bun --version && zig version

echo '== repository depth =='
# Fixture suites resolve HEAD~1; a shallow clone turns that into a fatal path error.
git fetch --unshallow 2>/dev/null || true
git log --oneline -2

echo '== submodules =='
git submodule update --init --recursive

# Fixture suites commit through git; a fresh VM ships no global identity.
echo '== git identity for fixture repositories =='
git config --global user.name "ferryx-ci"
git config --global user.email "ferryx-ci@users.noreply.github.com"
git config --global init.defaultBranch main

# The rollout rehearsal and worktree fixtures create temporary repositories under
# TMPDIR and assert it is set (isolated TMPDIR), because a temp repository inside
# the checkout is re-owned by git's ceiling rules.
export TMPDIR="$HOME/ferryx-ci-tmp"
mkdir -p "$TMPDIR"
export GIT_CEILING_DIRECTORIES="$TMPDIR"

echo '== disk before compilation =='
df -h . "$TMPDIR"

echo '== dependencies =='
bun install --frozen-lockfile || bun install
bun install --cwd ui --frozen-lockfile || bun install --cwd ui

echo '== frontend build (Tauri asset embed) =='
bun run --cwd ui build

# bridge_tests asserts remote-helper/target/debug/ferryx-remote-helper exists.
echo '== remote helper (debug) =='
cargo build --manifest-path remote-helper/Cargo.toml

echo '== lint gates =='
bun scripts/release-workflow-policy.mjs
bun scripts/native-terminal-thread-policy.mjs
bun test scripts/native-terminal-thread-policy.test.mjs

# The P19 overlay gate suites bind a CGNAT overlay address on a NON-loopback
# interface. Report the outcome so a missing capability shows up in this log.
echo '== overlay address for host-dependent suites =='
OVERLAY_ADDRESS=100.64.1.2
sudo -n true && echo "PROVISION sudo=available" || echo "PROVISION sudo=unavailable"
if ! ifconfig -a | grep -q "$OVERLAY_ADDRESS"; then
  for interface in $(ifconfig -l); do
    case "$interface" in lo*) continue ;; esac
    ifconfig "$interface" | grep -q 'inet ' || continue
    if sudo -n ifconfig "$interface" alias "$OVERLAY_ADDRESS" netmask 255.255.255.255 up 2>/dev/null \
      || sudo -n ifconfig "$interface" alias "$OVERLAY_ADDRESS" up 2>/dev/null; then
      echo "PROVISION overlay_interface=$interface"
      break
    fi
  done
fi
ifconfig -a | grep "$OVERLAY_ADDRESS" && echo "PROVISION overlay_alias=present" || echo "PROVISION overlay_alias=missing"

# Skips are the suites a scored CI VM cannot serve, each with the reason it was
# measured to fail (see .github/workflows/build-test.yml for the full list):
# the renderer suite builds a real WGPU device; the omo startup test needs the
# omo agent CLI on PATH.
echo '== cargo test (unit and integration suites) =='
cargo test --manifest-path src-tauri/Cargo.toml \
  --target aarch64-apple-darwin \
  --no-fail-fast \
  --lib \
  --test backend_hardening \
  --test daemon_persistence_contract \
  --test e2e_agent_workflow \
  --test ghostty_build_contract \
  --test ghostty_build_info_ffi \
  --test ipc_hardening_contract \
  --test native_terminal_capability_contract \
  --test native_terminal_child_surface_contract \
  --test native_terminal_composition_contract \
  --test native_terminal_engine_contract \
  --test native_terminal_input_boundary_contract \
  --test native_terminal_surface_host_contract \
  --test native_terminal_wayland_subsurface_contract \
  --test rorca_native_contract \
  --test session_persistence_integration \
  --test updater_config_contract \
  --test updater_endpoint_contract \
  --test worktree_safety \
  -- --test-threads=1 \
  --skip native_terminal::renderer:: \
  --skip terminal::tests::test_spawn_startup_command_omo

echo '== remote grid frame seam (Rust -> TypeScript) =='
bun run --cwd ui src/remote/gridFrameSeam.check.ts

echo '== disk after run =='
df -h . "$TMPDIR"
for directory in src-tauri/target remote-helper/target; do
  if [ -d "$directory" ]; then du -sh "$directory"; fi
done
