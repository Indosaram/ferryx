# Audit: remote
Repo: /Users/indo/code/project/orca-lite
Scanned: ui/src/remote/RemoteApp.tsx, ui/src/remote/RemoteSessionList.tsx, ui/src/remote/RemoteTerminal.tsx, ui/src/remote/PairingPage.tsx, ui/src/lib/remoteClient.ts, ui/src/remote/RemoteUI.test.tsx, ui/src/main.tsx, HMR_ROOT_CAUSE_AND_FIX.md
Date: 2026-08-22

## Findings

### F-remote-01
- Severity: High
- File: ui/src/main.tsx:3
- Mechanism: `main.tsx` statically imports both `App` (the full desktop application containing Monaco editor, diff views, settings dialogs, sidebar panels, terminal tab managers, and AI assistants) and `RemoteApp` at the top level. When a remote client opens the application in a mobile browser over HTTP/WSS, the browser is forced to download, parse, and evaluate the entire monolithic 490KB+ desktop JavaScript bundle (`index-*.js`) before evaluating `isTauriApp` to render `<RemoteApp />`. This causes unnecessary network latency, memory pressure, and cold-boot CPU overhead on mobile devices.
- Hot path: yes
- Suggested fix: Use dynamic imports / `React.lazy` with a `Suspense` boundary for `App` and `RemoteApp` so that mobile browsers only download the lightweight remote shell chunk without pulling the desktop application tree.
- Write scope: ui/src/main.tsx
- RED proof:
  ```tsx
  import App from "./App";
  import { RemoteApp } from "./remote/RemoteApp";
  ...
  const isTauriApp = typeof window !== "undefined" && Boolean((window as any).__TAURI_INTERNALS__);

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      {isTauriApp ? <App /> : <RemoteApp />}
    </React.StrictMode>
  );
  ```
  Both root applications are eagerly bundled into the initial entry chunk regardless of execution environment.

### F-remote-02
- Severity: Low
- File: ui/src/remote/RemoteTerminal.tsx:210
- Mechanism: In `RemoteTerminal.tsx`, `const enc = new TextEncoder();` is instantiated synchronously on every key press event dispatched from `MobileKeyDock` via `sendKey`. Allocating short-lived `TextEncoder` instances on every keystroke in the terminal input pipeline causes unnecessary object allocations and garbage collection overhead during rapid mobile terminal typing.
- Hot path: yes
- Suggested fix: Move `const textEncoder = new TextEncoder();` to module scope or declare it once outside `sendKey` so the encoder instance is reused across key events.
- Write scope: ui/src/remote/RemoteTerminal.tsx
- RED proof:
  ```ts
  const sendKey = (key: string) => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;

    const enc = new TextEncoder();
    if (key === "ctrl-c") {
      ws.send(JSON.stringify({ type: "signal", signal: "interrupt" }));
    } else if (key === "tab") {
  ```
  A new `TextEncoder` instance is allocated on every keystroke dispatched to the terminal WebSocket.

## Non-findings / accepted
- `RemoteTerminal` decoupled data pump: WebSocket `onmessage` writes incoming data directly to the xterm terminal instance (`term.write(...)`) using binary `Uint8Array` or string payloads without calling React `setState` or triggering tree re-renders during high-throughput output.
- `RemoteTerminal` xterm lifecycle & WebGL cleanup: xterm is initialized inside a single `useEffect` keyed on `[sessionId, token]`. Asynchronous WebGL renderer attachment is guarded with `AbortController` and `isDisposed` checks, and fully disposed on unmount without leaking WebGL contexts or terminal listeners. Settings updates dynamically call `applyTerminalSettings` without tearing down or recreating xterm.
- `RemoteTerminal` viewport and resize debouncing: Window and `visualViewport` resize handlers are coalesced through `requestAnimationFrame` with `cancelAnimationFrame` guard; layout heights are applied cleanly and resize WebSocket frames are sent only when dimensions change.
- `RemoteSessionList` hierarchy memoization: `buildHierarchy` grouping algorithm is wrapped in `useMemo([sessions, projects, activeWorkspaceId])`, preventing redundant Map allocations and array transformations on unrelated parent renders.
- `RemoteSessionList` virtualization: List size is naturally bounded by the number of active terminal sessions (typically 1–10 sessions per workstation). Omitting virtualized list containers avoids bundle weight and scroll measurement overhead with zero degradation for small collections.
- `PairingPage` lightweight form: Controlled 6-digit PIN input with standard form submission; QR code generation is isolated to the desktop settings dialog and not bundled or rendered on mobile.
- `remoteClient.ts` event stream: `connectEvents()` establishes a single WebSocket for system events with a bounded 3-second reconnect timeout; message parsing is scoped to subscribed event listeners and does not poll the server.
- `RemoteApp` atomic workspace state fetching: `refreshWorkspace()` fetches sessions, projects, and active workspace in a single atomic HTTP call (`/api/v1/workspace/state`), avoiding race conditions and eliminating background polling timers.
- Vite `server.watch.usePolling`: Dev-only file watching configuration documented in `HMR_ROOT_CAUSE_AND_FIX.md`; not present in production builds.

## Scan coverage
- files read:
  - ui/src/remote/RemoteApp.tsx
  - ui/src/remote/RemoteSessionList.tsx
  - ui/src/remote/RemoteTerminal.tsx
  - ui/src/remote/PairingPage.tsx
  - ui/src/lib/remoteClient.ts
  - ui/src/remote/RemoteUI.test.tsx
  - ui/src/main.tsx
  - HMR_ROOT_CAUSE_AND_FIX.md
- patterns checked: whole-store re-render, missing memo, inline object/fn identity, list virtualization, work in render, effect churn, rAF loops, layout reads during drag, JSON.parse on hot path, xterm recreate, code-splitting
