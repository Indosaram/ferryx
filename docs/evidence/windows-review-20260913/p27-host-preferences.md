# P27 / REMOTE-RC-04: selected-host web terminal preferences

2026-09-13, child st_01a0997a. Local production repair and contract delivered; actual Windows HTTP/appearance acceptance and parent C002 remain pending. No build or native/SSH/desktop/ref/worktree/commit/push operations performed.

## Repair and scope

RemoteTerminal now passes its existing selected transport base and device token through useTerminalSettings -> getTerminalPreferences -> RemoteClient in web runtime. Explicit-token HTTP clients do not borrow or clear origin-wide credentials. Scoped preferences bypass the global local-appearance cache; resolved state is bound to base URL and token, and the existing request generation rejects old completions. No host-service/settings framework or RemoteApp changes.

Desktop Tauri terminals, including embedded remote terminals, retain local native appearance and its cache. Local font overrides retain precedence. Existing preference fallback behavior, wheel/input/pinch logic, and all lead-owned wheel changes are preserved. The existing duplicate initial refresh behavior is not refactored here.

The task's `/h` wording is interpreted as the existing selected-host prefix, not a route change: the read production hostTransportUrl helper emits `/host/<encoded-machineId>`. The contract asserts exact `/host/machine-a` and `/host/machine-b` preference URLs and bearer headers on a shared relay.

## Contract and provenance

Exact registered invocation, executed from repository root:

```sh
bun run --cwd ui test src/remote/RemotePreferences.contract.test.tsx
```

Real RemoteApp, host store, legacy-to-scoped token migration, RemoteTerminal, settings resolver/cache, Tauri wrapper and RemoteClient are retained. Only HTTP/WebSocket and native runtime boundaries are mocked; jsdom's existing zero terminal geometry avoids opening terminal sockets. Requests still run through real production HTTP construction. No component/settings/client mocks, polling, findBy, waitFor or sleeps were added. Deferred request signals are registered before actions; the 1500ms timeout is failure-only. Response bodies are cloned per request, matching independent real HTTP responses. React act drains controlled response completion and resulting rendering before assertions.

Three cases establish:

1. Real origin-wide token migration removes the origin-wide credential, makes authenticated local preference requests, then switches through two relay hosts. Distinct 17/19/23 font sizes and themes resolve. A is gated while B resolves, then A finishes late without changing B or global terminal background; returning to A cannot use B's cache.
2. In-place token replacement rejects late old-token responses; new-token requests use the new bearer. Local font size/family subsequently override remote appearance without replacing remote theme.
3. Desktop embedded terminal uses native 22px/theme preferences and issues zero remote preference requests.

Evidence under `docs/evidence/windows-review-20260913/p27-preferences/`:

- `red.log`: initial original-product RED, 2 failed / 1 passed (no authenticated preference signals).
- `fixture-error.log`: first repaired run, 2 failed / 1 passed. Discovered fixture reused one Response body for duplicate refreshes; second JSON read failed. Preserved, not hidden.
- `red-identical.log`: corrected fixture against original library chain (caller context present but ignored), 2 failed / 1 passed.
- **`red-original-identical.log`**: corrected final fixture against the entire original preference chain, including original component caller, 2 failed / 1 passed. Only this child's changes were temporarily removed; wheel changes stayed intact.
- **`green.log`**: identical corrected fixture and exact command on restored repair, **3 passed / 3**, exit 0.
- `red-source.sha256` and `green-source.sha256`: SHA-256 of all four production files and contract at authoritative RED/GREEN. Contract hash in both: `210b0b004328779fff3bd90d9eb6907827875682b1e6b0a50bc3cc16f49de20f`.
- `related.log`: related suite command below, 122 passed / 3 failed. Five files entirely pass: settings 15, client 3, terminal contract 58, mobile 8, gestures 10.
- `tauri-baseline.log`: original preference production chain also fails the same three unrelated Tauri payload assertions, 28 passed / 3 failed. They are not repaired here.
- `typecheck.log`: project `tsc --noEmit` reports only four errors in unchanged `src/lib/nativeMenu.test.ts` (TS7023/TS2456/TS2502/TS2554). No build executed. An initial incorrectly formed Bun x invocation failed before running tsc; corrected command used the installed binary.

Related command:

```sh
bun run --cwd ui test src/lib/terminalSettings.test.tsx src/lib/remoteClient.test.ts src/lib/tauri.test.ts src/remote/RemoteTerminal.contract.test.tsx src/remote/RemoteTerminal.mobile.test.tsx src/remote/RemoteTerminalGestures.test.tsx
```

The three baseline failures are `publishes focused terminal payload to native IPC`, `publishes focused terminal with optional activityState and agentType on terminal tabs`, and `names the argument the Rust command actually binds, so a test notification is really sent`. Raw expected/received payload differences are in both logs.

Execution: Darwin arm64, Bun 1.4.0 at `/Users/indo/.bun/bin/bun`, SHA-256 `539598c775882420b9d8deb7dc14d845f20f7d26f5600c50ab067dde6ac3f3bf`; Vitest 3.2.7, maxWorkers=1. No Windows browser or gateway binary was executed or identified by this task.

## Diagnostics and cleanup

LSP returned no diagnostics on the three library production files; RemoteTerminal has no errors/warnings and retains the pre-existing deprecated keyCode hint. Initial contract diagnostics were clean. Fresh contract diagnostics after the one-line Response.clone fixture fix timed out twice; project tsc subsequently reported no errors in any changed file, but is globally blocked by the unrelated nativeMenu test errors above. Thus a fully clean project validator is not claimed. `git diff --check` passes.

All tests unmount components, reset host store/settings caches, clear test storage/history/CSS, reset runtime mocks, and restore global transport spies. No network calls, persistent services, real credentials, native surfaces or browser profiles were created. Temporary repair patch was deleted. Evidence logs are intentionally retained. All foreign changes survive; only the four allocated production paths, the new contract, and this evidence are owned here. Work remains uncommitted in the shared tree and vulnerable to concurrent writers.

## Pending acceptance

Lead combined build and real Windows browser reload/host-switch checks remain pending. Use an isolated browser profile with no local override, distinct selected-host preferences, and exact UI bundle/gateway binary provenance. Capture actual preference URL/prefix and bearer ownership, resolved numeric/theme values, late-response isolation and host switching. Actual font pixels, font availability, Windows HTTP behavior and native input are not certified by this jsdom contract.
