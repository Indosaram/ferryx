# Wheel and PR3 local lead verification

2026-09-13. Local integration only, not Windows runtime or final acceptance.
The original brief, coverage obligations and C001-C003 remain outstanding.

## P02 frontend normalization

Lead read the complete production/test diff and child receipt. Two changed
files had no error diagnostics. Independent exact command:

```sh
bun run --cwd ui test src/components/NativeTerminalPane.test.tsx --config vitest.wheel-mutation.config.ts
```

mon_5QZA8N7Z8TPVPKC4 / bash_30: exit 1, start 14:47:35, 3.67s.
WHEEL_MUTATION_APPLIED:original-normalization. Original visible-only gate
and trunc(deltaY/20)||sign restored in memory, retaining production IPC suffix.
15 intended failures / 152 passes. All failure output read in bounded chunks.
Zero/horizontal emitted -1; line/page units, fractional accumulation, bounds,
updated/sibling metrics and binding transitions failed the registered exact
payload assertions. The 151 original tests and non-Tauri control passed.
Temporary config deleted and checked absent. Production SHA256 unchanged:
3c005e88fc56066503bbe76a1ab784d5c8912a18f3680201a3b6596e64aa20b6.

No current Windows HWND delivery or backend coordinate/modifier propagation
has been verified. Child's 167-test GREEN is documented separately; combined
lead verification is recorded below when completed.

## P05 actual store ownership validator

Production useWorkspaceStore.closePane and closeTab were read, including
closeBackendSessionAndWait. Existing tests did not cover the full requested
tagged/pinned/first-versus-second matrix with precise survivor identity.
Only workspaceStore.test.tsx was extended, not production store code.

Exact baseline/final command:

```sh
bun run --cwd ui test src/state/workspaceStore.test.tsx
```

- Baseline mon_3ES84T4DC85FK7YP / bash_31, exit 0: 50 cases, 1.84s.
- Extended mon_Z4D2FSBSX7KP1DGP / bash_32, exit 0: 58 cases, 951ms.

Eight cases execute the real mounted hook and reducer, with a deferred
backend-close service and request signal subscribed before closing. Before
and after releasing the selected close, the exact sibling session object,
backend ID, leaf layout and pinned value survive; no new backend is spawned.
Close requests must contain only the selected backend. Finally releases
both close/exit signals and unmounts even on assertion failure.

Independent wrong-whole-tab mutation:

```sh
bun run --cwd ui test src/state/workspaceStore.test.tsx --config vitest.close-ownership-mutation.config.ts -t "preserves the sibling backend while closing"
```

mon_5DK0S82YRHXHKAQB / bash_33, exit 1, 667ms.
CLOSE_OWNERSHIP_MUTATION_APPLIED:whole-tab. All eight cases failed at exact
request cardinality: unpinned closed both backends; pinned closed neither.
The 50 cases outside the explicit filter were not run, not disabled.
No timeout/setup failure. Full output read; temporary config deleted/absent,
production store hash unchanged. LSP freshness timeout remains reported.
This proves store lifetime behavior with controlled services, not real PTYs.

## App PR3 and existing updater validator

Lead read the complete child App diff/report. Product change remains the
single discriminator correction at App.tsx:1814, preserving current
confirmation-aware handlers. The 28 new App cases are wiring coverage with
mocked store actions, separate from the real-store cases above.

The child observed one existing updater-startup assertion failing both
before and after the PR3 fix. Lead inspected actual startUpdatePolling,
checkForUpdate, App startup and installed Tauri core. The exported-function
spy cannot intercept module-local checkForUpdate. In addition, actual
isTauri() reads globalThis.isTauri, not the fixture's __TAURI_INTERNALS__.

Lead retained the actual App -> startUpdatePolling -> checkForUpdate path,
moved the spy to plugin-updater.check, and explicitly set/restored the real
runtime flag in this test only. It subscribes to the plugin request before
mount, awaits an exact signal with a bounded failure deadline, asserts one
call and cleans up the component/deadline/descriptor. No updater product
change was made for this validator repair.

Exact command: `bun run --cwd ui test src/App.test.tsx`.

- bash_34 / mon_FESETPJHC0PSMH41, exit 1, 127 pass / 1 fail, 4.30s:
  lower-boundary spy alone still lacked the runtime flag; failure was
  Startup plugin check not received. This is fixture failure, not product RED.
- bash_35 / mon_3NPEYB6MGYZHC1RB, exit 0, all 128 pass, 3.21s.

Independent production mutations, preregistered before execution:

```sh
FERRYX_APP_MUTATION=close bun run --cwd ui test src/App.test.tsx --config vitest.app-mutation.config.ts
FERRYX_APP_MUTATION=updater bun run --cwd ui test src/App.test.tsx --config vitest.app-mutation.config.ts -t "checks for a signed update when the native app starts"
```

- close: bash_36 / mon_1X2VXKMKEGW0JNFG, exit 1, 3.36s,
  APP_MUTATION_APPLIED:close, 14 intended PR3 failures / 114 passes.
  Untagged routes skipped closePane or wrongly closed the tab; focused-agent
  and busy-sibling guards failed at the exact registered assertions.
- updater: bash_37 / mon_H1JTV1BNJ2R657CK, exit 1, 2.10s,
  APP_MUTATION_APPLIED:updater, selected test failed with
  Startup plugin check not received; 127 cases outside the filter.

Full mutation outputs read. Temporary App config deleted after both exits.
App source has no error diagnostics; App/store test LSP freshness timed out,
so those are not called clean. Canonical TypeScript build is separately required.

## Combined batch receipt

Registered command is the preceding 12-file batch in
combined-local-verification.md plus NativeTerminalPane.test.tsx,
App.test.tsx and state/workspaceStore.test.tsx: 15 files, expected 575 cases.
Started bash_38 / mon_WJQZEW9HFX3E1039. Actual `bun run --cwd ui build`
started bash_39 / mon_1AWH7VCA5EAENK2G.

Both completed with exit 0, full output read. Tests: 15 files / 575 cases,
11.75s, start 14:57:10, no filters or retries. Existing attach-rejection
fixtures emitted expected error logs; all their assertions passed.
Build: tsc and Vite, 1,892 modules, 2.01s bundling; unsuppressed
505.66 kB App chunk warning. No error suppression.
App/updater production hashes unchanged across mutation; all three temporary
mutation configs absent; git diff --check exit 0.

No native Windows launch, installation, daemon operation, branch/worktree,
commit, PR disposition or push. Both implementation children returned.
