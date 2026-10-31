#!/usr/bin/env bash
# Temporary macOS gate for feat/rex-20261030 (rex-learnings-20261030). Arg: <repo-dir>
set -u
cd "$1" || exit 2
mkdir -p ui/dist && [ -f ui/dist/index.html ] || echo '<!doctype html>' > ui/dist/index.html
cd src-tauri
for f in native_terminal::program_status::tests terminal::program_status::tests daemon::agent_hook_install::tests ssh::remote_hooks cli::api_catalog::tests daemon::agent_extension daemon::agent_state daemon::protocol remote::machine_agent_state agent_control::tests daemon::agent_observer terminal::screen_mirror terminal::output_hub terminal::program_status_ingest terminal::remote::tests::program_status native_terminal::bell daemon::client terminal::foreground daemon::server::protocol_release_tests; do
  echo "== TEST $f"; cargo test --lib "$f" 2>&1 | tail -n 60; echo "MAC_TEST_EXIT $f ${PIPESTATUS[0]}"
done
cargo build --bin ferryx-cli 2>&1 | tail -n 20; echo "MAC_CLI_BUILD_EXIT ${PIPESTATUS[0]}"
./target/debug/ferryx-cli api list > /tmp/api.json; echo "MAC_API_LIST_EXIT $?"; echo "MAC_API_LIST_LEN $(jq '.commands|length' /tmp/api.json)"
cargo clippy --all-targets -- -D warnings > /tmp/clippy.txt 2>&1; echo "MAC_CLIPPY_EXIT $?"
grep -E -- "--> src/(terminal/program_status|native_terminal/program_status|terminal/program_status_ingest|daemon/agent_hook_install|cli/api_catalog|daemon/agent_observer|terminal/screen_mirror|native_terminal/bell)" /tmp/clippy.txt | sort | uniq -c; echo "MAC_CLIPPY_TOUCHED_DONE"
echo GATE-DONE
