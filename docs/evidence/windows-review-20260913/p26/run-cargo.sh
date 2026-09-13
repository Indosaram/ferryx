#!/bin/bash
# Lead only: execute after granting the shared Cargo target exclusive slot.
set -euo pipefail
cd "$(dirname "$0")/../../../.."
cargo test --manifest-path src-tauri/Cargo.toml --test permissions_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib permissions:: -- --nocapture
