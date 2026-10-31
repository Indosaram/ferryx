#!/usr/bin/env bash
# Temporary gate for feat/rex-20261030 (rex-learnings-20261030). Arg: <repo-dir>
set -uo pipefail
cd "$1" || exit 2
mkdir -p out
logs="$PWD/out"
status=0
for step in install ui helper; do
  case "$step" in
    install) bun install --cwd ui --frozen-lockfile 2>&1 | tee "$logs/setup-install.log" ;;
    ui) bun run --cwd ui build 2>&1 | tee "$logs/setup-ui.log" ;;
    helper) cargo build --manifest-path remote-helper/Cargo.toml 2>&1 | tee "$logs/setup-helper.log" ;;
  esac
  e=$?
  echo "SETUP_EXIT $step $e"
  if [ "$e" -ne 0 ]; then exit "$e"; fi
done
cd src-tauri
for f in native_terminal::program_status::tests terminal::program_status::tests daemon::agent_hook_install::tests ssh::remote_hooks cli::api_catalog::tests daemon::agent_extension daemon::agent_state daemon::protocol remote::machine_agent_state agent_control::tests daemon::agent_observer terminal::screen_mirror terminal::output_hub terminal::program_status_ingest terminal::remote::tests::program_status native_terminal::bell daemon::client terminal::foreground daemon::server::protocol_release_tests remote::protocol outcome_acknowledgement every_reference_chat_route_is_registered the_relay_allowlist_admits_exactly; do
  echo "== TEST $f"
  cargo test --lib "$f" 2>&1 | tee "$logs/test-${f//::/-}.log"
  e=$?
  echo "TEST_EXIT $f $e"
  if [ "$e" -ne 0 ]; then status=1; fi
done
cargo build --bin ferryx-cli 2>&1 | tee "$logs/cli-build.log"
e=$?
echo "CLI_BUILD_EXIT $e"
if [ "$e" -ne 0 ]; then status=1; fi
./target/debug/ferryx-cli api list > "$logs/api-list.json"
e=$?
echo "API_LIST_EXIT $e"
if [ "$e" -ne 0 ]; then status=1; fi
n=$(jq -er '.commands | length' "$logs/api-list.json")
e=$?
echo "API_LIST_LEN $n"
if [ "$e" -ne 0 ] || [ "$n" != 13 ]; then status=1; fi
cargo clippy --all-targets -- -D warnings 2>&1 | tee "$logs/clippy.log"
e=$?
echo "CLIPPY_EXIT $e"
if [ "$e" -ne 0 ]; then status=1; fi
cargo test --lib 2>&1 | tee "$logs/full-lib.log"
e=$?
echo "FULL_LIB_EXIT $e"
if [ "$e" -ne 0 ]; then status=1; fi
# Preserve baseline evidence for the unresolved renderer and PTY assertions on the
# same runner. A passing isolated rerun is not proof of a full-suite flake.
if [ "$e" -ne 0 ]; then
  baseline="$(dirname "$1")/ferryx-wt/rex-baseline-20261031"
  git -C "$1" worktree add --detach "$baseline" a3beca3f
  baseline_setup=$?
  if [ "$baseline_setup" -eq 0 ]; then
    git -C "$baseline" submodule update --init --recursive &&
      bun install --cwd "$baseline/ui" --frozen-lockfile &&
      bun run --cwd "$baseline/ui" build
    baseline_setup=$?
    echo "BASELINE_SETUP_EXIT $baseline_setup"
    if [ "$baseline_setup" -eq 0 ]; then
      for f in native_terminal::renderer::font_manager native_terminal::renderer::linux_cluster_raster a_wedged_pty_does_not_block_the_async_runtime_thread; do
        CARGO_TARGET_DIR="$1/src-tauri/target" cargo test --manifest-path "$baseline/src-tauri/Cargo.toml" --lib "$f" 2>&1 | tee "$logs/baseline-${f//::/-}.log"
        echo "BASELINE_TEST_EXIT $f $?"
      done
    fi
    git -C "$1" worktree remove --force "$baseline"
    git -C "$1" worktree prune
    git -C "$1" worktree list
  else
    echo "BASELINE_WORKTREE_EXIT $baseline_setup"
  fi
fi
echo GATE-DONE
exit "$status"
