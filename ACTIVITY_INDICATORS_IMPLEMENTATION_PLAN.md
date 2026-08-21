# rorca Activity & Status Indicator Implementation Plan (Parity with Orca)

## 1. Executive Summary & Objective

The goal of this implementation plan is to bring **complete parity** between original Orca and rorca (`orca-lite`) regarding real-time status indicators, running state spinners, unread attention indicators, and notification badges across **Tabs**, **Worktrees**, **Projects**, and the **Sidebar**.

### Key Finding from Source Code Audit:
1. **Tabs (`TabBar.tsx`)**:
   - *Original Orca*: Renders `AgentStateDot` / activity indicator (`tab-agent-activity-indicator`) directly inside the tab item based on the terminal's live agent status (`working` -> spinner, `permission`/`waiting` -> attention amber dot, `done` -> green dot, `unread` -> blue unread dot).
   - *Current rorca*: Only renders a static `TerminalSquare` / `Globe` icon. Does not reflect agent state or live terminal title spinners on the tab strip.
2. **Worktrees (`WorktreeList.tsx` / `WorktreeCard`)**:
   - *Original Orca*: Computes `selectWorktreeAgentActivitySummary` aggregated across all panes/tabs in the worktree (`hasLiveWorking`, `hasPermission`, `hasLiveDone`, `hasRetainedDone`). Renders a live `StatusDot` (working spinner / amber permission / green done / blue unread dot).
   - *Current rorca*: Only looks at `agents.find(...)` which is based on static lifecycle state and doesn't compute aggregated live activity or unread status properly.
3. **Projects / Sidebar (`Sidebar.tsx`)**:
   - *Original Orca*: Aggregates active/working/permission/unread counts across all worktrees in each registered project and displays status badges (e.g. running count or attention dot) on the project header.
   - *Current rorca*: Shows a plain folder icon without any running agent count or attention indicators.
4. **Terminal / Title-based Activity Detection (`agentTitle.ts` / `terminalEvents.ts`)**:
   - *Original Orca*: Continuously monitors terminal OSC titles (`detectAgentStatusFromTitle`, `containsAgentSpinnerGlyph`, Braille spinners `\u2800-\u28FF`, `✦`, `✳`, `✋`) to update pane/tab activity state in real time.
   - *Current rorca*: `agentTitle.ts` has basic parsing, but it is not hooked into live title update events or workspace status aggregation.

---

## 2. Original Orca Architecture Breakdown

### 2.1 State Hierarchy & Resolution
Original Orca defines four distinct activity states:
- **`working`**: Agent is currently executing a tool, thinking, or running a command. (Visual: Animated spinner / pulsing blue/cyan dot).
- **`permission` / `waiting`**: Agent is waiting for user approval, input, or confirmation. (Visual: Amber/yellow dot or `MessageCircleQuestion` icon).
- **`done`**: Agent finished its task. (Visual: Green check/dot).
- **`unread`**: Background activity completed or notification arrived while tab/worktree was unfocused. (Visual: Blue unread dot / badge).

### 2.2 Title & Hook-based Detection (`agent-status`)
1. **Hook Status (Authoritative)**: If agent hook emits state (`working`, `waiting`, `done`), it takes precedence.
2. **OSC Title Heuristic (Fallback)**:
   - Braille spinner (`[\u2800-\u28FF]`), `✦`, `⏲` -> `working`
   - `✋`, `action required`, `permission`, `waiting`, `needs input`, `approval` -> `permission` (needs input)
   - `◇`, `*`, `done`, `completed`, `idle` -> `done` / `idle`

### 2.3 Tab & Worktree Activity Aggregation (`worktree-status-DR0Zr8Ht.js`)
- `resolveWorktreeStatus(args)`:
  - If any pane has `hasPermission` -> `"permission"` (Amber)
  - Else if any pane has `hasLiveWorking` -> `"working"` (Blue spinner)
  - Else if any pane has `hasLiveDone` / unread -> `"done"` / `"unread"` (Green/Blue dot)
  - Else -> `"active"` / `"inactive"`

---

## 3. Step-by-Step Implementation Plan

### Phase 1: Real-Time Tab & Pane Activity State (`ui/src/state/activityStore.ts` & `layout.ts`)
1. **Activity State Slice**:
   - Maintain `activityByTabId: Record<string, { state: AgentState; agentType?: string; isLive: boolean }>` in state.
   - Update `activityByTabId` whenever:
     - `onTitleChange` is received from xterm (`TerminalPane.tsx`).
     - Terminal lifecycle event is received from backend.
2. **Title Activity Classifier**:
   - Enhance `ui/src/lib/agentTitle.ts` with `classifyTerminalTitleActivity(title: string)` matching Orca's `detectAgentStatusFromTitle`.

### Phase 2: Tab Bar Indicator Integration (`ui/src/components/TabBar.tsx`)
1. **Tab Leading Icon & Status**:
   - If tab is running an agent (`activity.state === "working"`): render an animated spinner / `StatusDot` next to the tab label.
   - If tab is waiting for user input (`activity.state === "waiting"`): render amber `StatusDot` / attention indicator.
   - If tab has unread activity (`unreadTabIds[tab.id]`): render unread blue dot.
2. **Close / Action Button alignment**:
   - Keep tab close button hoverable without covering status dots.

### Phase 3: Worktree List Aggregation (`ui/src/components/WorktreeList.tsx`)
1. **Worktree Status Resolver**:
   - For each worktree, aggregate status across all its tabs:
     - Working count & spinner
     - Needs-attention count & amber dot
     - Unread indicator dot
2. **Worktree Row Visuals**:
   - Display `StatusDot` with appropriate pulse/animation in the worktree list item.
   - Display branch / dirty status cleanly beside the agent status.

### Phase 4: Project & Sidebar Summary Badges (`ui/src/components/Sidebar.tsx`)
1. **Project Aggregation**:
   - For each registered project in the sidebar:
     - Count active running agents across all child worktrees.
     - Show running count badge (e.g. `2 running`) or spinning dot.
     - Show attention/unread dot if any child worktree requires user action.

### Phase 5: Automated Testing & Verification
1. **Unit Tests**:
   - `agentTitle.test.ts`: test spinner detection, waiting keywords, done transitions.
   - `TabBar.test.tsx`: test tab rendering with working spinner, waiting dot, and unread dot.
   - `WorktreeList.test.tsx`: test worktree aggregation across multiple panes.
   - `Sidebar.test.tsx`: test project-level running count badge.
2. **E2E & Build**:
   - `bun run test` all pass.
   - `bun run build` clean build.

---

## 4. Verification Checklist

- [ ] Tab shows animated working spinner when agent is running (`working`).
- [ ] Tab shows amber dot when agent needs permission/input (`waiting`).
- [ ] Tab shows unread blue dot when background agent finishes.
- [ ] Worktree list item displays aggregated status of its child tabs.
- [ ] Sidebar project header displays running agent count badge and attention markers.
- [ ] Unread state clears immediately upon switching to that tab or worktree.
