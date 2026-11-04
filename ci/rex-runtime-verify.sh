#!/usr/bin/env bash
# Verify Rex Beta (build 1758) runtime claims from REX-BETA-STRUCTURE-REPORT.md section 16,
# items 1, 2, 3 and 6, on a disposable or owner test macOS machine (never a developer desktop).
#
# Usage: bash ci/rex-runtime-verify.sh [--app PATH] [--out DIR] [--gui] [--screenshot]
#   --app PATH  Rex Beta.app copied from the analysis artifact (default: ./Rex Beta.app)
#   --out DIR   artifact directory (default: ./out)
#   --gui        also launch the GUI from the private copy (off by default)
#   --screenshot with --gui, run screencapture (only where Screen Recording is already granted,
#                because macOS otherwise shows a permission prompt)
#
# Steps (each writes a receipt into $OUT):
#   1. preflight: macOS, >= 2 GiB free, bundle build 1758 (passwordless sudo optional)
#   2. install the app into a private work dir (never /Applications)
#   3. disable Sparkle through user defaults (automatic checks off, SUFeedURL pointed at a dead
#      local port) before the first launch; with passwordless sudo, also block the feed host in
#      /etc/hosts for this run only (an EXIT trap restores it from a backup and hashes the result)
#   4. start the bundled server on a private Unix socket with private HOME/XDG dirs and no
#      Tailscale flags (no tailnet login, no account credentials); record its PID
#   5. exercise items 1, 2, 3, 6 and write $OUT/verdicts.json with exactly those four items
#   6. uninstall: kill only recorded PIDs (checked by executable path), remove the app copy,
#      private dirs and the defaults domain; prove the bundle version did not change
# No real account credentials are used. Anything that needs an account is INCONCLUSIVE.
set -euo pipefail

APP="./Rex Beta.app"
OUT="./out"
GUI=0
SHOT=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --app) APP="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --gui) GUI=1; shift ;;
    --screenshot) SHOT=1; shift ;;
    -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

EXPECTED_BUILD=1758
BUNDLE_ID=com.superlogical.rex
FEED_HOST=blob.superlogical.com
HOSTS_MARK="# rex-runtime-verify"
DEAD_FEED=https://127.0.0.1:9/appcast.xml
FEED_BLOCK=none

mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
LOG="$OUT/run.log"
: > "$LOG"
rm -f "$OUT/verdicts.json"
log() { printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$LOG"; }
json_item() { printf '    {"item": %s, "verdict": "%s", "reason": "%s", "evidence": "%s"}' "$1" "$2" "${3//\"/}" "$4"; }
write_verdicts() {
  {
    echo '{'
    echo "  \"rex_build\": \"${build:-unknown}\","
    echo "  \"rex_build_after\": \"${build_after:-unknown}\","
    echo "  \"update_feed_block\": \"$FEED_BLOCK\","
    echo '  "items": ['
    json_item 1 "$V1" "$R1" "${ABORT_EVIDENCE:-out/item1-upgrade.txt}"; echo ','
    json_item 2 "$V2" "$R2" "${ABORT_EVIDENCE:-out/item2-osc.txt}"; echo ','
    json_item 3 "$V3" "$R3" "${ABORT_EVIDENCE:-out/item3-web.txt}"; echo ','
    json_item 6 "$V6" "$R6" "${ABORT_EVIDENCE:-out/item6-skill-status.txt}"; echo
    echo '  ]'
    echo '}'
  } > "$OUT/verdicts.json"
}
# An aborted run still leaves all four items, INCONCLUSIVE with the abort reason.
die() {
  log "FATAL: $*"
  local why="not exercised: run aborted: $*"
  V1=INCONCLUSIVE V2=INCONCLUSIVE V3=INCONCLUSIVE V6=INCONCLUSIVE
  R1=$why R2=$why R3=$why R6=$why
  ABORT_EVIDENCE=out/run.log
  write_verdicts
  exit 1
}

# ---- 1. preflight ---------------------------------------------------------------------------
[[ "$(uname -s)" == Darwin ]] || die "not macOS ($(uname -s)); run this only on a macOS test machine"
HAVE_SUDO=0
if sudo -n true 2>/dev/null; then HAVE_SUDO=1; fi
free_kb=$(df -k / | awk 'NR==2 {print $4}')
(( free_kb > 2 * 1024 * 1024 )) || die "less than 2 GiB free on / (${free_kb} KiB)"
[[ -d "$APP" ]] || die "app bundle not found: $APP"
build=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$APP/Contents/Info.plist")
[[ "$build" == "$EXPECTED_BUILD" ]] || die "bundle build is $build, expected $EXPECTED_BUILD"
bundle_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$APP/Contents/Info.plist")
[[ "$bundle_id" == "$BUNDLE_ID" ]] || die "bundle id is $bundle_id, expected $BUNDLE_ID"
{ sw_vers; uname -m; echo "free_kb=$free_kb"; echo "build=$build"; echo "sudo=$HAVE_SUDO"; } > "$OUT/preflight.txt"
log "preflight ok: build $build, free ${free_kb} KiB"

REAL_HOME=$HOME
WORK=$(mktemp -d "${TMPDIR:-/tmp}/rex-verify.XXXXXX")
PIDS=()
HOSTS_BACKUP="$WORK/hosts.orig"
HOSTS_TOUCHED=0

cleanup() {
  local rc=$? pid exe
  set +e
  if [[ -n "${REX:-}" && -f "$WORK/Rex Beta.app/Contents/Info.plist" ]]; then
    build_after=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$WORK/Rex Beta.app/Contents/Info.plist")
    echo "CFBundleVersion before=$build after=$build_after" > "$OUT/bundle-version-receipt.txt"
    if [[ -f "$OUT/verdicts.json" ]]; then
      sed -i '' "s/\"rex_build_after\": \"[^\"]*\"/\"rex_build_after\": \"$build_after\"/" "$OUT/verdicts.json"
    fi
  fi
  for pid in ${PIDS[@]+"${PIDS[@]}"}; do
    exe=$(ps -o comm= -p "$pid" 2>/dev/null)
    if [[ -n "$exe" && "$exe" != "$WORK"/* ]]; then echo "pid $pid is now $exe, not ours: left alone"; continue; fi
    kill "$pid" 2>/dev/null
    for _ in 1 2 3 4 5 6 7 8 9 10; do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
    kill -9 "$pid" 2>/dev/null
    if kill -0 "$pid" 2>/dev/null; then echo "pid $pid STILL ALIVE"; else echo "pid $pid GONE"; fi
  done >> "$OUT/uninstall-receipt.txt"
  if (( HOSTS_TOUCHED )); then
    sudo cp "$HOSTS_BACKUP" /etc/hosts && sudo dscacheutil -flushcache
    {
      echo "hosts original: $(shasum -a 256 "$HOSTS_BACKUP" | cut -d' ' -f1)"
      echo "hosts restored: $(shasum -a 256 /etc/hosts | cut -d' ' -f1)"
      if grep -q "$HOSTS_MARK" /etc/hosts; then echo "hosts marker STILL PRESENT"; else echo "hosts marker absent"; fi
    } > "$OUT/hosts-restore-receipt.txt"
  fi
  HOME=$REAL_HOME defaults delete "$BUNDLE_ID" >/dev/null 2>&1
  rm -rf "$WORK"
  {
    if [[ -e "$WORK" ]]; then echo "work dir STILL PRESENT: $WORK"; else echo "work dir removed: $WORK"; fi
    # cfprefsd may keep an empty domain cached after delete; what matters is that no key is left.
    if HOME=$REAL_HOME defaults read "$BUNDLE_ID" SUFeedURL >/dev/null 2>&1; then echo "defaults keys STILL PRESENT"; else echo "defaults keys absent ($(HOME=$REAL_HOME defaults read "$BUNDLE_ID" 2>&1 | tr -d '\n'))"; fi
    for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -f "rex-verify\.${WORK##*.}" >/dev/null || break; sleep 0.5; done
    left=$(pgrep -fl "rex-verify\.${WORK##*.}" || true)
    if [[ -n "$left" ]]; then echo "processes under work dir STILL RUNNING: $left"; else echo "no processes under work dir"; fi
    echo "exit=$rc"
  } >> "$OUT/uninstall-receipt.txt"
  log "cleanup done (exit $rc)"
}
trap cleanup EXIT

# ---- 2. install into the private work dir ---------------------------------------------------
ditto "$APP" "$WORK/Rex Beta.app"
xattr -dr com.apple.quarantine "$WORK/Rex Beta.app" 2>/dev/null || true
REX="$WORK/Rex Beta.app/Contents/Helpers/Rex Server.rex/Contents/MacOS/rex"
[[ -x "$REX" ]] || die "server binary missing: $REX"
codesign -dv --verbose=2 "$WORK/Rex Beta.app" > "$OUT/codesign.txt" 2>&1 || true
log "installed into $WORK"

# ---- 3. block auto-update for this run only (before any launch) ------------------------------
defaults write "$BUNDLE_ID" SUEnableAutomaticChecks -bool NO
defaults write "$BUNDLE_ID" SUAutomaticallyUpdate -bool NO
defaults write "$BUNDLE_ID" SUFeedURL "$DEAD_FEED"
defaults read "$BUNDLE_ID" > "$OUT/defaults-block-receipt.txt" 2>&1
[[ "$(defaults read "$BUNDLE_ID" SUFeedURL)" == "$DEAD_FEED" ]] || die "could not set SUFeedURL in user defaults"
FEED_BLOCK=user-defaults
if (( HAVE_SUDO )); then
  cp /etc/hosts "$HOSTS_BACKUP"
  HOSTS_TOUCHED=1
  printf '0.0.0.0 %s %s\n::1 %s %s\n' "$FEED_HOST" "$HOSTS_MARK" "$FEED_HOST" "$HOSTS_MARK" \
    | sudo tee -a /etc/hosts > /dev/null
  sudo dscacheutil -flushcache
  {
    echo "hosts before: $(shasum -a 256 "$HOSTS_BACKUP" | cut -d' ' -f1)"
    grep "$HOSTS_MARK" /etc/hosts
    echo "resolve $FEED_HOST: $(dscacheutil -q host -a name "$FEED_HOST" | awk '/ip_address/ {print $2}' | tr '\n' ' ')"
  } > "$OUT/hosts-block-receipt.txt"
  FEED_BLOCK=user-defaults+etc-hosts
else
  echo "no passwordless sudo: /etc/hosts not modified; feed disabled through user defaults only" > "$OUT/hosts-block-receipt.txt"
fi
log "update feed disabled via $FEED_BLOCK"

# ---- 4. start the server in isolation -------------------------------------------------------
export HOME="$WORK/home" REX_HOME="$WORK/home/.rex"
export XDG_CONFIG_HOME="$WORK/xdg/config" XDG_DATA_HOME="$WORK/xdg/data"
export XDG_STATE_HOME="$WORK/xdg/state" XDG_RUNTIME_DIR="$WORK/xdg/run" XDG_CACHE_HOME="$WORK/xdg/cache"
mkdir -p "$HOME" "$REX_HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME"
mkdir -m 700 -p "$XDG_RUNTIME_DIR"
SOCK="$WORK/rex.sock"
"$REX" --help > "$OUT/rex-help.txt" 2>&1 || true
# A loopback-only HTTP listener lets item 3 see what a browser gets without a tailnet login.
# Plain TCP callers are anonymous, so Rex needs --allow-anonymous; 127.0.0.1 keeps it on this host.
HTTP_PORT=$(/usr/bin/python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
"$REX" server run --listen "unix://$SOCK" --extra-listen "http://127.0.0.1:$HTTP_PORT" --allow-anonymous \
  --restore=false > "$OUT/server.log" 2>&1 &
PIDS+=($!)
echo "server pid ${PIDS[0]}" > "$OUT/pids.txt"
for _ in $(seq 1 40); do [[ -S "$SOCK" ]] && break; sleep 0.25; done
[[ -S "$SOCK" ]] || die "server socket did not appear: see server.log"
export REX_SOCKET="unix://$SOCK"
"$REX" server status > "$OUT/server-status.txt" 2>&1 || true
log "server up at $SOCK (pid ${PIDS[0]})"

get() { curl -sS --max-time 10 --unix-socket "$SOCK" -D "$OUT/$2.headers" -o "$OUT/$2.body" "http://localhost$1" || true; }

# ---- 5. items -------------------------------------------------------------------------------
# Item 1: the local GUI<->server transport is WebSocket over a Unix socket.
ls -l "$SOCK" > "$OUT/item1-socket.txt"
lsof -nP -a -p "${PIDS[0]}" -U >> "$OUT/item1-socket.txt" 2>&1 || true
get /api/help item1-api-help
curl -sS --max-time 5 --unix-socket "$SOCK" -i -o "$OUT/item1-upgrade.txt" \
  -H 'Connection: Upgrade' -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' \
  -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' "http://localhost/api/control" || true
if grep -qE '^HTTP/1\.1 101' "$OUT/item1-upgrade.txt" 2>/dev/null; then
  V1=CONFIRMED; R1="WebSocket upgrade (101) accepted on the Unix socket"
elif grep -qE '^HTTP/' "$OUT/item1-api-help.headers" 2>/dev/null; then
  V1=INCONCLUSIVE; R1="HTTP is served on the Unix socket but the probed path did not upgrade to WebSocket"
else
  V1=REFUTED; R1="the Unix socket does not speak HTTP"
fi

# Item 2: OSC 7501/3008 are Rex shell-integration sequences with a documented payload.
get /llms.txt item2-llms
{
  grep -n -E '7501|3008' "$OUT/item2-llms.body" "$OUT/rex-help.txt" 2>/dev/null || true
  grep -r -n -E '7501|3008' "$HOME" "$WORK/xdg" 2>/dev/null || true
} > "$OUT/item2-osc.txt"
if grep -q 7501 "$OUT/item2-osc.txt"; then
  V2=CONFIRMED; R2="runtime docs or shell integration files describe OSC 7501"
  grep -q 3008 "$OUT/item2-osc.txt" || R2="$R2; OSC 3008 is not described at runtime"
else
  V2=INCONCLUSIVE; R2="no runtime artifact describes OSC 7501/3008"
fi

# Item 3: the web client is htmx server-rendered (not a bundled client/web build).
# The local Unix socket serves the API only; the browser surface is probed on the loopback listener.
get / item3-root
hget() { curl -sS --max-time 10 -D "$OUT/$2.headers" -o "$OUT/$2.body" "http://127.0.0.1:$HTTP_PORT$1" || true; }
hget / item3-http-root
: > "$OUT/item3-paths.txt"
for p in / /web /app /ui /static/htmx.min.js /htmx.min.js /bus/htmx-websocket.min.js; do
  printf 'unix %s %s\n' "$p" "$(curl -s --max-time 5 --unix-socket "$SOCK" -o /dev/null -w '%{http_code}' "http://localhost$p")" >> "$OUT/item3-paths.txt"
  printf 'http %s %s\n' "$p" "$(curl -s --max-time 5 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HTTP_PORT$p")" >> "$OUT/item3-paths.txt"
done
{
  cat "$OUT/item3-paths.txt"
  echo '--- loopback / headers'; cat "$OUT/item3-http-root.headers" 2>/dev/null
  echo '--- script and htmx references in loopback / body'
  grep -o -i -E 'htmx[^" ]*|<script[^>]*src="[^"]*"|hx-[a-z-]+=' "$OUT/item3-http-root.body" 2>/dev/null | sort | uniq -c || true
} > "$OUT/item3-web.txt"
if grep -qi -E 'htmx|hx-[a-z]+=' "$OUT/item3-http-root.body" "$OUT/item3-root.body" 2>/dev/null \
   || grep -qE 'htmx[^ ]* 200$' "$OUT/item3-paths.txt"; then
  V3=CONFIRMED; R3="the server serves an htmx page or htmx assets"
elif grep -qi '<script' "$OUT/item3-http-root.body" "$OUT/item3-root.body" 2>/dev/null; then
  V3=REFUTED; R3="the served page loads scripts but no htmx"
else
  V3=INCONCLUSIVE; R3="no web page on the Unix socket or the anonymous loopback listener; the web client likely needs a tailnet listener (account login)"
fi

# Item 6: the built-in agent list is exactly claude, codex, opencode. `rex skill status` reports,
# per known agent, whether it is found and whether Rex's skill and hooks are installed. Nothing is
# installed (HOME is private); every known harness is listed by id whether or not it is detected.
export REX_SERVER="unix://$SOCK"
{
  echo '$ rex skill status'; "$REX" skill status 2>&1 || true
  echo '$ rex skill status --json'; "$REX" skill status --json 2>&1 || true
  echo '$ rex skill --help'; "$REX" skill --help 2>&1 || true
} > "$OUT/item6-skill-status.txt"
"$REX" skill status --json > "$OUT/item6-skill-status.json" 2>/dev/null || true
# Harness ids come from the JSON, not from a name list, so an unexpected agent cannot be missed.
agents=$(/usr/bin/python3 -c 'import json,sys; print(" ".join(sorted(h["id"] for h in json.load(open(sys.argv[1]))["harnesses"])) + " ")' \
  "$OUT/item6-skill-status.json" 2>/dev/null || true)
[[ "$agents" == " " ]] && agents=""
echo "agents seen: $agents" >> "$OUT/item6-skill-status.txt"
if [[ "$agents" == "claude codex opencode " ]]; then
  V6=CONFIRMED; R6="rex skill status lists exactly claude, codex, opencode"
elif [[ -n "$agents" ]]; then
  V6=REFUTED; R6="rex skill status lists: $agents"
else
  V6=INCONCLUSIVE; R6="rex skill status printed no agent names"
fi

# Best-effort GUI screenshot (needs a logged-in console session and Screen Recording permission).
if (( GUI )); then
  "$WORK/Rex Beta.app/Contents/MacOS/Rex" > "$OUT/gui.log" 2>&1 &
  PIDS+=($!)
  echo "gui pid $!" >> "$OUT/pids.txt"
  sleep 8
  # Children the GUI spawned from our private copy are ours too; record them by path.
  for pid in $(pgrep -f "$WORK" || true); do
    case " ${PIDS[*]} " in *" $pid "*) ;; *) PIDS+=("$pid"); echo "gui child pid $pid" >> "$OUT/pids.txt" ;; esac
  done
  if (( SHOT )); then
    screencapture -x "$OUT/screenshot-gui.png" 2>> "$OUT/gui.log" || echo "screencapture failed" >> "$OUT/gui.log"
  fi
fi

write_verdicts
log "verdicts: 1=$V1 2=$V2 3=$V3 6=$V6"

# ---- 6. uninstall happens in the EXIT trap (recorded PIDs, /etc/hosts, files, defaults) -----
exit 0
