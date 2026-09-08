# manualQa - st_01a08186

Verdict: PASS for the assigned compilation and captured-regression-evidence checks only. No desktop verdict.

No ULW plan exists according to the supplied ownership notepad; caller evidence directory used. Task id used as goal identifier because none was supplied. Only the two authorized shell checks were executed, from /Users/indo/code/project/orca-lite. No tests rerun, source edits, commits, app launches, daemon launches, or process termination. LSP daemon unavailable per supplied constraint; not configured or independently probed.

## surfaceEvidence

| scenario id | criterion reference | surface | exact invocation | verdict | artifactRefs |
|---|---|---|---|---|---|
| compile | C2/C4 native compilation | CLI | cargo check --manifest-path /Users/indo/code/project/orca-lite/src-tauri/Cargo.toml | PASS: exit 0; dev profile completed in 8.53s; 16 warnings emitted on stderr | check |
| whitespace | patch hygiene | CLI | git diff --check | PASS: exit 0, no diagnostic output | check |
| red-audit | C2 captured RED | saved test output, read directly | read docs/terminal-pane-repair-20260908/evidence/c2-red.log | PASS evidence audit: named regression fails with left 640.0/right 900.0; 0 passed, 1 failed. Numeric process exit is not recorded in this log; diagnosis claims 101, not independently verified | red |
| green-audit | C2 captured GREEN | saved test output, read directly | read docs/terminal-pane-repair-20260908/evidence/c2-green.log | PASS evidence audit: deferred_bounds_retry_does_not_restore_obsolete_width is explicitly ok; 158 passed, 0 failed, 0 ignored, 597 filtered out. Numeric process exit not recorded; suite was not rerun | green |

## adversarialCases

| scenario id | criterion reference | adversarial class | expected behavior | verdict | artifactRefs |
|---|---|---|---|---|---|
| stale-width-audit | C2 | obsolete async completion after newer geometry | old 640-wide command must not replace accepted 900-wide geometry; VT and final queued PTY size match latest receipt | PASS captured-evidence audit only: RED fails on exact width assertion; GREEN includes same test. Current source checks session/host width, VT dimensions and queued resize; subscribes before output and awaits bounded state change, not fixed sleep | red, green, host, ipc |

## artifactRefs

Paths below are relative to /Users/indo/code/project/orca-lite.

| id | kind | description | path |
|---|---|---|---|
| check | stdout log | Exact command and exit receipts; command stdout only. Cargo diagnostics went to execution transcript stderr, not this file | docs/terminal-pane-repair-20260908/evidence/native-check.log |
| red | captured test log | Named regression fails 640.0 versus 900.0 | docs/terminal-pane-repair-20260908/evidence/c2-red.log |
| green | captured test log | Named regression passes in 158 passing tests | docs/terminal-pane-repair-20260908/evidence/c2-green.log |
| host | source | Current-layout render helper and deterministic regression, lines 1902-1943 and 2627-2692 inspected | src-tauri/src/native_terminal/surface_host.rs |
| ipc | source | Bounds command applies initial request once and retries render_current | src-tauri/src/ipc/native_terminal.rs |

Compilation warnings were not suppressed. Existing captured test logs already show most warning sites; the current check also reports an unused Manager import in notifications.rs. No baseline compilation was run, so warning provenance is not asserted. This is not proof of desktop pixels or actual kernel PTY resize acknowledgement. The diagnosis document's pending-native-GREEN entry is stale relative to the inspected GREEN artifact.
