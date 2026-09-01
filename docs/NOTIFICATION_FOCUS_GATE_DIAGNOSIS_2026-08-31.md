# Desktop notifications & dock badge never fire — diagnosis and fix (2026-08-31)

## Symptom

Agent-completion notifications never appear and the dock badge never appears, while the
in-app notification settings show everything enabled.

## Environment observed

- Running app: `/Applications/Ferryx.app` (release lineage `origin/main`, v2026.08.30.x).
  GUI PID 88298 (started 21:14 KST), daemon PID 46901 (started 20:59 KST), 2026-08-31.
- macOS (Darwin 25.6.0, arm64).

## Evidence chain (runtime evidence, not code reading)

1. **Settings are persisted and ON.** WKWebView localStorage (origin `_E9R9…`, release app)
   `ferryx.settings.notifications:v1` =
   `{"enabled":true,"agentTaskComplete":true,"terminalBell":true,"customSoundId":"system",…}`.
2. **OS permission + delivery path is ALIVE.** Unified log (`log show`, process `usernoted`)
   shows `com.ferryx.app` connecting at 21:32:53 and THREE notifications submitted
   21:33:00–21:33:01 (settings "Send Test Notification" presses), each
   `Delivering … Presenting as banner`. A record from Aug 28 (`req ferryx-83076`) expired
   today at 19:59:50 — further proof deliveries worked in the past.
3. **Screen-rule agent detection is ALIVE.** `~/Library/Application Support/com.ferryx.app/
   session_state.json` (saved 21:45) contains `activityBySessionId` entries with
   `source=screen` in live `working`/`done` states (omo, codex) across 6 workspaces.
4. **Backend IPC is healthy.** `cmd_notification_set_badge_count` hops through
   `app.run_on_main_thread` correctly; preflight logic is covered by unit tests.
5. **Zero automatic dispatches** appear in `usernoted` besides the manual test presses →
   the gate inside `NotificationCoordinator` swallows every real completion edge.

## Root cause

`NotificationCoordinator`'s focus gate defaults to
`isWindowForegroundFocused()` = `document.visibilityState === 'visible' && document.hasFocus()`.
In Tauri macOS (WKWebView) the DOM focus state does not reliably track the native
key-window status when the app window is backgrounded. Result: while the user is in another
app, `!isFocused()` evaluates false, and BOTH branches are silently skipped:

- `onMarkTabUnread` → `unreadTabIds` → `unreadBadgeCount` → dock badge never appears;
- `dispatchNotification` → no banner.

Every stage above the gate was independently proven alive at runtime; this is the only
remaining silent kill-switch that explains both symptoms simultaneously.

### Aggravating defect (same pass)

`submit_notification` (`src-tauri/src/notification/permission.rs`) used a per-process
constant UNUserNotificationCenter identifier `ferryx-{pid}`. Adding a request with an
existing identifier REPLACES the previous notification — the three rapid test presses at
21:33 collapsed into one visible banner; three rapid agent completions would too.

### Diagnosability defect (same pass)

No `tracing` subscriber is initialized anywhere in `src-tauri`, so every
`tracing::warn!` (notification submission failures, badge failures) is dropped silently.
This is why the failure produced zero backend diagnostics.

## Fix (this change set)

1. `ui/src/lib/nativeWindowFocus.ts` (new) — native window focus singleton:
   `getCurrentWindow().isFocused()` once + `onFocusChanged` subscription; `null` when
   unknown (non-Tauri runtime).
2. `ui/src/App.tsx` — coordinator now receives
   `isWindowFocused: () => getNativeWindowFocused() ?? isWindowForegroundFocused()`, making
   the gate native-authoritative with DOM focus only as pre-init fallback.
3. `src-tauri/src/notification/permission.rs` — unique per-submission identifiers
   `ferryx-{pid}-{n}` (AtomicU64 counter) so notifications no longer replace each other.
4. `src-tauri/src/lib.rs` — initialize `tracing_subscriber` (stderr; `RUST_LOG` EnvFilter
   when the feature is available) so backend notification failures become visible.

## Verification

- `npx vitest run src/App.notifications.test.tsx src/lib/notificationCoordinator.test.ts` —
  includes the new regression test (DOM focus reports focused, native reports unfocused →
  completion edge MUST still dispatch + mark unread).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib notification::`
- `cargo check --manifest-path src-tauri/Cargo.toml`

## Manual E2E (user)

1. Rebuild and relaunch (`bun tauri dev`).
2. Start a short agent run in a terminal tab, then switch to another app BEFORE it finishes.
3. Expect: a macOS banner for the completion AND a dock badge on the Ferryx icon.
4. Focus the Ferryx window: the badge clears when the unread tab is activated (by design).
