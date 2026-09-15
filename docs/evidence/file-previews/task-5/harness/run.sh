#!/usr/bin/env bash
# One-shot task-5 browser evidence run: build the harness bundle, start the range-capable
# media server on a dedicated port (never :5173), drive a throwaway Chromium, then clean up
# the server and the browser unconditionally.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EVIDENCE_DIR="$(cd "$HERE/.." && pwd)"
UI_DIR="$(cd "$EVIDENCE_DIR/../../../../ui" && pwd)"
PORT="${QA_VIDEO_PORT:-47531}"
BUNDLE_DIR="$(mktemp -d /tmp/task5-video-bundle.XXXXXX)"
SERVER_LOG="$EVIDENCE_DIR/server-requests.jsonl"
SERVER_STDOUT="$EVIDENCE_DIR/server-stdout.log"
: >"$SERVER_LOG"

SERVER_PID=""
cleanup() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$BUNDLE_DIR"
}
trap cleanup EXIT

(
  cd "$UI_DIR"
  QA_VIDEO_OUT_DIR="$BUNDLE_DIR" ./node_modules/.bin/vite build --config qa-file-preview-video.config.mjs
) >"$EVIDENCE_DIR/harness-build.log" 2>&1
[[ -f "$BUNDLE_DIR/qa-file-preview-video.html" ]] || { echo "harness bundle missing"; tail -20 "$EVIDENCE_DIR/harness-build.log"; exit 1; }

bun "$HERE/server.mjs" "$BUNDLE_DIR" "$EVIDENCE_DIR/fixtures" "$SERVER_LOG" "$PORT" >"$SERVER_STDOUT" 2>&1 &
SERVER_PID=$!

# Readiness is the server's own stdout marker, not a timer.
for _ in $(seq 1 200); do
  if grep -q QA_SERVER_READY "$SERVER_STDOUT" 2>/dev/null; then break; fi
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then cat "$SERVER_STDOUT"; exit 1; fi
done
grep -q QA_SERVER_READY "$SERVER_STDOUT" || { cat "$SERVER_STDOUT"; exit 1; }

# One server, one browser at a time: desktop width first, then a narrow width.
QA_CHROMIUM="${QA_CHROMIUM:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell}" node "$HERE/drive.mjs" "http://127.0.0.1:$PORT/" "$EVIDENCE_DIR" desktop 1280 800
QA_CHROMIUM="${QA_CHROMIUM:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell}" node "$HERE/drive.mjs" "http://127.0.0.1:$PORT/" "$EVIDENCE_DIR" narrow 420 760
