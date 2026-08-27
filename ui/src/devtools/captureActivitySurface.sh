#!/usr/bin/env bash
# Real-surface capture for agent-activity QA. Drives the browser-rendered harness
# (real TabBar + WorktreeList + real workspaceReducer) with agent-browser and
# screenshots each scenario, so the artifact reflects shipped rendering.
set -uo pipefail

EVIDENCE_DIR="${1:?usage: captureActivitySurface.sh <evidenceDir> <label>}"
LABEL="${2:-capture}"
URL="http://127.0.0.1:5173/activity-qa.html"

mkdir -p "$EVIDENCE_DIR"
LOG="$EVIDENCE_DIR/$LABEL-log.txt"
: >"$LOG"

say() { printf '%s\n' "$1" | tee -a "$LOG"; }

# Reads the rendered indicator for the background tab plus every worktree row dot.
PROBE_JS='(() => {
  const tabs = [...document.querySelectorAll("[data-tab-dnd-id]")];
  const byTab = {};
  for (const el of tabs) {
    const dot = el.querySelector("[data-status-state]");
    byTab[el.getAttribute("data-tab-dnd-id")] = {
      statusState: dot ? dot.getAttribute("data-status-state") : null,
      spinning: dot ? (dot.getAttribute("class") || "").includes("animate-spin") : false,
      agentIcon: Boolean(el.querySelector("[data-testid=\"tab-agent-icon\"]")),
    };
  }
  const worktrees = [...document.querySelectorAll("[data-testid=\"worktree-status-dot\"]")]
    .map((el) => el.getAttribute("data-activity-state"));
  return JSON.stringify({ byTab, worktrees });
})()'

agent-browser open "$URL" >>"$LOG" 2>&1
agent-browser wait '[data-testid="harness-tabbar"]' >>"$LOG" 2>&1
say "loaded $URL"

# agent-browser wraps page output in nonce delimiter lines; keep only the payload between them.
browser_eval() {
    agent-browser eval "$1" 2>>"$LOG" | sed -n '/^--- AGENT_BROWSER_PAGE_CONTENT/,/^--- END_AGENT_BROWSER_PAGE_CONTENT/p' | sed '1d;$d'
}

scenario() {
  local id="$1"; shift
  agent-browser click '[data-testid="qa-reset"]' >>"$LOG" 2>&1
  for step in "$@"; do
    agent-browser click "[data-testid=\"$step\"]" >>"$LOG" 2>&1
  done
  agent-browser wait '[data-testid="harness-state"]' >>"$LOG" 2>&1
  local probe
  probe="$(browser_eval "$PROBE_JS" | tr -d '\n')"
  agent-browser screenshot "$EVIDENCE_DIR/$LABEL-$id.png" >>"$LOG" 2>&1
  say "SCENARIO $id: $probe"
  say "  screenshot=$EVIDENCE_DIR/$LABEL-$id.png"
  printf '%s' "$probe"
}

say "=== agent-activity real-surface capture: $LABEL ==="
scenario working-background qa-working-background >"$EVIDENCE_DIR/$LABEL-working.json"
scenario nonstatus-after-working qa-working-background qa-nonstatus-after-working >"$EVIDENCE_DIR/$LABEL-nonstatus.json"
scenario shell-repaint qa-working-background qa-shell-repaint >"$EVIDENCE_DIR/$LABEL-shell.json"
scenario done-background qa-working-background qa-done-background >"$EVIDENCE_DIR/$LABEL-done.json"
scenario waiting-background qa-working-background qa-waiting-background >"$EVIDENCE_DIR/$LABEL-waiting.json"
scenario screen-working qa-screen-working-background >"$EVIDENCE_DIR/$LABEL-screen-working.json"
scenario screen-title-cannot-override qa-screen-working-background qa-screen-title-cannot-override >"$EVIDENCE_DIR/$LABEL-screen-hold.json"
scenario screen-blocked qa-screen-working-background qa-screen-blocked-background >"$EVIDENCE_DIR/$LABEL-screen-blocked.json"
scenario screen-idle-attention qa-screen-working-background qa-screen-idle-background >"$EVIDENCE_DIR/$LABEL-screen-idle.json"

agent-browser close >>"$LOG" 2>&1
say "browser closed"
say "=== capture complete: $EVIDENCE_DIR ==="
