# Settings and Remote Reliability Audit

Date: 2026-08-23

## Resolved user-visible behaviors

### 1. Settings survive app restart

- Appearance no longer owns a second localStorage implementation inside `SettingsDialog`.
- It uses the canonical `useAppearanceSettings` flow, which saves validated theme, accent, and density values, emits the appearance event, and is reapplied by the runtime bridge at startup.
- Terminal settings persistence coverage was repaired so the test hook receives its actual mocked native preferences instead of a stale browser fallback.

### 2. Remote PIN is clickable

- Once Remote Access is enabled, its visible PIN is an accessible `button` with `data-testid="remote-pairing-code"`.
- Clicking copies the exact current PIN to the clipboard and changes the text to `Copied`.
- The transient copy timer is cleaned up on Settings unmount.

### 3. Remote restart behavior is explicit and safe

- The Remote listener is intentionally **session-only**: it stops when Ferryx exits and starts OFF after relaunch.
- The Settings UI states this explicitly.
- Trusted paired devices and durable non-listener Remote configuration persist through `RemoteGatewayState::new_persistent`, now used by actual app construction.
- A restart never silently restores a listening network gateway.

### 4. Default Agent has an observable meaning

- A chosen, enabled, available Default Agent is listed first in the New Tab agent group and receives a visible `Default` badge.
- Choosing an agent still launches the one clicked; the preference does not auto-launch a process.
- Disabled, unavailable, missing, `Auto`, and `None` preferences retain natural agent order with no false badge.

### 5. Quick Commands removed

- Removed Settings navigation, component state, persistence flow, tests for the obsolete flow, and `QUICK_COMMANDS_STORAGE_KEY`.
- No application UI or storage implementation now uses `ferryx.settings.quickCommands`.

### 6. Workspace Settings receives live projects

- `App` now passes registered projects, active project, active worktree, and select/add callbacks into `SettingsDialog`.
- The Workspace section therefore renders registered workspaces rather than its default empty input state.

### 7. General is no longer blank

- General now renders a concise Ferryx desktop overview and a navigational explanation of Settings sections.
- It intentionally does not duplicate Appearance controls.

## Failing-first evidence

The regression assertions were deliberately mutated after implementation and run RED, then the exact implementation was restored and run GREEN.

| Mutated seam | RED observable | Restored GREEN observable |
| --- | --- | --- |
| Appearance `onChange` | remount persistence assertion failed | selected theme/accent/density persisted and reapplied |
| Pairing PIN test id | PIN copy test could not find the control | clickable PIN copied the exact code and displayed `Copied` |
| General overview test id | General overview assertion failed | overview rendered without Appearance controls |
| Default Agent eligibility | badge/order tests failed | selected usable agent moved first with `Default` badge |
| Quick Commands storage key | absence test failed | key is absent |
| App Settings projects prop | live Workspace props test failed | registered projects/callbacks reached Settings |
| Remote restart reset | Rust restart tests failed | listener starts Off and status reports session-only |

## Verification

- Focused Settings contracts:
  - `npx vitest run src/components/SettingsDialog.test.tsx src/components/NewTabPopover.test.tsx src/lib/storageKeys.test.ts src/App.test.tsx src/lib/settingsRuntime.test.ts src/lib/terminalSettings.test.tsx src/components/SettingsDialog.workspace.test.tsx --maxWorkers=1`
  - **7 files, 90 tests passed**
- Full UI suite:
  - `npm test`
  - **67 files, 489 tests passed**
- UI production build:
  - `npm run build`
  - **passed**
- Native Remote contracts:
  - `cargo test remote::state --manifest-path src-tauri/Cargo.toml`
  - `cargo test remote_status_after_reopen_is_disabled_session_only --manifest-path src-tauri/Cargo.toml`
  - `cargo test app_remote_state_persists_pairing_but_starts_off --manifest-path src-tauri/Cargo.toml`
  - **all passed**
- `cargo fmt --check` and `cargo check --manifest-path src-tauri/Cargo.toml` — **passed**.
- LSP diagnostics were clean for `ui/src`; `src-tauri/src` diagnostics initially reported clean then a later daemon request timed out while busy. Rust `cargo check` is the clean compiler-level fallback.
- `git diff --check` — **passed**.

### Final validation rerun

- Focused Settings/Remote suite — **7 files, 90 tests passed**.
- Full UI suite — **67 files, 489 tests passed**.
- Final UI production build — **passed**.
- Final native Remote lifecycle/persistence contracts — **all passed**.
- Local browser QA opened the Vite surface successfully, but Vite served only the Remote pairing route without the native Settings shell. The browser session and Vite server were closed; it is not represented as desktop Settings evidence.

### Completion-audit rerun

- Current full UI suite: `cd ui && npm test` — **67 files, 492 tests passed**.
- Current production build: `cd ui && npm run build` — **passed**.
- Current Remote lifecycle checks — **passed**:
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::state` — 2 tests.
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib remote_status_after_reopen_is_disabled_session_only` — 1 test.
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib app_remote_state_persists_pairing_but_starts_off` — 1 test.
- LSP error diagnostics were clean for both `ui/src` and `src-tauri/src` during this audit.
- The current workspace contains unrelated concurrent changes; this audit relies on the focused Settings/Remote contracts above and does not alter or claim those other changes.

## Native desktop manual QA

Desktop automation was intentionally not used. In the Ferryx desktop app, perform these checks:

1. **Persistence:** Settings → Appearance → set `Dark`, `Ocean Blue`, `Comfortable`; fully quit Ferryx; relaunch; reopen Appearance. All three selections must remain.
2. **PIN copy:** Settings → Remote Access → enable. Click the displayed PIN. It must change to `Copied`; paste into a text field to confirm the identical six-digit value.
3. **Restart policy:** With Remote Access enabled, quit and relaunch Ferryx. Remote Access must show disabled. Existing paired devices must remain listed. Enable again to create a new current-session listener.
4. **Default Agent:** Settings → Agents → select a detected enabled agent. In a terminal tab, open `+` → New Tab menu. The selected agent must be first in AGENTS with a `Default` badge. Click another agent to confirm that exact clicked agent launches instead.
5. **Quick Commands and General:** Settings navigation must have no Quick Commands entry. General must show the overview cards.
6. **Workspace:** Settings → Workspace must list the current registered project and active worktree rather than `No projects registered.`

## Known unrelated native-suite failure

The full Rust suite currently has one unrelated shared-worktree failure:

- `daemon::server::tests::test_pump_stream_compact_framing_and_exit` at `src-tauri/src/daemon/server.rs:444`

It reproduces in isolation and concerns daemon compact stream framing/session output. None of the Settings or Remote files in this audit import or exercise that path. It was left unchanged.
