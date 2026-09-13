# Remote and browser combined local verification

2026-09-13. P27 wheel, P28 selection lifetime and P05 browser targeting
are locally integrated. Windows runtime, remaining findings, PR disposition,
gate and push remain pending. This is not the final acceptance report.

## Combined command

```sh
bun run --cwd ui test src/components/Sidebar.sortableIdentity.test.tsx src/remote/deviceIdentity.test.tsx src/components/settings/PermissionsSection.test.tsx src/lib/nativeTerminalVisibility.test.tsx src/lib/shortcuts.test.tsx src/components/ShortcutHints.test.tsx src/remote/deviceIdentity.opera.test.ts src/lib/nativeWindowFocus.test.ts src/lib/notificationCoordinator.test.ts src/lib/terminalTransport/terminalTransport.test.ts src/lib/updater.test.ts src/lib/windowsStoreMigration.test.ts src/components/NativeTerminalPane.test.tsx src/App.test.tsx src/state/workspaceStore.test.tsx src/remote/RemoteTerminal.contract.test.tsx src/remote/RemoteUI.test.tsx src/components/BrowserPane.masking.test.tsx src/components/BrowserToolbar.omnibox.test.tsx src/components/BrowserToolbar.test.tsx src/components/BrowserPane.test.tsx src/components/BrowserPane.parity.test.tsx src/components/BrowserPane.findRace.test.tsx src/components/BrowserPane.privateHistory.test.tsx
```

mon_BAX9TS28PG8TGWVR / bash_44, exit 0: 24 files, 722 tests passed,
19.71s, start 15:22:52. All result lines read. The display abbreviated the
long initial command line; the registered command and 24 individual result
paths establish the exact selection. No test filters, skips or retries.
Existing attach-rejection tests emitted their expected error logs.

`bun run --cwd ui build`, mon_705WF86SNYRRPT4J / bash_45: exit 0,
TypeScript and Vite completed, 1,892 modules, 2.03s bundling. Unsuppressed
506.07 kB App chunk warning. Full output read. The interim child build
failure from an actively edited App test is superseded by this result,
not classified as an unrelated pre-existing failure.

## Browser downstream fixtures

Exact command before and after fixture adaptation:

```sh
bun run --cwd ui test src/components/BrowserPane.findRace.test.tsx src/components/BrowserPane.parity.test.tsx
```

- mon_T2ZANF9GJWRAJXAS / bash_42: exit 1, four failed / three passed,
  2.32s. All four failure blocks show missing Find in page after an
  action-only event. Output history dropped its first 124 lines due to the
  large DOM dump; retained failure blocks and final results were inspected.
  This is a downstream fixture mismatch, not new product RED.
- Four events now carry their fixture browserId. The touched parity find
  case observes synchronous input readiness and awaits the exact mock
  findBrowser result inside act rather than polling. Existing query-order,
  stale-browser, match-count and download assertions are preserved.
- mon_RF8WCVGSZYA7D0T6 / bash_43: exit 0, both files / seven tests passed,
  2.60s. Full output read. Both touched test files have clean error
  diagnostics; git diff --check exited 0.

## Independent repair evidence and limits

- P27: original normalization independently fails 17 cases; final 58 pass.
  See p27-wheel-normalization.md.
- P28 RC-01/02: original response lifetime independently fails seven cases;
  final 50 pass. See p28-selection-lifetime.md. No cross-worktree target or
  host preference fix is included.
- P05 WIN-UI-13: lead read the full production/test diff and exact failure
  blocks. Child receiver RED has four intended missing/unrelated target
  failures. Its initial App failures used an incomplete Windows fixture
  and are not accepted as behavioral RED. Corrected fixture plus original
  App dispatch yields three intended address-focus failures; final three
  targets pass 151 cases. See p05-browser-targeting.md. Combined App now
  has 133 cases including the prior PR3/updater regressions.
- All seven browser component test files are in the combined command,
  including masking, private history, omnibox, parity and find-race tests.
  This is not the full UI suite, real native WebView2 or Windows input proof.
- Browser focus-switch memo freshness and live hidden/remote target behavior
  still need the registered real Windows checks; preserving source guards
  alone is not runtime acceptance.

No child or lead test/build process in this batch remains active. P27/P28
temporary mutation configurations were deleted after exits and absence
checked, with production hashes unchanged. Browser child logs are retained
as repository artifacts with their historical failure distinctions.
Source changes remain uncommitted in the shared tree. No user daemon,
desktop, branch/worktree, installation, release, PR write or push occurred.
