# Tauri v2 + Vite + React HMR Root Cause and Fix

## Scope

This investigation covers the macOS desktop development path for rorca (`orca-lite`):

- Frontend: `ui/` — Vite 6, React 18, Bun
- Native shell: `src-tauri/` — Tauri v2 / macOS WebKit
- HMR transport: Vite WebSocket
- File watching: Vite/Chokidar on macOS
- React Fast Refresh lifecycle: xterm/WebGL and native child webviews

The target development endpoint is now deliberately singular and deterministic:

- HTTP: `http://127.0.0.1:5173`
- HMR WebSocket: `ws://127.0.0.1:5173`

## Root causes

### 1. The old development topology could address HTTP and HMR differently

The original Vite configuration mixed several endpoint rules:

- the ordinary desktop path could use Vite's default host / `localhost` behavior;
- `TAURI_DEV_HOST` changed the server host;
- when `TAURI_DEV_HOST` existed, HMR was moved to a separate port (`5174`) while the HTTP dev server remained on `5173`;
- Tauri's `devUrl` used `localhost`.

That made the app dependent on hostname resolution and environment state. On macOS, `localhost` may resolve through IPv4 or IPv6, while the Vite listener/HMR client can be bound to a different address. A separate HMR port also created another independent point of failure.

The fix is to use one IPv4 loopback address and one port for the complete desktop dev path. Vite's HMR socket now shares the HTTP server instead of creating a second `5174` topology.

### 2. The already-running Vite process was missing file-change events

The most important reproduced failure was not the WebSocket handshake itself. The existing process on port 5173 accepted both HTTP and a `vite-hmr` WebSocket connection and returned Vite's `{"type":"connected"}` message, but source-file replacements did not produce an HMR update.

This matters because the workspace is frequently edited through atomic file replacement. The old Vite process had been started with the native macOS/FSEvents watcher path and did not observe those replacements. It also failed to observe the later `vite.config.ts` change, so editing the config could not repair that already-running process.

The fix is deterministic polling:

```ts
watch: {
  usePolling: true,
  interval: 100,
  ignored: ["**/src-tauri/**", "**/target/**", "**/.git/**"],
}
```

This trades a small amount of development-only polling overhead for reliable detection of editor/automation atomic replacements. Changes are sampled every 100 ms rather than depending on a missed FSEvents notification.

### 3. A stale Vite process could occupy 5173 and keep serving the old watcher/module graph

During the investigation, starting `bun run --cwd ui dev` reproduced:

```text
Error: Port 5173 is already in use
```

The existing process was a real Vite server, but it was the stale process described above. `strictPort: true` is intentionally retained so rorca fails loudly instead of silently moving to another port while Tauri still loads 5173.

The Tauri hooks were also simplified from an explicit nested `sh -c 'cd ui && ...'` wrapper to direct Bun cwd commands:

```json
"beforeDevCommand": "bun run --cwd ui dev",
"beforeBuildCommand": "bun run --cwd ui build"
```

This removes an unnecessary wrapper process and gives Tauri a cleaner ownership/lifecycle relationship with the Vite child process.

**One-time migration requirement:** any Vite/Tauri process that was started before this fix must be terminated and `cargo tauri dev` started again. An old process cannot retroactively acquire the new polling watcher because it missed the config change itself. Once restarted with the committed configuration, the deterministic watcher is active.

### 4. Native child webviews could visually mask a successful React HMR update

The in-app browser is a native Tauri child webview, not a React DOM element. Before this fix:

- `BrowserPane` disconnected its `ResizeObserver` and window listener on unmount, but did not hide the native child webview;
- switching away from a browser tab or a Fast Refresh cleanup could therefore leave a native layer visible above the refreshed React UI;
- closing a browser tab removed the React tab state but did not call `cmd_browser_close`, leaving the native webview/session alive.

This can look exactly like "HMR did not update" even when the parent React tree did update.

The fix:

- `BrowserPane` hides its native child webview in the effect cleanup;
- native visibility and close operations are serialized per `browserId`, so Fast Refresh cleanup `hide` and remount `show` cannot complete out of order;
- closing a browser tab now awaits `closeBrowser(browserId)` before removing the React tab.

### 5. CSP was investigated but was not the primary failure

The previous CSP already contained broad `ws:` / `wss:` allowance, so CSP was not the reason the reproduced HMR WebSocket failed to update.

The policy is now explicit and aligned with the canonical development endpoint, including:

```text
ws://127.0.0.1:5173
http://127.0.0.1:5173
```

`localhost:5173` remains in the CSP as a compatibility allowance, but Tauri's `devUrl` and Vite's actual server/HMR addresses use `127.0.0.1`.

### 6. xterm/WebGL was audited and is not the HMR root cause

`TerminalPane` already has the required Fast Refresh/unmount cleanup symmetry:

- `AbortController` for asynchronous renderer attachment;
- `disposed` guards after async font/module loading;
- `ResizeObserver.disconnect()`;
- DOM listener removal;
- xterm input/bell/title disposable cleanup;
- terminal event-bus unsubscribe;
- WebGL addon disposal;
- `terminal.dispose()` and ref clearing.

`attachWebglRenderer` also disposes on context loss and handles an aborted async load. No xterm/WebGL lifecycle leak was found that explains the HMR transport failure, so those paths were not rewritten unnecessarily.

## Implemented changes

### `ui/vite.config.ts`

- fixed the macOS desktop server host to `127.0.0.1`;
- retained port `5173` with `strictPort: true`;
- placed HMR on the same server socket (`ws://127.0.0.1:5173`);
- removed the conditional `TAURI_DEV_HOST` / HMR `5174` split for this desktop app;
- enabled 100 ms polling to make atomic file replacement reliable;
- ignored Rust target/Git paths that do not need frontend watching.

### `src-tauri/tauri.conf.json`

- changed `devUrl` to `http://127.0.0.1:5173`;
- simplified `beforeDevCommand` and `beforeBuildCommand` to direct Bun cwd commands;
- explicitly allowed the canonical HTTP and WebSocket development endpoints in CSP.

### `ui/index.html`

- kept exactly one application module entry;
- documented that Vite injects `/@vite/client` itself, preventing an attempted manual second HMR client;
- removed a duplicate favicon declaration.

### `ui/src/components/BrowserPane.tsx`

- hides the native child webview during React effect cleanup / Fast Refresh remount;
- handles teardown-time native IPC rejection without an unhandled promise;
- keeps bounds and visibility updates lifecycle-safe.

### `ui/src/lib/browserTauri.ts`

- added a per-browser native lifecycle queue;
- serialized `setBrowserVisible` and `closeBrowser` operations to prevent hide/show/close races during Fast Refresh.

### `ui/src/state/workspaceStore.ts`

- browser-tab close now closes the native Tauri child webview before removing the React tab.

### Regression tests

Added/updated coverage for:

- `ui/src/test/viteHmrConfig.test.ts` — fixed IPv4 loopback endpoint, one HMR port, polling watcher;
- `ui/src/components/BrowserPane.test.tsx` — show on mount and hide on cleanup;
- `ui/src/lib/browserTauri.test.ts` — ordered hide/show/close lifecycle queue;
- `ui/src/state/workspaceStore.browserLifecycle.test.tsx` — native close before browser-tab removal;
- `ui/src/test/setup.ts` — test setup remains valid for the Node-only Vite config test.

## Live HMR verification

Two observations separated the broken old process from the fixed configuration.

### Existing stale 5173 process

- HTTP request: succeeded;
- HMR WebSocket: connected and returned `{"type":"connected"}`;
- source replacement: no `update` frame arrived, even after the module was loaded into Vite's module graph.

This proves that "WebSocket connects" alone was not sufficient; the existing watcher/module process was stale and not seeing the file mutation.

### Fresh Vite process using the fixed configuration

A fresh Vite instance was started on a temporary verification port while retaining the new watcher configuration. After `BrowserPane.tsx` was loaded into its module graph and the file was changed, the connected HMR client received:

```json
{
  "type": "update",
  "updates": [
    {
      "type": "js-update",
      "path": "/src/components/BrowserPane.tsx",
      "acceptedPath": "/src/components/BrowserPane.tsx"
    }
  ]
}
```

Vite simultaneously logged:

```text
hmr update /src/components/BrowserPane.tsx
```

This directly verifies the complete path: file mutation -> watcher -> Vite module graph -> HMR WebSocket update.

## Validation

The final implementation is verified with the required project commands:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
bun run --cwd ui test
bun run --cwd ui build
```

All four commands pass after the implementation is complete.

The UI suite also includes the HMR/native-webview regression tests above.

## Concurrent-workspace regression found during validation

The repository contained unrelated in-progress terminal-preference changes. The first full Rust test run exposed four pre-existing contract failures: a path-specific Ghostty loader was consulting the global Ghostty CLI before the supplied path, and the Ghostty parser was injecting an extra CSS `monospace` family not present in the parsed source.

Those two side effects were corrected without removing the concurrent font-size/theme import work:

- `load_terminal_preferences_from_path` is path-scoped again;
- `parse_ghostty_config` reports the configured font families exactly;
- global Ghostty CLI fallback remains available in `load_terminal_preferences` when no config file exists.

The native contract tests then passed, allowing the requested full Rust validation to succeed.

## Expected development behavior after this fix

1. Stop any pre-fix `cargo tauri dev` / Vite process that is still holding port 5173.
2. Start `cargo tauri dev` normally.
3. Tauri loads `http://127.0.0.1:5173`.
4. Vite's HMR client connects to `ws://127.0.0.1:5173` on the same server.
5. Frontend file replacements are detected by the 100 ms polling watcher and sent through React Fast Refresh without relying on FSEvents delivery.
6. Native browser child webviews are hidden/closed correctly during React cleanup, so they cannot cover a freshly updated React UI.

`strictPort` is intentionally retained. If 5173 is occupied, development stops with an explicit error instead of silently starting Vite elsewhere and creating another Tauri/HMR endpoint mismatch.

## Result

The failure was a combination of a stale/missed file-watcher path, an unnecessarily ambiguous HTTP/HMR endpoint topology, and a native child-webview lifecycle leak that could visually mask successful React refreshes. The development stack now uses a single deterministic IPv4 loopback endpoint, polling-based change detection, same-port HMR, explicit CSP connectivity, and lifecycle-safe native webviews.
