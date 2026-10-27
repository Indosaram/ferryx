#!/usr/bin/env bash
# One target/skip list for compilation and execution; never let diagnostic staging
# silently change which assertions run.
# macOS system Bash 3 treats empty arrays as unset under nounset.
set -eo pipefail
mode_flags=()
case "${1:-}" in
  compile) mode_flags=(--no-run) ;;
  run) ;;
  *) echo "usage: $0 compile|run" >&2; exit 2 ;;
esac
TEST_SUITE=${TEST_SUITE:-full}
case "$TEST_SUITE" in full|integration) ;; *) echo "invalid TEST_SUITE" >&2; exit 2 ;; esac
suite_flags=(--lib)
if [ "$TEST_SUITE" = "integration" ]; then
  suite_flags=()
  echo "TEST_SUITE=integration: library target skipped"
fi
# Preserve the existing renderer (real GPU) and omo CLI exclusions.
cargo test "${mode_flags[@]}" --manifest-path src-tauri/Cargo.toml \
  --target aarch64-apple-darwin \
  --no-fail-fast \
  "${suite_flags[@]}" \
  --test backend_hardening \
  --test daemon_handover_contract \
  --test daemon_handover_transfer_contract \
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
  --test platform_build_contract \
  --test rorca_native_contract \
  --test session_persistence_integration \
  --test updater_config_contract \
  --test updater_endpoint_contract \
  --test worktree_safety \
  -- --test-threads=1 \
  --skip native_terminal::renderer:: \
  --skip terminal::tests::test_spawn_startup_command_omo
