#!/usr/bin/env bash
# Temporary gate for feat/rex-20261030 (rex-learnings-20261030). Arg: <repo-dir>
set -u
cd "$1" || exit 2
mkdir -p ui/dist && [ -f ui/dist/index.html ] || echo '<!doctype html>' > ui/dist/index.html
cd src-tauri
for f in native_terminal::program_status::tests terminal::program_status::tests daemon::agent_hook_install::tests ssh::remote_hooks cli::api_catalog::tests daemon::agent_extension daemon::agent_state daemon::protocol remote::machine_agent_state agent_control::tests daemon::agent_observer terminal::screen_mirror terminal::output_hub terminal::program_status_ingest terminal::remote::tests::program_status native_terminal::bell daemon::client; do
  echo "== TEST $f"; cargo test --lib "$f" 2>&1 | tail -n 60; echo "TEST_EXIT $f ${PIPESTATUS[0]}"
done
cargo build --bin ferryx-cli 2>&1 | tail -n 20; echo "CLI_BUILD_EXIT ${PIPESTATUS[0]}"
n=$(./../target/debug/ferryx-cli api list 2>/dev/null | jq '.commands|length' 2>/dev/null || "$CARGO_TARGET_DIR/debug/ferryx-cli" api list | jq '.commands|length'); echo "API_LIST_LEN $n"
cargo clippy --all-targets -- -D warnings 2>&1 | grep -E "^(error|warning)|-->" | head -n 200; echo "CLIPPY_EXIT ${PIPESTATUS[0]}"
cargo test --lib 2>&1 | grep -E "^test .*FAILED|^failures:|test result" | tail -n 80; echo "FULL_LIB_EXIT ${PIPESTATUS[0]}"
echo GATE-DONE
