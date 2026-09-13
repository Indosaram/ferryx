# P33 / SHARED-NATIVE-01: popup ownership and lifetime

2026-09-13, child st_01a0997e. Local repair delivered; lead combined checks and actual Windows native menu/clipboard acceptance remain pending. Changes are uncommitted in the shared working tree.

## Scope and mechanism

Read root/UI instructions, current status/diff, shared-native-callers.md (including the dependency completion caveat), gap-packet-addendum P33, and latest parent C002 registration before production edits. The absolute-path LSP reference query found exactly three production consumers: TabBar, ProjectHeader in Sidebar, and WorktreeRow in WorktreeList. Initial relative-position LSP reference request failed; corrected request succeeded.

The Rust builder passes spec.id unchanged to both ordinary and submenu MenuItemBuilder/ IconMenuItemBuilder instances. register_menu_event_forwarder emits event.id().0.clone() as the sole payload field. Existing opaque string roundtrip is sufficient: no Rust/protocol edit or additional backend command was needed. src-tauri/src/ipc/native_menu.rs remained read-only (hash below).

The helper now generates a UUID namespace per popup and an ID-to-original-action map for all item entries, including submenu children. Only that popup's mapped action is accepted; unrelated or sibling-popup events do not consume its listener. Ownership is revoked before callback execution, including reentrancy/throw and queued duplicates. Cleanup is idempotent and late invoke completion cannot rearm an already-cleaned popup's timer.

An optional fifth AbortSignal retains the Promise<UnlistenFn> API and existing four-argument mocks. All three actual callers publish an AbortController disposer synchronously before opening; unmount/reopen can therefore cancel while listen or invoke is pending. Late completion no longer assigns an obsolete disposer to a current ref. TabBar clears the old owner before invoking the action, so an action that opens another menu cannot clear the new owner afterward. Opening failures are reported rather than silently swallowed.

The 200ms post-invoke dismissal grace remains unchanged. On the inspected Windows dependency path invoke resolves after native tracking returns, not when it starts; no long-open timeout defect is claimed. The existing bridge-delivery cutoff after dismissal remains a qualified limitation, not a newly repaired latency guarantee. Abort revokes frontend action ownership; it does not forcibly close native OS tracking already in progress.

Only popup-lifetime hunks were changed in TabBar/Sidebar/WorktreeList. Existing P31 qualified sortable registration, drag data, grouping/order, and legacy persistence hunks were retained unchanged. No other worker-owned source/test files were edited. No existing test mock adaptation or additional test-file registration was necessary.

## Exact RED/GREEN

Command, unchanged for every run:

```text
bun run --cwd ui test src/lib/nativeMenu.test.ts
```

Raw output files beside this report:

- p33-native-menu-harness-error.log: initial fixture error, NOT behavioral RED. An inherited fake Navigator failed jsdom's native getter brand check and prevented row context menus. Replaced it with an explicit Windows platform/clipboard boundary fixture and restored globals on every exit.
- p33-native-menu-red.log: original production, 12 discovered cases, 9 failed / 3 passed, exit 1. Exact original two-row failure: received `[[C:\owned\A], [C:\owned\B]]`, expected `[[C:\owned\B]]`. A's native completion had been released while its configured cleanup deadline remained pending; B then selected its actual submitted Copy Worktree Path item ID through the global event bridge. Thus the failure reaches both real row clipboard closures, not a mocked ownership helper.
- p33-native-menu-green.log: final source/tests, 12 passed, exit 0. First production-fixed execution also passed all 12. A test-only circular inferred type found by diagnostics was replaced with an explicit cleanup-record type; identical assertions then passed again in the recorded final run.

Cases retain the real helper and mounted WorktreeRow (createElement in test.ts). Only Tauri bridge and navigator platform/clipboard boundaries are mocked. Cases cover:

1. A dismissal grace overlapping B's same-original-ID copy selection, exactly B once and no A; duplicate event has no effect.
2. Real row unmount during pending listener registration; eventual unlisten once and no invoke.
3. Real row unmount during pending invoke; callback ownership revoked before native completion.
4. Real row reopen during pending registration; old registration cannot invoke or replace new ownership.
5. Real row reopen during pending invoke; obsolete completion cannot replace B's disposer.
6. Unrelated event does not consume the listener; submenu ID maps back to original action and input entries are not mutated.
7. Long-open native tracking remains selectable after 60 seconds of controlled clock advancement.
8. Dismissal cleanup at exactly the unchanged 200ms deadline and repeated disposer calls.
9. Invoke rejection cleans and propagates.
10. Registration rejection propagates without invocation or an acquired unlistener.
11. Callback throw plus queued duplicate releases exactly once, with no timer rearmed by late completion.
12. Already-aborted and non-native no-op controls.

Readiness/completion/clipboard/removal deferreds are created before triggering their corresponding actions. No waitFor/findBy, sleeps, or success polling. Vitest's bounded test timeout only detects missing signals. Fake clock advancement is limited to popup lifetime behavior and final cleanup. afterEach releases all owned registration/invoke gates, unmounts DOM, advances the existing dismissal deadline, asserts zero bridge listeners, exactly one unlisten per acquired subscription and zero timers, then restores clock/mocks/globals. A rejected registration explicitly models no acquired native listener, hence no unlisten is expected.

## Diagnostics and diff review

- nativeMenu.test.ts: final fresh LSP diagnostics clean. Initial circular return inference errors were fixed, not suppressed.
- Sidebar.tsx: clean.
- WorktreeList.tsx: no errors/warnings; existing deprecated navigator.platform hint.
- TabBar.tsx: no errors/warnings; existing deprecated navigator.platform hints.
- nativeMenu.ts: two fresh LSP requests timed out at 3000ms. This file's fresh diagnostics remain unverified; no clean result is claimed.
- Scoped git diff --check passed. Read the final full scoped production diff and confirmed P31 hunks remain present without modification.
- No build or runnable desktop entry was executed, per child scope. Existing component suites and lead combined build remain pending, not implicitly passing.

## Final byte receipts

SHA256 at final verification:

```text
d13fab5438009c2528b6053760ad295b22571d5f71efd4f2f047480b902fc022  ui/src/lib/nativeMenu.ts
9c3e08653966e5e7905f1506b43d235b147bbb4b4d79db4dab0c1ac4cdff5fe1  ui/src/lib/nativeMenu.test.ts
275ce62c037cd6e1091e3b6e07fa207a90295deb004c664bd00618ebe476caa5  ui/src/components/TabBar.tsx
9a0078b152866f0827e8cae309bb91f91ceccf0ad81b04a69a71caa15005a5f2  ui/src/components/Sidebar.tsx
4227892b6c73664f2ede22c06dbb6cc5ab8b5005cc438bc73f65c1360b19c9fb  ui/src/components/WorktreeList.tsx
aff91e57d23538e0e0834c0abd11b1a3cd2dff5edf3d7a29f5ec4923d0ae775d  src-tauri/src/ipc/native_menu.rs
```

## Owned cleanup and remaining acceptance

All test processes exited. No desktop, browser runtime, SSH, daemon, build, native test, ref/worktree, commit or push operation occurred. Test-owned DOM/listeners/timers/globals were cleaned as asserted above. Durable raw logs and this report intentionally remain; no temporary config or fixture paths were created. Foreign dirty/untracked work was preserved.

Lead must independently review and run combined caller/P31 checks and build; the helper's timed-out fresh diagnostics remain outstanding. Actual Windows debug native A/Escape then B/Copy Path under dismissal overlap must still prove B-only callback and clipboard identity, plus long-open selection, cancellation and removal cleanup. UUID generation assumes the modern Tauri WebView crypto.randomUUID API (already available in the local test environment); native Windows execution was not performed. This report is local source/bridge regression evidence, not Windows correctness acceptance.
