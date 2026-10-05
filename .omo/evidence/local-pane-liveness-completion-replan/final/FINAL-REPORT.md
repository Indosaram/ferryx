# Pane-liveness completion — final report

**Written:** 2026-10-05. **Request it answers:** "남은 거 없이 다 작업 해" (finish everything remaining), and
before that the standing brief to resume the work, find why it took so long, and restructure so it cannot repeat.

## 1. Outcome

**On main, verified by execution:**

| Landing | Content |
| --- | --- |
| `af95b9a7` | The whole pane-liveness completion candidate (14 conflict hunks resolved by reading both sides) |
| `c43b3e5e` | The pane-inventory budget-receiver regression (a regression **this work stream introduced**) |
| `a13494e0` | Three split-path defects found by reading, plus the earlier fix batch |
| `9be5eb3f` | The `split-cancel` capture gap — the last adapter without an owned-window capture |

**Product defects found and fixed in this campaign** (each with its own evidence):

1. Paired/ssh-remote sessions could never prove an incarnation, so attach always failed.
2. The handover successor consulted its own empty registry as authority, so transfer aborted before commit.
3. `split_journal`'s temp filename contained `::`, illegal on Windows.
4. The QA barrier channel was never installed on the GUI boot path.
5. Windows suspension had been replaced by an unconditional unsupported stub (F2 repair), plus
   `retain_spawn_owner` wiring, the `"leaf-default"` silent no-op, off-runtime handover abort, and the
   native-snapshot race flag.
6. `pane-binding.mjs` detached `budget.consume` from its receiver, silently disabling both pane reads.
7. `unregisterLocalSplit` had **zero callers**, `SPLIT_PANE` silently dropped its session on a layout refusal,
   and `run()` had silent pre-flow returns — the three reasons a stalled split was unattributable.
8. `runSplitCancelScenario` had no capture and no inspection handshake.

## 2. Verification

Tiered, and the tier is stated rather than implied:

| Change | Verification |
| --- | --- |
| Split-path defects (7) | Typecheck A/B **0 errors, identical error sets**; frozen gate A/B **98/98** both legs; scoped UI **119/119** both legs; re-run **on the merged tree** before landing |
| `split-cancel` capture (8) | Frozen gate **98 passed (98)** with the change; **mutation** (adapter tail removed) → **1 failed | 97 passed**, exactly the cancel test red |
| Budget-receiver regression (6) | Gate **98 passed (98)**; mutation (bare call restored) → **1 failed | 97 passed**, exactly the new test red |

**A/B discipline**: the stage was proven **byte-identical to the base revision** for every changed file before
each comparison, and both legs ran in the same session on the same host, so a difference is attributable to the
change and not to host load. Where a fix touched a file another session had also edited, the **merged tree** was
re-verified rather than assumed.

## 3. What is NOT verified, stated plainly

- **No native scenario has passed.** 23 BLOCKED / 18 FAIL / **0 PASS** across 41 runs.
- **No screenshot exists** from any native run, so F3 cannot pass.
- **Task 10 never ran**: no packaged bytes, no signature/notarization evidence.
- **The mac half is parked** by the user's instruction; several holes close only there.
- **Windows `cargo test --lib`** is the documented hang; `ui build` and the frozen gate run there, the full lib
  suite does not.
- The freeze's **historical stage is unknown** and is recorded as unknown, not invented.

## 4. Why it took so long

The recorded root causes, each measured rather than asserted:

1. **Attribution without observability.** The split flow logged `split.pane.dispatched` **unconditionally**, so a
   no-op was indistinguishable from a split whose backend call vanished. Many passes were spent inferring an exit
   from an **absence**. The fix was to make each exit name itself.
2. **A gate that could not see the defect it was written for.** The pane-binding reader test passed a hand-rolled
   stub whose `consume` was an **arrow function** — no `this` — so a detached method call passed. The mock did
   not preserve the behaviour under test.
3. **Success-path tests that never reached the code under audit.** Every `runSplitCancelScenario` test is a
   success path with a minimal inline driver; the sibling adapters' tests only exercise rejection paths. So
   wiring the missing capture broke nine tests at once, which is why the obvious one-line fix had to be reverted
   the first time and done properly with its tests.
4. **Host dependency treated as a code problem.** Long stretches were spent on items that needed an interactive
   native host (the scenario matrix), while the host was intermittently unavailable. Several holes were
   mis-classified as blocked when they were only *unrun*.
5. **My own misdiagnoses, corrected in place.** I attributed a host outage to disk exhaustion when the SSH banner
   said `Exceeded MaxStartups`; I read `split.pane.dispatched` as proof a layout leaf was created when it is
   logged unconditionally; and I set a disk guard at 6 GB on a host whose builds consume 30 GB+, so it fired only
   once the damage was done.

## 5. What was restructured so it cannot repeat

- **Exits name themselves.** A refusal or an early return now emits a typed diagnostic carrying the reason and
  the discriminators, so the next run attributes by reading one line instead of inferring from an absence.
- **Mocks must preserve the behaviour under test.** The regression the stub hid is now covered by a test that
  uses the **real** `MonotonicBudget`, which is why the mutation leg turns red.
- **Every adapter gets the full tail.** Capture and the inspection handshake are no longer optional per adapter,
  and the gap between them is closed by a test that fails when the tail is removed.
- **Land by merge, verify the merged tree.** `origin/main` may be ahead; the branch is merged and the gates are
  re-run on the merged content before main moves, because a merge can silently change a file another session
  edited.
- **A/B everything, with the stage proven byte-identical to the base.** No claim of "my change is neutral" is
  made from a single leg on a shared host.
- **Host reality is measured before it is planned around.** The disk guard threshold, the SSH failure **mode**
  (`refused` vs `MaxStartups` vs `timeout`), and whether a host even has a JS stage are all measured now.

## 6. Remaining work

Everything left is **execution on a host**, not code:

1. Re-run the native scenario matrix where the split step settles, producing the screenshots F3 needs and the
   success row F4 needs.
2. Task 10 packaged bytes (mac), then the signature/notarization and packaged-smoke clauses of IS-6.
3. The mac-parked holes (H-6, H-22, H-28) and the mac scenarios.
4. Re-audit F3 and F4 once a scenario passes.
