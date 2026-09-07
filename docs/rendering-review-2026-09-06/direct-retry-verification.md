# D5 independent direct-retry verification

## Verdict

**ACCEPT the isolated D5 host repair and host-level proof. Native visual acceptance
remains BLOCKED by the absent one-scroll Timeout fault-injection screenshot and
actual pixel/reveal evidence.** No host-code or regression-proof blocker was found.
This is not approval of a combined receipt/geometry integration or a native GPU
validation result.

Independent verifier: senpi task `st_01a0783f`, parent session
`01a0770c-1822-77a5-bf82-16601c4157fd`. Scope was the committed host/test delta,
`direct-retry-repair.md`, `DROPPED_FRAME_PHASE.md`, and their raw evidence. Only
this report was edited; no production edits, commit, desktop operation, daemon
launch, or broad test rerun was performed.

## Independently executed verification

The mandated regression was run **once** in the isolated worktree on Darwin
arm64, with normal resources and no `TAURI_CONFIG` override:

```sh
cd /Users/indo/code/project/orca-lite-render-retry-20260906
unset TAURI_CONFIG
CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite/src-tauri/target \
CARGO_BUILD_JOBS=8 \
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::one_shot_render_requeues_when_frame_is_dropped -- --exact --nocapture
```

Observed tool output in this verifier session:

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 0.77s
running 1 test
events=[Acquire, Dropped, Acquire, Presented]
test native_terminal::surface_host::tests::one_shot_render_requeues_when_frame_is_dropped ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 615 filtered out; finished in 0.39s
INDEPENDENT_FOCUSED_EXIT=0
```

The `events=` line above is the event suffix of the emitted `D5_ENTRY` trace,
not a separate instrumentation event. Seven existing compiler warnings remained
visible, at the same unrelated locations as the pre-extraction log. This report
records the independent execution receipt inline; the original raw evidence
files were not rewritten or augmented.

## Reconstruction and right-reason RED

The actual commit is `829fbe8e01e06418f329fa7022141b2fbab3e6e9`, directly on atlas
`17fb10608ad31e0e60ae565e8dc686afdc4735f9`. `git diff-tree` confirms exactly one
changed file: `src-tauri/src/native_terminal/surface_host.rs` (688 insertions,
35 deletions, including eight added tests). The frame-target extraction accounts
for the larger-than-policy-only diff; it remains private and within the allowed
host scope, with the injected variant compiled only for tests.

Both recorded patches were applied **in memory** to the actual atlas parent,
checking every context/deletion line. `final-host.patch` reconstructs the scoped
source byte-for-byte, which also equals the committed file. The reconstructed
pre-rearm source has SHA-256
`c39aa92514e38852b1826e5bac5921dfab5dc48895c8dfc852544468a959cc16`.
It contains neither `rearm_dropped_direct_frame` nor a direct rearm call, and its
line 2253 is exactly the assertion named by `red-one-shot.log`:

```text
dropped direct frame must rearm the attached session coordinator
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 609 filtered out
exit=101
```

Before reaching that assertion, the test has attached actual terminal history,
called the public attached-scroll helper and public `render`, and asserted the
original dropped receipt and `[Acquire, Dropped]`. It therefore fails for the
missing direct completion wiring, not a compile error, absent fixture, or an
unrelated expectation. Removing that wiring leaves this test failing; no test
calls the new policy helper to repair its own setup.

The RED and final GREEN regression bodies match modulo whitespace and trailing
commas. The final harness additionally checks that scrolling changes VT offset,
checks off-thread submission, and awaits submission JoinHandles. Those additions
strengthen rather than replace the original RED contract.

Chronology is accepted from the retained reconstructable artifacts: baseline
27-test characterization; extracted characterization compile failure (`E0106`,
missing sender lifetime); corrected extracted characterization (1 pass); actual
behavioral RED; repaired GREEN. The extraction patch preserves the old direct
return behavior and scheduled retry behavior. Native acquisition is equivalently
extracted as acquire -> one Lost/Outdated reconfigure/reacquire -> classify;
native resource destruction remains surface -> compositor target -> renderer.
This verifier reconstructed the historical states, not a second RED execution
or an independently witnessed historical edit timeline.

## Actual production-connected execution and completion

Source references below are in the scoped worktree. The macOS wheel callback
(`src-tauri/src/lib.rs:542-570`) calls `scroll_attached_native_terminal`, then
dispatches public host `render`. Portable scroll IPC does the same
(`src-tauri/src/ipc/native_terminal.rs:835-887`); focus IPC reaches public
`set_focus` (`:547-575`). The scroll helper (`:686-693`) mutates the real attached
VT and does not schedule a render itself.

In `surface_host.rs:1735-1833`, both direct methods acquire attached host
ownership, prepare/capture the session snapshot, release the session guard, and
render through the existing host. On `Ok(receipt)`, the host guard is explicitly
dropped before direct completion rearm. The original receipt is returned
unchanged; `?` bypasses rearm on fatal errors. Focus without a host retains its
old non-rendering receipt branch and does not enter rearm.

`rearm_dropped_direct_frame` (`:1837-1864`) ignores presented receipts and looks
up the still-attached session's existing coordinator. That lookup guard ends at
the assignment, before `schedule_render`. Only the idle-to-scheduled winner
submits a task. Existing scheduled/follow-up work coalesces, while active
rendering is marked for one follow-up by the existing coordinator (`:185-273`).

`defer_scheduled_render` (`:398-415`) uses the real Tauri async spawn before
main-thread submission; scheduled completion uses this same boundary. The test
queue substitutes only `run_on_main_thread` submission (`:418-428`), not the
scheduled closure. That closure (`:321-395`) performs real begin-render,
host/session lookup, attached/layout checks, snapshot capture and host render.
A dropped scheduled frame marks a follow-up; host ownership is released before
finish-render and any deferred follow-up. Fatal errors are logged without
self-rearming. Dispatch failure consumes pending state and logs its error.

The fixture (`:2257-2406`) keeps the actual session, VT, host map and coordinator.
Its injected frame target calls the **same acquisition helper as native WGPU**.
It simulates acquisition outcomes, reconfiguration and presentation; it does not
run native rendering or compositor reveal. `execute_dispatched` executes only
the closure received from production submission. It does not call render again,
schedule the missing retry, feed daemon output, or inject a second user action.

## D5 requirement dispositions

| Requirement | Checked disposition |
| --- | --- |
| Faithful pre-edit RED, same GREEN | PASS: reconstructable no-rearm source, exact failure line and unchanged behavioral test body; historical GREEN and one independent GREEN execution. |
| Real direct execution seam, not isolated policy | PASS: public scroll/render and focus reach real host completion; no test invocation of the new helper and no manually fabricated second render. |
| Subscription before trigger | PASS: dispatch channel and managed sender exist before attachment/direct operations. The empty daemon message channel supplies no later output trigger. |
| Exact events, deterministic completion | PASS: queued closures are awaited with bounded failure deadlines; no added sleeps/polling. Retry submission JoinHandles are awaited before queue-absence checks, including follow-ups/coalescing. Fatal direct branches have no production submission path and synchronously assert an idle coordinator. |
| Ownership and guard release | PASS: static lock scopes and explicit `drop(hosts)` in both direct entries; fixture asserts host lock held/session lock free during each injected frame and both locks free before executing queued work. Submission-time ordering is established by source, not overstated as a native concurrency measurement. |
| Deferred, nonrecursive dispatch | PASS: real off-thread spawn shared by direct/scheduled completion; fixture rejects retry submission on the caller thread. Repeated scheduled Timeout test covers another deferred follow-up. |
| Timeout then success, no new trigger | PASS: independent exact regression records acquire/drop/acquire/present, final idle coordinator and no extra queued task. |
| Lost/reconfigure then Timeout then success | PASS: `direct_retry_recovers_lost_then_timeout` source asserts the exact six-event sequence; host and final native-terminal logs show it passing. |
| Dropped focus and unchanged receipt | PASS: `direct_retry_focus_preserves_dropped_receipt` retains `presented: false`, changes real focus and completes a submitted retry; successful characterization remains nonpending. |
| Actual detach/close before queued work | PASS at host level: test invokes real lifecycle methods, then submitted work; only acquire/drop/destroy occurs and no host remains. Detach consumes pending work; close removes the session, so queued begin/finish reaches idle without host creation. Native late-reveal pixels are not measured. |
| Fatal error, no self-generated loop | PASS: direct render/focus OOM exits before rearm; scheduled OOM logs and finishes idle. Test acquisition queue and empty dispatch/coordinator assertions cover all three. Existing independently pending input is not promised to be cancelled by a fatal frame. |
| Coalescing with pending render | PASS: a real preedit request supplies the already-pending task; direct drop submits no duplicate, and exactly that task presents. This separate coalescing case is not used to manufacture recovery in the one-shot regression. |
| Host-only scope and neutral extraction | PASS: only the authorized host/test file changed; production target delegation, acquisition classification and field drop order preserved. No public interface/dependency/platform/renderer edits. |
| Related checks and runnable surface | PASS at host level: focused regression independently executed; broader historical checks and standalone host entry audited below, not rerun or represented as native app proof. |
| Native one-scroll screenshot/pixels | BLOCKED: no native Timeout injection screenshot, native presentation/reveal observation, or resulting visible-pixel proof exists in this D5 evidence. Desktop work was explicitly prohibited in this task. |
| Combined receipt/geometry integration | NOT PERFORMED: acceptance applies only to the isolated atlas-based commit; parent-owned integration still requires its own approval and fresh combined checks. |

## Evidence integrity, checks and cleanup

`shasum -a 256 -c SHA256SUMS` independently passed all 20 listed artifacts,
including both patches, RED/GREEN logs, build/check logs, cleanup and crash
receipts. Current source hash is
`afd47a245721c879346778748d217816401460e5f198cb6ef0ddc64abbaffc10`;
current focused-test executable hash is
`c43f9bf47a3502bdf8c34a01cbcee6dce0e7fc76613c8f6c12078b71a394c946`.
Both match the original runtime receipt. `git diff HEAD^ HEAD --check` passed,
and isolated tracked status remained empty after the independent execution.

Audited raw results: 35 host tests, 18 host contracts, 127 native-terminal library
tests, successful Cargo check/build, and successful actual UI production build.
The LSP artifact is a transcription of the implementing child's tool results,
including its earlier timeout and final no-diagnostics response; it is not an
independent LSP rerun. The check/build logs end in successful `Finished dev`
output; their exact shell environment/exit status comes from the implementing
receipt, not embedded environment dumps. Independently executed Cargo above
explicitly unsets `TAURI_CONFIG`. No build or warning suppression was introduced.

The standalone executable's first launch failed in DYLD before entry (missing
`@rpath/libghostty-vt.dylib`, PID 14088); the retained crash report confirms a
launch-time library failure. The corrected `DYLD_LIBRARY_PATH` launch passed
the same host test. Neither is native desktop fault-injection evidence. The
broader library log's Metal atlas checks also do not supply D5 screenshot proof.

The independent test exited; the subsequent process inspection found no matching
focused Cargo/test process. No `/cores/core.14088` or matching `ferryx_lib*`
DiagnosticReports artifact remained. The owned crash report is retained in the
evidence directory. Fixture teardown aborts its stream/pump and removes hosts
and sessions; the retry tests join deferred submission tasks. Existing ignored
`node_modules`, `ui/node_modules`, `ui/dist`, and `src-tauri/Cargo.lock` were
confirmed ignored and left intact. Historical no-kill/no-clean/no-stash/no-reset
claims are preserved in `owned-cleanup.log`; this verifier performed none of
those operations. No remaining verifier-owned resource requires cleanup.

## Archived implementing-child receipt (audited, not independently rerun)

Verified commit: `829fbe8e01e06418f329fa7022141b2fbab3e6e9`.
Source SHA-256 (`surface_host.rs`):
`afd47a245721c879346778748d217816401460e5f198cb6ef0ddc64abbaffc10`.
The remainder preserves the implementing child's command/evidence index.
Independent dispositions and execution results are above; neither section is a
native visual review.

Evidence directory:
`/Users/indo/code/project/orca-lite/.omo/evidence/ulw/rendering-review-20260906/D5/`.

## Ordinary build setup

Darwin arm64 / Apple M4 Max. Branch starts at atlas `17fb106`; prepared Ghostty
clone observed at `6a508fd5e34c7e222c052a6d00bb3891ff3feace`.

```sh
cd /Users/indo/code/project/orca-lite-render-retry-20260906
bun install --frozen-lockfile
bun install --cwd ui --frozen-lockfile
bun run --cwd ui build
export CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite/src-tauri/target
export CARGO_BUILD_JOBS=8
unset TAURI_CONFIG
```

Both frozen installs and the actual UI TypeScript/Vite production build passed.
No resource suppression or manifest/dependency edits were used. Cargo's ignored
lockfile was generated by the ordinary first test invocation.

## RED/GREEN and gates

The exact command below was executed before adding rearm and again after repair:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::one_shot_render_requeues_when_frame_is_dropped -- --exact --nocapture
```

| Evidence | Observed result |
| --- | --- |
| `pre-extraction-characterization.log` | Unmodified host filter: 27 passed. |
| `extracted-characterization.log` | Setup compilation error: missing fixture message lifetime; not RED. |
| `extracted-characterization-green.log` | Exact direct success/focus characterization: 1 passed, pre-rearm. |
| `neutral-seam-and-red-test.patch` | Neutral extraction and RED test before behavioral change. |
| `red-one-shot.log` | 1 failed, exit 101, actual missing coordinator rearm assertion. |
| `green-one-shot.log` | Same exact regression: 1 passed, exit 0. |
| `green-host-suite.log` | All 35 host tests passed, including eight added tests. |
| `final-green-one-shot.log` | Exact final formatted-source regression: 1 passed. |
| `host-contract.log` | Existing host integration contract: 18 passed, none ignored. |
| `final-native-terminal.log` | Native-terminal library filter: 127 passed, none ignored. |
| `cargo-check.log`, `cargo-build.log` | Normal-config check and full default build: exit 0. |
| `lsp.md` | Changed-file final diagnostics: no diagnostics. Earlier timeout retained in receipt. |
| `final-host.patch` | Complete committed host-only delta. |

Additional commands actually executed:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal -- --nocapture
cargo check --manifest-path src-tauri/Cargo.toml
cargo build --manifest-path src-tauri/Cargo.toml
git diff --check
```

Rustfmt was applied through targeted patches to edited regions only, retaining
unrelated baseline formatting. No unrelated warning or failure was suppressed.
Seven pre-existing warnings per Cargo configuration remain in the raw logs
(notifications, font manager, terminal session, worktree manager, and the lib-test
input variable). All final test/check/build gates passed; full repository tests
and native desktop fault injection were not run.

## Executed runtime entry and honest boundary

The final host-test executable was also invoked directly:

```sh
DYLD_LIBRARY_PATH=/Users/indo/code/project/orca-lite/src-tauri/target/debug/build/ferryx-a6612c59d6d9c6ba/out/ghostty_vt/lib \
  /Users/indo/code/project/orca-lite/src-tauri/target/debug/deps/ferryx_lib-c20491cebbac898a \
  native_terminal::surface_host::tests::one_shot_render_requeues_when_frame_is_dropped --exact --nocapture
```

`runtime-entry-green.log`: one passing test, exit 0; recorded actual frame events
`[Acquire, Dropped, Acquire, Presented]`. Public attached scroll/render executes
once; the final frame is from production deferred scheduled completion. Native
acquisition/presentation primitives are injected. No screenshot or native GPU
present is claimed. Executable SHA-256:
`c43f9bf47a3502bdf8c34a01cbcee6dce0e7fc76613c8f6c12078b71a394c946`.

`runtime-entry.log` retains the earlier direct-launch loader failure (exit 134,
missing `@rpath/libghostty-vt.dylib`). The corrected launch uses the observed
normal build output, not a code/configuration suppression. Its owned crash report
is `runtime-launch-setup-crash.ips`; cleanup is in `owned-cleanup.log`.

**Pending:** native app one-scroll Timeout injection screenshot and real visible
pixel/reveal verification. No desktop/daemon launch or user input was used.
