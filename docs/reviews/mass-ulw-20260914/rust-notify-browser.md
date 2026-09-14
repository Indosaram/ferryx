# Rust review: notifications, embedded browser, permissions

Scope: `src-tauri/src/notification/`, `src-tauri/src/browser/`, `src-tauri/src/permissions/`, plus the two
consumers needed to prove impact (`src-tauri/src/ipc/browser.rs`, `ui/src/App.tsx`). Only findings whose
decisive lines were opened and read are listed.

### [P0] Guest bridge accepts synthetic (page-generated) events, so any page in the embedded browser can drive privileged app actions

- Location: `src-tauri/src/browser/guest.rs:114` (and the click path at `src-tauri/src/browser/guest.rs:91`)
- Observed: the injected bridge installs capture-phase document listeners with no trust check on the event:
  `addDocumentListener('keydown', (event) => {` ... and, after mapping a chord to an action, unconditionally
  emits the control URL `route('shortcut.ferryx.invalid', 'action', action);` (`guest.rs:189`).
  `route` builds `https://${host}/?${key}=${enc(value)}&nonce=${...}` (`guest.rs:76-78`) using the real bridge
  nonce, so `parse_browser_guest_action` (`guest.rs:18-28`) accepts it, and `cmd_browser_create`'s
  `on_navigation` hook emits `BROWSER_SHORTCUT_REQUESTED_EVENT` to the app
  (`src-tauri/src/ipc/browser.rs:936,969-975`). `ui/src/App.tsx:2353` subscribes and executes the action:
  `handleAddTerminalTab();` (`App.tsx:2365`), `handleCloseActiveSurface();` (`App.tsx:2369`),
  `setIsCommandPaletteOpen(true);` (`App.tsx:2373`), sidebar/settings/split handlers below it. The only guard
  is `if (activeRemoteHostRef.current) return;` (`App.tsx:2354`) - there is no focus or visibility check.
  `rg -n isTrusted src-tauri/src ui/src` returns exactly one hit, in `ui/src/lib/shortcutDiagnostics.ts`; the
  bridge never inspects `event.isTrusted`.
- Why it is wrong: `document.dispatchEvent(new KeyboardEvent('keydown', { key: 't', metaKey: true }))` from
  ordinary page script fires the bridge's capture listener exactly like a real keystroke. A hostile or merely
  ad-injected page - including one sitting in a *hidden background* browser tab - can, with no user
  interaction, spawn terminal tabs in a loop (`tab-new-terminal` starts real shell processes), close the
  user's active surface and kill its running session (`tab-close`), pop the command palette and settings, and
  split panes. The same hole exists on the click path: a synthetic `MouseEvent` on a crafted
  `<a target="_blank">` routes `open.ferryx.invalid` and opens attacker URLs as new app tabs. The nonce design
  documented at `guest.rs:52-60` defends only against a page navigating directly to the control hosts; it does
  nothing here, because the bridge itself supplies the nonce on behalf of the forged event. This is untrusted
  web content reaching privileged app chrome.
- Minimal fix: bail out on untrusted events at the top of each bridge listener -
  `if (!event.isTrusted) return;` in the `keydown`, `click`, and `drop` handlers in `BRIDGE_SCRIPT_TEMPLATE`
  (`guest.rs:91`, `:114`, `:192`). Capture `const isTrustedOf = Object.getOwnPropertyDescriptor(Event.prototype, 'isTrusted').get`
  at document start alongside the other pristine builtins so a page cannot shadow the property.

### [P1] Cookie import silently targets the app's own UI webview when no Default-profile tab is open

- Location: `src-tauri/src/ipc/browser.rs:1313`
- Observed: after parsing the user's cookie file, the command collects target webviews by profile and then
  falls back to Ferryx's own window:
  `if targets.is_empty() && matches!(profile_id, BrowserProfileId::Default) { if let Some(main_webview) = app.get_webview("main") { targets.push(main_webview); } }`
  followed by `target.set_cookie(cookie.clone())` for every parsed cookie (`ipc/browser.rs:1327-1331`). There
  is no domain or origin restriction on the imported cookies (`browser/cookies.rs:170-186` copies
  name/value/domain/path/expiry straight from the file).
- Why it is wrong: the user asked to import cookies into the *Default browser profile*. With no Default
  browser tab open - the common case, since import lives in Settings - every cookie from an arbitrary
  third-party export file is instead injected into the cookie store of Ferryx's privileged application
  webview. The operation reports `imported_count` as success, so the user believes the cookies landed in the
  browser profile; meanwhile the app's own webview context now carries attacker- or vendor-controlled session
  cookies, and nothing in the UI shows or lets them clear that.
- Minimal fix: delete the `"main"` fallback and let the existing error path at `ipc/browser.rs:1319-1324`
  ("open a browser tab using the ... profile before importing cookies") handle the empty-target case for the
  Default profile too.

### [P2] Notification sound dedupe is a check-then-set race, so the double-alert it exists to prevent still fires

- Location: `src-tauri/src/notification/audio.rs:63`
- Observed: `play` opens with `if !force && self.is_deduped() { return PlaySoundResult::failed(PlaySoundReason::Deduped); }`
  and only records the timestamp at the very end via `self.mark_played()` (`audio.rs:126`), after path
  validation, `File::open`, and `Decoder::new` have run. `is_deduped` and `mark_played` take the
  `last_played_at` lock separately (`audio.rs:133-142`). The command entry point runs playback on the blocking
  pool - `run_blocking(move || Ok(player.play(&path, volume, force))).await`
  (`src-tauri/src/ipc/notifications.rs:174`) - so two dispatches can be in `play` concurrently.
- Why it is wrong: an agent-completion notification and an adjacent terminal bell arriving within the same
  decode window both observe an empty/stale `last_played_at`, both pass the check, and both reach
  `player.detach()`. The user hears the stacked double alert that `DEDUPE_WINDOW` (`audio.rs:24`) is documented
  to collapse.
- Minimal fix: make the claim atomic - take the `last_played_at` lock once at the top and, inside that same
  guard, compare `elapsed()` against `DEDUPE_WINDOW` and write `Some(Instant::now())` before releasing it;
  drop the trailing `mark_played()` calls.

### [P2] `all_granted` can never be true on Windows or Linux

- Location: `src-tauri/src/permissions/mod.rs:208`
- Observed: for any non-authoritative provider (every non-macOS build, see
  `permissions/mod.rs:195` onward), `notification_item` returns
  `status: PermissionStatus::Unknown, granted: false, can_request: false` (`permissions/mod.rs:208-211`).
  `get_system_permissions_status` then computes `let all_granted = notif_granted;`
  (`permissions/mod.rs:239`), where `notif_granted = notifications.granted` (`permissions/mod.rs:235`).
- Why it is wrong: on Windows and Linux the aggregate permissions status is hardwired to `false` forever, with
  `can_request: false` so the user has no action that can ever flip it - even on a Linux desktop where
  notifications are working and require no per-app grant. Any UI or onboarding surface keyed on `allGranted`
  presents a permanently unresolvable warning state on those platforms.
- Minimal fix: on non-authoritative platforms treat "unknown" as non-blocking for the aggregate, e.g. compute
  `all_granted` from `notifications.status != PermissionStatus::Denied` instead of `notifications.granted` in
  the `#[cfg(not(target_os = "macos"))]` branch at `permissions/mod.rs:239`.

## Checked and clean (no finding raised)

- Private-history isolation holds on the persistence path: `ui/src/components/BrowserPane.tsx:125` gates
  `recordBrowserHistory` on `tab.profileId !== PRIVATE_BROWSER_PROFILE.id`, and
  `src-tauri/src/ipc/browser.rs:903,934` builds private tabs with `.incognito(true)`.
- macOS main-thread requirements are respected where they exist: the Dock badge is applied inside
  `app.run_on_main_thread` (`src-tauri/src/ipc/notifications.rs:227`) and `apply_dock_badge_label` still
  re-checks with `MainThreadMarker::new()` (`src-tauri/src/notification/badge.rs:61`); child webview creation
  runs via `main_window.run_on_main_thread` with a bounded completion handshake
  (`src-tauri/src/ipc/browser.rs:925,1130-1145`).
- No shipped panics found in these modules: every `unwrap()`/`expect()` matched by
  `rg -n 'unwrap\(\)|expect\(' src-tauri/src/notification src-tauri/src/browser src-tauri/src/permissions`
  sits inside `#[cfg(test)]` code (e.g. `notification/macos_submission.rs:140` is in its `mod tests`), and the
  ObjC entry points wrap calls in `objc2::exception::catch` with timeouts
  (`notification/permission.rs:157,167,217,230`).
- Orphaned-webview handling on creation race is correct: `keep_or_discard_fresh_webview`
  (`src-tauri/src/ipc/browser.rs:845`) closes the child when the session was already abandoned
  (`ipc/browser.rs:1119`).

## Summary

P0: 1, P1: 1, P2: 2, P3: 0
