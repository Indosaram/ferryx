# Q4 Windows reviewed candidate: parent composition

## Outcome

Composed only the remaining reviewed hunks in the four authorized files into
`/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`. No whole-file
snapshot replacement occurred. The final private candidate's 14 hashes were
verified against `review-source-hashes.json` before composition. Minimal
apply_patch hunks preserved newer Q1 budget tests, terminalCreateV1 capability
expectations, A11 canonical dimension parsing/expired-ticket test and the newer
A11 joint machine relay fixture.

Composition is complete at the assigned four-file boundary. Related verification
is **not wholly green**; failures in concurrently owned output/PTY code remain
parent coordination boundaries. Current composed Windows acceptance is separate
from the already verified frozen Windows candidate and parent relay1/safety9 run.

## All 14 candidate file dispositions

Paths below are relative to src-tauri.

| File | Disposition |
| --- | --- |
| src/remote/filesystem_tests.rs | Composed portable fixture names plus Windows drive-root/UNC/device refusal assertions; preserved newer budget/capability work |
| src/remote/relay_server.rs | Composed reviewed close/drain production hunks and strengthened Close/ConPTY test hunks; preserved A11 additions |
| tests/worktree_safety.rs | Composed portable shell and incremental ConPTY-query test changes |
| tests/relay_pairing_generation_regression.rs | Composed only navigable drive-path conversion in replacement-filename fixture; retained full newer A11 fixture |
| src/remote/server.rs | Deferred exact test-shell line to parent/output owner coordination; untouched here |
| src/remote/workspace_api/worktree_authority_tests.rs | Newer source and already composed Linux markers preserved; untouched |
| src/remote/workspace_api/worktrees.rs | Parent already composed native Git NUL-row path adapter; untouched |
| src/remote/workspace_api.rs | Parent already composed canonical root/trusted-prefix fixes; untouched |
| src/remote/workspace_api_tests.rs | Byte-identical to candidate at preflight; Linux markers already composed; untouched |
| src/ssh/worktree.rs | Byte-identical to candidate at preflight; parent composed; untouched |
| src/worktree/git.rs | Parent composed native path adapter/parser/filename fixes; newer source preserved; untouched |
| tests/machine_catalog_persistence.rs | Byte-identical candidate at preflight; Linux marker composed; untouched |
| tests/machine_worktrees.rs | Byte-identical candidate at preflight; parent composed; untouched |
| tests/q4_windows_paths.rs | Byte-identical candidate at preflight; parent composed; untouched |

`Q4-windows-parent-composition-before.json` records the four pre-write hashes.
`Q4-windows-parent-composition-current-source.json` records all 14 current hashes,
byte counts and candidate hashes after verification. These describe current source,
not a false claim that newer shared files equal the frozen candidate. No daemon,
session, PTY, output or UI source was edited by this composition.

## Aggregate checks

Executed on this Darwin workstation after all writes, with private HOME/FERRYX/
XDG/TMP paths outside the checkout, explicit Cargo/Rustup homes, jobs=3, empty
wrapper, debug=0, incremental=0 and the resumed target. Exact argv and full output
are retained in `Q4-windows-parent-composition-*.log`; all six immediate Cargo
processes were bounded and waited by `Q4-windows-parent-composition-check.py`.

| Check | Actual result |
| --- | --- |
| LSP on four touched files | relay integration file: no errors; other three requests cancelled by server, not claimed clean |
| git diff --check on four touched files | Exit 0 |
| Native path test target | Exit 101, compilation blocked; no zero-test coverage claim on Darwin |
| remote::filesystem_tests | Exit 101, compilation blocked |
| worktree:: (includes SSH) | Exit 101, compilation blocked |
| test_relay_browser_ws_terminal_bridge_success | Exit 101, compilation blocked |
| machine_worktrees + relay_pairing_generation_regression + worktree_safety | Exit 101 overall; machine worktrees 2 passed, relay integration 7 passed, safety 8 passed/1 failed |
| headless lib + ferryx-cli + ferryx-relay build | Exit 0 |

First four commands report E0599 at
`src/remote/machine_output_writer.rs:12:16`: no method `len` for
`axum::extract::ws::Message`. This is the separately owned active output source,
not a Q4 hunk. Later integration compiled and the final build passed; source was
being developed concurrently, so these results must not be described as one
immutable whole-tree snapshot. No unchanged retry was used to conceal failures.

The integration safety failure is
`pty_spawn_failure_does_not_claim_exclusive_writer`: panic at
`src/terminal/pty.rs:202:13`, "there is no reactor running, must be called from
the context of a Tokio 1.x runtime". That test was already synchronous and is
not modified by this candidate; the new PTY/output startup boundary is outside
this assignment. It requires parent coordination, not speculative changes to
excluded PTY code or conversion of the regression to hide the new requirement.

No further aggregate was rerun. Current composed Windows overall validation,
the deferred server.rs shell line, cancelled diagnostics and the failures above
remain outstanding. The frozen native candidate receipts are retained unchanged.

## Cleanup and safety

The aggregate's private runtime root
`/tmp/ferryx-herdr-q4-windows-01a097f8/parent-composition-runtime` was removed and
absence recorded in the cleanup log. Immediate Cargo processes were waited;
fixture logs retain their owned listener/PTY/child cleanup results. This receipt
does not substitute for an all-process inventory. The private candidate source,
archives and prior native evidence remain retained. No installed application,
desktop, canonical daemon/credential, deployment or commit was used. Changes
and evidence are uncommitted; parent coordination remains necessary around
concurrent source ownership.
