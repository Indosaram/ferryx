# Cmd+W Tab Close and Close Confirmation

## Behavior delivered

- Cmd+W is now reserved by Ferryx's native File menu for **Close Tab**.
- The native `menu_close_tab` event is bridged into the renderer and invokes the same active-tab close intent used by the app's shortcut and tab-bar close controls.
- The Window menu's **Close Window** action now uses Cmd+Shift+W, so it no longer steals Cmd+W before the WebView can handle the active tab.
- Settings → General includes **Confirm before closing a tab**, disabled by default.
- When enabled, close intents present an accessible confirmation dialog. Cancel/Escape/backdrop keep the tab; the explicit Close tab action closes exactly the pending tab. Pinned-tab protection remains in the workspace store.

## Failing-first record

Before implementation, the focused test/typecheck command failed because the required settings module/storage key and native close-tab bridge did not exist:

```text
Cannot find module './generalSettings'
Module './tauri' has no exported member 'onCloseTabMenu'
```

The feature tests therefore represented unavailable required behavior, rather than an unrelated mock failure.

## Green verification

```text
bun run --cwd ui test -- App generalSettings storageKeys tauri SettingsDialog --reporter=verbose
PASS: 9 files, 91 tests

cargo check --manifest-path src-tauri/Cargo.toml
PASS

cargo test --manifest-path src-tauri/Cargo.toml output_hub
PASS: 2 tests

bun run --cwd ui test
PASS: 73 files, 515 tests

bun run --cwd ui build
PASS: tsc && vite build
```

The focused tests cover persisted default/updated/migrated settings, the Settings → General checkbox, the native `menu_close_tab` bridge, native active-tab closure, and confirmation cancel/confirm behavior.

The Rust command emits three pre-existing `WriterLeaseGuard` dead-code warnings in `src-tauri/src/worktree/manager.rs`.

## Manual desktop acceptance

Desktop input automation is prohibited by the workspace owner. Run Scenario E in [MANUAL_DESKTOP_QA.md](terminal-scale/MANUAL_DESKTOP_QA.md) and return a screenshot or short recording plus PASS/FAIL. The manual check verifies native macOS accelerator precedence, which a renderer test cannot simulate.

## Browser child-webview follow-up

### Root cause

The original renderer/menu bridge did not cover a focused Ferryx BrowserTab. Browser tabs use a separate native child `WKWebView`, so its key events bypass the main Ferryx renderer's capture listener. macOS could therefore fall through to the parent window's default Cmd+W close action.

### Fix

- `src-tauri/src/lib.rs` installs a macOS-only `NSEvent` local `KeyDown` monitor during application setup.
- It recognizes only Command-W without Shift, Control, or Option; it accepts Caps Lock and either `w` case.
- A match emits the existing host-only `menu_close_tab` event and returns `nil`, preventing AppKit from delivering the event to the parent window close action or the menu accelerator a second time.
- `Cmd+Shift+W` remains the explicit Close Window shortcut. No browser guest receives Tauri IPC permission and non-macOS/mobile paths are unchanged.

### Failing-first and green evidence

```text
# RED: native predicate stub always returned false
cargo test --manifest-path src-tauri/Cargo.toml --lib tests::test_macos_cmd_w_shortcut_predicate_boundaries
FAIL: assertion failed for Command-W

# GREEN: native predicate boundaries
cargo test --manifest-path src-tauri/Cargo.toml --lib tests::test_macos_cmd_w_shortcut_predicate_boundaries
PASS: 1 test

# GREEN: native event monitor compiles
cargo check --manifest-path src-tauri/Cargo.toml
PASS

# RED: focused browser-tab test name was absent before adding its regression
vitest run --maxWorkers=1 App -t "routes the native Cmd+W menu accelerator to close an active browser tab"
SKIP: test did not yet exist

# GREEN: host bridge closes a BrowserTab through the normal close intent
bun run --cwd ui test -- App.test.tsx --reporter=dot
PASS: 1 file, 43 tests

# GREEN: full regression and compatibility gates
bun run --cwd ui test
PASS: 76 files, 527 tests

bun run --cwd ui build
PASS: tsc && vite build

cargo test --manifest-path src-tauri/Cargo.toml output_hub
PASS: 7 tests

rustfmt --check --edition 2021 src-tauri/src/lib.rs
PASS
```

`cargo check` and native tests retain the existing `WriterLeaseGuard` dead-code warnings in `src-tauri/src/worktree/manager.rs`; no new warnings were introduced by this fix. The repository-wide `cargo fmt --check` remains blocked by unrelated formatting drift in `src-tauri/src/main.rs` and `src-tauri/tests/daemon_persistence_contract.rs`; the changed native source itself is formatted.

## Korean IME and final-browser-tab correction

### Root cause corrected

The browser-child key monitor initially recognized Cmd+W only from `charactersIgnoringModifiers()`. With Korean/CJK IME composition or some keyboard layouts, macOS can return `None` or a non-`w` character for the physical W key. The monitor then allowed Cmd+W to continue to AppKit's window-close path.

`src-tauri/src/lib.rs` now matches an unshifted Command modifier plus either a `w` character **or physical ANSI W key code `13`**. The monitor emits `menu_close_tab` and returns `nil` for that matched event, so it does not fall through to macOS window close. Shift, Control, and Option variants remain excluded; Cmd+Shift+W remains Close Window.

An attempted close-request fallback was removed after review: it could not repair an event the key monitor never observed and could interfere with a later red-button close. The final code contains no global native close guard.

### Browser lifecycle correction

Closing the only BrowserTab previously removed the last tab outright. `ui/src/state/workspaceStore.ts` now creates a replacement terminal tab/session before dispatching the BrowserTab close. This keeps a successful tab close from presenting an empty workspace that can look like the app disappeared.

### Failing-first and current evidence

```text
# RED: keycode fallback omitted temporarily
cargo test --manifest-path src-tauri/Cargo.toml --lib test_macos_cmd_w_shortcut_predicate_boundaries
FAIL: Command + None + ANSI_KEY_CODE_W assertion

# GREEN: Korean-IME/layout safe physical key fallback
cargo test --manifest-path src-tauri/Cargo.toml --lib test_macos_cmd_w_shortcut_predicate_boundaries -q
PASS: 1 test

# RED: sole BrowserTab close left the workspace empty
bun test ui/src/state/workspaceStore.test.tsx
FAIL: expected one replacement tab; received zero

# GREEN: native browser disposal plus terminal replacement
bun run --cwd ui test -- workspaceStore.browserLifecycle workspaceStore.test --reporter=dot
PASS: 2 files, 23 tests

cargo test --manifest-path src-tauri/Cargo.toml --lib -q
PASS: 161 tests

cargo check --manifest-path src-tauri/Cargo.toml -q
PASS (only existing WriterLeaseGuard dead-code warnings)

cargo test --manifest-path src-tauri/Cargo.toml output_hub -q
PASS: 7 tests

rustfmt --edition 2021 --check src-tauri/src/lib.rs
PASS
```

The fresh debug binary was built at 12:50 after the corrected native source at 12:47. The full UI suite has one separate blocker in concurrently modified `ui/src/lib/sessionPersistence.test.ts`: 3 expectations fail because restored sessions become `exited`. Cmd+W/browser lifecycle source does not modify `sessionPersistence.ts`; the Cmd+W-focused UI regressions above pass.

### Final verification boundary

The fresh `target/debug/ferryx` binary was confirmed newer than the corrected source. After that build, concurrent unrelated daemon work made a new Cargo invocation fail in `src-tauri/src/daemon/server.rs` on unrelated missing daemon symbols. The last clean native-library run before that concurrent edit passed 162 tests; the focused macOS Cmd+W predicate, focused BrowserTab lifecycle regressions, UI production build, and changed-file diagnostics remained clean. The GUI itself is not currently running, so the remaining acceptance check is Scenario E from a manually launched fresh GUI process.
