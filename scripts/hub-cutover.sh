#!/usr/bin/env bash
# Cut omo-gateway over to the ferryx hub, or roll that cutover back.
#
# Two Slack Socket Mode connections must not run at once. This script unloads
# ai.omon.gateway and ai.omon.gateway-watchdog before the ferryx daemon is
# asked to open the hub. It prints labels and row counts only. It does not
# print plist contents or secret files.
#
# Usage:
#   scripts/hub-cutover.sh cutover [--db PATH] [--ferryx PATH] [--dry-run] [--yes]
#   scripts/hub-cutover.sh rollback [--yes]
set -euo pipefail

GATEWAY_LABEL="${OMO_GATEWAY_LABEL:-ai.omon.gateway}"
WATCHDOG_LABEL="${OMO_WATCHDOG_LABEL:-ai.omon.gateway-watchdog}"
FERRYX_LABEL="${FERRYX_DAEMON_LABEL:-com.ferryx.daemon}"
DB="${OMO_DB:-${HOME}/.omon/gateway.db}"
FERRYX_BIN="${FERRYX_BIN:-ferryx}"
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

if [[ "$MODE" == "cutover" ]]; then
  bootout "$WATCHDOG_LABEL" || true
  bootout "$GATEWAY_LABEL"
  wait_until_gone "$GATEWAY_LABEL"
  if [[ ! -f "$DB" ]]; then
    echo "omo database not found: ${DB}" >&2
    exit 1
  fi
  echo "migrate dry-run ${DB}"
  "$FERRYX_BIN" hub migrate --from-omo "$DB" --dry-run
  if [[ "$DRY" == 1 ]]; then
    echo "dry-run only; ferryx hub was not started"
    exit 0
  fi
  echo "migrate ${DB}"
  "$FERRYX_BIN" hub migrate --from-omo "$DB"
  if launchctl print "${domain}/${FERRYX_LABEL}" >/dev/null 2>&1; then
    echo "kickstart ${FERRYX_LABEL}"
    launchctl kickstart -k "${domain}/${FERRYX_LABEL}"
  else
    echo "ferryx daemon label ${FERRYX_LABEL} is not loaded; start Ferryx so the hub binds 127.0.0.1:9119"
  fi
  echo "cutover requested. confirm http://127.0.0.1:9119/api/health before leaving omo unloaded"
  exit 0
fi

# Rollback restarts the ferryx daemon with the hub off, then loads omo again.
# Restarting the daemon drops live PTY sessions.
export FERRYX_HUB=off
if launchctl print "${domain}/${FERRYX_LABEL}" >/dev/null 2>&1; then
  echo "kickstart ${FERRYX_LABEL} with FERRYX_HUB=off"
  launchctl kickstart -k "${domain}/${FERRYX_LABEL}" || true
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
