# Ferryx Built-in Browser — Full Code Review (2026-09-07)

**Scope:** every source file implementing the built-in browser feature. Read-only review; **no source file was modified.**

**Reviewed files**

Backend (Rust):
- `src-tauri/src/browser/{mod,model,manager,security,guest,cookies,download,find,tests}.rs`
- `src-tauri/src/ipc/browser.rs` (1604 lines), `src-tauri/src/ipc/browser_cli.rs` (755 lines)
- `src-tauri/src/ipc/error.rs` (BrowserError → IpcErrorCode mapping), `src-tauri/src/main.rs` (`ferryx browser` CLI), `src-tauri/src/lib.rs` (registration), `src-tauri/src/session/mod.rs` (persisted browser tab state)

Frontend (TypeScript/React):
- `ui/src/components/{BrowserPane,BrowserToolbar,BrowserDuplicateControl,TerminalLinkActions}.tsx`, `ui/src/components/settings/BrowserSection.tsx`
- `ui/src/lib/{browserTauri,browserSettings,browserHistory,linkRouting}.ts`
- `ui/src/state/{browserSessionHydration,workspaceStore,layout,tabPaneDrop}.ts`, `ui/src/lib/sessionPersistence.ts`
- `ui/src/components/TerminalSplitView.tsx` (browser leaf rendering), `ui/src/App.tsx` (shortcut/open-request wiring)

---

## Findings

### F1 — HIGH: "Open links in built-in browser" never works; every terminal link goes to the system browser

`ui/src/lib/linkRouting.ts:31-38` holds a module-level `builtInBrowserOpener` that is only ever populated by `registerBuiltInBrowserLinkOpener`. That function has **no production caller** — the only call sites are `ui/src/lib/linkRouting.test.ts` and `ui/src/components/NativeTerminalPane.test.tsx:3065`.

Consequences in `routeHttpLink` (`linkRouting.ts:40-63`):
- The default branch `settings.openLinksInBuiltInBrowser && builtInBrowserOpener ? "builtin" : "external"` always evaluates to `"external"`, so the "Open links in built-in browser" switch in `BrowserSection.tsx` is inert.
- Worse, the explicit choice is silently overridden: `TerminalLinkActions.tsx:23` calls `routeHttpLink(url, { destination: "builtin" })` when the user presses **"Built-in Browser"** in the chooser toast, and `linkRouting.ts:52-57` then falls through `if (!builtInBrowserOpener) { await openExternalUrl(...); return "external"; }`. The user asks for the built-in browser and gets Safari/Chrome, with no error surfaced (`TerminalLinkActions.tsx:24-26` swallows failures).

The tests pass because each one registers an opener itself, so the missing production registration is invisible to the suite. A real fix registers the opener where `createBrowserTab` is available (`App.tsx`, near the `onBrowserOpenRequested` effect at `App.tsx:1813-1828`), plus one test that asserts routing lands on a built-in tab **without** the test registering the opener.

### F2 — HIGH: browser sessions and native webviews leak when a browser pane is closed inside a split

`cmd_browser_close` is the only thing that removes a manager session and closes the child webview (`ipc/browser.rs:1475-1487`). The frontend calls it from exactly two places (`workspaceStore.ts:915` in `closeTab`, and `workspaceStore.ts:1105` for the project-switch rollback).

A browser can also live as a **pane leaf**, not just a whole tab: `layout.ts:632-657` (`createBrowserPaneContent` for restored/dropped content), `layout.ts:669-681` (`defaultContentForTab`), and `tabPaneDrop.ts:46-65` (dragging a browser tab into another tab's split). `TerminalSplitView.tsx:1189-1213` renders those leaves as `BrowserPane`.

`closePane` (`workspaceStore.ts:935-953`) only tears down terminal sessions — it looks up `tabLayout.sessionIdsByLeafId[leafId]` and calls `closeBackendSession`; browser leaves carry `sessionIdsByLeafId[leafId] = ""` (`layout.ts:742`, `tabPaneDrop.ts:63`) so nothing is closed. The reducer path is worse: `workspaceStore.ts:1742` bails with `if (!tab || tab.kind === "browser" || ...) return state`, which guards on the **tab** kind, not the leaf content kind, and `layout.ts:494-495` just deletes `contentsByLeafId[leafId]` from state.

Net effect: closing a browser pane inside a split drops the only reference to `browserId` while the manager entry and the native child webview stay alive for the process lifetime — an invisible webview still holding its bounds, cookies, and profile. `cmd_browser_list` (and therefore Settings → "Active Browser Tabs" and `ferryx browser list`) keeps reporting the orphan, and the CLI can still snapshot and drive it.

### F3 — MEDIUM: the guest bridge hijacks three real `https://*.ferryx.invalid` hostnames from page content

`browser/guest.rs:8-10` routes control messages by navigating the guest page to `https://open.ferryx.invalid/?url=...`, `download.ferryx.invalid`, and `shortcut.ferryx.invalid`, and `parse_browser_guest_action` (`guest.rs:22-30`) matches purely on `url.host_str()` with no marker that the navigation came from the injected bridge.

Any page can therefore forge a control message with plain HTML — `<a href="https://open.ferryx.invalid/?url=https://evil.example">`, an iframe, a redirect, or a `fetch`-driven `location.assign`. The payload is constrained (`Open`/`Download` re-validate through `validate_url`, so only http/https reaches a new tab), so this is not arbitrary code execution; the concrete abuse is unsolicited tab spawning, a spoofed download prompt for an attacker-chosen URL, and synthesized shortcut actions (`back`/`forward`/`reload`/`focus-address`/`find`) delivered to the UI. A nonce minted per webview and required in the query would close it.

Secondary issue in the same script: `guest.rs:47-50` uses `location.assign(target)` for control messages, which **replaces the page's history entry chain** and is observable to the page (the `on_navigation` handler returns `false`, so the navigation is cancelled, but the page's own `beforeunload`/navigation observers still fire).

### F4 — MEDIUM: `validate_url` accepts credentials and unbounded URLs, and the manager trusts caller-supplied `browserId`

- `browser/security.rs:70-84`: `validate_url` allows `http(s)://user:password@host/`. Those credentials then flow into `BrowserState.url`, are emitted on `browser_state_changed`, shown in the toolbar address bar, and written to `localStorage` history (`browserHistory.ts:78-98`). There is no length bound either, so a multi-megabyte URL is accepted and stored.
- `manager.rs:95-101`: a caller-provided `browser_id` is accepted with only `!is_empty() && len() <= 256`. Unlike `BrowserProfileId` (`model.rs:57-65`, strict charset) it permits any bytes including newlines and path separators. It is not used for filesystem paths today, but `webview_label` derives from a fresh UUID rather than this id, so tightening it costs nothing.

### F5 — MEDIUM: `cmd_browser_create` returns success while the webview creation silently fails

`ipc/browser.rs:848-1042`: the session is registered in the manager first, then the actual webview is built inside `main_window.run_on_main_thread(move || ...)` whose result is discarded (`let _ = main_window.run_on_main_thread`), and inside the closure `if let Ok(child) = window_clone.add_child(...)` swallows the failure. `Ok(state)` is returned regardless.

So a failed `add_child` yields a manager entry with no webview. Every later command degrades quietly rather than reporting: `cmd_browser_navigate` (`browser.rs:1049-1082`) wraps its work in `if let Some(webview) = app.get_webview(...)` and returns `Ok(())` when absent; the same pattern appears in `cmd_browser_reload`, `cmd_browser_set_visible`, `cmd_browser_set_zoom`, and `cmd_browser_focus`. The user sees a browser tab that never loads and never errors. Note the inconsistency: `cmd_browser_set_bounds`, `cmd_browser_find`, and `cmd_browser_clear_find` **do** return `WebviewNotFound` — which is why `setBrowserBounds` needs its 5-attempt retry loop (`browserTauri.ts:126-142`).

### F6 — MEDIUM: back/forward history is modelled twice and diverges from WebKit

`manager.rs` keeps its own `history: Vec<String>` + `history_index` and derives `can_go_back`/`can_go_forward` from it (`sync_history_flags`, `push_history_url`). On macOS the same flags are then overwritten from the real `WKWebView` (`browser.rs:820-836`, `update_webview_state`), and `update_navigation_state` explicitly ignores its own `_can_go_back`/`_can_go_forward` parameters (`manager.rs:340-347`) before calling `sync_history_flags` again at the end (`manager.rs:381`) — so the vector-derived value wins on the non-macOS path and races the native value on macOS.

The shadow history is also wrong by construction: in-page navigations that WebKit records (fragment changes, `pushState`) reach `push_history_url` only if they surface as a page-load URL change, and `cancel_history_navigation` (`manager.rs:246-268`) reconstructs the index by ±1, which cannot represent a WebKit history that diverged. On non-macOS, `history_navigation` (`browser.rs:1180-1194`) fires `history.back()` via `eval` and treats "the eval dispatched" as success, so the buttons stay enabled/disabled per the shadow model regardless of what the page actually did. WebKit's `canGoBack`/`canGoForward` should be the single source of truth.

### F7 — LOW/MEDIUM: find-in-page match count is a plain-text substring scan, not the real find result

`browser/find.rs:3-27`: `matchCount` comes from lowercased `document.body.innerText` `indexOf` scanning, while highlighting comes from `window.find`. The two disagree routinely — text inside `<input>`/`<textarea>` values, `::before`/`::after` content, shadow DOM, and collapsed whitespace differ between `innerText` and what `window.find` traverses. The UI presents this number as authoritative (`BrowserPane.tsx:279`, "N matches"), and the count ignores `backwards` entirely, so it is really "occurrences in body text", not "matches found". Also `window.find` is non-standard and its `wrapAround` argument is the 4th positional parameter (passed `true` here) with no cross-engine guarantee.

### F8 — LOW: browsing history is written even for the `private` profile

`BrowserPane.tsx:120-127` calls `recordBrowserHistory` on every settled load, keyed only on `rememberBrowsingHistory` and `isHttpUrl` (`browserHistory.ts:82-84`). `BrowserStateChangedPayload` (`model.rs:151-162`) carries no `profileId`, so the pane cannot tell a private tab from a normal one at that point (the tab prop does have `profileId`, so the check is available client-side). A tab created with the Private profile — which `ipc/browser.rs:908` correctly builds with `.incognito(true)` — still leaves its URLs and titles in `localStorage` under `ferryx.browser.history`, surviving restart. That contradicts the Settings copy: "Private is ephemeral."

### F9 — LOW: browser CLI has no per-request authentication or size bound, and error codes leak Rust debug formatting

`ipc/browser_cli.rs`:
- Access control is filesystem-only: `0o700` runtime dir + `0o600` socket on Unix (`browser_cli.rs:139-155`). That is reasonable for a same-UID design, but on Windows the fallback is a `127.0.0.1` TCP listener (`browser_cli.rs:196-206`) whose port is written to a plain file — **any local process of any user on the machine can connect and drive automation** (snapshot page content, click, fill, keypress) with no token. The port file itself is not permission-restricted.
- `handle_connection` (`browser_cli.rs:246-283`) does `reader.read_line(&mut line)` with no cap, so one connection can grow a `String` without bound; there is also no read/idle timeout, so an open connection with no newline pins a task indefinitely.
- `execute_request` (`browser_cli.rs:285-315`) emits `code: format!("{:?}", error.code)`, i.e. the Rust enum `Debug` name (`BrowserNotFound`), while the IPC layer's own wire format is the screaming-snake string. That makes CLI codes diverge from IPC codes for the same failure and pins them to an internal identifier.

### F10 — LOW: several correctness and consistency nits

- **Zoom range disagreement.** The manager clamps to `0.25..5.0` (`manager.rs:317`), the toolbar buttons clamp to `0.5..2.0` (`BrowserToolbar.tsx:222,231`), and Settings offers `75..200` (`browserSettings.ts:44`). A restored 250% zoom is accepted by the backend but unreachable and unrepresentable in the UI.
- **`update_navigation_state` bumps `generation` on URL change and clears automation targets** (`manager.rs:359-365`) while `begin_reload`/`update_url` also bump it — a single navigation can advance the generation twice, invalidating a CLI snapshot the agent just took, surfacing as `AutomationSnapshotStale` with no explanation.
- **`has_sessions()`** (`manager.rs:390`) has no non-test caller — dead API.
- **`cmd_browser_open_external`** (`browser.rs:1497-1520`) discards the spawn result on all three platforms (`let _ = ...spawn()`), so "Open in system browser" cannot report failure. Windows goes through `cmd /C start <url>`, which is the one path where a crafted URL string interacts with `cmd` parsing; `validate_url` normalizing through `Url` makes this safe today, but `ShellExecute`/the `opener` plugin would remove the class of risk.
- **Download prompt has no size/type limit or progress.** `download_url_to_path` (`download.rs:7-45`) streams an unbounded body to the chosen path with no progress reporting and no cancellation; `BrowserPane.tsx:311-317` shows only "Saving…"/"Saved".
- **Cookie import silently targets whichever webviews happen to be open** and, when none match the Default profile, falls back to the **main app webview** (`browser.rs:1258-1266`), i.e. imported third-party cookies are set on Ferryx's own UI webview context.
- **`BrowserToolbar` `useEffect` suppresses `react-hooks/exhaustive-deps`** (`BrowserToolbar.tsx:174`) while depending on `handleGoBack`/`handleGoForward` closures; the listed `tab.canGoBack`/`tab.canGoForward` deps are what keep it fresh, which is fragile but currently correct.
- **Two storage keys for one setting.** `rememberBrowsingHistory` lives both in the settings blob and in a separate `ferryx.browser.history.enabled` override key (`browserSettings.ts:224-231, 246-259`), with the override winning on read. It works, but it is a second source of truth for one boolean.

---

## What is done well

- **URL scheme allow-listing** is tight and well tested: `validate_url` permits only `http`, `https`, and `about:blank`, with `tests.rs:31-46` explicitly rejecting `file://`, `javascript:`, `tauri://`, and `asset://`.
- **All injected scripts JSON-encode untrusted values.** `find.rs:4`, `browser.rs:automation_script` for both selector and fill value, with regression tests naming the injection they prevent (`browser.rs` `fill_script_encodes_untrusted_values_as_json_strings`, `find.rs` `find_script_json_encodes_query_and_direction`).
- **Automation generation fencing** (`manager.rs:275-311`) makes a stale snapshot reference fail loudly instead of clicking the wrong element after a navigation.
- **The find-race fix in `BrowserPane`** (`findRequestSeqRef`, `BrowserPane.tsx:76-111,225-243`) correctly invalidates in-flight responses on query supersession, browser change, and bar close — and the reasoning is documented in-line.
- **`enqueueBrowserLifecycle`** (`browserTauri.ts:39-56`) serializes `set_visible`/`close` per browser id, which is what prevents a hide/show race from stranding a webview visible over app chrome.
- **The native-surface occlusion handling** (`BrowserPane.tsx:80-84` `maskAwareVisible`, and `browserPanesVisible: activeDrag === null` at `TerminalSplitView.tsx:488`) matches the documented macOS child-webview ordering constraint rather than fighting it with z-index.
- **Cookie parsing** handles both JSON and Netscape formats with `#HttpOnly_` support and per-line error messages (`cookies.rs:97-146`), and rejects malformed input instead of importing partially.
- **Port-file handling** refuses to overwrite a symlink or directory (`browser_cli.rs:57-70`) and validates the parsed port including the `0` case, with tests for each malformed variant.
- **Profile capability gating is honest about the platform.** macOS named profiles are rejected at the IPC boundary (`browser.rs:861-871`) and hidden in the UI (`browserSettings.ts:88-105`), with Settings copy explaining why.

---

## Suggested priority

1. **F1** — the built-in browser is the feature's headline behavior and it is unreachable from terminal links; the explicit "Built-in Browser" button lies.
2. **F2** — unbounded native webview leak with no user-visible cause; also makes `ferryx browser list` and Settings report phantom tabs.
3. **F3** — page-forgeable control channel; a nonce is a small, contained change.
4. **F5/F6** — silent-failure and dual-source-of-truth defects that make every downstream navigation bug hard to diagnose.
5. **F9** — before Windows ships this CLI surface, the loopback TCP listener needs a token; the unbounded `read_line` should get a cap regardless.
6. **F4/F7/F8/F10** — hardening and consistency.

## Verification performed

Review only, as instructed: **no file under `src-tauri/` or `ui/src/` was modified**, and no build or test command was run. Every finding cites the file and line range it was read from. F1 and F2 were confirmed by exhaustive reference search (`registerBuiltInBrowserLinkOpener` → test files only; `closeBrowser`/`cmd_browser_close` → `closeTab` and the project-switch rollback only, with `closePane` and the `CLOSE_PANE` reducer both bypassing it).

## Pre-existing state note

The working tree is dirty with ~45 modified files from other concurrent sessions (notifications, SSH, native terminal, DAG). None of them are browser-feature files, and this review touched none of them. This report is uncommitted.
