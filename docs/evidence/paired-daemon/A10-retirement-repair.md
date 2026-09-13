# A10 final-close retirement - focused GREEN

## Final continuation (supersedes historical incomplete sections below)

Parent released session_api.rs and machine journal reconciliation scope. Actual separate-process GREEN in A10-retirement-reconcile-GREEN2.log: predecessor13790, replacement13830, original shell13827, private root /tmp/fx-v02-xYj2kB. Close returned204; predecessor retired exit0; parsed durable record was status204/state completed/outcome noContent; replacement operation GET returned completed; identical DELETE replay returned204 after owner retirement. Final assertion, test result (1 passed), child reaping/output drain/root cleanup receipts all captured. Supervisor /tmp/a10-retirement-Y6Yyri removed. No sentinel or retirement disable.

Root cause of replacement404: journal instances read their startup snapshots and stale writers could overwrite newer predecessor records. The shared seam now acquires a private .tx.lock, refreshes current durable state, and retains the file lock across read/modify/write. Reconcile refreshes under the same lock. Adapter reconciles before routing close so completed retries need not contact a retired owner. Parent admission/revocation/deadline ownership is preserved. The initial malformed cfg was corrected; parent compiler failure is separately retained. A mistaken .lock name collided with write_private_json's lock: A10-retirement-reconcile-GREEN.log records actual timeout, owner12366 reaped and supervisor /tmp/a10-retirement-Skmm5B removed. Corrected .tx.lock follows existing auth transaction practice.

Bounded transport: A10-retirement-idle-RED.log records idle read exceeding12s watchdog, exit101. Connection read/write/flush/shutdown now enforce10s progress deadlines with timers owned by the connection. A10-retirement-final-batch.log records idle test passing in10.01s, all5 daemon_handover_contract tests passing, and machine_sessions passing. Its journal_contention selector accidentally matched0 tests: explicitly NOT coverage. Correct selector machine_operation_journal::contention_tests subsequently passed6 in A10-retirement-contention-GREEN.log and again in final verification after explicit permit-drop warning fix.

Final A10-retirement-verification.log: six real HTTP contention/revocation/deadline tests pass (40.13s), cancellation guard accounting test passes, headless ferryx-cli/ferryx-relay check exits0. Cancellation guard test uses exact oneshot completion and join; it proves an independently retained operation survives connection cancellation. Real HTTP tests assert401/504 before held writer release, retained admission, then reacquisition after drainage. These are scoped checks, not a claim of every possible network-cancellation race.

Final integrated retirement case in A10-retirement-final-batch.log: predecessor15595, replacement15664, original shell15661, request51b11809-3fc7-4612-bc06-266bfda8dac6, root /tmp/fx-v02-wfnnZH; final assertion and cleanup reached. Full existing harness also exercises prepare/abort, rolling Local session retirement, and error/panic cleanup. Replacement is explicitly killed/reaped by private harness after sessions cleaned, not mistaken for natural retirement. Machine_sessions original PID15833, second16021, managed16210, nested16305/inherited16322; cleanup receipts in same log. Supervisors /tmp/a10-retirement-F4IWpE, /tmp/a10-retirement-i0VUfx and /tmp/a10-retirement-Xhqm8t removed.

Final source hashes: A10-retirement-source.sha256 (nine files, including test module registration, new timer test and explicit permit drop). LSP attempts on changed production files were cancelled in the final parallel wave; earlier individual handover/machine_gateway/support calls returned no errors. Do not label cancellations clean. Compiler and requested runtime validators pass. git diff --check exits0. No dependency/Cargo, remote/server machine socket, output hub, PTY/session, workspace/event, UI or foreign-worktree writes in this continuation. A12 workspace/event domains released read-only.

Architecture review: lifetime coordination and durable journal transaction each have one responsibility; JSON is typed at the durable boundary, file locks release by RAII, no unsafe/type suppressions introduced, no fixed-delay synchronization. Existing large files remain large; no unrelated split performed in concurrently owned modules. New guard and transport tests remain narrow. No final full-plan/A10 acceptance inferred from this focused retirement GREEN. Inherited /tmp/a10-owner-parent.YmuzgZ remains untouched and parent-owned; no cleanup claim for its historical PTY77838/temp77763.

## Historical progress and failures (retained)

## Scope and source

Task st_01a09869; separate-process retirement continuation. No sentinel PTY, retirement-disable flag, canonical daemon, GUI, commit or release. Existing private subprocess harness reused via a nested support/machine_retirement.rs module in daemon_handover_contract.rs. Production changes are limited to handover.rs, session_service.rs close/request-lifetime methods and machine_gateway.rs. Parent output method retained.

Retirement now counts live machine close operations and owner-gateway connection lifetimes. Lifecycle empty-session retirement is deferred while either guard is held; last guard drop rechecks draining/empty and invokes the existing retirement path. Operation guard survives through journal completion; gateway guard survives response connection shutdown. Existing public signatures unchanged.

Current hashes: A10-retirement-source.sha256. Parent/worker concurrent edits mean hashes are a checkpoint, not final combined-source freeze.

## Actual commands/results

All cargo commands: --locked --manifest-path src-tauri/Cargo.toml --no-default-features, private existing src-tauri/target, CARGO_BUILD_JOBS=2, DEV/TEST_DEBUG=0, CARGO_INCREMENTAL=0, RUSTC_WRAPPER empty. Supervisor /tmp/a10-retirement-* initializes private HOME/runtime/data/session/XDG/TMPDIR before state; CARGO_HOME=/Users/indo/.cargo and RUSTUP_HOME=/Users/indo/.rustup preserved. Child subprocess harness clears environment and allocates separate private /tmp/fx-v02-* roots.

1. `cargo test ... --test daemon_handover_contract final_machine_close_is_durable -- --nocapture`: A10-retirement-RED.log, exit101 BEFORE production repair. Predecessor80654, replacement80686, shell80683, root /tmp/fx-v02-NydAV7. Final forwarded close returned HTTP503 HOST_UNAVAILABLE, expected204. Predecessor exited0; test harness reached FAILED and reaped both children, replacement SIGKILL after owned cleanup. Supervisor /tmp/a10-retirement-owsUjN removed.
2. Same command after guards: A10-retirement-GREEN-attempt1.log, exit101, NOT GREEN. Predecessor82027, replacement82081, shell82078, root /tmp/fx-v02-Tnu38L. HTTP204 delivered; predecessor exited0; durable journal contained close status204/state completed/outcome noContent. Replacement operation GET returned HTTP404 OPERATION_NOT_FOUND, exposing stale replacement journal state. Both children reaped; supervisor /tmp/a10-retirement-s0PQTr removed. Original log contains full disposable journal; subsequent test now asserts parsed fields and emits minimal receipt.
3. `cargo test ... --test daemon_handover_contract test_rolling_handover -- --nocapture`: A10-retirement-handover-regression.log, exit101 at compilation, no tests started. Concurrent output worker source machine_output_writer.rs:12 calls nonexistent Message::len (E0599). Not edited here. Supervisor /tmp/a10-retirement-5FnEN5 removed. No blind retry.

LSP handover.rs, machine_gateway.rs and support/machine_retirement.rs: no errors. session_service.rs request cancelled; not clean-LSP proof. git diff --check exit0.

## Unresolved cross-owner boundary

Parent owns session_api.rs, workspace operation adapter and journal. Retirement proof now exposes a durable completed record absent from replacement in-memory reconciliation. Parent was notified with exact failure and requested boundary coordination. This task does not rewrite excluded journal code or weaken operation GET assertion. Final close/replay acceptance remains incomplete until actual operation retrieval succeeds after predecessor retirement.

The tunnel connection guard also requires bounded idle lifetime review. Existing machine sockets have deadlines, but the negotiated HTTP connection wrapper currently has no independent idle timeout. No claim that all bounded-resource checks are done. No cancellation/queued-request drain acceptance claimed solely from happy close.

## Cleanup accounting

New subprocess RED and first repair run use existing PrivateDaemons Drop cleanup with exact-child wait/drain receipts in logs. Test harness reached a final failure result rather than treating predecessor exit0 as test success. No new retained daemon/root from these runs.

Inherited early-exit artifact /tmp/a10-owner-parent.YmuzgZ and its tmp/.tmp810Q43 remain present and untouched, including reported journal temp77763 and historical PTY77838. This report does NOT claim those cleaned or that historical PTY is currently alive/dead. Parent owns artifact capture/disposition. No deletion performed.

## Remaining completion gates

Cross-process journal reconciliation/replay; bounded idle owner connection and cancellation-drain verification; final separate-process test result and explicit cleanup; relevant handover regressions after output source stabilizes; final diagnostics/headless build; updated hashes. Source remains uncommitted. This report is not full A10 or paired-daemon acceptance.
