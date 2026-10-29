#!/usr/bin/env bash
# int2 follow-up: daemon_handover_contract int-only candidate, 3x int + 3x base (base built in a 94c4a3f5 worktree, same runner).
set -u; mkdir -p ci-logs
run(){ n=$1; shift; echo "+ $*" | tee ci-logs/$n.log; "$@" 2>&1 | tee -a ci-logs/$n.log; rc=${PIPESTATUS[0]}; echo "EXIT_CODE=$rc" | tee -a ci-logs/$n.log; echo "$n exit=$rc" >> ci-logs/exits.txt; }
echo "HEAD=$(git rev-parse HEAD) TREE=$(git rev-parse HEAD^{tree})" > ci-logs/provenance.txt
M=src-tauri/Cargo.toml; T=machine_retirement::final_machine_close_is_durable_before_predecessor_retires
run int-build cargo test --manifest-path $M --test daemon_handover_contract --no-run
for k in 1 2 3; do run int-exact-$k timeout 900 cargo test --manifest-path $M --test daemon_handover_contract -- --exact $T; done
run int-file timeout 1800 cargo test --manifest-path $M --test daemon_handover_contract
git worktree add --detach /tmp/basewt 94c4a3f5 >/dev/null 2>&1; rm -rf /tmp/basewt/src-tauri/vendor/ghostty; cp -a src-tauri/vendor/ghostty /tmp/basewt/src-tauri/vendor/ghostty; cp -a ui/dist /tmp/basewt/ui/dist 2>/dev/null
mv src-tauri/target /tmp/basewt/src-tauri/target
cd /tmp/basewt; ln -s $GITHUB_WORKSPACE/ci-logs ci-logs
run base-build cargo test --manifest-path $M --test daemon_handover_contract --no-run
for k in 1 2 3; do run base-exact-$k timeout 900 cargo test --manifest-path $M --test daemon_handover_contract -- --exact $T; done
run base-file timeout 1800 cargo test --manifest-path $M --test daemon_handover_contract
cat $GITHUB_WORKSPACE/ci-logs/exits.txt; exit 0
