# Orca Agent Status & Worktree State Management Architecture

## Overview & Evidence Sources

This document provides a precise, reverse-engineered specification of how Orca models per-session agent status, worktree card activity, and unread badges. The analysis is based on extraction and inspection of the production Orca runtime binaries and bundle assets:
- `/Applications/Orca.app/Contents/Resources/app.asar` (extracted to `/tmp/orca-asar/`)
- `/Applications/Orca.app/Contents/Resources/app.asar/out/shared/agent-status-types.js`
- `/Applications/Orca.app/Contents/Resources/app.asar/out/shared/agent-title-status.js`
- `/Applications/Orca.app/Contents/Resources/app.asar/out/shared/agent-status-osc.js`
- `/Applications/Orca.app/Contents/Resources/app.asar/out/shared/agent-hook-status-cache.js`
- `/Applications/Orca.app/Contents/Resources/app.asar/out/shared/workspace-statuses.js`
- `/Applications/Orca.app/Contents/Resources/app.asar/out/shared/workspace-status-defaults.js`
- `/Applications/Orca.app/Contents/Resources/app.asar/out/renderer/assets/worktree-status-DR0Zr8Ht.js`
- `/Applications/Orca.app/Contents/Resources/app.asar/out/renderer/assets/terminal-tab-activity-status-BaKqQAEL.js`
- `/Applications/Orca.app/Contents/Resources/app.asar/out/renderer/assets/agent-status-3vUKbY6l.js`
- `/Applications/Orca.app/Contents/Resources/app.asar/out/renderer/assets/StatusIndicator-CJ9TRLK4.js`
- `/Applications/Orca.app/Contents/Resources/app.asar/out/renderer/assets/WorktreeCard-DHnjoc37.js`
- `/Applications/Orca.app/Contents/Resources/app.asar/out/renderer/assets/store-CgXrfmaH.js`
- `/Users/indo/Library/Application Support/orca/opencode-hooks/shared/plugins/orca-opencode-status.js`
- `/Users/indo/.omo/agent/extensions/orca-agent-status.ts`
- `/usr/local/bin/orca` (CLI help and runtime guides)

---

## 1. Discrete Status Values & Concepts

Orca separates status into three distinct concepts:
1. **Per-Pane / Session Explicit Agent Hook State (`AgentStatusIpcPayload.state`)**
2. **Per-Worktree Card Activity Status (`WorktreeActivityStatus`)**
3. **Manual / Durable Kanban Workspace Status (`workspaceStatus`)**

### 1.1 Per-Pane Explicit Agent Hook States (`AGENT_STATUS_STATES`)
**Source:** `/tmp/orca-asar/out/shared/agent-status-types.js` (`exports.AGENT_STATUS_STATES = ['working', 'blocked', 'waiting', 'done']`)

- `working`: The agent model is actively thinking, streaming assistant tokens, or executing a tool (e.g., executing bash commands, reading/writing files).
- `waiting`: The agent is blocked awaiting interactive human input (e.g., `AskUserQuestion`, `RequestUserInput`).
- `blocked`: The agent is blocked awaiting explicit security permission approval (e.g., tool execution permission request).
- `done`: The turn has completed, prompt goals have been satisfied, or the agent run has settled to idle.

*(Note: Subagent child rows normalize states to `'working' | 'blocked' | 'waiting' | 'idle'`, while the parent hook protocol uses `working`, `blocked`, `waiting`, `done`)*.

### 1.2 Per-Worktree Card Activity Status
**Source:** `/tmp/orca-asar/out/renderer/assets/worktree-status-DR0Zr8Ht.js` (`STATUS_LABELS`) & `resolveWorktreeStatus`

At the worktree level (and rendered on the sidebar card and terminal tabs), Orca evaluates an aggregated activity state:
- `permission`: At least one live tab/pane is in `'blocked'` or `'waiting'` (from explicit hook) or inferred `'permission'` (from title keywords/spinners). Display label: `"Needs permission"`.
- `working`: At least one live tab/pane is in `'working'` (from hook or title spinner). Display label: `"Working"`.
- `done`: An agent finished a turn (`'done'` from hook), but the result has not yet been cleared/retained. Display label: `"Done"`.
- `active`: The worktree has open, live PTY terminal tabs or browser tabs, but no active agent work or permission blocker is pending. Display label: `"Active"`.
- `inactive`: No live PTY sessions or browser tabs exist for this worktree. Display label: `"Inactive"`.

### 1.3 Manual / Durable Workspace Status (`workspaceStatus`)
**Source:** `/tmp/orca-asar/out/shared/workspace-statuses.js` & `workspace-status-defaults.js`

This represents durable kanban project tracking metadata set by the user or via CLI (`orca worktree set --workspace-status <id>`):
- `todo`: Default label `"Todo"` (neutral circle).
- `in-progress`: Default label `"In progress"` (blue/brand circle-dot). Default for new worktrees.
- `in-review`: Default label `"In review"` (violet/brand PR icon).
- `completed`: Default label `"Done"` (emerald/brand checkmark).

---

## 2. How Status is Produced (The Generation & Ingestion Mechanism)

Orca uses a **multi-tiered authority model** (`resolvePaneAgentActivity` in `agent-status-3vUKbY6l.js`):
1. **Tier 1 (Authoritative):** Explicit Agent Hook Server via Local HTTP Loopback & Custom Injected Plugins
2. **Tier 2 (Authoritative in PTY Stream):** OSC 9999 In-Band Escape Sequences
3. **Tier 3 (Fallback / Non-Hook Agents):** Terminal Title (OSC 0/2) Heuristic Classification & Spinner Detection
4. **Tier 4 (Safety Net):** Process Table Sampling & Terminal Idle Timers

```
+-------------------------------------------------------------------------------+
|                               Tier 1: Hook Server                             |
| Agent Plugin (Pi/Claude/OpenCode) --POST /hook/pi--> Orca Agent Hook Server   |
+---------------------------------------+---------------------------------------+
                                        |
+---------------------------------------v---------------------------------------+
|                         Tier 2: In-Band Stream Parser                         |
| PTY Raw Byte Stream ------------> OSC 9999 Parser (\x1b]9999;{JSON}\x07)      |
+---------------------------------------+---------------------------------------+
                                        |
+---------------------------------------v---------------------------------------+
|                        Tier 3: OSC Title Fallback                             |
| Terminal Title (OSC 0/2) -------> detectAgentStatusFromTitle (Spinners/Regex) |
+---------------------------------------+---------------------------------------+
                                        |
+---------------------------------------v---------------------------------------+
|                    Tier 4: Process Sampling & Exit Net                        |
| PTY Child Process Lifecycle -----> Foreground Process Scanner & Exit Watcher  |
+-------------------------------------------------------------------------------+
```

### 2.1 Tier 1: Explicit Agent Hooks via Loopback HTTP
**Source:** `/Users/indo/.omo/agent/extensions/orca-agent-status.ts`, `/Users/indo/Library/Application Support/orca/opencode-hooks/shared/plugins/orca-opencode-status.js`, `/tmp/orca-asar/out/shared/agent-hook-status-cache.js`

When Orca launches an agent (e.g. Claude Code, Codex, Pi, OpenCode, Gemini):
1. Orca writes an endpoint discovery file at `$ORCA_AGENT_HOOK_ENDPOINT` (e.g. `agent-hooks/endpoint.env`) containing `ORCA_AGENT_HOOK_PORT` and `ORCA_AGENT_HOOK_TOKEN`.
2. Orca injects per-pane environment variables into the spawned PTY: `ORCA_PANE_KEY`, `ORCA_TAB_ID`, `ORCA_WORKTREE_ID`, `ORCA_AGENT_LAUNCH_TOKEN`.
3. The agent runtime loads an Orca plugin/extension that hooks lifecycle events:
   - `before_agent_start` / `agent_start` $\rightarrow$ posts `{ hook_event_name: 'agent_start' }` $\rightarrow$ State: `working`.
   - `tool_execution_start` $\rightarrow$ posts `{ tool_name, tool_input }` $\rightarrow$ State: `working`.
   - `permission.asked` / `question.asked` $\rightarrow$ posts `{ hook_event_name: 'AskUserQuestion' | 'PermissionRequest' }` $\rightarrow$ State: `waiting` / `blocked`.
   - `permission.replied` / `question.replied` $\rightarrow$ clears attention state.
   - `agent_settled` / `agent_end` $\rightarrow$ posts `{ hook_event_name: 'agent_end' }` $\rightarrow$ State: `done`.
4. Freshness & Expiry: Explicit hook status has a staleness window of 30 minutes (`AGENT_STATUS_STALE_AFTER_MS = 30 * 60 * 1000`). If no updates arrive within 30 minutes, it decays.

### 2.2 Tier 2: OSC 9999 Escape Sequence Processor
**Source:** `/tmp/orca-asar/out/shared/agent-status-osc.js` (`createAgentStatusOscProcessor`)

Orca parses raw PTY byte streams for proprietary OSC 9999 escape sequences:
`\x1b]9999;{JSON_PAYLOAD}\x07` or `\x1b]9999;{JSON_PAYLOAD}\x1b\\`
When detected, it extracts the JSON payload, strips the sequence from the visible terminal buffer (`cleanData`), and passes the parsed payload to `parseAgentStatusPayload` to update pane state.

### 2.3 Tier 3: Terminal Title (OSC 0/2) Classification
**Source:** `/tmp/orca-asar/out/shared/agent-title-status.js` (`detectAgentStatusFromTitle`, `createAgentStatusTracker`)

For agents without hook plugins (or bare CLI runs), Orca inspects the terminal title set via standard terminal OSC 0 / OSC 2 sequences:
- **Spinners:** Braille spinner glyphs (`\u2800`–`\u28ff`), Gemini spinners (`⠋`, `⠙`), quarter-circle spinners (`◜`, `◠`, `◝`, `◞`) $\rightarrow$ classified as `'working'`.
- **Permission keywords:** Title contains `'action required'`, `'permission'`, `'waiting'`, or Gemini permission glyph $\rightarrow$ classified as `'permission'`.
- **Idle / Done keywords:** Title starts with `CLAUDE_IDLE` (`Claude`), `Pi`, `* `, or matches `STRONG_IDLE_KEYWORDS_RE` $\rightarrow$ classified as `'idle'`.
- **Title Tracking State Machine:** `createAgentStatusTracker` monitors transitions:
  - `working` $\rightarrow$ `idle` (fires `onBecameIdle`)
  - `non-working` $\rightarrow$ `working` (fires `onBecameWorking`)
  - `idle/permission` $\rightarrow$ shell prompt / `null` (fires `onAgentExited`)

### 2.4 Tier 4: Process Inspection & Turn Settling
**Source:** `/tmp/orca-asar/out/web/assets/remote-runtime-pty-recovery-state-CcyktY20.js` (`Ji`, `Hn`, `le`)

Orca samples foreground child processes on a dynamic cadence (750ms when active, 2000ms when idle, up to 15s when inactive):
- When a recognized agent process exits back to a plain shell (e.g. bash/zsh), Orca detects `process-exit` and synthesizes a terminal turn completion.
- When an agent turn finishes, Orca starts a settling quiet timer (1000ms–1500ms `Yi` / `Zi`) to coalesce duplicate/rapid notifications before dispatching task completion.

---

## 3. UI Surfacing ("Needs Attention" vs "Working") & Clearing Mechanism

### 3.1 UI Surfacing in Orca
**Source:** `/tmp/orca-asar/out/renderer/assets/StatusIndicator-CJ9TRLK4.js`, `WorktreeCard-DHnjoc37.js`, `terminal-tab-activity-status-BaKqQAEL.js`

1. **Working State (`working`):**
   - **Worktree Card:** Renders `AgentWorkingSpinner` (an animated spinning indicator).
   - **Terminal Tab:** Shows `AgentStateDot` in working animation mode (`data-agent-activity-status="working"`).
   - **Kanban / Dashboard:** Card moves to the **"Working"** bucket column.
2. **Needs Attention / Permission (`permission` / `waiting` / `blocked`):**
   - **Worktree Card:** Renders `MessageCircleQuestionMark` in bright amber/yellow (`text-amber-500`) with tooltip `"Needs permission"`.
   - **Terminal Tab:** Displays an amber attention indicator and badge.
   - **Kanban / Dashboard:** Card moves to the **"Needs You"** (`attention`) bucket column with an amber highlight border (`border-amber-500/40 bg-amber-500/[0.06]`).
3. **Turn Completed / Unread (`done` + `isUnread`):**
   - **Worktree Card:**
     - Displays an amber unread dot badge (`newCardUnreadAlertClassName`: `rounded-full bg-amber-500 ring-2 ring-sidebar`).
     - A solid emerald dot (`bg-emerald-500`) represents settled `done` / `active`.
   - **Terminal Tab:** Displays `FilledBellIcon` (`data-testid="tab-activity-bell"`) with amber text (`text-amber-500 drop-shadow-sm`) and subtle ambient background highlight (`bg-amber-500/10`).

### 3.2 Clearing Transitions
**Source:** `/tmp/orca-asar/out/renderer/assets/store-CgXrfmaH.js` (`setActiveWorktree`, `clearTerminalPaneUnread`, `clearWorktreeUnread`), `/tmp/orca-asar/out/renderer/assets/remote-runtime-pty-recovery-state-CcyktY20.js` (`Qt`)

1. **Clearing "Needs Attention" (`permission` / `waiting` / `blocked`):**
   - **Hook Event Resolution:** The agent emits a resolution hook (`permission.replied`, `question.replied`, `question.rejected`, or `SessionBusy`/`SessionIdle`). This removes the blocker from `pendingAttentionByKey` and transitions the pane back to `working` or `idle`.
   - **Terminal Input / Title Change:** User sends input or the terminal title drops the permission keyword, transitioning the title tracker out of `permission`.
2. **Clearing "Done / Unread Badge":**
   - **Worktree Activation / Window Focus:** When the user switches to the worktree (`setActiveWorktree(worktreeId)`), or focuses the window while on that worktree, `clearWorktreeUnread(worktreeId)` is called:
     - Sets `isUnread = false` in the worktree store.
     - Calls `clearTerminalPaneUnread(paneKey)`: deletes `unreadTerminalPanes[paneKey]` and `unreadAgentCompletionPanes[paneKey]`.
     - Removes the amber unread alert dot and bell icon from the UI.
   - **Explicit User Acknowledgment:** Clicking the card's unread toggle button or clicking the agent in the Dashboard popout calls `ackAgent(paneKey)` / `clearTerminalTabUnread(tabId)`.
3. **Clearing "Working":**
   - Hook emission of `agent_end` / `SessionIdle` $\rightarrow$ transitions to `done`.
   - Terminal title title change from spinner to static text/prompt $\rightarrow$ transitions to `idle`.
   - Staleness timeout: 30 minutes of silence decays `working` back to `active`.

---

## 4. Concrete Recommendation for Ferryx

### 4.1 Ferryx Observable Surface Matrix

Ferryx currently receives four concrete event channels from its backend:
- **(a) Native Terminal OSC Title Changes:** `on_title_change(pane_id, title)`
- **(b) Terminal BEL Events:** `on_bell(pane_id)`
- **(c) PTY Lifecycle:** `spawned(pane_id)` / `exited(pane_id, exit_code)`
- **(d) Raw PTY Output Bytes:** `on_data(pane_id, bytes)`

### 4.2 Recommended Ferryx State Machine

```
                              +------------------------+
                              |        INACTIVE        |
                              +-----------+------------+
                                          | PTY spawned
                                          v
      +----------------------------->+----+----+<------------------------------+
      |                              |  IDLE   |                               |
      |                              +----+----+                               |
      |                                   |                                     |
      |       OSC 9999 idle /             | OSC 9999 working /                  |
      |       Title spinner stopped /     | Title spinner started /             |
      |       BEL received /              | PTY output burst after title match  |
      |       Staleness timeout (30m)     v                                     |
      |                              +----+----+                                |
      |   +--------------------------+ WORKING +----------------------------+   |
      |   |                          +----+----+                            |   |
      |   |                               |                                 |   |
      |   | OSC 9999 done /               | OSC 9999 waiting/blocked /      |   |
      |   | Tracker working->idle /       | Title permission keyword        |   |
      |   | BEL during working            v                                 |   |
      |   |                          +----+----+                            |   |
      |   |                          | WAITING | (Needs Attention)          |   |
      |   |                          +----+----+                            |   |
      |   |                               |                                 |   |
      |   |                               | OSC 9999 replied /              |   |
      |   |                               | Title permission cleared /      |   |
      |   |                               | User terminal input             |   |
      |   |                               +----------------->---------------+   |
      |   v                                                                     |
+-----+---+----+                                                                |
|     DONE     | ---------------- User activates worktree / -------------------+
|  (Unread)    |                  focuses pane (Clears Unread)
+--------------+
```

#### State Definitions
1. **`Inactive`**: No live PTY process exists for this worktree/tab.
2. **`Idle` / `Active`**: PTY process is running; normal shell prompt or interactive agent waiting for user turn. No unread notifications.
3. **`Working`**: Agent is actively processing/generating tokens/executing tools. UI shows working spinner.
4. **`Waiting` (`Needs Attention`)**: Agent is blocked on user permission or interactive question. UI shows amber attention badge/question indicator.
5. **`Done` (`Unread Alert`)**: Agent turn finished while the worktree or tab was backgrounded/unfocused. UI shows amber unread bell/dot until acknowledged or activated.

#### State Transition Table

| Current State | Event | Next State | Ferryx Observable Mechanism |
|---|---|---|---|
| `Inactive` | PTY process launched | `Idle` | **(c)** PTY started event. |
| `Idle` | In-band OSC 9999 `state: "working"` | `Working` | **(d)** Raw PTY byte stream parsed for `\x1b]9999;{"state":"working"}\x07`. |
| `Idle` | Title OSC 0/2 matching braille/spinner glyphs | `Working` | **(a)** Native terminal OSC title update (`⠋`, `⠙`, `◜`, etc.). |
| `Working` | In-band OSC 9999 `state: "waiting"` or `"blocked"` | `Waiting` | **(d)** Raw PTY byte stream parsed for `\x1b]9999;...`. |
| `Working` | Title OSC 0/2 containing `"action required"`, `"permission"` | `Waiting` | **(a)** Native terminal OSC title update. |
| `Waiting` | User sends stdin / Title drops permission keyword | `Working` or `Idle` | **(a)** Native title change / User input event. |
| `Working` | In-band OSC 9999 `state: "done"` | `Done` (if unfocused) / `Idle` (if focused) | **(d)** Raw PTY byte stream parsed for `\x1b]9999;{"state":"done"}`. |
| `Working` | Title changes from spinner $\rightarrow$ static title | `Done` (if unfocused) / `Idle` (if focused) | **(a)** Native title tracker detects `working` $\rightarrow$ `idle`. |
| `Working` | Terminal BEL received while working | `Done` (if unfocused) / `Idle` (if focused) | **(b)** Terminal BEL event emitted by CLI turn completion. |
| `Working` | 30 minutes staleness timeout without events | `Idle` | Internal timer decay. |
| `Done` | Worktree selected or pane focused | `Idle` (Unread cleared) | Worktree / tab selection handler clears unread flag. |
| Any | PTY process exits | `Inactive` (or `Idle` if shell remains) | **(c)** PTY exited lifecycle event. |

### 4.3 Key Architectural Lessons to Adopt
1. **Never rely on title strings alone:** Titles are lossy, high-churn, and truncated on Windows/tmux. Always implement Tier 2 (OSC 9999 stream parsing) and Tier 1 (environment-injected hook endpoints) alongside Tier 3 (title regex).
2. **Coalesce & Quiet Turn Transitions:** To avoid false flashes, wait 1000ms after a title working $\rightarrow$ idle transition before committing `Done` if output is still streaming.
3. **Decouple Activity State from Kanban Status:** Keep `WorktreeActivityStatus` (`working`, `permission`, `done`, `active`) strictly dynamic in memory, separate from durable user-managed `workspaceStatus` (`todo`, `in-progress`, `in-review`, `completed`).
4. **Clear Unread on Activation:** Entering a worktree or focusing its pane immediately clears its unread bit, mirroring Orca's `clearWorktreeUnread`.
