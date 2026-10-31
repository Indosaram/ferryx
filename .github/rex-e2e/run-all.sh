#!/usr/bin/env bash
# todo-9 live E2E (plan rex-learnings-20261030). Linux host, python3 stdlib only.
# Usage: run-all.sh [scenario ...]   (default: every scenario)
# Env: FERRYX_CLI (default ../ferryx-cli next to this directory, the rex-linux-bin artifact
#      layout), FERRYX_HELPER (default ../ferryx-remote-helper), E2E_LOG_DIR (default $PWD).
# Each scenario writes $E2E_LOG_DIR/task-9-<name>.log with PASS/FAIL/INFO lines and a final
# `SCENARIO <name> PASS|FAIL|SKIP <reason>` line; this script echoes that line per scenario and
# ends with E2E-DONE. Never run on the owner's MacBook.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
export E2E_LOG_DIR=${E2E_LOG_DIR:-$PWD}
export FERRYX_CLI=${FERRYX_CLI:-$HERE/../ferryx-cli}
export FERRYX_HELPER=${FERRYX_HELPER:-$HERE/../ferryx-remote-helper}
mkdir -p "$E2E_LOG_DIR"
rc=0
if [ -f "$(dirname "$FERRYX_CLI")/SHA256SUMS" ]; then
  if (cd "$(dirname "$FERRYX_CLI")" && sha256sum -c SHA256SUMS); then echo "BIN-SHA256 PASS"; else echo "BIN-SHA256 FAIL"; rc=1; fi
fi
if [ "$#" -gt 0 ]; then scenarios=("$@"); else scenarios=(emitter replay release ack observer80 claude); fi
for s in "${scenarios[@]}"; do
  timeout 1200 python3 "$HERE/ferryx_e2e.py" "$s" > "$E2E_LOG_DIR/task-9-$s.driver.out" 2>&1
  line=$(grep -E "^SCENARIO $s " "$E2E_LOG_DIR/task-9-$s.log" 2>/dev/null | tail -n 1)
  if [ -z "$line" ]; then line="SCENARIO $s FAIL no-verdict"; fi
  case "$line" in *" FAIL"*) rc=1 ;; esac
  echo "$line"
done
echo E2E-DONE
exit "$rc"
