#!/bin/bash
# Lead-only: execute after granting P13 the exclusive Darwin Cargo slot.
set -euo pipefail
cd "$(dirname "$0")/../../.."
if [[ "${P13_EXCLUSIVE_CARGO_SLOT:-}" != "granted-by-lead" ]]; then
  printf '%s\n' 'Refusing Cargo: lead exclusive slot required.' >&2
  exit 2
fi
for test in \
  ipc::browser::tests::external_open_preserves_target_without_shell \
  ipc::browser::tests::external_open_propagates_failure \
  ipc::browser::tests::file_link_expands_bare_home \
  ipc::browser::tests::file_link_expands_profile_relative_path \
  ipc::browser::tests::windows_keypress_returns_typed_unsupported \
  browser::tests::native_history_invalidates_same_url_snapshot \
  browser::tests::engine_history_flags_win_over_the_shadow_vector
 do
  log="docs/evidence/windows-review-20260913/p13-cargo-${test##*::}.log"
  cargo test --manifest-path src-tauri/Cargo.toml --lib "$test" -- --exact --nocapture --test-threads=1 2>&1 | tee "$log"
  grep -q '1 passed; 0 failed;' "$log"
 done
