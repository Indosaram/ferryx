# Ferryx Mobile Active-Terminal Mirror Implementation Audit

Date: 2026-08-23
Audit Status: VERIFIED GREEN (All native, frontend, and browser gates clean)

## Product Contract

The Ferryx mobile mirror architecture connects a mobile web client to a running Ferryx desktop instance. It mirrors only the currently focused desktop terminal rather than exposing an arbitrary session switcher or background processes.

Key contract invariants:

1. **Singular Active Terminal Mirror**: The mobile interface renders only the server-declared active desktop terminal. If no terminal is active on desktop, or if the active tab is a browser tab, the mobile view presents an empty state explaining that desktop terminal focus is required.
2. **Background Session Isolation**: Extant background PTY sessions are strictly excluded from `GET /api/v1/sessions` and `GET /api/v1/workspace/state`. WebSocket attach requests to `/api/v1/terminal/:sessionId` require that `sessionId` matches the active selection. Requests for any other session receive `403 Forbidden`.
3. **Safe Mobile Context Selection**: Mobile clients can request context switches via `POST /api/v1/workspace/select` using only safe identifiers (`workspaceId` and optional `worktreeSlug`). The route verifies `Control` permission on the device token (rejecting View-only tokens with `403 Forbidden`), validates target existence in `WorkspaceRegistry`, and fires the Tauri desktop bridge event `remote_selection_requested`.
4. **Desktop Focus Derivation and Publishing**: The desktop application derives the active session across tab groups, active tabs, split view leaves, and terminal sessions. It publishes safe metadata (`workspaceId`, `worktreeSlug`, `worktreeLabel`, `backendSessionId`) to the native remote gateway over IPC whenever focus changes.
5. **No Filesystem Path Leakage**: Absolute paths, repo roots, and working directories are never stored in gateway selection state, never sent across remote APIs, and never rendered on mobile.
6. **Consistent Ferryx Branding**: Public branding displays "Ferryx" across the UI, headers, and fallback assets, leaving no obsolete public references.

## Changed Files and Implementation Evidence

### 1. Native Active-Terminal Contract and Selection Bridge

**Changed Files:**
- `src-tauri/src/remote/protocol.rs`
- `src-tauri/src/remote/state.rs`
- `src-tauri/src/remote/server.rs`
- `src-tauri/src/ipc/remote.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/remote/tests.rs`

**Implementation Details:**
- Added `RemoteActiveDesktopSelection` and `RemoteSelectionRequest` in `protocol.rs`.
- Added atomic active selection storage and Tauri event sink hook on `RemoteGatewayState`.
- Implemented `get_active_running_sessions` in `server.rs` so session endpoints return at most one declared active session.
- Added active session validation in `ws_terminal_handler`, returning `403 Forbidden` for background session IDs.
- Added `POST /api/v1/workspace/select` endpoint enforcing `DevicePermission::Control`, validating workspace and worktree identity, and emitting `remote_selection_requested`.
- Added IPC commands `cmd_remote_set_active_selection` and `cmd_remote_get_active_selection`.

**RED Test Evidence:**
- Command: `cargo test --manifest-path src-tauri/Cargo.toml test_active_desktop_terminal_contract`
- Captured failure: `panicked at src/remote/tests.rs:382:5: Must not expose background sessions when no active desktop selection is declared, got: [RemoteTerminalSession { session_id: "95988caa-75f1-4fcc-94fc-2a02ac1b4aed", ... }, RemoteTerminalSession { session_id: "ef850cec-d601-4d93-81dd-ced7c5ac4565", ... }]`

**GREEN Test Evidence:**
- Command: `cargo test --manifest-path src-tauri/Cargo.toml remote`
- Result: 13 passed, 0 failed.

### 2. Desktop Focus Publisher and Remote Selection Listener

**Changed Files:**
- `ui/src/App.tsx`
- `ui/src/lib/tauri.ts`
- `ui/src/App.test.tsx`
- `ui/src/lib/tauri.test.ts`

**Implementation Details:**
- Implemented `deriveFocusedTerminal` in `App.tsx` following the complete derivation chain: focused group -> active tab -> terminal check -> active leaf -> leaf local session -> backend session ID.
- Returns `null` when a browser tab or non-terminal tab is active.
- Added React layout effect to publish focused metadata via `publishFocusedTerminal` with argument key `request` matching native command expectations.
- Added listener for `remote_selection_requested` using `onRemoteSelectionRequested`, dispatching existing `handleSelectProject` and `handleSelectWorktree` actions.

**RED Test Evidence:**
- Command: `cd ui && npx vitest run src/App.test.tsx src/lib/tauri.test.ts --maxWorkers=1`
- Captured failure: 6 failed tests across 2 files (`deriveFocusedTerminal is not a function`, `publishFocusedTerminal is not a function`, `onRemoteSelectionRequested is not a function`).

**GREEN Test Evidence:**
- Command: `cd ui && npx vitest run src/App.test.tsx src/lib/tauri.test.ts --maxWorkers=1`
- Result: 2 test files passed, 55 tests passed.

### 3. Mobile Singular Terminal Mirror and Safe Context Selector

**Changed Files:**
- `ui/src/remote/RemoteApp.tsx`
- `ui/src/remote/RemoteSessionList.tsx`
- `ui/src/remote/RemoteTerminal.tsx`
- `ui/src/remote/RemoteUI.test.tsx`

**Implementation Details:**
- Replaced multi-session list with singular active terminal mirror view.
- Added desktop context breadcrumb display (`workspaceId / worktreeLabel`).
- Added touch-friendly context selector dialog making `POST /api/v1/workspace/select` requests with safe identifiers only.
- Added pending spinner, confirmed state feedback, and explicit "No focused terminal" empty state.
- Added client-side path rejection in `safeContextText` to prevent accidental rendering of absolute filesystem strings.

**RED Test Evidence:**
- Command: `cd ui && npx vitest run src/remote/RemoteUI.test.tsx --maxWorkers=1`
- Captured failure: 2 failed tests (rendered background sessions, missed singular mirror view).

**GREEN Test Evidence:**
- Command: `cd ui && npx vitest run src/remote/RemoteUI.test.tsx --maxWorkers=1`
- Result: 1 test file passed, 6 tests passed.

## Verified Gate Commands and Results

All verification suites ran clean across native and frontend layers.

| Command | Exit Code | Result | Details |
|---|---|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml remote` | 0 | PASS | 13 native tests passed. |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | 0 | PASS | Code formatting clean. |
| `cargo check --manifest-path src-tauri/Cargo.toml` | 0 | PASS | Native compilation clean. |
| `cd ui && npm test` | 0 | PASS | 76 test files, 527 tests passed. |
| `cd ui && npm run build` | 0 | PASS | Frontend production build succeeded with Vite output. |
| `cd ui && npx tsc --noEmit` | 0 | PASS | TypeScript check clean with zero diagnostics. |

## Real-Browser QA Evidence

Browser testing was conducted using `agent-browser 0.16.3` with Google Chrome at mobile viewport `390 x 844` against a controlled local fixture server (`127.0.0.1:51942`, PID 2977). The desktop application was not automated or controlled during this test.

### Browser Test Matrix

| Scenario | Result | Observable Evidence |
|---|---|---|
| Focused Terminal Mirror | PASS | Mobile screen rendered title `ACTIVE_TERMINAL_TITLE` and context `safe-ui / main`. Background title `BACKGROUND_TERMINAL_TITLE` and path sentinels were not rendered. |
| Active-Only Routing | PASS | Fixture logged single `WS_UPGRADE session-active`. Zero requests occurred for `session-background`. |
| Safe Context Selector | PASS | Selector menu displayed only safe identifiers `safe-ui / main` and `safe-api / feature-safe`. No local paths were exposed. |
| Selection Pending State | PASS | Target button showed loading spinner. Screen reader accessibility announced `Switching to safe-api / feature-safe...` and interactions were disabled. |
| Selection Confirmed State | PASS | View updated with `Desktop context confirmed` banner and updated context badge. |
| No Active Session Fixture | PASS | Screen displayed `No focused terminal` card with clear explanatory copy. No terminal input mounted. |

### Setup and Cleanup Execution Log

```text
cd ui && ./node_modules/.bin/vite build --outDir /tmp/ferryx-remote-qa-st_01a02c9b/dist --emptyOutDir
node /tmp/ferryx-remote-qa-st_01a02c9b/fixture-server.mjs (PID 2977, Port 51942)

agent-browser --session ferryx-remote-qa-st_01a02c9b open http://127.0.0.1:51942
agent-browser --session ferryx-remote-qa-st_01a02c9b set viewport 390 844
agent-browser --session ferryx-remote-qa-st_01a02c9b storage local set ferryx_remote_token fixture-token
agent-browser --session ferryx-remote-qa-st_01a02c9b reload
agent-browser --session ferryx-remote-qa-st_01a02c9b screenshot /tmp/ferryx-remote-qa-st_01a02c9b/01-active-terminal.png

agent-browser --session ferryx-remote-qa-st_01a02c9b click @e3
agent-browser --session ferryx-remote-qa-st_01a02c9b screenshot /tmp/ferryx-remote-qa-st_01a02c9b/02-selector-safe-contexts.png

agent-browser --session ferryx-remote-qa-st_01a02c9b click @e11
agent-browser --session ferryx-remote-qa-st_01a02c9b screenshot /tmp/ferryx-remote-qa-st_01a02c9b/03-selection-pending.png

agent-browser --session ferryx-remote-qa-st_01a02c9b screenshot /tmp/ferryx-remote-qa-st_01a02c9b/04-selection-confirmed-no-focus.png

agent-browser --session ferryx-remote-qa-st_01a02c9b eval "fetch('/__qa/mode/no-focus',{method:'POST'}).then(r=>r.json())"
agent-browser --session ferryx-remote-qa-st_01a02c9b reload
agent-browser --session ferryx-remote-qa-st_01a02c9b screenshot /tmp/ferryx-remote-qa-st_01a02c9b/05-no-active-fixture.png

agent-browser --session ferryx-remote-qa-st_01a02c9b close
kill -TERM 2977
```

Browser session and temporary server were confirmed terminated after test completion.

## Privacy and Authorization Guarantees

1. **Safe Slugs and Identifiers**:
   - Native models and UI bridges transmit only `workspaceId`, `worktreeSlug`, `worktreeLabel`, and `backendSessionId`.
   - Local filesystem paths, user home paths, and git repository root paths are never exposed over network payloads.
2. **Permission Gate**:
   - Pairing authentication requires valid device tokens.
   - Context selection requires `Control` permission. Devices holding `View` permission receive `403 Forbidden` on selection attempts.
3. **Session Hiding**:
   - Inactive background terminals cannot be discovered via HTTP session listings.
   - Direct connection guessing to inactive WebSocket endpoints fails immediately with `403 Forbidden`.
4. **Client-Side Defense**:
   - The mobile UI inspects incoming context strings and suppresses rendering if a value starts with `/`, `~`, or `file:`.

## User-Run Desktop Manual QA Steps

Desktop automation was deliberately avoided. Execute these exact manual steps in your local desktop environment to validate end-to-end user flows:

### Step 1: Enable Remote Access
1. Launch Ferryx Desktop.
2. Open Settings -> Remote Access.
3. Toggle Remote Access to ON.
4. Copy the pairing PIN or access URL.

### Step 2: Pair Mobile Client
1. Open the Remote access URL on your mobile phone or a separate browser window.
2. Enter the PIN from the desktop Settings dialog to complete pairing with Control permission.
3. Confirm the mobile view shows the Ferryx header and context breadcrumb.

### Step 3: Verify Active Mirroring
1. On Ferryx Desktop, open Project A -> main branch. Run a recognizable command such as `top` or `tail -f`.
2. Look at the mobile screen. Confirm it mirrors the active terminal output from Project A.
3. On desktop, split the pane and switch focus to the second terminal running a different command.
4. Confirm mobile switches to mirror the newly focused pane.

### Step 4: Verify Browser Tab Focus Clearing
1. On desktop, open a new Browser tab or switch focus to an existing Browser tab.
2. Check the mobile interface. Confirm the terminal view unmounts and displays the "No focused terminal" empty state.

### Step 5: Verify Mobile Context Switching
1. On mobile, tap the context selector button at the top.
2. Choose Project B -> feature branch from the list.
3. Confirm the button displays the pending spinner.
4. Check Ferryx Desktop. Verify the desktop window automatically switches its active workspace and tab to Project B.
5. Check mobile. Confirm the banner displays "Desktop context confirmed" and mirrors Project B's active terminal.

### Step 6: Verify Restart Isolation
1. Quit Ferryx Desktop.
2. Confirm mobile disconnects and attempts reconnection.
3. Relaunch Ferryx Desktop.
4. Confirm Remote Access starts in the OFF state until explicitly re-enabled in Settings.
