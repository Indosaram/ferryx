# Orca Browser and Computer-Use Reuse Audit

Date: 2026-08-26

## Verdict

Orca's browser and computer-use implementation is publicly source-available and may be forked or selectively reused under the MIT License. A direct application fork is technically possible, but is the wrong integration strategy for Ferryx because Orca's implementation is Electron/Node-based while Ferryx is Tauri/Rust with a React frontend.

Recommended approach: retain Ferryx as the host application, adopt Orca's command contract and safety model, and selectively port the platform adapters. Do not depend on the installed `orca` binary in production.

## Evidence

### Public source and license

- Canonical repository: <https://github.com/stablyai/orca>
- Repository HEAD resolved during this audit: `5e5457983a4368c3ff254af91125827da633cecc`.
- License: [MIT](https://raw.githubusercontent.com/stablyai/orca/main/LICENSE), copyright `2026 Lovecast Inc.` The license permits use, copying, modification, distribution, sublicensing, and sale, provided the copyright and license notice are retained.
- The upstream README explicitly identifies Orca as open source under MIT and advertises both its CLI browser automation and Computer Use surfaces.

### Computer-use implementation

Upstream provides a typed CLI contract in `src/cli/specs/computer.ts`, plus platform implementations:

- macOS: `native/computer-use-macos/`, a Swift Package targeting macOS 14+ with an executable and reusable `OrcaComputerUseMacOSCore` library.
- Linux: `native/computer-use-linux/`.
- Windows: `native/computer-use-windows/`.

The contract supports discovery (`list-apps`, `list-windows`, `get-app-state`), semantic accessibility actions (`click`, `set-value`, `perform-secondary-action`), coordinate fallback, scrolling, drag, keyboard input, and clipboard input. The official computer-use guide requires the refresh-safe sequence: snapshot, action, snapshot.

The local installed Orca 1.4.184 bundle also contains `Orca Computer Use.app`, confirming that the published macOS helper is a separately packaged native permission boundary. Its CLI reports that both Orca app and runtime must be running before use.

### Browser implementation

Orca's browser surface is an embedded Electron browser, not generic control of an external browser. Its upstream command executor is `src/main/browser/browser-client-page-command-executor.ts`; it owns per-page lifecycle, navigation fencing, authority transitions, automation dispatch, and cleanup.

Its version-matched CLI exposes browser tabs/profiles; accessibility snapshots; navigation; element-ref actions; screenshots/PDF; uploads/downloads; storage/cookies; network/console capture; and JavaScript evaluation. Element refs are intentionally invalidated after navigation and tab changes.

The installed CLI's runtime was not running during this audit (`orca status --json` returned `runtime.state: not_running`), proving it is a client bridge to a live Orca process rather than a standalone reusable library.

## Ferryx fit

| Area | Reuse directly | Port/adapt | Reason |
| --- | --- | --- | --- |
| Command vocabulary | Yes | — | The snapshot/action/refresh model and typed verbs are host-agnostic. |
| Agent skill/CLI guidance | Yes | Rename and scope to Ferryx | The documented operational rules are portable. |
| macOS computer-use core | Possible | Swift helper + Ferryx IPC | It is MIT Swift code, but Ferryx's core is Rust/Tauri. |
| Linux/Windows computer use | Possible | Repackage and expose through Ferryx backend | Upstream adapters exist but require an explicit lifecycle and permissions design. |
| Orca browser executor | No | Reimplement against Ferryx browser host | It depends on Electron `WebContents`, guest views, and Orca authority state. |
| Installed `orca` CLI/runtime | Development-only option | Do not ship as dependency | Requires Orca runtime, its versioned private transport, its app state, and its macOS permissions. |

Ferryx currently has a Tauri/Rust backend and a React UI. Its generated Tauri capabilities already model webview-scoped IPC permissions, so it has a natural containment boundary for a Ferryx-owned browser controller. The Orca Electron page executor cannot be dropped into this stack without importing Electron and its page lifecycle architecture.

## Recommended implementation path

1. Adopt an explicit provider boundary in Ferryx: `BrowserController` and `ComputerController` behind typed backend commands, with no direct renderer privileges.
2. Implement browser use first using a Ferryx-owned browser surface and structured snapshot references. Support only create/list/navigate/snapshot/click/fill/keypress/screenshot/wait initially. Bind every action to a page ID and snapshot generation; reject stale refs.
3. Port the macOS `OrcaComputerUseMacOSCore` concepts into a small Ferryx-signed helper or call a forked Swift executable through a narrow Rust adapter. Require Accessibility and Screen Recording status checks before any capture/action.
4. Keep computer use opt-in and scope each command to an explicit app bundle ID plus window ID. Prefer accessibility element actions; use coordinates only as a fallback. Return a fresh snapshot after mutating actions.
5. Expose the controller through structured agent tools first. Add a human-facing `ferryx browser` / `ferryx computer` CLI only after the backend contract is stable.
6. Preserve the upstream MIT notice in every copied or substantially derived source file and rebrand all user-facing identifiers, iconography, and product references.

## Explicit non-recommendation

Do not fork Orca wholesale solely to gain browser/computer use. That would import a separate Electron/Node runtime, app lifecycle, workspace model, and transport implementation into a Tauri/Rust product. Fork the upstream repository only when maintaining a parallel Orca-derived application is itself the product decision.

## Sources

1. <https://github.com/stablyai/orca>
2. <https://raw.githubusercontent.com/stablyai/orca/main/LICENSE>
3. <https://raw.githubusercontent.com/stablyai/orca/main/src/cli/specs/computer.ts>
4. <https://raw.githubusercontent.com/stablyai/orca/main/native/computer-use-macos/Package.swift>
5. <https://raw.githubusercontent.com/stablyai/orca/main/src/main/browser/browser-client-page-command-executor.ts>
6. <https://www.onorca.dev/docs/cli/computer-use>
