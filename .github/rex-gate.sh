#!/usr/bin/env bash
# Temporary gate for feat/rex-20261030 (rex-learnings-20261030). Arg: <repo-dir>
set -uo pipefail
cd "$1" || exit 2
mkdir -p out
logs="$PWD/out"
status=0
mkdir -p ui/dist && [ -f ui/dist/index.html ] || echo '<!doctype html>' > ui/dist/index.html
cd src-tauri
for f in native_terminal::program_status::tests terminal::program_status::tests daemon::agent_hook_install::tests ssh::remote_hooks cli::api_catalog::tests daemon::agent_extension daemon::agent_state daemon::protocol remote::machine_agent_state agent_control::tests daemon::agent_observer terminal::screen_mirror terminal::output_hub terminal::program_status_ingest terminal::remote::tests::program_status native_terminal::bell daemon::client terminal::foreground daemon::server::protocol_release_tests; do
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
echo GATE-DONE
exit "$status"
