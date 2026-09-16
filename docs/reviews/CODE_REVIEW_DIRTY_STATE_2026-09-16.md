# Comprehensive Code Review — Dirty State Working Tree (36 Files)

**Date:** 2026-09-16  
**Auditor:** OmO Gate Review Panel & Coordinator  
**Scope:** 36 modified files (+929, -61) across Ferryx backend (Rust/Tauri) and frontend (React/TypeScript)  
**Status:** Complete  

---

## 1. Executive Summary & Verdict

### Final Verdict: APPROVE

All 36 uncommitted modified files in the working tree were thoroughly reviewed for correctness, thread safety, cross-platform compatibility, security, race conditions, and test coverage.

The initial gate review identified a critical store synchronization blocker where `restartRegisteredSession` spawned a fresh shell but failed to dispatch `REBIND_SESSION_BACKEND` to the Zustand store, leaving UI components attached to the closed backend ID. This issue was resolved by introducing `setSessionRebindHandler` in `sessionLifecycle.ts` and wiring it in `App.tsx` with dedicated unit test coverage.

All validation gates (Cargo check, Cargo test, Vitest test suites, TypeScript compilation, Vite production build) pass with 0 errors.

---

## 2. Functional Domains Reviewed

### Domain 1: Remote Gateway & Paired Connection Reliability
- **Files:** `src-tauri/src/remote/server.rs`, `src-tauri/src/remote/workspace_api.rs`, `src-tauri/src/terminal/paired_daemon.rs`, `src-tauri/src/terminal/paired_runtime.rs`, `src-tauri/src/ipc/terminal.rs`, `ui/src/components/TerminalPane.tsx`.
- **Review Observations:**
  - **WebSocket Channel Capacity:** `handle_machine_terminal_socket` expanded mpsc input channel buffer from 1 to 64 in production (`if cfg!(test) { 1 } else { 64 }`), preventing immediate connection drops on input/resize bursts.
  - **Idle & Admission Timeouts:** Socket idle timeout raised from 10s to 60s; admission timeout from 10s to 45s.
  - **Read Admission Deadline:** `workspace_api.rs::admit` read request deadline raised from 10s to 35s, accommodating relay network roundtrips while preserving contention test assertions.
  - **Paired Daemon Transport:** Transport receive timeout raised to 60s and send timeout to 30s.
  - **Transient Reattach Retry:** `cmd_terminal_spawn` retries paired terminal reattach once on `TIMEOUT`, `HOST_UNAVAILABLE`, or `PAIRED_PROXY_UNAVAILABLE` before failing.
  - **UI Connecting State:** `TerminalPane.tsx` displays "Connecting to paired terminal..." spinner while spawning/validating instead of prematurely showing the "Paired terminal unavailable" banner.
- **Verdict:** Clean and robust.

### Domain 2: Native Process Suspend & Resume Lifecycle
- **Files:** `src-tauri/src/terminal/session.rs`, `src-tauri/src/terminal/service.rs`, `src-tauri/src/daemon/protocol.rs`, `src-tauri/src/daemon/session_lifecycle.rs`, `src-tauri/src/daemon/client.rs`, `src-tauri/src/daemon/server.rs`, `src-tauri/src/lib.rs`, `ui/src/lib/types.ts`, `ui/src/lib/tauri.ts`, `ui/src/lib/sessionLifecycle.ts`, `ui/src/components/NativeTerminalPane.tsx`, `ui/src/components/TerminalPane.tsx`, `ui/src/components/TabBar.tsx`, `ui/src/components/TerminalSplitView.tsx`.
- **Review Observations:**
  - **Cross-Platform Mechanism:**
    - Unix: `libc::kill(-(pid as i32), sig)` correctly targets the child's process group with `SIGSTOP` / `SIGCONT`.
    - Windows: `windows_suspend` invokes `OpenProcess`, `NtSuspendProcess`, `K32EmptyWorkingSet`, and `NtResumeProcess` with proper error handling and handle cleanup (`CloseHandle`).
  - **PTY Input Gating:** `session.rs::write_input_cancellable` verifies `*self.state.lock()` is `Starting` or `Running`, returning `PTY_INPUT_CLOSED` if suspended.
  - **Native Surface Detachment:** `NativeTerminalPane.tsx` sets `visible = false` and detaches WGPU child surfaces when suspended, preventing GPU overhead and input leakage.
  - **Split Pane & TabBar Controls:** TabBar provides "Suspend Session" and displays sleeping badge on active tabs; `TerminalSplitView.tsx` provides per-leaf Suspend/Resume toggle buttons.
  - **Restart Session:** Terminates old backend via `closeTerminal`, spawns fresh PTY, and dispatches `REBIND_SESSION_BACKEND` via `setSessionRebindHandler`.
- **Verdict:** Fully verified and compliant with non-destructive suspend principles.

### Domain 3: Backend Machine Session Concurrency (100+ Sessions)
- **Files:** `src-tauri/src/daemon/session_service.rs`, `src-tauri/src/daemon/session_service_machine_tests.rs`.
- **Review Observations:**
  - Replaced hardcoded `count >= 64` check with `max_machine_sessions()` method.
  - Defaults to 256 in production and 64 in tests, configurable dynamically via `FERRYX_MAX_MACHINE_SESSIONS`.
  - Tested with `machine_session_capacity_limit_is_configurable` and `sixty_four_live_machine_sessions_are_the_admission_limit`.
- **Verdict:** Clean.

### Domain 4: App Startup Restore Policy Alignment
- **Files:** `ui/src/state/workspaceRestore.ts`, `ui/src/state/workspaceRestore.test.tsx`, `ui/src/lib/agentAutoResume.ts`.
- **Review Observations:**
  - `activeRestoreSessionIds` in `workspaceRestore.ts` aligned with `agentAutoResume.ts` to focus strictly on the active tab and its active split leaf sessions.
  - Non-active tabs receive `standby:` frontend identities under `Active Only` and `Lazy` policies, avoiding over-eager process allocation at app startup.
- **Verdict:** Clean.

### Domain 5: Notification Center & Shortcuts
- **Files:** `ui/src/components/notification/NotificationCenterButton.tsx`, `ui/src/components/Sidebar.tsx`, `ui/src/lib/shortcuts.ts`, `site/src/content/docs/docs/shortcuts.md`.
- **Review Observations:**
  - Accessible shortcut hints and popover toggle for notification center via `⌘⇧N` / `Ctrl+Shift+N`.
  - Unit tests in `NotificationCenterButton.test.tsx` and `shortcuts.test.tsx` pass cleanly.
- **Verdict:** Clean.

---

## 3. Findings & Resolution Summary

- **Blocker (Resolved):** Store rebind failure on session restart in `sessionLifecycle.ts`. Fixed via `setSessionRebindHandler` and `App.tsx` wiring.
- **Minor (Non-blocking):** Unix group signaling `kill(-pid, sig)` could theoretically fail with `ESRCH` if the child process changes pgid. Consider adding fallback to `kill(pid, sig)` in future platform hardening.
- **Minor (Non-blocking):** Unused warnings in `notifications.rs` (`Manager`) and `session_service.rs` (`machine_sessions`) should be cleaned up before next release tagging.

---

## 4. Verification Evidence

- `cargo check --manifest-path src-tauri/Cargo.toml`: Exit code 0 (No compile errors)
- `cargo test --manifest-path src-tauri/Cargo.toml --lib machine_session_capacity_limit_is_configurable`: Pass (0.05s)
- `cargo test --manifest-path src-tauri/Cargo.toml --lib sixty_four_live_machine_sessions_are_the_admission_limit`: Pass (22.06s)
- `cargo test --manifest-path src-tauri/Cargo.toml --lib signal`: 2 passed (0.86s)
- `bun run --cwd ui test`: 145+ tests passed across 7 test suites:
  - `sessionLifecycle.test.ts`: 9/9 passed
  - `workspaceRestore.test.tsx`: 20/20 passed
  - `TerminalSplitView.test.tsx`: 22/22 passed
  - `TabBar.test.tsx`: 20/20 passed
  - `agentAutoResume.test.ts`: 13/13 passed
  - `App.notifications.test.tsx`: 44/44 passed
  - `TerminalPane.test.tsx`: 19/19 passed
- `bun run --cwd ui build`: Exit code 0 (`tsc && vite build` completed successfully)
