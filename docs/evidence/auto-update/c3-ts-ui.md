# C3 evidence - TypeScript bridge and Settings control

Files: `ui/src/lib/updater.ts`, `ui/src/lib/updater.test.ts`,
`ui/src/components/SettingsDialog.tsx`, `ui/src/components/SettingsDialog.update.test.tsx`,
`ui/package.json` (added `@tauri-apps/plugin-updater` 2.10.1, `@tauri-apps/plugin-process` 2.3.1)
Captured 2026-08-26T13:26:48.447097

Starting point: `ui/src/lib/updater.ts` was 43 lines of dead declarations (`UpdaterApi`,
`window.api.updater`) with no implementation and no caller, plus two functions that ARE used -
`registerWindowCloseGuard` / `flushCloseGuards`, consumed by `ui/src/App.tsx:63`. Both were preserved.

## Part A - status machine

RED (specs written first, implementation absent):
```
$ bunx vitest run src/lib/updater.test.ts
 Test Files  1 failed (1)
      Tests  12 failed | 1 passed (13)
TypeError: updater.getCurrentVersion is not a function
```
The single passing case is the preserved close-guard behavior, which must not regress.

GREEN:
```
$ bunx vitest run src/lib/updater.test.ts
 ✓ src/lib/updater.test.ts (13 tests) 206ms
 Test Files  1 passed (1)
      Tests  13 passed (13)
```
Covered: idle -> checking -> available with version and release notes; `check()` resolving null
returning to idle; a check failure becoming `error` with the message instead of rejecting into the
caller; download progress that is monotonic, reaches 1, and ends `downloaded`; a download failure
becoming `error`; refusing to download before an update was found; unsubscribe silencing a listener;
relaunch delegating to plugin-process; and a non-Tauri runtime (the remote web client, jsdom) never
touching the plugin at all. The download mock invokes the real progress callback the implementation
passes in, so the progress math is exercised rather than asserted through call counts.

## Part B - Settings > General control

RED (control absent):
```
$ bunx vitest run src/components/SettingsDialog.update.test.tsx
 Test Files  1 failed (1)
      Tests  5 failed (5)
Unable to find an accessible element with the role "button" and name `/check for updates/i`
```

GREEN:
```
$ bunx vitest run src/components/SettingsDialog.update.test.tsx
 ✓ src/components/SettingsDialog.update.test.tsx (5 tests) 2637ms
 Test Files  1 passed (1)
      Tests  5 passed (5)
```
Covered: current version rendered and the check button invoking `checkForUpdate`; an `available`
status announcing the version and the download button calling `downloadAndInstallUpdate`; a
`downloading` status exposing `role="progressbar"` with `aria-valuenow`; install-and-relaunch
disabled while downloading and enabled only once `downloaded`, then calling `relaunchApp`; and an
`error` status rendered inside the `aria-live="polite"` region.

Two defects in my own first draft, both fixed: it imported `@testing-library/user-event`, which this
repo does not depend on (switched to `fireEvent`, the existing convention), and it omitted
`cleanup()` between cases, so leaked DOM produced "found multiple elements".

VERDICT: PASS.
