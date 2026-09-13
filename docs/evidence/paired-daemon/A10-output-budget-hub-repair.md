# A10 machine output hub candidate

Task st_01a0986b; 2026-09-13; macOS arm64. Additive candidate, NOT a completed socket repair. No Local/SSH receiver types or capacities changed. No service, owner-routing, remote socket, Cargo, UI, PTY implementation or foreign worktree files edited.

## API and minimal integration contract

`TerminalOutputHub::subscribe_machine(session_id, after_sequence) -> Option<Result<MachineAttachment, MachineOutputError>>`

Module: `terminal::output_hub::machine_output`. Missing session is None; oversized replay is Overflow. `MachineAttachment` contains `ChargedOutput<AttachmentSnapshot>` and `MachineReceiver`. Snapshot registration and capture share the publisher's write lock; machine sender is installed first. Machine snapshot uses flat history; its segmented history is empty. Sequence/gap and bracketed-paste replay semantics use the existing history implementation. Legacy SessionAttachment stays exactly compatible.

1. Resolve the authoritative owning hub first. Do NOT subscribe a gateway-local hub for a routed legacy owner. Owner plumbing is outside this task.
2. Destructure attachment; clone `receiver.termination()` before entering the writer loop. Select `status.wait_for(|state| state.is_some())` against **every** socket write, including attached JSON and snapshot. This sees already-fired overflow, unlike waiting only for a future change. Overflow immediately publishes this independent watch state and removes that sender from hub fanout even if consumer never calls recv again.
3. Keep the snapshot guard alive through encoding, sending and flushing its binary replay frame. Drop it only after successful flush or after discarding the transport. Then use `receiver.recv().await` and hold EACH returned guard through send/flush. No clone-and-release into a second queue, detached writer, `feed` without flush, or buffering across guard release. Releasing a guard means the socket no longer owns pending output for that frame. The queue shares original Arc payloads; no per-machine payload copying occurs during publish.
4. Charge is **logical pending serialized output**, not process RSS: queued payloads + in-flight payloads + wire allowance + control reservation <= 1,048,576. Replay reserves selected retained payload bytes plus 8 possible bracketed-paste prefix bytes plus 512 wire bytes. Live frames reserve payload length plus 512. The current OSC codec's fixed field names, at most five decimal u64 strings, prefix, BEL, hard reset and maximum server WebSocket header fit within 512 bytes. Integrator must verify actual encoded frame length <= payload length + 512 before send; reject transport serialization changes exceeding it. Do not split one charged payload into arbitrarily many framed messages. Encoding transiently copies payload bytes; that is not a second logical pending message and this bound is not an RSS claim.
5. A permanent 16,384-byte reservation covers **all simultaneously pending non-output socket controls**, including initial attached JSON, pong/error/status. Bound their aggregate encoded bytes to this allowance, including WS headers, and serialize/flush them without a separate backlog. Do not assume a 16KiB JSON payload plus framing fits. This reservation stays until receiver drop. Session ID/target must be validated at the existing authenticated boundary. `pending_bytes()` includes this reservation and in-flight guards.
6. On overflow/closed, cancel the active send, discard/drop socket and attachment/guards promptly, release controller using existing fencing semantics. Never keep using the partially written connection. A slow consumer is disconnected, not the PTY. Reconnect with last successfully applied cursor and the original owner target; replay/gap is explicit. Keep the ten-second progress deadline and authorization/fencing selections. Hub tests do not prove those socket operations.
7. There are at most 1024 pending entries, independently of byte permits; empty gap controls cost 512 each. A full entry queue also signals Overflow. No producer async waits. Semaphore acquisition and mpsc send are try-only; publication holds the pre-existing synchronous hub lock, never a lock across network work. Dropping a receiver releases queued payloads immediately; inert sender handles are pruned on next publish/subscribe. In-flight guards intentionally retain their charge until dropped.

If existing owner attachment abstractions cannot carry this additive type, coordinate the minimal shared-file edit with parent. No such files were crossed here.

## Actual RED before production edits

`cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test a10_output_budget -- --nocapture`

Log `A10-output-budget-hub-RED.log`, exit 101:

```
pending payload bytes=1100000, ceiling=1048576
test result: FAILED. 0 passed; 1 failed
```

Baseline used the real `subscribe_with_sequence` receiver with unequal 700000/400000-byte publishes and counted unread payloads. This is a runtime byte-ceiling failure, not a missing symbol or compile error. The regression now selects the additive machine subscription and verifies bounded charge plus independent overflow. Legacy broadcast intentionally remains count-bound.

## GREEN and production publisher evidence

- `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test a10_output_budget`: 7 passed (A10-output-budget-hub-GREEN.log).
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib terminal::output_hub::tests`: 13 passed.
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test a10_output_budget_pty -- --nocapture`: 1 passed.
- `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib`: exit 0.
- `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib`: exit 0.

Last four commands: A10-output-budget-hub-validation.log. Existing compiler warnings remain visible, not suppressed. LSP had no errors in both production files and focused tests; first parallel production requests were cancelled, subsequent requests succeeded.

Actual owned PTY signature:

```
A10_HUB_PTY pid=53668 sibling_bytes=2097152 peak_charged=1047560 ceiling=1048576 reaped=true reader_finished=true
```

The test spawns one private `/bin/sh`/dd PTY using production PtyManager, subscribes two machine consumers before publishing, holds one throughout, and consumes the sibling after each real PTY read. The sibling gets the full 2MiB while held consumer overflows; both consume the same publisher, not mocked payload delivery. The PTY runs concurrently with the test publisher/drainer. Reader output EOF is the exact completion signal, bounded by a 15-second watchdog; close has a 10-second watchdog. No sleep or polling for test readiness. Production close reaps the child and finishes its reader. The test closes its private temp root. This is PTY-to-hub integration, NOT WebSocket slow-transport/deadline/reconnect or multi-pane UI evidence.

Focused tests cover unequal exact-boundary admission, one-byte over-boundary publication with an in-flight guard, release after actual guard drop, immediate dropped-subscription payload release, independent signal without another recv, zero-payload entry saturation, replay eviction gap/order, shared Arc identity, oversized initial replay rejection and up-to-date suffix admission.

All commands used existing private worktree src-tauri/target; jobs=2, dev/test debug=0, incremental=0, RUSTC_WRAPPER empty, explicit /Users/indo/.cargo and /Users/indo/.rustup. Each invocation used a fresh /tmp/a10-hub.* root with private HOME/runtime/data/session/XDG/TMPDIR, removed after command completion. No listeners, canonical daemons, desktop sessions or foreign resources were opened.

## Review and limits

New module owns machine output admission (140 pure LOC); tests own budget lifecycle (86) and owned PTY publisher integration (76 after drain repair). Existing output_hub.rs remains oversized (872 pure LOC): only seven additive wiring lines were added there; restructuring its legacy buffer/tests is deliberately excluded from this ownership-limited fix. Machine behavior lives in the sibling module rather than increasing the legacy module's responsibilities further.

Boundary inputs remain the existing typed session/cursor. No unsafe, casts, production unwrap/expect, dependency, logging, producer task, custom lock-free primitive, or asynchronous producer wait was introduced. Enum errors use exhaustive match. Permits provide deterministic cleanup. No negative-form names or destructive-operation re-query loops were added. New signatures have at most three parameters. The new queue behavior has runtime regression tests. All changes remain uncommitted and subject to concurrent parent integration.

Parent still owns real WS blocked-write cancellation, ten-second controlled-time deadline, owner routing, grant/controller cleanup, reconnect/original PID proof and aggregate Local/SSH/mirror regressions. Do not mark the whole A10 output repair complete from these hub passes.

## Parent cleanup review correction

The original fixture's claim that teardown preceded assertions was incorrect: publish/receive unwraps and the sequence assertion inside the progress future could unwind before close. The prior successful runtime evidence above remains valid for its success path, but did NOT establish panic-path cleanup.

The fixture now wraps the entire bounded progress future in `AssertUnwindSafe(...).catch_unwind()`. Both production close and TempDir close are attempted before inspecting cleanup results or the captured panic. Ordinary assertion panics are resumed after teardown; assertions were not weakened. A second test injects a typed `InjectedPublishFailure` immediately after the first real PTY publish, accepts only that exact panic type after cleanup, and still requires the original session's reaped/reader-finished state plus successful root close. It emits `A10_HUB_CLEANUP` only after those operations succeed. The helper has two callers and owns one responsibility: the owned publisher fixture lifecycle. No production source or API changed in this correction.

Verification attempt: LSP returned no errors for the modified fixture. The single requested focused command (`cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test a10_output_budget_pty -- --nocapture`) was attempted with the same private environment and jobs=2. It did not reach either test: concurrent owner wiring currently fails compilation at `src/daemon/client.rs:118`, E0004, missing `DaemonRequest::MachineSessionDetail` in the request match. Log `A10-output-budget-hub-cleanup-GREEN.log` is an unsuccessful attempt (exit 101), despite its filename. No full build or redundant test rerun was performed; the private invocation root was removed. That foreign shared file was not edited. **The new panic-path cleanup remains runtime-unverified until the owner match compiles and this focused command runs.** No injected-path PID or cleanup signature is claimed yet.

Final source inspection reread machine_output.rs and both test files and inspected the seven-line output_hub diff against this report. Existing admission behavior and documented caller obligations remain unchanged.

### After-source SHA-256 (cleanup candidate, before parent composition)

Paths relative to the worktree:

| Path | SHA-256 |
| --- | --- |
| src-tauri/src/terminal/output_hub.rs | `2647eb6e9d80680669349e2b270eb08f1dcbc5f9e4856326fa9d6718bf2aabfc` |
| src-tauri/src/terminal/machine_output.rs | `72ad1a9ca5bf874dab494af015751e565675372177e23836c237d69b9d7b15c4` |
| src-tauri/tests/a10_output_budget.rs | `40148b7081f6933c4e43f2fcd48b9ebf2f72077b251bc97689b704cb7f4d8c04` |
| src-tauri/tests/a10_output_budget_pty.rs | `11003e2c05474f8eec7835e7d6e68766160953f4de70f2fc56aadd5c744cce19` |

## Composed cleanup failure and drain repair (supersedes blocked verification above)

Parent runtime RED: A10-budget-parent-composed.log, injected mid-publish case failed with `Timed out reaping killed PTY session` while the normal publisher passed. The first cleanup candidate dropped the only PTY output receiver before asking the manager to close. Production session reader exits its blocking_send loop when that receiver is dropped; production close then signals and waits for child reaping before closing I/O. Thus the fixture deliberately stopped output drainage while its high-output child was still alive. The normal path had already drained to EOF and did not create this condition. Keeping drainage active during close changed the actual failing path to GREEN with unchanged deadlines and assertions. This establishes the fixture-level teardown cause; exact macOS kernel wait state was not captured and is not claimed.

Smallest repair: retain the original output receiver and use tokio::join! to drain it concurrently with the existing bounded close. No detached task, sleep, larger timeout, production edit or API change. Both futures finish before cleanup assertions. Panic catching and exact typed failure discrimination remain. Original owned PID/session are now logged before injection.

Runtime GREEN: `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test a10_output_budget_pty -- --nocapture`, 2 passed, 0 failed, 0.09s. Log A10-output-budget-hub-drain.log. LSP: no errors. No standalone build or unrelated suite rerun.

```
A10_HUB_CLEANUP pid=63878 injected=false reaped=true reader_finished=true root_removed=true
A10_HUB_PTY pid=63878 sibling_bytes=2097152 peak_charged=1047560 ceiling=1048576 reaped=true reader_finished=true
A10_HUB_CLEANUP pid=63879 injected=true reaped=true reader_finished=true root_removed=true
```

A subsequent exact-PID process query returned neither 63878 nor 63879. Before this run the process listing showed no surviving a10_output_budget_pty test or dd publisher from the failed parent run, but that old log lacked the injected PID, so identity-specific reaping of that earlier child cannot be reconstructed. The parent supervisor `/tmp/herdr-journal-follow.SkBPnb` was left intact; only this run own /tmp/a10-hub-drain.* environment was removed.

Production follow-up for parent coordination, not changed here: receiver-drop cleanup in PtyManager remains a separate potentially affected path. Also start_lifecycle_watcher takes the reader JoinHandle before close can take it; `is_reader_finished` proves the reader completion flag, not that close itself joined that handle. The focused test observes completion plus output EOF and child reaping, not an exported watcher-join API. Do not mislabel that as a production ownership/join fix. Parent may route that lifecycle issue separately; it is unnecessary to widen this output-budget fixture repair.

The after-source hash table above is refreshed after this repair; the other three hashes are unchanged.
