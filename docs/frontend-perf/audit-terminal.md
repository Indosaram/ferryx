# Audit: terminal
Repo: /Users/indo/code/project/orca-lite
Scanned: ui/src/components/TerminalPane.tsx, ui/src/components/TerminalSplitView.tsx, ui/src/components/TerminalSearchOverlay.tsx, ui/src/components/TerminalLinkActions.tsx, ui/src/lib/terminalHostManager.ts, ui/src/lib/terminalRenderer.ts, ui/src/lib/terminalRendererMetrics.ts, ui/src/lib/terminalEvents.ts, ui/src/lib/terminalOutput.ts, ui/src/lib/terminalSettings.ts, ui/src/lib/terminalTransport/index.ts, ui/src/lib/terminalTransport/tauriTransport.ts, ui/src/lib/terminalTransport/remoteTransport.ts
Date: 2026-08-22

## Findings
### F-terminal-01
- Severity: High
- File: ui/src/lib/terminalEvents.ts:134
- Mechanism: On every terminal output chunk from the PTY, `publishOutput` performs full 512KB string concatenation and slicing (`${this.backlog.get(sessionId) ?? ""}${text}`). Once the buffer reaches `MAX_BACKLOG_CHARS` (512KB), every small incoming chunk (e.g. 10-100 bytes) allocates a new 512KB string for concatenation and a second 512KB string for `.slice(-MAX_BACKLOG_CHARS)`. At standard streaming rates (e.g. 200 chunks/s during compilation or log tailing), this generates >200MB/s of ephemeral garbage string allocations on the main JavaScript thread, causing severe V8/JSC GC thrashing and frame drops.
- Hot path: yes
- Suggested fix: Replace monolithic string concatenation with a chunk ring buffer / array of string chunks (`string[]`) tracking total character length, trimming oldest chunks only when exceeding the size threshold, and joining chunks lazily only when replaying output on initial subscribe.
- Write scope: ui/src/lib/terminalEvents.ts
- RED proof:
```ts
    const nextBacklog = `${this.backlog.get(sessionId) ?? ""}${text}`;
    this.backlog.set(sessionId, nextBacklog.slice(-MAX_BACKLOG_CHARS));
```
Why slow: Concatenating and slicing a 512KB string on every incoming PTY chunk copies hundreds of thousands of characters and allocates megabytes of garbage memory per second on the UI thread.

### F-terminal-02
- Severity: High
- File: ui/src/lib/terminalOutput.ts:4
- Mechanism: On every incoming base64 output chunk from the Tauri PTY IPC, `decodeBase64` runs an unvectorized JS `for` loop executing `binary.charCodeAt(index)` character-by-character to populate a `Uint8Array` before passing it to `TextDecoder.decode()`. For high-throughput output (megabytes of stream output), running interpreted byte-by-byte iteration blocks the main thread event loop, delaying UI rendering and user input handling.
- Hot path: yes
- Suggested fix: Use direct typed-array decode via `Uint8Array.fromBase64` when supported, or fast chunked decoding / native array buffer transfer to avoid per-character JS iteration.
- Write scope: ui/src/lib/terminalOutput.ts
- RED proof:
```ts
function decodeBase64(data: string): Uint8Array {
  const binary = globalThis.atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}
```
Why slow: Interpreted JS loop iterating over every single byte of PTY output consumes substantial CPU time on the main thread during high-volume output bursts.

### F-terminal-03
- Severity: Medium
- File: ui/src/components/TerminalPane.tsx:38
- Mechanism: `TerminalPane` invokes `useTerminalSettings()`, which on mount triggers an asynchronous `refreshNativePreferences(true)` call. When resolved, `useTerminalSettings` emits a new `settings` reference, firing an effect in `TerminalPane` that invokes `terminalHostManager.applySettings(settings)`. `applySettings` iterates across ALL terminal instances in the app and calls `applyTerminalSettings` and `fitAddon.fit()` synchronously. Mounting multiple split panes creates N concurrent native preference fetches and N x M redundant `fit()` layout passes across all open terminals.
- Hot path: no
- Suggested fix: Hoist terminal settings subscription to the manager level or subscribe `TerminalPane` to changes without re-triggering global manager refits on local pane mounts; apply settings on mount only to the newly mounted instance.
- Write scope: ui/src/components/TerminalPane.tsx, ui/src/lib/terminalSettings.ts
- RED proof:
```ts
  const { settings } = useTerminalSettings();

  useEffect(() => {
    terminalHostManager.applySettings(settings);
  }, [settings]);
```
coupled with `ui/src/lib/terminalHostManager.ts:187`:
```ts
  applySettings(settings: EffectiveTerminalSettings) {
    for (const inst of this.instances.values()) {
      applyTerminalSettings(inst.terminal, settings);
      inst.fitAddon.fit();
    }
  }
```
Why slow: Every pane mount triggers a settings update cycle that forces synchronous xterm layout recalculation (`fitAddon.fit()`) across all other terminal instances.

### F-terminal-04
- Severity: Medium
- File: ui/src/components/TerminalPane.tsx:64
- Mechanism: Two redundant `ResizeObserver` instances exist for every terminal pane: one in `TerminalPane` observing `containerRef` and one in `TerminalHostManager` observing the child `hostElement`. On resize, `TerminalPane` schedules two chained `requestAnimationFrame` passes executing `fitMountedTerminal` (which calls `container.getBoundingClientRect()` and `fitAddon.fit()`), while `TerminalHostManager` concurrently schedules another `requestAnimationFrame` calling `fitAddon.fit()` and `resizeTerminal` IPC. This results in 3 separate `fit()` passes per resize event with interleaved DOM reads and writes.
- Hot path: yes
- Suggested fix: Consolidate resize observation to a single observer per instance inside `TerminalHostManager`, eliminate duplicate chained `requestAnimationFrame` calls in steady-state resize, and debounce the backend IPC resize call.
- Write scope: ui/src/components/TerminalPane.tsx, ui/src/lib/terminalHostManager.ts
- RED proof:
```ts
    const scheduleStableFit = (instance: TerminalInstance, focus = false) => {
      cancelScheduledFit();
      firstFitFrame = requestAnimationFrame(() => {
        firstFitFrame = 0;
        if (cancelled) return;
        const fitted = fitMountedTerminal(container, instance);
        if (focus && fitted) instance.terminal.focus();
        secondFitFrame = requestAnimationFrame(() => {
          secondFitFrame = 0;
          if (cancelled) return;
          fitMountedTerminal(container, instance);
        });
      });
    };
```
and `ui/src/lib/terminalHostManager.ts:128`:
```ts
    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(hostElement);
```
Why slow: Duplicate ResizeObservers fire in parallel, scheduling 3 layout reads/writes and `fit()` recalculations across multiple frames for a single layout resize.

### F-terminal-05
- Severity: Medium
- File: ui/src/components/TerminalSplitView.tsx:121
- Mechanism: During drag-and-drop operations (tab or pane movement), `collisionDetection` executes on every pointermove event. Within the callback, `dataFor(id)` performs `args.droppableContainers.find((container) => container.id === id)` repeatedly inside both `.filter()` and `.sort()`. For N droppables and K collisions, this performs quadratic array searches (`O(K * log(K) * N)`) on every drag frame.
- Hot path: yes
- Suggested fix: Build a `Map<UniqueIdentifier, Data>` from `args.droppableContainers` once at the beginning of `collisionDetection` for O(1) container lookups during filtering and sorting.
- Write scope: ui/src/components/TerminalSplitView.tsx
- RED proof:
```ts
      const dataFor = (id: UniqueIdentifier) =>
        args.droppableContainers.find((container) => container.id === id)?.data.current;
      return pointerWithin(args)
        .filter((collision) => {
          const data = dataFor(collision.id);
          return isWorkspaceDropData(data) && dropPriority(activeData, data) < 100;
        })
        .sort((left, right) => {
          const leftData = dataFor(left.id);
          const rightData = dataFor(right.id);
          if (!isWorkspaceDropData(leftData) || !isWorkspaceDropData(rightData)) return 0;
          return dropPriority(activeData, leftData) - dropPriority(activeData, rightData);
        });
```
Why slow: Linear search across droppable containers repeated for every comparison in `.sort()` on every 60-120Hz drag pointer movement.

### F-terminal-06
- Severity: Low
- File: ui/src/components/TerminalSplitView.tsx:354
- Mechanism: In `PaneRenderer`, when `sessions[sessionId]` is undefined, a new fallback object literal is created on every render. Because `PaneRenderer`, `PaneLeafView`, and `TerminalPane` are not memoized, any parent state change (e.g. `activityByTabId` or `unreadTabIds` updates from background terminal output) re-renders the entire split tree and passes a new `session` reference, causing `TerminalPane`'s `useEffect([session])` to invoke `terminalHostManager.updateSession` on every render.
- Hot path: no
- Suggested fix: Define a static module-level fallback session object and wrap `PaneRenderer` and `PaneLeafView` in `React.memo`.
- Write scope: ui/src/components/TerminalSplitView.tsx
- RED proof:
```ts
    const session = sessions[sessionId] ?? {
      id: sessionId,
      cwd: "",
      worktreePath: "",
      workspaceId: "",
      worktree: null,
      backendSessionId: null,
      lifecycle: "working" as const,
    };
```
Why slow: Inline object allocation breaks referential equality, causing downstream effects in `TerminalPane` to re-run needlessly during layout re-renders.

## Non-findings / accepted
- xterm instance reuse: `TerminalHostManager.getOrCreate` reuses existing `TerminalInstance` and DOM host elements rather than destroying and recreating xterm instances across tab switches.
- WebGL addon lifecycle and fallback: `attachWebglRenderer` gracefully handles WebGL initialization failures and context losses by tracking context loss and falling back to canvas rendering without leaking memory or throwing unhandled exceptions.
- Code-splitting of terminal assets: `loadTerminalAssets` bundles xterm and addons into dynamically loaded asynchronous chunks via `Promise.all([import(...)])`, preventing initial startup bundle bloat.
- Drag divider bounding rect calculation: `PaneResizeDivider` measures `parent.getBoundingClientRect()` once in `handlePointerDown` and reuses the coordinates during drag rather than reading layout on every `pointermove`.
- Terminal link actions modal: `TerminalLinkActions` subscribes to custom window events and unmounts cleanly without polling loops or rAF overhead.
- Vite polling watcher: Vite `server.watch.usePolling` is strictly development-only and is omitted in the production build.

## Scan coverage
- files read:
  - ui/src/components/TerminalPane.tsx
  - ui/src/components/TerminalSplitView.tsx
  - ui/src/components/TerminalSearchOverlay.tsx
  - ui/src/components/TerminalLinkActions.tsx
  - ui/src/lib/terminalHostManager.ts
  - ui/src/lib/terminalRenderer.ts
  - ui/src/lib/terminalRendererMetrics.ts
  - ui/src/lib/terminalEvents.ts
  - ui/src/lib/terminalOutput.ts
  - ui/src/lib/terminalSettings.ts
  - ui/src/lib/terminalTransport/index.ts
  - ui/src/lib/terminalTransport/tauriTransport.ts
  - ui/src/lib/terminalTransport/remoteTransport.ts
- patterns checked: whole-store re-render, missing memo, inline object/fn identity, list virtualization, work in render, effect churn, rAF loops, layout reads during drag, JSON.parse on hot path, xterm recreate, code-splitting
