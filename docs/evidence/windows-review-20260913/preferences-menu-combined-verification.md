# Preferences, selection and native-menu local verification

2026-09-13. Local integration evidence, not Windows runtime or final gate.
P27, P28 and P33 source/bridge regressions are verified below. P32 has an
unchanged real-file oracle RED/GREEN, but adjacent history checks and native
Windows execution remain pending. No source commit or push occurred.

## Remote preferences, selection and IPC

```sh
bun run --cwd ui test src/remote/RemotePreferences.contract.test.tsx src/remote/RemoteAttention.test.tsx src/remote/RemoteUI.test.tsx src/remote/RemoteTerminal.contract.test.tsx src/remote/RemoteRouting.test.tsx src/remote/RemoteReconnect.test.tsx src/lib/terminalSettings.test.tsx src/lib/remoteClient.test.ts src/lib/tauri.test.ts src/remote/RemoteTerminal.mobile.test.tsx src/remote/RemoteTerminalGestures.test.tsx
```

`mon_MAXZDC4C1RDRP31H` / `bash_49`: exit 0, 11 files / 201 tests,
7.68s, start 15:46:50. Full output read. Counts: UI 50, terminal contract
58, attention 15, preferences 3, gestures 10, mobile 8, routing 6,
reconnect 1, settings 15, Tauri 32, client 3.

P27's identical final fixture fails twice against its complete original
production chain, then passes three cases after repair. Lead read the
contract, production diff and authoritative original/final logs in
p27-preferences/. The actual route is `/host/<machineId>`, not `/h`.
The contract hash matches both receipts. This is real component/settings/
HTTP-construction logic with mocked transport, not Windows HTTP/font proof.

The related run originally had three Tauri failures, also reproduced on
the original preference chain. Lead read the actual Rust selection DTO and
notification command and corrected only tauri.test.ts: omitted/null
attentionInventory explicitly expects [], supplied inventory is preserved,
default sound is system and explicit silent stays silent. Full argument
equality remains strict. `bun run --cwd ui test src/lib/tauri.test.ts`,
mon_YRY9JM8D2YV7AB35 / bash_48, passed 32 cases in 1.61s, exit 0.
These are stale-validator corrections, not new production fixes.

## Native menu and every existing row/tab caller suite

```sh
bun run --cwd ui test src/lib/nativeMenu.test.ts src/components/Sidebar.sortableIdentity.test.tsx src/components/Sidebar.activity.test.tsx src/components/Sidebar.dnd.test.tsx src/components/Sidebar.projectIdentity.test.tsx src/components/Sidebar.remote.test.tsx src/components/Sidebar.sshActivity.test.tsx src/components/Sidebar.test.tsx src/components/TabBar.appearance.test.tsx src/components/TabBar.browserTab.runtime.test.tsx src/components/TabBar.test.tsx src/components/WorktreeList.test.tsx
```

First combined run mon_X7EABFDRNM844EG3 / bash_50: exit 1, three failed /
117 passed in 12 files, 12.57s. All failures were Sidebar.dnd.test.tsx:
its mock accumulated useSensor calls across rerenders (four instead of
two), and two persisted-value assertions still expected raw paths instead
of P31's qualified IDs. Full output and all three failure blocks were read.

Lead registered and corrected that test only. DndContext now captures its
actual supplied sensor descriptors; exactly two, five-pixel threshold and
keyboard coordinate assertions remain. Explicit qualified saved IDs replace
the two stale expectations. Legacy raw-path seeds, deleted-path pruning,
visible order and remount assertions remain. No production rollback or
relaxed count assertion.

Same command after correction: mon_Y66NR3Y50PYER1N2 / bash_52, exit 0,
12 files / 120 cases, 9.50s, start 15:49:42. Full output read. Counts:
DnD 7, Sidebar 38, remote 11, activity 4, sortable identity 3, TabBar 19,
WorktreeList 17, browser-tab runtime fixture 2, project identity 2,
SSH activity 4, native menu 12, appearance 1.

P33 child original RED is nine failures / three passes; final 12 pass.
Lead read full test, production diff and raw RED/GREEN. Same-ID selection
calls A and B in original two-row fixture but only B after repair. The
long-open test's original failure is a leftover timer after selection,
not failure to keep the popup selectable while open. Actual Windows menu
tracking, clipboard and delayed bridge acceptance remain required.

## Build, diagnostics and cleanup

`bun run --cwd ui build`, mon_5F2YA2HB3Q5TCPKH / bash_51: exit 0,
TypeScript and Vite completed; 1,892 modules, 2.30s bundling. Unsuppressed
506.68 kB App chunk warning. Full output read. This supersedes P27's
interim typecheck errors from P33's then-active test edits. Fresh LSP still
timed out for nativeMenu.ts and RemotePreferences.contract.test.tsx; the
actual tsc build passed. Lead diagnostics are clean on the two corrected
test files and history/mod.rs. The DnD test-only correction followed the
build and has its own clean diagnostics and passing suite.

P32 raw logs, isolation wrapper and cleanup were fully read from
p32-history-ancestry.md. RED exits 101 for abandoned match 1 instead of 0;
GREEN exits 0 with unchanged a,c read assertion. Repaired source and
unchanged test hashes match. The owned root locator is absent. The child
used a Python asyncio process wrapper instead of the instructed native
monitor; that workflow deviation is preserved, not represented as compliant
Bun orchestration. No wrapper is retained for reuse. Sixteen existing Rust
warnings are not suppressed. Paging/corruption/native Windows are unrun.

All lead test/build monitors above exited. P27/P32/P33 workers returned.
Only the read-only exact branch inventory remains active. No desktop,
daemon, branch/worktree, installation, release, PR disposition or push was
performed by these checks. Source is uncommitted in the shared tree.
