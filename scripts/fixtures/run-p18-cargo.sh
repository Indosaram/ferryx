#!/usr/bin/env bash
set -euo pipefail
# Lead must issue the shared-target slot before invoking this runner.
if [[ "${P18_CARGO_SLOT:-}" != "lead-authorized" ]]; then
  echo 'P18 requires a lead-issued exclusive Cargo slot.' >&2
  exit 2
fi
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
cargo test --offline --manifest-path src-tauri/Cargo.toml --test windows_edge_probe_contract --no-run
cargo test --offline --manifest-path src-tauri/Cargo.toml --test windows_edge_probe_contract -- --exact edge_wrapper_stages_and_cleans_its_protocol_driver --nocapture --test-threads=1
