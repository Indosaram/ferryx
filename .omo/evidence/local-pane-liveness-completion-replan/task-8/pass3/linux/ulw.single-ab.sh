#!/bin/sh
set -u
B=/home/indo/ferryx-pane-completion
OUT=$B/task8-21dea3c0
LOG=$OUT/ab-single-test.log
: > "$LOG"
T=test_daemon_output_sequence_contiguity_and_replay_gap

run_side() {
  side=$1
  root=$2
  export CARGO_TARGET_DIR="$root/target"
  cd "$root" || return 9
  lf="$OUT/logs/ab-single-$side.log"
  cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract \
    -- --nocapture --test-threads=1 "$T" >"$lf" 2>&1
  rc=$?
  res=$(grep -aE '^test result:' "$lf" | tail -1)
  printf 'SINGLE side=%s native=%s | %s\n' "$side" "$rc" "$res" | tee -a "$LOG"
  printf 'SINGLE side=%s panic:\n' "$side" | tee -a "$LOG"
  grep -a -A4 "panicked at" "$lf" | head -8 | sed 's/^/    /' | tee -a "$LOG"
}

run_side base "$B/source-base"
run_side cand "$B/source-21dea3c0"
printf 'SINGLE_AB_DONE\n' | tee -a "$LOG"
