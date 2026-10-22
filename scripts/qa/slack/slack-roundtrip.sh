#!/usr/bin/env bash
# Slack round trip for scripts/qa/real-machine-check.sh slack, against an isolated daemon.
#
#   slack-roundtrip.sh mock   # loopback Slack stand-in, fake tokens: proves the wiring end to end
#   slack-roundtrip.sh real   # the owner's Slack app; needs the env below and two human clicks
#
# real mode needs (a *_FILE path wins over the plain var):
#   SLACK_APP_TOKEN or SLACK_APP_TOKEN_FILE   xapp-...  app-level token, scope connections:write
#   SLACK_BOT_TOKEN or SLACK_BOT_TOKEN_FILE   xoxb-...  bot token (scopes in ferryx-slack-manifest.json)
#   FERRYX_HUB_SLACK_OWNER                    U...      owner's Slack member id
#   SLACK_CHANNEL                             C...      channel the bot is in (for messages and chat.reply)
# optional: CARD_CMD (default: recorded Claude approval card), CLI, KEEP=1 (leave daemon up)
set -euo pipefail
MODE="${1:-}"; [[ "$MODE" == mock || "$MODE" == real ]] || { sed -n 2,15p "$0"; exit 2; }
HERE="$(cd "$(dirname "$0")" && pwd)"
CLI="${CLI:-/workspace/ferryx/src-tauri/target/debug/ferryx-cli}"
ROOT="${ROOT:-/tmp/ferryx-slk}"
PY="${PY:-/tmp/slackmock-venv/bin/python}"
MOCK_PORT="${MOCK_PORT:-9801}"
pass=0; fail=0
ok()  { echo "PASS: $*"; pass=$((pass+1)); }
bad() { echo "FAIL: $*"; fail=$((fail+1)); }
fx()  { python3 "$HERE/fx.py" "$@"; }
b64() { printf '%s' "$1" | base64 -w0; }
screen() { "$CLI" agents read "$1" --json | python3 -c 'import json,sys;print(json.load(sys.stdin)["session"]["screen"]["text"])'; }

# --- isolated daemon ---------------------------------------------------------------
# Only the daemon serving this script's own socket is ever stopped.
daemon_pid() { ss -xlpH 2>/dev/null | awk -v s="$ROOT/runtime/daemon.sock" '$5==s' | grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2 || true; }
stop_daemon() { local p; p=$(daemon_pid); [[ -n "$p" ]] && kill "$p" && sleep 2 || true; }
mock_pid() { ss -tlpnH "sport = :$MOCK_PORT" 2>/dev/null | grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2 || true; }
stop_daemon; [[ "$MODE" == mock ]] && { p=$(mock_pid); [[ -n "$p" ]] && kill "$p" || true; }
rm -rf "$ROOT"; mkdir -p "$ROOT"/{runtime,data,hub,work}; chmod 700 "$ROOT/runtime"
META="$(python3 -c 'import uuid;print(uuid.uuid4())')"
echo "{\"metaSessionId\":\"$META\"}" > "$ROOT/hub-config.json"
# A clean environment: no hub kill switch, relay token, or notify delay inherited from the shell.
unset FERRYX_HUB FERRYX_MACHINE_TOKEN FERRYX_HUB_TRACKER_TOKEN FERRYX_HUB_IMPORT FERRYX_HUB_SUPERVISOR_PANE
export FERRYX_RUNTIME_DIR="$ROOT/runtime" FERRYX_DATA_DIR="$ROOT/data" FERRYX_HUB_DB="$ROOT/hub/hub.sqlite"
export FERRYX_HUB_CONFIG="$ROOT/hub-config.json" FERRYX_HUB_WORKSPACE=slk FERRYX_HUB_TRACKER_ADDR=127.0.0.1:47813
export FERRYX_HUB_NOTIFY_AFTER_SECS="${SLK_NOTIFY_AFTER_SECS:-3}" FERRYX_HUB_QUIET_HOURS=off
export RUST_LOG="${RUST_LOG:-info,ferryx_hub=debug}" FAKE_CARD_LOG="$ROOT/card-keys.log"
if [[ "$MODE" == mock ]]; then
  printf 'xapp-1-LOCAL-MOCK' > "$ROOT/app.token"; printf 'xoxb-LOCAL-MOCK' > "$ROOT/bot.token"
  export SLACK_APP_TOKEN_FILE="$ROOT/app.token" SLACK_BOT_TOKEN_FILE="$ROOT/bot.token"
  export FERRYX_HUB_SLACK_OWNER=UOWNER SLACK_CHANNEL=CMOCK FERRYX_HUB_SLACK_API="http://127.0.0.1:$MOCK_PORT/api"
  : > "$ROOT/mock.jsonl"
  (setsid nohup "$PY" "$HERE/mock-slack.py" "$MOCK_PORT" "$ROOT/mock.jsonl" > "$ROOT/mock.out" 2>&1 < /dev/null &) > /dev/null 2>&1
  sleep 1.5
else
  : "${FERRYX_HUB_SLACK_OWNER:?set FERRYX_HUB_SLACK_OWNER=U...}" "${SLACK_CHANNEL:?set SLACK_CHANNEL=C...}"
  [[ -n "${SLACK_APP_TOKEN:-}${SLACK_APP_TOKEN_FILE:-}" ]] || { echo "set SLACK_APP_TOKEN(_FILE)"; exit 2; }
  [[ -n "${SLACK_BOT_TOKEN:-}${SLACK_BOT_TOKEN_FILE:-}" ]] || { echo "set SLACK_BOT_TOKEN(_FILE)"; exit 2; }
  unset FERRYX_HUB_SLACK_API
fi
(cd "$ROOT" && setsid nohup "$CLI" --daemon > "$ROOT/daemon.out" 2>&1 < /dev/null &) > /dev/null 2>&1
DLOG="$ROOT/data/logs/daemon.log"
for _ in $(seq 1 60); do [[ -S "$ROOT/runtime/daemon.sock" ]] && break; sleep 0.5; done
sleep 2
fx "{\"type\":\"registerWorkspace\",\"workspaceId\":\"slk\",\"repoRoot\":\"$ROOT/work\"}" >/dev/null
spawn() { fx "{\"type\":\"spawn\",\"clientRequestId\":\"slk-$1\",\"workspaceId\":\"slk\",\"worktree\":null,\"cwd\":\"$ROOT/work\",\"cols\":120,\"rows\":40$2}" | python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["sessionId"])'; }
write() { fx "{\"type\":\"write\",\"sessionId\":\"$1\",\"data\":\"$(b64 "$2")\"}" >/dev/null; }
M=$(spawn meta ",\"sessionId\":\"$META\""); [[ "$M" == "$META" ]] && ok "meta pane is $META (hub config metaSessionId)" || bad "meta pane id $M != $META"
sleep 1.5; write "$M" $'cat\r'
C=$(spawn card ""); sleep 1.5; write "$C" "${CARD_CMD:-python3 $HERE/fake-claude-card.py}"$'\r'
for _ in $(seq 1 20); do grep -q "slack socket mode connected" "$DLOG" 2>/dev/null && break; sleep 0.5; done
grep -q "slack socket mode connected" "$DLOG" && ok "Socket Mode connected" || bad "Socket Mode did not connect (see $DLOG)"

inject() { curl -fsS -XPOST "http://127.0.0.1:$MOCK_PORT/inject" -H 'content-type: application/json' -d "$1"; echo; }

# --- 1. owner message -> meta pane, fenced ---------------------------------------------
NONCE="slk-$(date +%s)-$RANDOM"
if [[ "$MODE" == mock ]]; then
  inject "{\"type\":\"events_api\",\"payload\":{\"event\":{\"type\":\"message\",\"channel\":\"$SLACK_CHANNEL\",\"user\":\"$FERRYX_HUB_SLACK_OWNER\",\"text\":\"status? $NONCE\",\"ts\":\"1700000000.0001\"}}}" >/dev/null
else
  echo ">>> As the owner, post this in <#$SLACK_CHANNEL>:  status? $NONCE"
fi
for _ in $(seq 1 ${WAIT_SECS:-120}); do screen "$M" | grep -q "$NONCE" && break; sleep 1; done
S=$(screen "$M")
if grep -q '```untrusted' <<<"$S" && grep -q "origin: slack channel=$SLACK_CHANNEL user=$FERRYX_HUB_SLACK_OWNER" <<<"$S" && grep -q "$NONCE" <<<"$S"; then
  ok "step 1: the message reached the meta pane inside an untrusted fence with its origin"
else bad "step 1: meta pane did not show the fenced message"; echo "$S" | tail -15; fi

# --- 2. approval card -> owner click answers, stranger refused -------------------------
for _ in $(seq 1 60); do "$CLI" agents read "$C" --json | grep -q '"pending":{' && break; sleep 1; done
READ=$("$CLI" agents read "$C" --json)
PROMPT=$(python3 -c 'import json,sys;d=json.loads(sys.argv[1])["session"];print((d.get("pending") or {}).get("id",""))' "$READ")
[[ -n "$PROMPT" ]] && ok "card pane shows a detected prompt ($PROMPT, agent $(python3 -c 'import json,sys;print(json.loads(sys.argv[1])["session"]["agent"])' "$READ"))" || bad "no pending prompt on the card pane"
VALUE=""
if [[ "$MODE" == mock ]]; then
  for _ in $(seq 1 60); do grep -q '"blocks"' "$ROOT/mock.jsonl" && break; sleep 1; done
  VALUE=$(python3 - "$ROOT/mock.jsonl" <<'PY'
import json,sys
for line in open(sys.argv[1]):
    e=json.loads(line)
    if e["kind"]=="chat.postMessage" and "blocks" in e["body"]:
        print(e["body"]["blocks"][1]["elements"][0]["value"]); break
PY
)
  [[ -n "$VALUE" ]] && ok "owner notice was posted to $FERRYX_HUB_SLACK_OWNER with answer buttons ($VALUE)" || bad "no card with buttons was posted"
  : > "$FAKE_CARD_LOG"
  inject "{\"type\":\"interactive\",\"payload\":{\"type\":\"block_actions\",\"user\":{\"id\":\"USTRANGER\"},\"actions\":[{\"value\":\"$VALUE\"}]}}" >/dev/null; sleep 2
  [[ ! -s "$FAKE_CARD_LOG" ]] && grep -q "not from the owner" "$DLOG" && ok "a stranger's click was refused and typed nothing" || bad "stranger click typed: $(cat "$FAKE_CARD_LOG")"
  inject "{\"type\":\"interactive\",\"payload\":{\"type\":\"block_actions\",\"user\":{\"id\":\"$FERRYX_HUB_SLACK_OWNER\"},\"actions\":[{\"value\":\"$VALUE\"}]}}" >/dev/null
else
  echo ">>> In Slack: have a non-owner click the first button on the ferryx DM card (must do nothing),"
  echo ">>> then click it as the owner ($FERRYX_HUB_SLACK_OWNER)."
fi
for _ in $(seq 1 ${WAIT_SECS:-120}); do [[ -s "$FAKE_CARD_LOG" ]] && break; sleep 1; done
sleep 1
if [[ -s "$FAKE_CARD_LOG" ]]; then ok "the pane received the option keys: $(tr '\n' ' ' < "$FAKE_CARD_LOG")"; else bad "the pane got no keys from the click"; fi

# --- 3. chat.reply posts the caller's text, not a screen copy --------------------------
REPLY="ferryx reply $NONCE-r (typed by the caller, not on any screen)"
OUT=$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
  "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"chat.reply\",\"arguments\":{\"channelId\":\"$SLACK_CHANNEL\",\"text\":\"$REPLY\"}}}" | timeout 60 "$CLI" mcp 2>&1 | tail -1)
echo "$OUT" | grep -q '"error"' && bad "chat.reply: $OUT" || ok "chat.reply returned $OUT"
if [[ "$MODE" == mock ]]; then
  grep -q "$NONCE-r" "$ROOT/mock.jsonl" && ok "chat.postMessage carried exactly the caller's text" || bad "chat.reply text never reached Slack"
  ! screen "$M" | grep -q "$NONCE-r" && ! screen "$C" | grep -q "$NONCE-r" && ok "the reply text is on no pane screen (not scraped)" || bad "reply text found on a screen"
  # --- 4. Slack asks to reconnect: the hub opens a new socket --------------------------
  inject '{"type":"disconnect","payload":{}}' >/dev/null; sleep 4
  [[ $(grep -c '"connections.open"' "$ROOT/mock.jsonl") -ge 2 ]] && ok "after a disconnect frame the hub reconnected" || bad "no reconnect after disconnect"
else
  echo ">>> Confirm the reply appears in <#$SLACK_CHANNEL>: $REPLY"
fi
sqlite3 "$FERRYX_HUB_DB" 'select id, session_key, delivered_at is not null from chat_outbox' 2>/dev/null || true
echo "== $pass passed, $fail failed  (daemon log: $DLOG)"
if [[ "${KEEP:-0}" != 1 ]]; then stop_daemon; [[ "$MODE" == mock ]] && { p=$(mock_pid); [[ -n "$p" ]] && kill "$p" || true; }; fi
[[ $fail == 0 ]]
