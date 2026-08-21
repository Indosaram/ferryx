# rorca Remote — Desktop 1:1 Parity & Non-Slop Experience Implementation Plan

## 1. Executive Summary & Problem Definition

### 1.1 Current State ("Slop" Assessment)
The current Remote Terminal implementation in `orca-lite` (`src-tauri/src/remote/server.rs` & `ui/src/remote/`) is an isolated, minimal single-session web wrapper.
- **Visual Disconnect**: It uses a separate, simplistic HTML/CSS template that does not inherit Orca's typography, Geist font, low-chrome aesthetic, Tailwind tokens, or dark palette (`#09090b` / `#0a0a0a`).
- **Zero Workspace Awareness**: It only exposes raw PTY session IDs with no project trees, no worktrees, no dirty indicators, no branch labels, and no unread badges.
- **No Layout System**: Single terminal view with no multi-tab bar, no split panes (horizontal/vertical), no pane resizing, and no mirror panes.
- **Degraded Terminal Fidelity**: Missing WebGL rendering, Ghostty font fallbacks, custom selection colors, IME buffer synchronization, and live search.
- **No Agent Presence**: Completely blind to OMO, Claude Code, and Codex agent states, tasks, and status spinners.
- **Mobile Interaction Deficit**: Static button bar that lacks touch ergonomics, gesture navigation (swipe to switch tabs), visual viewport keyboard handling, and full special-key docks.

### 1.2 Target Vision
The Remote Web Client must be the **exact same React application (`App.tsx` + `useWorkspaceStore`)** running in a browser environment via `WebSocketTerminalTransport` and a remote state sync layer. Whether opened on a desktop browser, iPad/tablet, or mobile Safari/Chrome, the user experiences:
1. **Identical Visual Design**: Pixel-perfect match to Orca desktop (32px tab strip, 236px collapsible sidebar, Geist font, status dots, context menus).
2. **Full Workspace & Git Control**: Projects, worktrees, branches, and dirty status live-synced from the native backend.
3. **Full Multi-Pane & Tab Engine**: Horizontal/vertical splits, tab pinning, rename, close-others, pane ratio resizing.
4. **Agent Monitoring**: Real-time agent status cards (working/waiting spinners, task title formatting).
5. **Touch-First Mobile System**: Ergonomic bottom navigation, smooth swipe gestures, soft keyboard avoidance (`window.visualViewport`), and customizable swipeable macro keydock.
6. **Production PWA**: Home screen standalone app, offline reconnect banner, vibration feedback on bell, and service worker caching.

---

## 2. Target Architecture: Dual-Transport Unified Shell

```text
┌────────────────────────────────────────────────────────────────────────┐
│                        Unified Orca Frontend                           │
│  (App.tsx / Sidebar / TabBar / TerminalSplitView / AgentCards / Etc.)   │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
                   ┌───────────────┴───────────────┐
                   ▼                               ▼
       ┌───────────────────────┐       ┌───────────────────────┐
       │ Tauri Native Adapter  │       │ Remote Web Adapter    │
       │ (IPC invokes & listen)│       │ (REST + WS Multiplex) │
       └───────────┬───────────┘       └───────────┬───────────┘
                   │                               │
                   │                               ▼
                   │                   ┌───────────────────────┐
                   │                   │ Remote Gateway Daemon │
                   │                   │ (Axum HTTP & WS Hub)  │
                   │                   └───────────┬───────────┘
                   ▼                               ▼
┌────────────────────────────────────────────────────────────────────────┐
│                         Core Native Backend                            │
│  (WorkspaceRegistry / PtyManager / OutputHub / SessionPersistence)     │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Phased Implementation Breakdown

### Phase 1: Remote Gateway State Sync API (Backend)
Expand the Axum Remote Gateway to serve full workspace state beyond raw session IDs:
- **`GET /api/v1/workspace/state`**:
  - Registered projects and active repository paths.
  - Full worktree list with branch names, dirty state, HEAD commits, and lock status.
  - Active terminal sessions mapped to workspace identity, title, and agent status.
- **`POST /api/v1/workspace/action`**:
  - Create worktree, delete worktree (safe & preview), register project, switch active worktree.
- **`WS /api/v1/workspace/events`**:
  - Live broadcast of `worktree_changed`, `terminal_lifecycle`, `agent_status_changed`, `terminal_title_changed`.

### Phase 2: Unified Frontend Shell & Remote Transport Router
- Refactor `ui/src/main.tsx` and `ui/src/App.tsx`:
  - Detect runtime environment: `isTauri()` vs `isRemoteBrowser()`.
  - When in Remote Browser mode:
    - If unauthenticated, present high-polish `PairingScreen` (with instant camera QR scanner & 6-digit PIN).
    - If authenticated, initialize `RemoteWorkspaceAdapter` which implements the same interface as `useWorkspaceRuntime` using HTTP + SSE/WebSocket events.
- Reuse all core components verbatim:
  - `Sidebar.tsx`: Render project tree, worktree nested rows, dirty indicators.
  - `TabBar.tsx`: 32px tab strip, tab pins, context menu actions, split buttons.
  - `TerminalSplitView.tsx`: Full `paneTree` layout reducer, split resize drag handles, focus tracking.
  - `AgentCards.tsx`: Live agent parsing (`agentTitle.ts`), working spinners, waiting tags.

### Phase 3: Terminal Rendering Engine Parity
- Extract shared terminal renderer pipeline:
  - WebGL rendering addon with automatic 2D Canvas fallback on mobile/low-power GPUs.
  - Load Ghostty font settings and system monospace fallbacks (`Geist Mono`, `JetBrains Mono`, `MesloLGS NF`, `Noto Sans KR`).
  - Terminal theme colors matching native desktop `#0a0a0a` background and truecolor ANSI pallet.
  - CJK IME composition handling and seamless cursor blinking.
  - Terminal search addon (`@xterm/addon-search`) accessible via Mobile search bar and `Mod+F`.

### Phase 4: Touch-First Mobile & Tablet Ergonomics
- **Responsive Shell Adaptations**:
  - **Desktop Browser (width >= 768px)**: Standard 236px collapsible/resizable sidebar, top tab bar, multi-pane splits.
  - **Mobile Phone (width < 768px)**:
    - Slide-over project/worktree drawer with backdrop blur.
    - Compact tab switcher bar with horizontal swipe gesture between tabs.
    - Auto-single-pane focus (swiping between split panes instead of cramping).
- **Soft Keyboard & Viewport Adaptation**:
  - Use `window.visualViewport` resize events to dynamically anchor the terminal and touch dock above the virtual keyboard without layout jitter or broken scroll.
- **Customizable Mobile Keydock**:
  - Floating/docked macro bar with haptic feedback:
    - Primary row: `[Ctrl]` `[Esc]` `[Tab]` `[↑]` `[↓]` `[←]` `[→]` `[Ctrl-C]`
    - Swipeable utility row: `[ | ]` `[ ~ ]` `[ ` ]` `[ / ]` `[ - ]` `[ _ ]` `[ = ]` `[PageUp]` `[PageDown]` `[Home]` `[End]` `[Ctrl-D]` `[Ctrl-Z]`
    - Sticky modifier state: Tapping `Ctrl` or `Alt` latches it for the next keypress.

### Phase 5: PWA, Biometrics & Seamless Persistence
- Web App Manifest (`manifest.webmanifest`) & Apple Touch Icons for standalone home-screen installation.
- Service worker for instant offline caching and asset loading.
- Local credential storage (`WebAuthn` / FaceID / TouchID biometric unlock option for paired sessions).
- Reconnect resilience:
  - Heartbeat ping/pong every 15s.
  - Exponential backoff reconnect with persistent history buffer replay upon connection drop.
- Notification bridge: Web Notification API & Vibration API trigger on terminal bell (`\x07`) or OMO agent task completion.

---

## 4. File-by-File Change Plan

| Target File | Scope of Changes |
|---|---|
| `src-tauri/src/remote/server.rs` | Add workspace state API, worktree actions API, event WebSocket channel, and serve production Vite bundle instead of inline HTML. |
| `src-tauri/src/remote/protocol.rs` | Define typed DTOs for `RemoteWorkspaceState`, `RemoteWorktreeAction`, and `RemoteEventPayload`. |
| `ui/src/lib/remoteAdapter.ts` | Create comprehensive remote bridge matching `ui/src/lib/tauri.ts` methods over REST/WS. |
| `ui/src/state/workspaceRuntime.ts` | Abstract platform hooks so state sync seamlessly uses Tauri events or Remote WS events. |
| `ui/src/components/Sidebar.tsx` | Add responsive mobile drawer mode with slide-in animation and gesture dismiss. |
| `ui/src/components/TabBar.tsx` | Add mobile touch scroll and tab overflow carousel. |
| `ui/src/components/TerminalPane.tsx` | Add visualViewport keyboard listener, pinch-to-zoom font scaling, and touch-drag selection. |
| `ui/src/components/MobileKeyDock.tsx` | Dedicated touch macro bar with latched modifiers, custom shortcuts, and haptic feedback. |
| `ui/public/manifest.webmanifest` | PWA manifest defining standalone display, theme color `#09090b`, and rorca icons. |
| `ui/src/sw.ts` | Service worker for offline asset caching and push notifications. |

---

## 5. Verification & Acceptance Criteria

1. **Visual Parity**: Side-by-side screenshot audit between Desktop Tauri app and Chrome/Safari remote browser shows 100% theme, font, and spacing parity.
2. **Feature Parity**: Multi-project switching, worktree creation/deletion, multi-tab layout, and split-pane resizing function identically on remote.
3. **Agent Integration**: Agent cards update live with spinning spinners and formatted task titles when OMO/Claude is active in a remote terminal.
4. **Mobile Usability**: On iPhone Safari and Android Chrome, opening the soft keyboard refits the terminal cleanly without hiding the prompt or macro keydock.
5. **Zero Test Regressions**: All unit tests (`cargo test`, `vitest`), linter (`cargo clippy -D warnings`), and production builds (`tsc && vite build`) remain 100% green.
