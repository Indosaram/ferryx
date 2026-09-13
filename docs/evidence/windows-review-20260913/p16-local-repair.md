# P16 local repair - 2026-09-13

## Outcome and scope

Repaired PKG-01 with exactly two production string changes:

- `ui/src/lib/updater.ts`: `updater_managed_externally` -> `cmd_updater_managed_externally`.
- `ui/src/lib/windowsStoreMigration.ts`: `distribution_channel` -> `cmd_distribution_channel`.

Only those files, their existing test files, and this receipt were edited by this task. Existing unrelated working-tree modifications were left untouched. No Rust edits, builds, installation, real updater downloads/relaunches, network, GUI, SSH, daemon, branches, commits or pushes were performed.

## Registry and call-flow evidence

Read `src-tauri/src/ipc/updater.rs`: the two `#[tauri::command]` functions have the `cmd_` names; the unprefixed names are internal Rust helpers. Read `src-tauri/src/lib.rs`: `builder.invoke_handler(tauri::generate_handler![...])` registers both fully qualified `crate::ipc::updater::cmd_*` symbols (lines 1068-1070 at inspection). The builder omits the updater plugin for externally managed installations.

Read frontend callers: App startup invokes polling and the migration notice; GeneralSection's update button calls `checkForUpdate`. The shared updater probe caught the unknown-command error and cached `false`, allowing Store installs into the plugin path. The notice probe caught the unknown-command error and returned without a toast.

Both test files now read the actual Rust handler registry from disk and extract updater command symbols from `generate_handler!`. Each native boundary rejects identifiers absent from that registry or outside its expected command. The configured response spy runs only after registry admission. This binds fixtures to production registration rather than merely matching a frontend typo. Rust registry and implementation were read-only.

## Exact registered command and RED/GREEN

C002/P16 command, unchanged between behavioral RED and GREEN:

```sh
bun run --cwd ui test src/lib/updater.test.ts src/lib/windowsStoreMigration.test.ts
```

Environment: Darwin arm64, working tree `/Users/indo/code/project/orca-lite`; existing package script runs `vitest run --maxWorkers=1`; Vitest 3.2.7, jsdom. No additional packages installed.

### Fixture prerequisite failure (not behavioral RED)

13:20:19: initial registry read used `new URL(..., import.meta.url)`. Vite transformed that URL to a non-file scheme. Both suites failed collection with `TypeError: The URL must be of scheme file`, zero tests, exit 1. Replaced that fixture path construction with `resolve(__dirname, ...)` before behavioral RED. No production code had been edited.

### Intended behavioral RED

13:20:47, before either production edit:

```text
src/lib/updater.test.ts (22 tests | 4 failed)
src/lib/windowsStoreMigration.test.ts (8 tests | 5 failed)
Test Files  2 failed (2)
Tests       9 failed | 21 passed (30)
error: script "test" exited with code 1
```

Observed intended failures:

- Store-managed updater: `expected "spy" to not be called at all, but actually been called 1 times` at the plugin-check assertion.
- Installer migration notice: `expected "spy" to be called 1 times, but got 0 times` at the toast assertion.
- Installer/native update routing: expected one admitted `cmd_updater_managed_externally` call, received zero.
- Store/native migration silence: expected one admitted `cmd_distribution_channel` call, received zero (prevents false silence from an unknown command from passing).
- Store probe cache and installer dismissal fixtures also failed because the probe never reached its registered handler.

### Same-assertion GREEN

Only the two production command strings changed after behavioral RED. No test/assertion changes between behavioral RED and GREEN.

13:21:32:

```text
src/lib/updater.test.ts (22 tests) 16ms
src/lib/windowsStoreMigration.test.ts (8 tests) 10ms
Test Files  2 passed (2)
Tests       30 passed (30)
Duration    845ms
P16_GREEN_EXIT=0
```

Preserved all original tests. Added non-Store updater routing checks, registered Store/native channel silence checks, and actual `onDismiss` callback persistence coverage. Existing action-dismissal persistence and suppression tests remain. Existing prose assertions were not expanded. Installer/native updater fixtures both use `false`, matching their shared Rust boolean contract; they do not claim to execute Windows channel detection.

## Diagnostics, scope, ownership

- LSP diagnostics with severity `all` on all four changed TypeScript files: `No diagnostics found` for each.
- Scoped `git diff --check`: exit 0, no output.
- Reviewed scoped diff: two production string replacements; test-only registry admission and behavioral coverage. Source/test diff: 76 insertions, 4 deletions.
- Nonblank/non-comment LOC: updater.ts 152; updater.test.ts 283; windowsStoreMigration.ts 45; windowsStoreMigration.test.ts 114. Existing large updater test module retained under the explicit no-split scope instruction.
- Directly awaited the real module functions/callbacks; no sleeps or readiness polling added. Existing virtual-clock polling tests exercise time as their behavior, stop their polling, and restore real timers. Tests use fresh module state, reset mocks, and per-test in-memory storage. Native updater/process calls remain mocked, including existing simulated download/install tests. No owned external resources were created; test runner exited normally, so there is no child/server/temp-root cleanup outstanding.

## Architectural review

Single responsibilities remain updater lifecycle and migration notice, with corresponding tests. Production boundary contracts and variant handling are unchanged; no new production parsing, error handling, logging, abstractions, defensive layers, type assertions, parameter bloat, negative names or redundant post-mutation verification were introduced. The regression demonstrably fails when the two command strings are wrong and passes after only those strings are corrected. Test persistence assertions intentionally observe the behavior under test. Existing test casts/prose checks and large-file layout were preserved, not broadened.

## Acceptance limits

This is local registry-faithful frontend regression evidence, not native Tauri execution or Windows packaging/runtime acceptance. The real Tauri invoke surface, MSIX Store install, NSIS installer update route, visible native toast and packaged restart behavior remain pending. No installation is authorized. Combined batch/build verification belongs to the lead and was not independently run here.
