#!/usr/bin/env bash
# Health check for the ferryx hub HTTP surface.
#
# launchd can run this every 60 seconds. It does not read or print secret files.
# A Slack DM is sent only when WATCHDOG_SLACK_TOKEN_FILE and WATCHDOG_SLACK_CHANNEL
# are both set. The token stays in the request header and is not logged.
set -u

LABEL="${WATCHDOG_LABEL:-com.ferryx.daemon}"
DASHBOARD_URL="${WATCHDOG_DASHBOARD_URL:-http://127.0.0.1:9119/api/health}"
STATE_DIR="${WATCHDOG_STATE_DIR:-${HOME}/.ferryx/hub/watchdog-state}"
SLACK_TOKEN_FILE="${WATCHDOG_SLACK_TOKEN_FILE:-}"
SLACK_CHANNEL="${WATCHDOG_SLACK_CHANNEL:-}"
FAIL_LIMIT="${WATCHDOG_FAIL_LIMIT:-3}"
ALERT_COOLDOWN="${WATCHDOG_ALERT_COOLDOWN:-1800}"
DRY_ALERT="${WATCHDOG_DRY_ALERT:-0}"
FERRYX_STATUS_BIN="${WATCHDOG_FERRYX_BIN:-}"

mkdir -p "$STATE_DIR"
now() { date +%s; }
log() { printf '%s hub-watchdog: %s\n' "$(date '+%F %T')" "$*"; }
counter() { cat "$STATE_DIR/$1" 2>/dev/null || echo 0; }
set_counter() { echo "$2" >"$STATE_DIR/$1"; }

http_ok() { [[ "$(curl -s -m 5 -o /dev/null -w '%{http_code}' "$1")" == 200 ]]; }

alert() {
  local key="$1" message="$2" last
  last="$(cat "$STATE_DIR/alert.$key" 2>/dev/null || echo 0)"
  if (( $(now) - last < ALERT_COOLDOWN )); then
    return 0
  fi
  now >"$STATE_DIR/alert.$key"
  log "ALERT[$key] $message"
  if [[ "$DRY_ALERT" == 1 ]]; then
    printf 'DRY_ALERT %s %s\n' "$key" "$message"
    return 0
  fi
  if [[ -z "$SLACK_TOKEN_FILE" || -z "$SLACK_CHANNEL" || ! -f "$SLACK_TOKEN_FILE" ]]; then
    return 0
  fi
  local token
  token="$(tr -d '[:space:]' <"$SLACK_TOKEN_FILE" 2>/dev/null || true)"
  [[ -z "$token" ]] && return 0
  curl -s -m 10 -o /dev/null -X POST https://slack.com/api/chat.postMessage \
    -H "Authorization: Bearer ${token}" -H 'Content-Type: application/json; charset=utf-8' \
    --data-binary "$(printf '%s' "$message" | python3 -c 'import json,sys; print(json.dumps({"channel":sys.argv[1],"text":sys.stdin.read()}))' "$SLACK_CHANNEL")"
}

# With WATCHDOG_FERRYX_BIN set, `hub status --check` must pass too: hub tasks running,
# HTTP bound, Slack Socket Mode connected when configured, no failed inbox watcher.
# /api/health alone stays 200 while Slack is disconnected.
status_ok() {
  [[ -z "$FERRYX_STATUS_BIN" ]] && return 0
  "$FERRYX_STATUS_BIN" hub status --check >/dev/null 2>&1
}

reason=""
if ! http_ok "$DASHBOARD_URL"; then
  reason="http ${DASHBOARD_URL}"
elif ! status_ok; then
  reason="hub status --check"
fi

if [[ -z "$reason" ]]; then
  set_counter gateway_fail 0
  log "healthy ${DASHBOARD_URL}"
  exit 0
fi

fails="$(( $(counter gateway_fail) + 1 ))"
set_counter gateway_fail "$fails"
log "unhealthy ${reason} fail ${fails}"
if (( fails >= FAIL_LIMIT )); then
  alert health "ferryx hub ${reason} failed ${fails} checks (label ${LABEL})"
fi
exit 1
