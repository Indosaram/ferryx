#!/usr/bin/env bash
set -euo pipefail
export PATH=$HOME/.cargo/bin:$HOME/.bun/bin:/opt/homebrew/bin:$PATH
cd /Users/I552267/ferryx-release-2026.930.1-016936d4
export CARGO_TARGET_DIR=/Users/I552267/ferryx-input-diag-9297/src-tauri/target
export ZIG=/opt/homebrew/bin/zig
bun install --cwd ui --frozen-lockfile
bun run --cwd ui build
tauri build --bundles app --config '{"build":{"beforeBuildCommand":""},"bundle":{"createUpdaterArtifacts":false}}'
echo REMOTE_RELEASE_BUILD_OK
