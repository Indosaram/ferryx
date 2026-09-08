# manualQa - st_01a08187

Overall verdict: FAIL. Frontend build exited 2. Captured C1 evidence proves the targeted RED assertion and a passing 150-test main suite, but the combined run is not GREEN.

Evidence directory is the caller-provided directory. The task restricts executable commands to the build, so ulw-loop status was not invoked; currentAttemptDir/goalId could not be queried. Task id is used for the matrix filename.

## surfaceEvidence

| Scenario id | Criterion reference | Surface | Exact invocation | Verdict | artifactRefs |
| --- | --- | --- | --- | --- | --- |
| BUILD-1 | Frontend compilation exit 0 | Actual CLI, executed once | `bun run --cwd /Users/indo/code/project/orca-lite/ui build > /Users/indo/code/project/orca-lite/docs/terminal-pane-repair-20260908/evidence/ui-build.log 2>&1` | FAIL: exit 2; tsc rejects Promise.withResolvers at NativeTerminalPane.test.tsx(648,36) and RemoteDirectoryPicker.test.tsx(88,29), TS2550. Vite build not reached. | BUILD |
| C1-RED | Backing assertion existed and failed before removal | Existing Vitest log; inspected, not rerun | Captured: `vitest run --maxWorkers=1 src/components/NativeTerminalPane.test.tsx -t "keeps the terminal uncovered when a bounds failure is shown"` | PASS (evidence classification only): assertion expected null but received full-pane native-terminal-error-backing; 1 failed, 149 skipped, exit 1. Skipped tests are not credited. | RED |
| C1-MAIN | Main NativeTerminalPane suite passes after removal | Existing Vitest combined log; inspected, not rerun | Captured: `vitest run --maxWorkers=1 src/components/NativeTerminalPane.test.tsx src/components/NativeTerminalPane.lifecycle.test.tsx` | PASS (captured main suite only): NativeTerminalPane.test.tsx has 150 passing tests. Current source retains uncovered-bounds assertion and localized alert, with no error-backing match. Not fresh runtime or desktop proof. | GREEN, PANE, TEST |
| C1-COMBINED | Explicitly classify overlay failures; no total GREEN | Existing Vitest combined log; inspected, not rerun | Same captured combined invocation as C1-MAIN | FAIL: 177 passed, 3 failed, exit 1. Existing Settings, New Tab dialog, and Terminal search lifecycle cases expect detach but receive attach only; panes remain visible. | GREEN |

## adversarialCases

| Scenario id | Criterion reference | Adversarial class | Expected behavior | Verdict | artifactRefs |
| --- | --- | --- | --- | --- | --- |
| ADV-BOUNDS | C1 uncovered terminal | Bounds IPC rejection | Alert remains localized; no full-pane backing or detach | PASS for captured unit evidence only: targeted RED proves old backing and captured main suite passes all 150 tests after removal. No desktop verdict. | RED, GREEN, TEST, PANE |
| ADV-MODAL | Overlay lifecycle integration | Covering Settings / New Tab / search surfaces | Current macOS policy keeps terminal visible but blocks input; old tests expect detach | FAIL: all three captured cases fail at detach assertion. Subsequent input-blocking behavior is not established by these failed cases. Tests use waitFor timeouts; no reliability or integration pass is claimed. | GREEN, VISIBILITY |
| ADV-INPUT | Preserve ambiguous-delivery behavior | Receipt error after possible input delivery | Do not automatically replay unless inputWritten is explicitly false | PASS for captured unit evidence only: combined log explicitly passes three ambiguous Ctrl+C cases (Error, receipt unavailable, inputWritten=true); current source keeps explicit false guard. | GREEN, PANE |

No skipped, inferred, partial, or desktop execution is credited as an executable QA pass. Product source and concurrent visibility/pane edits were not modified. No test reruns, commits, or desktop actions were performed. The build errors were observed in the current tree; their authorship or pre-existing status was not established by this bounded run.

## artifactRefs

| Id | Kind | Description | Path |
| --- | --- | --- | --- |
| BUILD | CLI stdout/stderr | Non-empty build output; invocation tool returned exit 2 | /Users/indo/code/project/orca-lite/docs/terminal-pane-repair-20260908/evidence/ui-build.log |
| RED | Captured test log | Backing received instead of null; targeted failure and exit 1 | /Users/indo/code/project/orca-lite/docs/terminal-pane-repair-20260908/evidence/c1-red.log |
| GREEN | Captured test log | Main 150 pass; lifecycle 27 pass / 3 fail; combined exit 1 | /Users/indo/code/project/orca-lite/docs/terminal-pane-repair-20260908/evidence/c1-green.log |
| PANE | Source | Explicit inputWritten=false recovery guard and localized retry alert | /Users/indo/code/project/orca-lite/ui/src/components/NativeTerminalPane.tsx |
| TEST | Source | Uncovered-bounds regression assertion, inspected lines 382-405 | /Users/indo/code/project/orca-lite/ui/src/components/NativeTerminalPane.test.tsx |
| VISIBILITY | Source | macOS visible/noninteractive overlay policy | /Users/indo/code/project/orca-lite/ui/src/lib/nativeTerminalVisibility.tsx |
