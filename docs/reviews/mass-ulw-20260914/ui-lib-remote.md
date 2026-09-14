# UI lib review: remote / SSH / pairing / updater

Scope reviewed: `ui/src/lib/remoteClient.ts`, `ui/src/lib/pairClient.ts`, `ui/src/lib/pushSubscription.ts`, `ui/src/lib/sshHosts.ts`, `ui/src/lib/sshRecovery.ts`, `ui/src/lib/updater.ts`, `ui/src/lib/ipcErrors.ts`. Call sites were confirmed with `rg` and narrow line reads in `ui/src/lib/tauri.ts`, `ui/src/state/remoteHostStore.ts`, `ui/src/main.tsx`, `ui/public/sw.js`, `src-tauri/tauri.conf.json`, and the read-only `ui/src/remote/RemoteApp.tsx`.

---

### [P1] Browser-mode remote fallbacks are gated on an unscoped token that the remote app deletes
- Location: ui/src/lib/remoteClient.ts:10
- Observed: `getRemoteAuthToken()` with no `hostId` reads only the unscoped keys - `return localStorage.getItem(TOKEN_KEY) ?? localStorage.getItem(LEGACY_TOKEN_KEY);` (remoteClient.ts:11-12). The remote app stores credentials host-scoped and actively removes the unscoped copy: `setRemoteAuthToken(legacy, hostId); clearRemoteAuthToken();` (ui/src/remote/RemoteApp.tsx:302-303), and `remoteHostStore` keeps tokens per host under `ferryx_remote_token_${hostId}` (ui/src/state/remoteHostStore.ts:44-46). Meanwhile the browser-mode fallbacks in `tauri.ts` branch on the *unscoped* getter: `if (getRemoteAuthToken()) { return defaultRemoteClient.listWorktrees(workspaceId); } return [] as Worktree[];` (ui/src/lib/tauri.ts:262-266) and `if (remote || getRemoteAuthToken())` (ui/src/lib/tauri.ts:191).
- Why it is wrong: once a user has paired (or once the one-time legacy migration has run), the unscoped key no longer exists, so every one of these guards is false. `listWorktrees` silently returns an empty array instead of the machine's worktrees, and `getTerminalPreferences` silently returns hard-coded defaults instead of the user's font/theme settings. The failure is silent - no error, no retry, just missing data in the browser client.
- Minimal fix: resolve the active host's token rather than the unscoped key in these guards, e.g. have `getRemoteAuthToken()` fall back to `remoteHostStore.getState()` active host's `deviceToken` before reading the unscoped legacy key.

### [P1] `connectEvents` ignores the instance's `baseUrl` and `token`, so a per-host client can never stream events
- Location: ui/src/lib/remoteClient.ts:119
- Observed: the constructor accepts `baseUrl` and `private readonly token`, and `fetchJson` honours both (`this.token ?? getRemoteAuthToken()`, remoteClient.ts:50). `connectEvents` does not: `if (this.ws || !getRemoteAuthToken()) return;` (line 119), `const token = getRemoteAuthToken();` (line 121), and both the ticket request and the socket URL are pinned to the page origin - `await fetch(`${window.location.origin}/api/v1/socket-ticket`, ...)` (line 126) and `wsUrl = `${protocol}//${window.location.host}/api/v1/events?ticket=...`` (line 134).
- Why it is wrong: a client constructed for a specific host (`new RemoteClient(remote.baseUrl, remote.token)`, ui/src/lib/tauri.ts:193) can issue REST calls to that host but will mint its socket ticket against the page origin with a different host's token - and when the unscoped token is absent (see the finding above) it returns at line 119 and opens nothing at all. The event stream silently never delivers; the UI just looks frozen.
- Minimal fix: use `this.token ?? getRemoteAuthToken()` for the guard and the ticket request, and build both the ticket URL and `wsUrl` from `this.baseUrl` (upgrading `http`->`ws`, `https`->`wss`) instead of `window.location`.

### [P1] A single failed socket-ticket request kills the event stream permanently - no retry
- Location: ui/src/lib/remoteClient.ts:136
- Observed: the ticket-minting block ends with `} catch (error) { console.error("Failed to open the remote event stream:", error); return; }` (lines 135-137). No `WebSocket` is created, so `onclose` (lines 155-166, the only place that schedules `this.reconnectTimer`) never fires, and `this.ws` stays `null` with no timer armed.
- Why it is wrong: the backoff-with-jitter reconnect machinery only covers sockets that were successfully opened. Any transient failure before that - gateway 502, daemon restarting, brief offline blip, a 401 during token rotation - leaves the client permanently event-less until the page is reloaded. The user sees a UI that stops updating with no error surfaced beyond a console line.
- Minimal fix: in the `catch`, schedule the same backoff retry used by `onclose` (increment `reconnectAttempts` and `setTimeout(() => this.connectEvents(), backoffMs + jitterMs)`) instead of bare `return`.

### [P2] `listen()` teardown leaks the socket and the reconnect chain; concurrent `listen()` calls can open duplicate sockets
- Location: ui/src/lib/remoteClient.ts:181
- Observed: the unsubscribe closure is only `return () => { listeners.delete(handler); };` (lines 181-183). It never closes `this.ws`, never clears `this.reconnectTimer`, and never removes the now-empty `Set` from `this.eventListeners`. `listen` also calls `this.connectEvents()` unconditionally (line 179), and `connectEvents` guards only on `this.ws` being non-null (line 119) - during the backoff window `this.ws` is `null` while a timer is already pending.
- Why it is wrong: after the last listener unsubscribes the WebSocket stays open and, on every server-side close, reschedules itself forever - a permanently reconnecting socket per abandoned consumer. And a `listen()` call that lands inside a backoff window starts a second connection, so both sockets' `onclose` handlers spawn independent reconnect chains that multiply on each outage.
- Minimal fix: in the unsubscribe closure, when the event's `Set` is empty, delete the map entry and - if no event has listeners - `clearTimeout(this.reconnectTimer)` and `this.ws?.close()`. Track a `connecting` flag (or the pending timer) so `connectEvents` bails when a reconnect is already scheduled.

### [P2] Remote HTTP failures are flattened into a prose string, discarding the structured `{code, message, details}` contract
- Location: ui/src/lib/remoteClient.ts:67
- Observed: every non-2xx response becomes `throw new Error(`Remote API error ${res.status}: ${await res.text()}`);`. The rest of the codebase relies on the structured contract instead: `toIpcError` / `isStructuredIpcError` (ui/src/lib/tauri.ts:736-756) and `worktreeErrorMessage(error: StructuredIpcError)` which switches on `error.code` (ui/src/lib/ipcErrors.ts:4-13).
- Why it is wrong: remote-mode errors cannot be routed through `worktreeErrorMessage`; a `DIRTY_WORKTREE` rejection from `createWorktree`/`deleteWorktree` (remoteClient.ts:91, 102) arrives as the opaque string `Remote API error 409: {...}`. Callers that want the friendly wording have only two options, both bad: show raw JSON to the user, or substring-match the message - exactly the fragile pattern the structured contract exists to prevent.
- Minimal fix: parse the body as JSON and, when it satisfies `isStructuredIpcError`, throw that object (or an `Error` carrying it) so remote errors flow through `toIpcError`/`worktreeErrorMessage` like local IPC errors; keep the string form only as the unparseable-body fallback.

### [P2] Updater never closes the `Update` resource it obtains from `check()`
- Location: ui/src/lib/updater.ts:97
- Observed: `const update = await check();` (line 91) then `pendingUpdate = update;` (line 97). On the no-update and error paths the handle is dropped with `pendingUpdate = null;` (lines 93, 104). `Update` is a `Resource` subclass with an explicit `close()` (`@tauri-apps/plugin-updater` `dist-js/index.d.ts:65,95`), and `rg '\.close\(\)' ui/src` finds no updater call site anywhere in the UI.
- Why it is wrong: each `check()` that yields an update allocates a Rust-side resource that is never released. `checkForUpdate` re-runs on every poll tick while `status.state` is `"idle"` or `"error"` (line 84, `UPDATE_CHECK_INTERVAL_MS` = 1h, lines 146-172), so a host that keeps failing mid-download accumulates one leaked resource per hour for the lifetime of the app process.
- Minimal fix: `await pendingUpdate?.close()` before reassigning or nulling `pendingUpdate` (lines 93, 97, 104) and after `downloadAndInstall` completes.

### [P2] Push registration falls back to a service worker path that does not exist, and the shipped worker has no `push` handler
- Location: ui/src/lib/pushSubscription.ts:33
- Observed: the fallback registration is `() => navigator.serviceWorker.register("/service-worker.js")`. The shipped worker is `ui/public/sw.js` and that is what the app actually registers: `navigator.serviceWorker.register("/sw.js")` (ui/src/main.tsx:66). `ls ui/public/service-worker.js` -> no such file. `grep addEventListener ui/public/sw.js` lists only `install`, `activate`, and `fetch` - there is no `push` or `notificationclick` listener.
- Why it is wrong: if `navigator.serviceWorker.ready` ever rejects, the fallback registers a 404 and the whole function silently resolves `false`. Even on the happy path the subscription is created against a worker that cannot display anything, so `userVisibleOnly: true` subscriptions receive pushes and show nothing - browsers penalise this by surfacing a generic "site updated in background" notification and may drop the subscription. (Impact today is bounded: `rg` finds no non-test caller of `registerPushSubscription`, so this is a latent break rather than an active one.)
- Minimal fix: change the fallback path to `"/sw.js"` and add `push` / `notificationclick` handlers to `ui/public/sw.js` before this function is wired to a caller.

### [P3] `RemoteClient.spawnTerminal` fabricates a session id without contacting the server
- Location: ui/src/lib/remoteClient.ts:115
- Observed: the whole body is a comment plus `return { sessionId: `remote-${Date.now()}` };`; the `_request` argument is unused.
- Why it is wrong: any caller that trusts this id will address a session that does not exist on the daemon. It is harmless today only because `rg 'spawnTerminal'` shows no route from `ui/src/lib/tauri.ts` into this method, but it is a live trap for the next remote-mode change.
- Minimal fix: delete the method, or make it `throw new Error("spawnTerminal is not supported in remote mode")` until a real `/api/v1` spawn route exists.

### [P3] `resetSshHostsCache()` clears the global listener set, permanently deafening any mounted `useSshHosts` hook
- Location: ui/src/lib/sshHosts.ts:103
- Observed: `resetSshHostsCache` nulls the cache and the in-flight promise, then calls `listeners.clear();`. `useSshHosts` registers through `subscribeSshHosts` on mount and only re-subscribes on unmount/remount (sshHosts.ts:253-267).
- Why it is wrong: a reset drops subscriptions belonging to components that are still mounted, so they never receive another inventory update and silently show stale hosts. `rg` confirms the only current callers are tests, which is why this is P3 and not higher - but the export is public API with no such caveat in its name.
- Minimal fix: drop `listeners.clear()` from `resetSshHostsCache` (listeners already remove themselves via the returned unsubscribe), or move the clearing into a separate test-only helper.

---

## Notes on focus areas with no finding

- **Regex/substring matching on backend IPC error strings**: `ipcErrors.ts` switches on `error.code` (lines 4-12) and only falls through to `error.message` in `default`, which is correct. `sshHosts.extractIpcErrorMessage` (lines 106-121) reads the `message` field off the serialized error object rather than pattern-matching text. The only contract violation found is the remote HTTP path above (remoteClient.ts:67).
- **Updater manifest validation**: signature and transport are enforced by plugin configuration, not UI code - `"endpoints": ["https://github.com/.../latest.json"]` and a minisign `"pubkey"` are set in `src-tauri/tauri.conf.json:34-38`, and `updater.ts` only consumes the already-verified `Update` object. No missing validation in the reviewed module.
- **Backoff**: `remoteClient.ts:159-165` implements capped exponential backoff with jitter and resets `reconnectAttempts` in `onopen`; `sshRecovery.ts` is event-driven with an explicit `stopped` guard on every async continuation (lines 21-50). The gaps are the missing pre-socket retry and the missing teardown, reported above.

## Summary

P0: 0, P1: 3, P2: 4, P3: 2
