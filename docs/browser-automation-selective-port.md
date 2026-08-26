# Browser Automation Selective Port

## Runtime Architecture and Boundaries

- **OMO Browser Runtime**: OMO does not provide an app-owned browser runtime; its ultimate-browsing flow uses externally installed `agent-browser` over CDP (optionally CloakBrowser).
- **Ferryx Scope**: Ferryx intentionally ports only app-owned top-level WebView discovery, snapshot, and generation-bound reference actions (`list -> snapshot -> generation-bound eN reference -> click/fill/keypress`), explicitly avoiding CDP capabilities.
- **Local UDS Socket**: The local Unix domain socket (`browser.sock`) is started during GUI setup and has owner-only permissions.

## Discovery and Automation Agent Workflow

The safe agent automation workflow proceeds as follows:
1. `ferryx browser list`: List active Ferryx-owned browser sessions.
2. Pick a Ferryx-owned `browserId` from the returned list.
3. `ferryx browser snapshot --browser-id <id>`: Capture the current page snapshot, obtaining the current generation and semantic interactive element references.
4. Use the returned generation and `eN` references with `click`, `fill`, or `keypress` commands (e.g. `ferryx browser click --browser-id <id> --generation <gen> --ref <ref>`).

### Safe Discovery Scope
`ferryx browser list` (backed by the local UDS `BrowserCliRequest::List` flow) returns only `BrowserSessionSummary` values (`browserId`, `url`, `title`) tracked by the UI-owned `BrowserManager`. It does not discover, query, or interact with external tabs, CDP endpoints, or system browser profiles.

## Bridge and Action Contract

- `BrowserCliRequest::List` / `ferryx browser list` returns a list of active `BrowserSessionSummary` records managed by `BrowserManager`.
- `cmd_browser_automation_snapshot(browserId)` returns the current browser generation, document URL/title, and up to 200 semantic interactive controls. Each control has a short-lived `eN` reference, role, accessible name, and tag name.
- `cmd_browser_automation_act({ browserId, generation, action })` supports `click`, `fill`, and `keypress`. Callers can only act through references cached from the snapshot; arbitrary JavaScript or custom selectors cannot be supplied.
- `BrowserManager` stores `eN -> CSS selector` mappings per browser session. Navigation increments the generation and clears the mapping. Actions with an earlier generation return `BROWSER_AUTOMATION_SNAPSHOT_STALE`; unknown references return `BROWSER_AUTOMATION_TARGET_NOT_FOUND`.

## Settings-Based CLI Installation

- **Settings Action**: `Settings > Browser > Ferryx CLI` provides an explicit `Install Ferryx CLI` action.
- **Symlink Management**: It creates or refreshes only `~/.local/bin/ferryx` as a symlink pointing to the active Ferryx executable.
- **Environment Boundary**: It does not modify `PATH` or shell profiles; users must ensure `~/.local/bin` is on their `PATH` and open a new terminal.
- **Collision and Symlink Safety**: Installation safely rejects regular-file and directory collisions as well as symlinked parent directories.
- **Typed IPC Commands**: Uses typed IPC commands `cmd_cli_launcher_status` and `cmd_cli_launcher_install`, which return user-visible status results.

## Validation

Current validation was clean:
- 9 CLI installer backend tests (`cargo test --manifest-path src-tauri/Cargo.toml --lib cli_install`)
- 6 Settings card tests (`bun run --cwd ui test -- src/components/SettingsDialog.cli.test.tsx`)
- `cargo test --manifest-path src-tauri/Cargo.toml --bin ferryx` (8 binary CLI tests)
- `cargo test --manifest-path src-tauri/Cargo.toml --lib browser` (15 focused browser/IPC tests)
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `rustfmt --check --edition 2024 src-tauri/src/ipc/browser_cli.rs src-tauri/src/main.rs src-tauri/src/ipc/browser.rs src-tauri/src/ipc/error.rs src-tauri/src/browser/manager.rs src-tauri/src/browser/model.rs src-tauri/src/browser/security.rs src-tauri/src/browser/tests.rs`
- `git diff --check`

Direct desktop end-to-end automation was not performed because project user preferences prohibit desktop UI automation and require an explicit manual E2E verification request when desktop E2E cannot be automated. Automated checks validate CLI parsing, the UDS transport boundary, and typed generation/reference guards, while a user must manually verify a live Ferryx browser tab using three behavior checks: (1) snapshot returns semantic controls, (2) fill changes an input value, and (3) click or keypress triggers the expected page behavior.
