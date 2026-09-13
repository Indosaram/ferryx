# A08 publication refresh repair - st_01a09926

## Outcome and mechanism

The durable-publication contract is intact. No production behavior correction was needed. The regression injected its alleged post-catalog journal failure before catalog receipt acquisition. `catalog_receipt` calls `reconcile`, which now refreshes the journal from disk. Replacing that journal file with a directory at `worktreePublication` therefore prevents receipt acquisition and catalog commit. Absence of a committed event was correct for that earlier failure, not metadata channel contamination. The subscriber is the dedicated `worktree_changes` channel.

Current-source RED reproduced 3 passed / 1 failed with event `false` expected `true`. Strengthened RED retained the injection and assertions, and additionally recorded `journal_failure=true baseline=1 durable=1 receipt=None`. This directly establishes that the injected failure happened before the claimed durable boundary. Catalog receipt refresh remains unchanged: bypassing it or reading stale cached pending records would weaken journal authority.

## Narrow change

- `src-tauri/src/remote/workspace_api/worktrees.rs`: two-line `cfg(test)` probe `worktreeJournalCompletion` immediately before final journal completion. It is after durable catalog persist and the committed event on the successful Git/publication path. No release-build behavior changed.
- `src-tauri/src/remote/workspace_api/worktree_authority_tests.rs`: move only journal-failure obstruction to that explicit completion stage. Catalog-failure obstruction stays at `worktreePublication`. Preserve response, event, Git HEAD, replay and prunable-preview assertions; strengthen with disk catalog revision/receipt, exact committed event identity/revision, no duplicate event, and no replay revision/event side effect.

Given/When/Then blocks identify the original scenario. Real private Git creates from HEAD~1 and is inspected directly. Disk state is loaded independently of the in-memory catalog. Assertions distinguish:

| Failure | Response | Durable state | Event | Restart replay |
| --- | --- | --- | --- | --- |
| Catalog write | 409 | baseline revision 1, unchanged observation, no receipt | none | 409 OPERATION_OUTCOME_UNKNOWN |
| Post-catalog journal completion | 503 | revision 2, exact request completed receipt with 201 and original non-HEAD worktree | exact workspace/revision created event, once | 201 original head; no revision increment/event |

The intended failure phase, not a weakened assertion, is the changed input. Strengthened RED and GREEN use the same assertions; RED's old phase produces missing disk receipt/event and GREEN's completion phase produces durable receipt/event and recoverable replay. No shared journal, workspace service, metadata, controller, relay, paired-host, UI, or parent-owned fixture files changed by this lane.

## Runtime evidence

All evidence logs use `docs/evidence/paired-daemon/A13-issuer-repeat-A08-publication-` prefix because the existing `A13-issuer-repeat-run.sh` isolation runner was reused. Each has matching `.exit`, `.monitor`, `.cleanup` receipts.

Common Cargo options: `--locked --manifest-path src-tauri/Cargo.toml --no-default-features`.

- `RED.log`: `cargo test ... --lib followthrough_ -- --nocapture` -> 3 passed, 1 failed at original event assertion (false/true).
- `strengthened-RED.log`: same command -> 3 passed, 1 failed at retained event assertion; journal obstruction leaves disk revision 1 and no receipt.
- `GREEN.log`: same command -> 4 passed, 0 failed; catalog-failure disk revision 1, journal-completion-failure disk revision 2 with exact receipt.
- `authority.log`: `cargo test ... --lib remote::workspace_api:: -- --nocapture` -> 15 passed, 0 failed (also one separately invoked child crash fixture passed).
- `catalog.log`: `cargo test ... --lib catalog -- --nocapture` -> 8 passed, 0 failed (also one separately invoked child fixture passed).
- `machine-worktrees.log`: `cargo test ... --test machine_worktrees -- --nocapture` -> 2 passed, 0 failed. Real authenticated HTTP/Git surface covers lost reply with zero forwarded response bytes, locked/unmerged refusal, partial branch retention, 204 deletion, restart same-row/one-target replay, and revision fencing. Original private owner PID 99183 was reaped; listener joined and private root removed. This is executable HTTP scenario coverage, not a separately claimed manual curl run.
- `check.log`: `cargo check ... --lib` -> exit 0.
- `build.log`: `cargo build ... --bin ferryx-cli` -> exit 0.
- LSP error diagnostics on both changed Rust files: none. Focused `git diff --check`: clean. Existing compiler warnings retained, not suppressed.

Hashes of both changed source files and all eight logs: `A08-publication-refresh-source.sha256`. Shared composition was active during this lane; hashes identify the verified source snapshots rather than claiming all other concurrent code was frozen.

## Isolation and cleanup

Existing runner uses env -i with private HOME, FERRYX runtime/data/session paths, XDG paths and TMP/TMPDIR/TEMP; explicit toolchain caches and target; jobs=2. All eight exact Cargo children were waited/reaped. All eight cleanup receipts report removal_exit=0. Regression fixtures drain workers and remove private roots even when RED panics. No canonical daemon, user PTY, desktop, commit, deployment, release, destructive Git operation, or foreign source edit. Temporary root debug journal removed after report delivery; evidence logs intentionally retained.

## Architectural review and scope

Worktrees owns worktree transactions; authority tests own transaction-failure scenarios. No new input boundary, tagged-variant dispatch, unsafe, cast, production unwrap, defensive layer, one-off helper, parameter expansion, negative name, logger, or swallowed error was introduced. Disk/event checks are assertions of the requested durability behavior, not production post-write defensive checks. Existing source files measure 456 and 624 pure LOC respectively: inherited size debt remains explicitly disclosed; broad restructuring conflicts with the assigned transaction-only/no-refactor scope and was not performed. This lane's only handler lines are test-only instrumentation. The strengthened regression supplied failing-first proof before its phase correction.
