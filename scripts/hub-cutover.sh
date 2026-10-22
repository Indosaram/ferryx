#!/usr/bin/env bash
# Cut omo-gateway over to the ferryx hub, or roll that cutover back.
#
# Two Slack Socket Mode connections must not run at once. The live database is
# snapshotted and imported while omo is still running. ai.omon.gateway and
# ai.omon.gateway-watchdog are unloaded only after that import succeeds, and
# only then is the ferryx daemon asked to open the hub. A dry run prints the
# bootout it would do and does not unload anything. It prints labels and row
# counts only. It does not print plist contents or secret files.
#
# Usage:
#   scripts/hub-cutover.sh cutover [--db PATH] [--ferryx PATH] [--dry-run] [--yes]
#   scripts/hub-cutover.sh rollback [--yes]
set -euo pipefail

GATEWAY_LABEL="${OMO_GATEWAY_LABEL:-ai.omon.gateway}"
WATCHDOG_LABEL="${OMO_WATCHDOG_LABEL:-ai.omon.gateway-watchdog}"
FERRYX_LABEL="${FERRYX_DAEMON_LABEL:-com.ferryx.daemon}"
DB="${OMO_DB:-}"
# `ferryx hub` and `ferryx-cli hub` both dispatch the hub command. Prefer
# ferryx-cli when it is already on PATH and no binary was chosen.
if [[ -z "${FERRYX_BIN:-}" ]]; then
  if command -v ferryx-cli >/dev/null 2>&1; then FERRYX_BIN=ferryx-cli; else FERRYX_BIN=ferryx; fi
fi
MODE=""
DRY=0
YES=0

usage() {
  echo "usage: scripts/hub-cutover.sh cutover|rollback [--db PATH] [--ferryx PATH] [--dry-run] [--yes]" >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    cutover|rollback) MODE="$1" ;;
    --db) DB="${2:-}"; shift ;;
    --ferryx) FERRYX_BIN="${2:-}"; shift ;;
    --dry-run) DRY=1 ;;
    --yes) YES=1 ;;
    -h|--help) usage ;;
    *) echo "unknown argument $1" >&2; usage ;;
  esac
  shift
done

[[ -n "$MODE" ]] || usage

uid="$(id -u)"
domain="gui/${uid}"

bootout() {
  local label="$1"
  if launchctl print "${domain}/${label}" >/dev/null 2>&1; then
    echo "bootout ${label}"
    launchctl bootout "${domain}/${label}"
  else
    echo "not loaded ${label}"
  fi
}

bootstrap_plist() {
  local label="$1"
  local plist="${HOME}/Library/LaunchAgents/${label}.plist"
  if [[ ! -f "$plist" ]]; then
    echo "missing plist for ${label}" >&2
    return 1
  fi
  echo "bootstrap ${label}"
  launchctl bootstrap "${domain}" "$plist"
}

wait_until_gone() {
  local pattern="$1"
  local i
  for i in 1 2 3 4 5 6 7 8 9 10; do
    if ! pgrep -f "$pattern" >/dev/null 2>&1; then
      echo "stopped ${pattern}"
      return 0
    fi
    sleep 1
  done
  echo "still running ${pattern}" >&2
  return 1
}

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "launchctl cutover runs on macOS" >&2
  exit 1
fi

if [[ "$YES" != 1 ]]; then
  echo "refusing ${MODE} without --yes" >&2
  exit 2
fi

# The live database is the gateway plist's WorkingDirectory, then the project
# checkout, then ~/.omon/gateway.db. plutil prints one string. The plist body
# is not printed.
discover_db() {
  local plist="${HOME}/Library/LaunchAgents/${GATEWAY_LABEL}.plist"
  local dir=""
  if [[ -f "$plist" ]] && command -v plutil >/dev/null 2>&1; then
    dir="$(plutil -extract WorkingDirectory raw "$plist" 2>/dev/null || true)"
  fi
  if [[ -n "$dir" && -f "${dir}/omon_gateway.db" ]]; then
    printf '%s\n' "${dir}/omon_gateway.db"
    return
  fi
  if [[ -f "${HOME}/code/project/omon-gateway/omon_gateway.db" ]]; then
    printf '%s\n' "${HOME}/code/project/omon-gateway/omon_gateway.db"
    return
  fi
  if [[ -f "${HOME}/.omon/gateway.db" ]]; then
    printf '%s\n' "${HOME}/.omon/gateway.db"
    return
  fi
  printf '\n'
}

if [[ -z "$DB" ]]; then
  DB="$(discover_db)"
fi

if [[ "$MODE" == "cutover" ]]; then
  if [[ ! -f "$DB" ]]; then
    echo "omo database not found: ${DB}" >&2
    exit 1
  fi
  ORIG_DIR="$(cd "$(dirname "$DB")" && pwd)"
  if [[ "$DRY" == 1 ]]; then
    # A dry run must not touch launchd: omo keeps serving Slack. The source is
    # opened read-only, so counting it while the gateway runs is safe. The hub
    # database is not created.
    for label in "$WATCHDOG_LABEL" "$GATEWAY_LABEL"; do
      if launchctl print "${domain}/${label}" >/dev/null 2>&1; then
        echo "would bootout ${label}"
      else
        echo "not loaded ${label}"
      fi
    done
    echo "migrate dry-run ${DB}"
    "$FERRYX_BIN" hub migrate --from-omo "$DB" --cwd "$ORIG_DIR" --dry-run
    echo "dry-run only; omo was left running and ferryx hub was not started"
    exit 0
  fi
  if ! command -v sqlite3 >/dev/null 2>&1; then
    echo "sqlite3 is required to snapshot the live database; omo was left running" >&2
    exit 1
  fi
  SNAP=""
  DELTA=""
  booted_out=0
  restore_omo() {
    local status=$?
    if [[ -n "${SNAP}" ]]; then
      rm -f "$SNAP"
    fi
    if [[ -n "${DELTA}" ]]; then
      rm -f "$DELTA"
    fi
    if [[ "$status" != 0 && "$booted_out" == 1 ]]; then
      echo "cutover failed (exit ${status}); loading omo again" >&2
      bootstrap_plist "$GATEWAY_LABEL" || echo "omo was NOT restored; load ${GATEWAY_LABEL} by hand" >&2
      bootstrap_plist "$WATCHDOG_LABEL" || true
    fi
  }
  trap restore_omo EXIT
  SNAP="$(mktemp "${TMPDIR:-/tmp}/omo-gateway.XXXXXXXX.backup")"
  # Consistent read of the live database. omo keeps its sockets until the copy
  # is in the hub database.
  if ! sqlite3 "$DB" ".backup '$SNAP'"; then
    echo "could not snapshot ${DB}; omo was left running" >&2
    exit 1
  fi
  echo "migrate dry-run ${SNAP}"
  if ! "$FERRYX_BIN" hub migrate --from-omo "$SNAP" --cwd "$ORIG_DIR" --dry-run; then
    echo "migrate dry-run failed; omo was left running" >&2
    exit 1
  fi
  echo "migrate ${SNAP}"
  if ! "$FERRYX_BIN" hub migrate --from-omo "$SNAP" --cwd "$ORIG_DIR" --imported-from "$DB"; then
    echo "migrate failed; omo was left running" >&2
    exit 1
  fi
  rm -f "$SNAP"
  SNAP=""
  # Import succeeded. Unload omo, then start hub tasks. kickstart -k would drop live PTYs.
  booted_out=1
  bootout "$WATCHDOG_LABEL" || true
  bootout "$GATEWAY_LABEL"
  wait_until_gone "$GATEWAY_LABEL"
  # Rows written after the first snapshot. omo is unloaded, so this backup is the
  # last consistent copy. Re-import updates changed cron, issue, and inbox rows.
  DELTA="$(mktemp "${TMPDIR:-/tmp}/omo-gateway-delta.XXXXXXXX.backup")"
  if ! sqlite3 "$DB" ".backup '$DELTA'"; then
    echo "could not snapshot ${DB} after bootout; loading omo again" >&2
    exit 1
  fi
  echo "migrate delta ${DB}"
  if ! "$FERRYX_BIN" hub migrate --from-omo "$DELTA" --cwd "$ORIG_DIR" --imported-from "$DB"; then
    echo "delta migrate failed; loading omo again" >&2
    exit 1
  fi
  rm -f "$DELTA"
  DELTA=""
  if "$FERRYX_BIN" hub start; then
    echo "hub start sent on the control socket; the daemon was not restarted"
  else
    echo "hub control socket is not up. omo will be loaded again. A running daemon was not kickstarted, so live PTY sessions were left alone." >&2
    exit 1
  fi
  trap - EXIT
  echo "cutover requested. confirm http://127.0.0.1:9119/api/health before leaving omo unloaded"
  exit 0
fi

# Rollback stops hub tasks through the control socket, then loads omo again.
# The ferryx daemon keeps its PTY sessions. omo is not started if the hub
# cannot be stopped, so two Slack sockets are not opened by accident.
echo "hub stop"
if ! "$FERRYX_BIN" hub stop; then
  echo "hub did not stop. omo was not started, and the ferryx daemon was not restarted." >&2
  exit 1
fi
plist="${HOME}/Library/LaunchAgents/${GATEWAY_LABEL}.plist"
if [[ -f "$plist" ]]; then
  bootstrap_plist "$GATEWAY_LABEL"
else
  echo "gateway plist is not installed; omo was not started" >&2
  exit 1
fi
watchdog_plist="${HOME}/Library/LaunchAgents/${WATCHDOG_LABEL}.plist"
if [[ -f "$watchdog_plist" ]]; then
  bootstrap_plist "$WATCHDOG_LABEL" || true
fi
echo "rollback requested. omo Socket Mode may connect; keep the ferryx hub off until that is confirmed"
