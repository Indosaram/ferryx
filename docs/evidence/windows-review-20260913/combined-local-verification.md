# Combined local repair verification

2026-09-13. Partial acceptance only: transport validator, P34 focus, P04
digit shortcuts and P28 Opera suggestion. Native Windows QA, remaining
packets and final aggregate gate are not covered.

## Lead inspection

Lead opened all P04/P28 claimed source, test and report files, reviewed
their actual diffs, read the real PairingPage submission and shortcut/hint
consumers, and obtained clean error diagnostics on the changed files.
P28 old-source reproduction and P04 old-binding mutation were independently
observed, not accepted from worker summaries.

## Independently executed combined GREEN

Exact registered command:

```sh
bun run --cwd ui test src/lib/shortcuts.test.tsx src/components/ShortcutHints.test.tsx src/remote/deviceIdentity.opera.test.ts src/lib/nativeWindowFocus.test.ts src/lib/notificationCoordinator.test.ts src/lib/terminalTransport/terminalTransport.test.ts
```

mon_MTFJPP07XTM3HQPV / bash_13, exit 0, 2.66s, runner summary start
13:12:55. Six files and all 156 tests passed:

- shortcuts.test.tsx: 72.
- ShortcutHints.test.tsx: 18.
- deviceIdentity.opera.test.ts: 8.
- nativeWindowFocus.test.ts: 10.
- notificationCoordinator.test.ts: 40.
- terminalTransport.test.ts: 8.

The lead read the completed output. No test was skipped or retried in this
run. This independently confirms the producers' normal GREEN assertions
on their combined source, not their Windows runtime claims.

## Independently executed UI build

`bun run --cwd ui build`

mon_47H0G7N12FZ18WF1 / bash_14, exit 0. The actual script ran tsc then
Vite 6.4.3, transformed 1,892 modules and completed bundling in 2.13s.
Full completed output was read. Build output is ui/dist.

Vite warned about a chunk above 500 kB (App bundle 504.77 kB minified);
the warning was not suppressed. No native application, release bundle,
installation, daemon or remote host was launched. This is a frontend
build, not a Windows Rust/native build or a visual acceptance result.

## Outstanding

Actual Windows actions for
all repaired behaviors, remaining confirmed packets, exhaustive final
source qualification, full test suite/native build, owned runtime cleanup,
gate approval, atomic commits, PR dispositions and verified push remain.

## P04 independent original-binding mutation

Registered command:
`bun run --cwd ui test src/lib/shortcuts.test.tsx src/components/ShortcutHints.test.tsx --config vitest.shortcuts-mutation.config.ts`.
The loader checked exactly nine non-Mac workspace overrides then discarded
only those override fields, restoring the original Mod-digit bindings.

mon_4MRKFDWVYCFSQR0W / bash_15, 13:13:46, 1.37s, exit 1;
SHORTCUT_MUTATION_APPLIED:original-digits; 11 failed and 79 passed.
Lead read full output: all nine Alt workspace actions received zero calls
at shortcuts.test.tsx:82, workspace label was Ctrl+1 rather than Alt+1
at line 155, and Alt-held hint was absent at ShortcutHints.test.tsx:70.
Ctrl hints, Mac hint controls and existing IME/AltGr tests passed.

Config was deleted after exit and checked absent. Production shortcuts
SHA256 stayed 54533e35c68ed80181331740982434bd2f11aeda5f04cbfab2247d730cc4f795.
The earlier combined GREEN used identical unchanged production/test bytes.
Foreign binary diff excluding eight owned tracked source/test files matched
the pre-batch baseline exactly. git diff --check exited 0.

## Second batch: P07, P21 and P16

Lead read every claimed source/test/report and the actual Rust command
registration at src-tauri/src/lib.rs:1068-1070. No new product defect was
found in this bounded review. P21 test LSP freshness timed out at 3000ms;
this is not a clean LSP result. All other requested error diagnostics were
clean. The full tsc run below covers the final P21 test file.

Exact registered combined command:

```sh
bun run --cwd ui test src/components/settings/PermissionsSection.test.tsx src/lib/nativeTerminalVisibility.test.tsx src/lib/shortcuts.test.tsx src/components/ShortcutHints.test.tsx src/remote/deviceIdentity.opera.test.ts src/lib/nativeWindowFocus.test.ts src/lib/notificationCoordinator.test.ts src/lib/terminalTransport/terminalTransport.test.ts src/lib/updater.test.ts src/lib/windowsStoreMigration.test.ts
```

mon_KC5DC2308ECAQB8H / bash_17, start 13:23:50, 4.47s, exit 0:
all ten files / 210 tests passed. Added counts are permissions 6,
native visibility 18, updater 22 and migration 8. Complete output read.

`bun run --cwd ui build`: mon_F0DT6XS20K4S5HBV / bash_18, exit 0,
tsc then Vite, 1,892 modules, bundling 2.15s. App chunk 504.80 kB warning
remains unsuppressed. Neither build nor tests launch the desktop or install.

### Independent original-behavior reproduction

- P07: `bun run --cwd ui test src/lib/nativeTerminalVisibility.test.tsx --config vitest.toast-mutation.config.ts`.
  mon_CEJSHA73E8NJ5P8M / bash_16, 13:22:35, 741ms, exit 1,
  TOAST_MUTATION_APPLIED:original-selector. Exactly 2 failed / 16 passed:
  non-Mac visibility/interaction and Mac interaction incorrectly stayed true
  with real toast DOM present. Failure site line 65; empty/hidden/opt-out
  controls passed and cleanup did not mask assertions.
- P16/P21: `bun run --cwd ui test src/components/settings/PermissionsSection.test.tsx src/lib/updater.test.ts src/lib/windowsStoreMigration.test.ts --config vitest.boundary-mutation.config.ts`.
  mon_SGNTW79CV59D5J1C / bash_19, 13:25:03, 1.91s, exit 1,
  three BOUNDARY_MUTATION_APPLIED sentinels. Exactly 10 failed / 26 passed:
  permissions 1, updater 4, migration 5. The original unconditional warning
  violates Windows alert absence; old unprefixed names bypass registered
  probes, call the plugin for Store and suppress installer notice. Mac
  permissions controls pass. Full completed output read.

Both temporary configs were deleted after exit and checked absent.
All four affected source hashes were checked identical across mutation:

- nativeTerminalVisibility.tsx: c2288445bfc3fbe0ccbbef759b075eaa9048995fe42735b239bcbb1d5d69c3e6
- PermissionsSection.tsx: bd8ec49e3fb5907c52bd2ae1d7a96b46e9a01b5d75d927866efab37492084661
- updater.ts: 57db89af754a75c40f19817114d1b795516ec9d0190e0a955b74459e4dd56ae4
- windowsStoreMigration.ts: fe343bce66f2cc77b14ec615ec2f264f3a34ebad789b18fffa9d05e11b71cf64

git diff --check exited 0. P22 is still running separately. These receipts
close local batch checks only; native Windows behavior, full suite/native
build, final coverage/gate, PR dispositions and push remain outstanding.

## P31 and pairing validator combined batch

Lead read both complete production files, P31's new test/report and their
actual diffs. The real Sidebar grouping, registration, keyboard sensor,
displacement and persisted reload assertions remain in the test.
P31's compiler-API fallback reported a pre-existing .at lib diagnostic;
the actual project build below passed. The fallback is not evidence that
the canonical build fails. New test LSP freshness timed out; Sidebar,
WorktreeList and the pairing test had no error diagnostics.

Registered lead command:

```sh
bun run --cwd ui test src/components/Sidebar.sortableIdentity.test.tsx src/remote/deviceIdentity.test.tsx src/components/settings/PermissionsSection.test.tsx src/lib/nativeTerminalVisibility.test.tsx src/lib/shortcuts.test.tsx src/components/ShortcutHints.test.tsx src/remote/deviceIdentity.opera.test.ts src/lib/nativeWindowFocus.test.ts src/lib/notificationCoordinator.test.ts src/lib/terminalTransport/terminalTransport.test.ts src/lib/updater.test.ts src/lib/windowsStoreMigration.test.ts
```

mon_XAP8EV33BMQQCFEM / bash_27, start 14:37:04, 6.04s, exit 0:
12 files / 222 tests passed, no filtered cases. Full output read.
`bun run --cwd ui build`, mon_MF13PDN2JA1SPBTY / bash_28, exit 0:
tsc then Vite, 1,892 modules, 2.06s bundling; unsuppressed 505.33 kB
App chunk warning. No app/daemon launch or installation.

P31 original-registration mutation command:
`bun run --cwd ui test src/components/Sidebar.sortableIdentity.test.tsx --config vitest.sortable-mutation.config.ts`.
mon_00K5CNXKEKWN1S42 / bash_29, start 14:37:35, 1.72s, exit 1,
SORTABLE_MUTATION_APPLIED sentinel. Two intended unique-ID failures
(2 instead of 3 at test line 51), legacy path case passed. Full output read.
Config deleted after exit and checked absent; both production hashes unchanged.

### Pairing validator synchronization only

OTHERUI-GAP-01 existing filtered baseline passed one test (eight outside
the registered -t filter) in 853ms, bash_24 / mon_5818W8S2Y5XD7F2J.
This is not a product RED. Lead replaced polling with a pre-subscribed
completion callback and controlled real Response completion inside act,
retained exact POST/callback values and added finally cleanup of timeout,
DOM, globals and storage. PairingPage production source was not changed.

Registered mutation command, with MODE expanded to wrong then absent:

```sh
FERRYX_PAIRING_MUTATION=MODE bun run --cwd ui test src/remote/deviceIdentity.test.tsx -t 'renders device name pre-filled, allows editing, and submits installationId with deviceName' --config vitest.pairing-mutation.config.ts
```

- Wrong: mon_C9KMCF0Z8M0XA06S / bash_25, exit 1, 799ms. Expected callback
  token mock-token-xyz was wrong-token at line 121; exact assertion failed.
- Absent: mon_2GTGB339DSQFGXFX / bash_26, exit 1, 1.79s. The registered
  bounded failure deadline rejected with Pairing callback not received,
  not a Vitest timeout, setup failure or timing-based success.
- Both emitted their exact mutation sentinel, selected one failing test
  and excluded the same eight nonmatching cases. No tests were disabled.
  Final combined run above passed all nine file cases without filters.

Both mutation configs deleted/absent. git diff --check exit 0.
Production SHA256 values checked unchanged:

- Sidebar.tsx: 7b47135c29fc0d6b513c0f2fde8a7c6e0f85b55f9586a8da6f041ebec5422fb6
- WorktreeList.tsx: a096eb31f084c5920d4264b8ff397e5859dc10454fcfba6acc0487705abc28ef
- PairingPage.tsx: 1847bcba83b26a1faea8bed27883ef6eabe915d26feeb74235f60a48145f2227

No Windows GUI, native input, full-suite or final gate acceptance inferred.
