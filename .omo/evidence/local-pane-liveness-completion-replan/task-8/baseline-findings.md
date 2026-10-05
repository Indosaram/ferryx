# Task 8 first-pass findings

Candidate: 5464da0dd65a7a6312d096229d2067ec02735c78. Only the two explicitly A/B-confirmed cases below are classified pre-existing.

## Frontend lane — reopened
Mac UI build native exit 2:
```text
src/lib/sessionPersistence.ts(2,33): error TS6133: 'LocalSplitIntent' is declared but its value is never read.
src/state/workspaceStore.ts(773,22): error TS6133: 'layout' is declared but its value is never read.
src/state/workspaceStore.ts(2695,38): error TS2304: Cannot find name 'isLegacyUnknown'.
```
Mac scoped lifecycle native exit 1: Test Files 2 failed | 2 passed (4); Tests 9 failed | 102 passed (111). Eight sessionPersistence assertions and one sessionLifecycle inactivity-suspend assertion fail. Full verbatim log ui-lifecycle.log.

## Scripts adapters lane — reopened
Mac exact runner gate native exit 1:
```text
No test files found, exiting with code 1
include: scripts/qa/pane-liveness.test.mjs
```
Gate runs from ui root; config has no repository root override. Verifier leaves argv/config untouched. Full log runner.log.

## Verifier staging correction — not product failure
Archive submodule placeholder folder caused nested Unix symlinks and Windows mklink exit 1. Corrected only owned links/empty folders. Mac interrupted first Rust list before candidate compile result; not pass, selected count unknown. Node 23961 / cargo group 24813 identity verified and exited before resume.

## Omission and barrier audit
Frozen diff contains 20 new files including scripts runner/adapters, split_journal, qa_barrier, pane_liveness_contract, suspension platform modules and UI new contract/lifecycle tests. No untracked product files found.
qa_barrier.rs:323 emits <name>.bound-ack.json; :310 target_backend_session_id set from bound session. arm scan accepts .arm.json. Task9 scenarios not executed in Task8.

## Mac Rust gates — compile blocked by frontend build
First completed local_split_reliability_ --list native exit 101. Exact custom-build output:
```text
resource path `../ui/dist` doesn't exist
```
Ghostty build reports pinned SHA 6a508fd5e34c7e222c052a6d00bb3891ff3feace. No placeholder dist or old assets supplied. Selector count is UNKNOWN because listing never reached test executable; parsed zero lines must not be labeled a naming hole without successful compilation. Backend/native behavior remains unproven.

## Audited platform UI comparison
All three UI build logs contain the same three compiler errors, native 2. All three split suites pass 19/19, native 0. Mac/Windows lifecycle fail 9/111; Linux fails 12/111, adding explicit-browser, parked-worktree, mixed-pane roundtrip cases. No baseline attribution without A/B. NativeTerminalPane and nativeTerminalLifecycle files pass in these scoped runs; this does not prove native GUI behavior. Linux Rust first list also native 101, resource path ../ui/dist missing.

## Windows Rust prerequisite — frontend lane
Exact first-list log lines 506-507:
```text
thread 'main' (22896) panicked at build.rs:122:39:
tauri build failed: resource path `..\ui\dist` doesn't exist
```
Native exit 101 captured by transferred PowerShell file; pinned Ghostty SHA appears at line 474. Missing dist is a consequence of failed candidate UI build. Backend daemon/terminal/ipc-terminal and native native_terminal/ipc-native_terminal remain UNPROVEN, not assigned fabricated compiler errors. Frontend owns prerequisite repair; scripts owns runner discovery repair.

## Full UI candidate first-pass findings — frontend lane reopened
Mac snapshot already contains NativeTerminalPane.test.tsx 187 failures/197, App.test.tsx 5/154, workspaceStore 7/63, workspaceRestore 3/20, tauri 3/41, App.remote 1/20, workspaceNativeActivity 3/14, App.pairedDaemon 1/8, agentReconnect.integration 1/7, NativeTerminalPane.presentation 12/14. These are callers of changed frontend behavior, so none is waved away as baseline. Full raw log pending suite exit. TerminalSearchOverlay 1/11 and updater 1/23 are being A/B checked on immutable base before any out-of-scope pre-existing claim.

## A/B confirmed pre-existing — out-of-scope
Base d82b35e4 on maho-mac, `bun run --cwd ui test src/components/TerminalSearchOverlay.test.tsx src/lib/updater.test.ts`: native 1, 2 failed / 32 passed (34). Same candidate failures: TerminalSearchOverlay.tsx test :332 expects 2 calls, got 3; updater.test.ts:141 expects relaunch once, got 0. Neither production module changed in candidate. Evidence logs/base-ab.log and logs/mac/full-ui-snapshot.log. These two only are baseline-confirmed; no other failure gets this label.

## Final Mac/Linux suites
Mac full-UI native 1: 243 failed / 6277 passed (6520), 19 failed / 353 passed files, 2 unhandled errors. Linux full-UI native 1: 1271 failed / 5249 passed (6520), 107 failed / 265 passed files, 1 unhandled error. Linux broader failures are UNCLASSIFIED relative to base, not pre-existing claims. Complete raw failures remain verbatim in logs/linux/full-ui.log; failure-index.jsonl is a convenience index, not coverage/count authority. Only the Mac two-case A/B gets baseline label.
Linux changed Unix suspension and split_journal list/run gates each native 101 with resource path ../ui/dist missing; behavior remains UNPROVEN.

## Final Windows suite
Full-UI native 1: 244 failed / 6271 passed / 5 skipped (6520), 21 failed / 351 passed files, 3 errors. Raw verbatim failures: logs/windows/full-ui.log. Skips are not passes; all failures except the two Mac A/B cases remain unclassified relative to base. Frontend lane reopened.
