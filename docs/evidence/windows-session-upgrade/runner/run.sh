#!/usr/bin/env bash
# Local driver for the maho-win phase1 runner. Every remote call propagates its exit code.
set -euo pipefail

W=/Volumes/T9-Mac/project/ferryx-windows-session-upgrade
EVID=$W/docs/evidence/windows-session-upgrade
KEY=/Users/indo/code/project/maho-workspace/.secrets/signing/maho_win_builder_ed25519
HOST=sook@100.126.171.58
OPTS=(-i "$KEY" -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=15)
BASE_WIN='C:\Users\sook\ferryx-wsu'
BASE_FWD='C:/Users/sook/ferryx-wsu'
STEPS=' ui-install ui-build build-bin lib-session-host lib-output-hub contract lib-full '

usage() { echo "usage: run.sh {push|preflight|checkout|cleanup|fetch|purge} <RUN> | run.sh step <name> <RUN>" >&2; exit 64; }
[[ $# -ge 2 ]] || usage
op=$1; shift
if [[ $op == step ]]; then
  [[ $# -ge 2 ]] || usage
  name=$1; RUN=$2
  [[ $STEPS == *" $name "* ]] || usage
else
  case $op in push | preflight | checkout | cleanup | fetch | purge) ;; *) usage ;; esac
  name=$op; RUN=$1
fi
[[ $RUN =~ ^wsu-[0-9a-f]{8}-[0-9a-f]{8}-r[0-9]{2}$ ]] || { echo "BAD_RUN_ID $RUN" >&2; exit 64; }

STAGE=/tmp/ferryx-wsu/$RUN
OUT=$EVID/$RUN
ROOT_WIN="$BASE_WIN\\$RUN"
[[ -f $OUT/run.json ]] || { echo "NOT_STAGED: $OUT/run.json missing (run stage.sh first)" >&2; exit 65; }
mkdir -p "$OUT/local" "$OUT/remote"
LOG=$OUT/local/$name-$(date -u +%Y%m%dT%H%M%SZ).log

# Run one PowerShell script string remotely (UTF-16LE EncodedCommand); returns its exit code.
rps() {
  local enc
  enc=$(printf '%s' "$1" | iconv -f UTF-8 -t UTF-16LE | base64 | tr -d '\n')
  ssh "${OPTS[@]}" "$HOST" "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $enc; exit \$LASTEXITCODE"
}
# Run an uploaded runner script; returns its exit code.
rfile() {
  local script=$1; shift
  ssh "${OPTS[@]}" "$HOST" "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $ROOT_WIN\\in\\runner\\$script -Run $RUN $*; exit \$LASTEXITCODE"
}
logged() {
  printf '# %s utc=%s\n' "$*" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$LOG"
  set +e
  "$@" 2>&1 | tee -a "$LOG"
  local rc=${PIPESTATUS[0]}
  set -e
  return "$rc"
}
fetch_one() { # $1 remote relative name, $2 required|optional
  local rel=$1 need=$2 prc=0
  logged rps "if (Test-Path -LiteralPath '$ROOT_WIN\\$rel') { exit 0 } else { exit 20 }" || prc=$?
  if [[ $prc -eq 20 ]]; then
    if [[ $need == required ]]; then echo "FETCH_MISSING_REQUIRED $rel" | tee -a "$LOG"; return 21; fi
    echo "FETCH_ABSENT_OPTIONAL $rel" | tee -a "$LOG"
    return 0
  fi
  [[ $prc -eq 0 ]] || return "$prc"
  logged scp "${OPTS[@]}" -r "$HOST:$BASE_FWD/$RUN/$rel" "$OUT/remote/"
}
fetch_all() {
  local frc=0 r rel
  fetch_one logs required || { r=$?; [[ $frc -ne 0 ]] || frc=$r; }
  for rel in run-start.txt procs.jsonl evidence cleanup; do
    fetch_one "$rel" optional || { r=$?; [[ $frc -ne 0 ]] || frc=$r; }
  done
  if [[ $frc -eq 0 ]]; then
    (cd "$OUT/remote" && find . -type f -print0 | sort -z | xargs -0 shasum -a 256) > "$OUT/local/fetched-sha256.txt" || frc=67
  fi
  return "$frc"
}

rc=0
case $op in
  push)
    [[ -f $STAGE/remote/in/run.json ]] || { echo "STAGE_MISSING $STAGE" | tee -a "$LOG"; rc=65; }
    [[ $rc -ne 0 ]] || logged rps "if (Test-Path -LiteralPath '$ROOT_WIN') { Write-Output 'REMOTE_ROOT_EXISTS'; exit 10 }; New-Item -ItemType Directory -Force -Path '$BASE_WIN' | Out-Null; exit 0" || rc=$?
    [[ $rc -ne 0 ]] || logged scp "${OPTS[@]}" -r "$STAGE/remote" "$HOST:$BASE_FWD/$RUN" || rc=$?
    [[ $rc -ne 0 ]] || logged rps "if ((Test-Path -LiteralPath '$ROOT_WIN\\in\\run.json') -and -not (Test-Path -LiteralPath '$ROOT_WIN\\remote')) { exit 0 }; Write-Output 'PUSH_LAYOUT_WRONG'; exit 11" || rc=$?
    ;;
  preflight) logged rfile preflight.ps1 || rc=$? ;;
  checkout) logged rfile checkout.ps1 || rc=$? ;;
  step) logged rfile step.ps1 -Step "$name" || rc=$? ;;
  cleanup)
    logged rfile cleanup.ps1 || rc=$?
    fetch_all || { r=$?; [[ $rc -ne 0 ]] || rc=$r; }
    ;;
  fetch) fetch_all || rc=$? ;;
  purge)
    # Receipts must already be local; purge deletes the remote root, so nothing is fetched afterwards.
    if [[ "$(head -n1 "$OUT/remote/cleanup/cleanup-exit.log" 2>/dev/null || true)" != "EXIT=0" ]]; then
      echo "PURGE_REFUSED: fetched cleanup receipt missing or nonzero" | tee -a "$LOG"; rc=66
    else
      logged rfile purge.ps1 || rc=$?
    fi
    ;;
esac
echo "WSU $RUN local:$name EXIT=$rc" | tee -a "$LOG"
exit "$rc"
