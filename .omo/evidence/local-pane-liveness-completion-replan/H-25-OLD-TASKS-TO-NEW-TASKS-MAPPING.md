# H-25 — old1–old16 → replan-task mapping with retained acceptance

**Plan clause satisfied:** `/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-completion-replan.md`
line 214 (Final verification wave, F1): "**Independently map old1-16/F1-F4 and IS1-6 to current
evidence**; verify exact hashes/argv/counts and no omitted acceptance. Read-only; E/final/F1.md.
PASS only zero unmapped requirements; missing evidence reopens owning task."

**Hole closed:** F1 audit `H-25` — `E/F1-PLAN-TO-ARTIFACT-AUDIT.md`, HOLES / Holes B, row H-25 ("No
artifact enumerates each of old1-16 against its new task with the retained acceptance (the plan's
'Original coverage' row is an assertion, not a mapping artifact)"); audit row `L8` carries the same
finding.

**Author:** host-independent F1-hole lane (`st_01a108d2`), 2026-10-05. Nothing was built, run or
tested. Every correspondence below is derived from the two plans' own text and the F1 audit's rows.

## Sources and authority

| Ref | Source |
|---|---|
| **ORIG** | `/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-root-remediation.md` — the original plan, tasks **old1–old16** + `F1–F4` (16 implementation tasks; old1/12/13/14/16 are marked `[x]`, old15 is `[ ]`) |
| **REPLAN** | `/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-completion-replan.md` — tasks **1–10** + `F1–F4` |
| **COVERAGE** | REPLAN line 236, "Original coverage:" — the replan's own correspondence assertion, used here as the spine of the table (nothing below contradicts it; where it is coarse, the granularity is derived from the two plans' task text and marked) |
| **AUDIT** | `E/F1-PLAN-TO-ARTIFACT-AUDIT.md` — rows `J1`–`J10` (per-new-task requirements), `L8` (the missing mapping), `D1`–`D6` (IS rows), `H-1`–`H-28` (holes) |
| **E/** | `C/.omo/evidence/local-pane-liveness-completion-replan/` (evidence root); `P3/` = `E/task-8/pass3/` |

**Reading the table.** "Retained" = the original acceptance criterion that still governs, verbatim or
in substance, in the new task. "Superseded by" = the original wording/deliverable that the replan
replaced, and what replaced it. "Evidence now" = where the proving artifact lives today, with the
verdict the F1 audit gave it (SATISFIED / PARTIALLY / NOT SATISFIED). Nothing is inferred from a lane
summary.

---

## A. The mapping

### old1 — Freeze provenance and recover compatible unfinished split work → **new1**
- **Retained:** isolated candidate from a pinned commit plus explicitly owned patches; no local
  builds; real remote host/toolchain/source paths recorded; "one intentionally mismatched fixture
  patch is rejected before modifying candidate, record nonzero rejection and unchanged source hash".
- **Superseded by:** the deliverable path. ORIG's `E/task-1-provenance.json` and its "No test/build
  yet" stance → REPLAN's `E/task-1/{manifest.json,logs,ownership.json}` **with** a remote build and
  scoped gates (REPLAN Todo 1 Acceptance: "candidate builds remotely with UI assets first").
- **Evidence now:** `E/task-1/manifest-r3.json` (27 selected files, sha256 `987e2738…`),
  `E/task-1/receipts-r3.json` (`allFilesHashVerified: true`, `unownedResourcesTouched: false`),
  `E/task-1/disposable-fixture-receipt.json` (`rejectionExitCode: 1`, `candidateUnchanged: true`).
  Audit rows `E1`, `J1a`–`J1d` = **SATISFIED**; `J1b`/`J1e` **PARTIALLY** (headless smoke NOT_RUN;
  declared `logs/` subdirectory absent — content present flat).

### old2 — Capture causal input-to-presentation state with bounded diagnostics → **new1/2/5/7/9**
- **Retained:** extend the existing switch-debug/describe observation, never a second tracing
  platform; correlate operationId/incarnation/epoch/binding/attempt generation; never log bytes or
  clipboard data; distinguish idle / unknown / observed blocked; bounded buffers; "Do not assert
  incident cause until the affected stage is observed".
- **Superseded by:** the deliverable bundle `E/task-2-diagnostics.json` (a single JSON) → the
  diagnostics selector's receipts (`E/task-1/rust-diag-r3.*`) plus the frozen contract
  (`E/task-2/contracts.md`) and the native `diagnostic-classifier` scenario owned by new7/new9.
- **Evidence now:** `E/task-1/rust-diag-r3.exit` = 0, `rust-diag-r3.log` (9 passed, windows, r3);
  `pane_liveness_diagnostics_*` inside the linux `pane_liveness_` 54/54 (`P3/linux/logs/K-pane-list.log`);
  `E/task-2/contracts.md`. Audit `D5`, `E3`, `L6` = **PARTIALLY** (the native classifier scenario and
  the headless smoke are NOT RUN; `E/task-2/{logs,manifest.json}` absent = audit `H-8`).

### old3 — Build deterministic private fixtures and real native QA runner → **new2/7/9**
- **Retained:** the runner CLI contract `node scripts/qa/pane-liveness.mjs --scenario NAME --binary ABS
  --evidence-dir ABS --isolation-root ABS`; private event-barrier controls armed before trigger; clock
  tests use paused clocks, real deadlines use bounded event timeouts and no sleeps; registers every
  child/socket/root and verifies exit by platform event/handle, never a broad kill; "Runner cannot
  mark unsupported native automation PASS".
- **Superseded by:** the evidence shape `E/task-3-harness/{actions.jsonl,result.json,screenshot.png,cleanup.json}`
  → REPLAN's `E/task-7/{logs,headless-result,cleanup.json}` for the adapter and
  `E/task-9/SCENARIO/run-UUID/{actions.jsonl,screenshot.png,result.json,cleanup.json}` for the runs.
- **Evidence now:** runner Vitest 28/28 on all three hosts (`P3/{mac,linux,windows}/logs/runner.log`);
  task-1 r3 windows runner 19/19 (`E/task-1/receipts-r3.json` `canonicalRunnerVitest`); **zero scenario
  invocations**. Audit `J7a`/`J7d`/`J7e`, `H-5`, `H-21` = **PARTIALLY / NOT SATISFIED**.

### old4 — Reconcile retained session identity through attach and restore → **new2/4/5/6**
- **Retained:** stable incarnation carried unchanged in trusted transfer metadata, never derived from
  PID alone; typed native attach receipt carrying authoritative backend/incarnation/epoch + attach
  generation; restore accepts a changed epoch only with a matching retained incarnation; older
  metadata stays "recoverable unconfirmed, not auto-dead or auto-adopted"; carry
  workspace/worktree/validated CWD; replay cursor reused only with a proven same sequence domain.
- **Superseded by:** the deliverable `E/task-4-identity` → the frozen contract (`E/task-2/contracts.md`)
  plus `pane_liveness_identity_*` inside the linux `pane_liveness_` set.
- **Evidence now:** `pane_liveness_identity_rejects_wrong_incarnation_and_domain`
  (`terminal/pty.rs:1093` test) and the 4 adoption refusals pass inside linux `pane_liveness_` 54/54;
  `39e722ce` gives paired/ssh-remote sessions a provable lifetime incarnation. Audit `D1`, `J4a`,
  `J4c`, `J4d` = **SATISFIED (unit/contract level)**; `J4f` = **NOT SATISFIED** (no `E/task-4/`).

### old5 — Preserve attributed suspension without changing user job control → **new4/6/9**
- **Retained:** capture the Ferryx suspension receipt only after successful OS actuation against
  verified identities and transfer it with the incarnation; auto-resume only verified matching
  targets; manual/external stops never auto-resume; a vanished/reused PID invalidates signal
  authority; explicit user Resume preserved; platform modules must have real Windows behaviour or an
  explicit unsupported result, "not pretend Unix parity".
- **Superseded by:** `E/task-5-suspension` → `pane_liveness_suspension_*` in the linux set plus the
  suspension module split (`terminal/suspension/{unix,windows}.rs`).
- **Evidence now:** 4 `pane_liveness_suspension_*` tests pass inside linux `pane_liveness_` 54/54;
  windows returns typed `UnsupportedPlatform` (`terminal/suspension/windows.rs`) and is reported as
  unsupported-with-reason (`E/task-9/win/NOTEPAD.md` note 7). Audit `J4a`, `J4e`, `H11` =
  **SATISFIED (unit)**; the native `suspension-ownership` scenario is **NOT RUN**.

### old6 — Connect reliable split create/status/cancel through IPC and frontend → **new2/3/6**
- **Retained:** restore the U coordinator and register `cmd_terminal_spawn_operation` with
  `createOnly`/`preparedLocalSplit`/`remainingMs`; reliable split opts in only with an advertised
  capability and an unsupported old daemon yields an explicit upgrade/unavailable action, never an
  unbounded legacy spawn; validate the frozen identity/fingerprint at the daemon boundary; create
  returns an owned backend before attach; one monotonic remaining budget; same-ID retry asks status
  first; cancel uses a request tombstone and ownership-qualified cleanup; persist unconfirmed state
  until authoritative settlement; existing tests stay green.
- **Superseded by:** `E/task-6-split` → `local_split_reliability_` (15) + `E/task-2/contracts.md`.
- **Evidence now:** linux raw exit 0, 15 selected, `ok. 15 passed; 0 failed` (`P3/linux/logs/H-split.log`,
  `P3/linux/rustK-gates.log:3-4`); mac 15/15; windows 14/14 (both at `39e722ce`). Audit `J3a`, `J3d` =
  **SATISFIED**; `J3c` = **NOT SATISFIED** (the registered command is never exercised with a
  frontend-shaped request; no pump-generation mutation RED); `J3f` = **NOT SATISFIED** (no `E/task-3/`).

### old7 — Fence attachment and presentation generations with truthful UI → **new3/5/6/9**
- **Retained:** a generic pump handle carries a monotonic binding/attempt token compared
  install/remove under one lock; stale attachment aborts only its own stream; positive `presented=true`
  receipt required with every identity field matching; fresh split says Starting and an
  attach/presentation failure offers working Retry/Cancel with no backend loss; hidden-workspace bind
  may finish without claiming presentation; "UI text itself is not pinned in new tests; assert
  state/action behavior".
- **Superseded by:** ORIG's **five-field** receipt match (`frontendSessionId, paneIdentity,
  backendSessionId, bindingKey, attemptGeneration`) → REPLAN's **seven-field** `PaneAttachTuple`
  (adds `incarnation`, `daemonEpoch`). This is the plan's one deliberate contract extension; the F1
  audit records it as "audited as deliberate and strictly stronger (not a defect)"
  (`F1-PLAN-TO-ARTIFACT-AUDIT.md`, closing section; `daemon/protocol.rs:190-215`).
- **Evidence now:** `pane_liveness_pump_generation_*` (3) and `pane_liveness_native_binding_*` (16)
  pass inside linux `pane_liveness_` 54/54; UI split 19/19 ×3 hosts. Audit `J5a`, `J5b`, `J5c`,
  `G10` = **SATISFIED**; `J5d` = **PARTIALLY** (no mutation RED for the generation compare = audit
  `H-14`); `J5e` = **NOT SATISFIED** (no `E/task-5/`).

### old8 — Prove and close handover reader rollback liveness → **new4/9**
- **Retained:** exactly one authorized reader (predecessor parked during export; a provisional
  successor cannot race a resumed predecessor); abort releases the provisional successor's
  reader/handles and returns a relinquishment receipt before the predecessor resumes; a failed or
  ambiguous commit queries the authoritative transaction decision and never blindly resumes; reuse
  the existing handover transaction machinery, no new protocol; per-side read counters and process
  identities recorded.
- **Superseded by:** `E/task-8-handover` → `daemon_handover_transfer_contract` +
  `pane_liveness_reader_rollback_*`.
- **Evidence now:** `daemon_handover_transfer_contract` linux raw exit 0, 5 selected, `ok. 5 passed`
  in 6.21 s at `70eefafe` (vs 1/4 exit 101 at `39e722ce`) — `P3/linux/logs/J-handover-xfer.log`;
  6 `pane_liveness_reader_rollback_*` pass inside linux `pane_liveness_` 54/54. Audit `J4b`, `J4c`,
  `J4e` = **SATISFIED (unit/contract)**; the native `handover-abort` scenario is **NOT RUN** (`H10`).

### old9 — Verify coherent candidate and adjacent regressions remotely → **new8**
- **Retained:** single verifier composing owned patches in a pinned isolated source; changed-file LSP
  before builds and **unavailable LSP recorded accurately**; UI command plus coordinator, input queue,
  agent adoption, workspace ownership/HMR and native pane suites; Rust daemon/client/terminal/IPC/
  preparation/reliable-split scopes; no hidden flags skipping failures; a baseline failure gets
  isolated A/B proof and a scoped repair only if it blocks the deliverable, "never change test to
  green"; exact nonzero counts/exit 0 required; source digest altered after test invalidates the
  candidate receipt.
- **Superseded by:** task number 9 → **8** (the replan reserves 9 for the native matrix). Deliverable
  path `E/task-9-gates/{manifest,commands,results,baseline-findings}` →
  `E/task-8/{manifest.json,commands.jsonl,logs,baseline-findings.md}`.
- **Evidence now:** all four declared files exist; linux `rustK-gates.log` (all-targets 0, split 15/15,
  pane 54/54, qa_barrier 11/11, handover 5/5, full-lib 101 classified); A/B classification per host.
  Audit `J8a`–`J8e` = **PARTIALLY** (LSP record was the gap → closed by `E/task-8/LSP-AVAILABILITY-RECORD.md`;
  mac gates NOT_RUN = `H-22`; full final gates RED with no explicit final finding = `H-24`).

### old10 — Execute real native happy, fault and retention scenarios → **new9**
- **Retained (in substance, unchanged):** run the runner against the exact built candidate in
  authorized isolated native desktops; macOS mandatory and Windows for portable changes; no browser
  substitution; every scenario enumerated; one resource register/cleanup receipt per run; correlate
  the OS key action to the write receipt, the output sequence and the photographed marker; recreate
  the reported conditions; "If reported freeze cannot be reproduced, explicitly retain incident-cause
  unknown and require all causal-class scenarios plus recurrence diagnostics"; "GUI authority lacking
  means this task remains open, not a test-only PASS".
- **Superseded by:** the evidence shape `E/task-10-native/SCENARIO/{actions.jsonl,screenshot.png,result.json,cleanup.json}`
  → REPLAN's `E/task-9/SCENARIO/run-UUID/{actions.jsonl,screenshot.png,result.json,cleanup.json}`.
- **Evidence now:** **NOT SATISFIED** — zero scenario directories; `E/task-9/` holds recon reports only
  (`win/NOTEPAD.md`, `REPORT-PASS*.md`). Audit `J9a`–`J9e`, `H-10`, `H-21`, `I15`.

### old11 — Bind release artifacts to evidence and close owned resources → **new10**
- **Retained:** new unique version; remote build following the current runbook with a host override and
  **no local build**; signed/notarized macOS bundle; record commit + repair deltas, lockfiles/Ghostty,
  host/toolchain, test manifest, binary/bundle hashes; run native split/retained scenarios on the final
  packaged bytes, "not merely prior debug artifact"; production install stays a separate explicit
  authorization; remove only task-owned disposable runtimes/source copies after archival; verify no
  ports/processes/sockets left.
- **Superseded by:** the runbook host (a local macbook build) → the recorded remote build/sign host
  split (`E/task-1/hosts.json` `signingHostSplit`: build on maho-mac, sign/notary on the control Mac).
- **Evidence now:** **NOT SATISFIED** — no `E/task-10/`; version metadata still `2026.928.7`
  (`src-tauri/tauri.conf.json:4`, `src-tauri/Cargo.toml:3`) against the frozen `2026.1003.2`; the three
  signature gates have never run. Audit `J10a`–`J10h`, `H-6`, `I16`.

### old12 — Repair baseline compiler closure with remote verification → **new1 (preserved baseline)**
- **Retained:** the baseline repair is reused as a **source-bound receipt**, not re-executed: ORIG's
  requirement that a designated remote verifier capture the unchanged baseline failure and the
  corrected exit 0 is carried by the preserved O receipts.
- **Superseded by:** re-execution → reuse. REPLAN Must-have bullet 3: "Reuse original completed
  tasks1/12/13/14/16 and named source-bound receipts."
- **Evidence now:** `O/minimal-two-test-repair/{final-index.md,result-v4.md,runner-result.md}` referenced
  with on-disk sha256 in `E/task-1/receipt-clarification-addendum.md`; the reused hunks are named per
  file in `E/task-1/manifest-r3.json`. Audit `E3`, `C2` = **SATISFIED**.

### old13 — Clean task-twelve remote QA resources with receipt → **new1 (preserved baseline)**
- **Retained:** exact-path/process checks proving owned resources removed with a receipt; refusal on
  identity/path mismatch; no broad process kill; retained directories have an owner.
- **Superseded by:** re-running the cleanup → the preserved receipt.
- **Evidence now:** the preserved O cleanup receipt is referenced by `E/task-1/source-inventory.md` /
  `receipt-clarification-addendum.md`; the equivalent later cleanup obligation is carried by new10.
  Audit `E3` = **SATISFIED** for reuse; **not established** here: a line-by-line re-verification of the
  old13 receipt's own contents was not performed in this lane.

### old14 — Restore input queue contract required by baseline callers → **new1 (preserved baseline)**
- **Retained:** the typed requestId callback / optional supplied ID / truthful per-session running age
  contract, without breaking FIFO, generation invalidation, preedit coalescing or bounds.
- **Superseded by:** re-implementation → promotion of the two queue files into the composed candidate.
- **Evidence now:** `E/task-1/task-1-revision2-source-review-addendum.md:62` records the two queue files
  being promoted from excluded to selected in revision 2; the queue semantics are then proven inside
  the UI lifecycle suites (`ui-lifecycle.log` 112/112 ×3). Audit `E3` = **SATISFIED**; audit `J6d`/`H-16`
  record that the input-queue suite itself has no separately recorded run (**PARTIALLY**).

### old15 — Clean diagnostic verification resources after remote checks → **new10**
- **Retained:** bounded exact-path/process checks proving disposable resources removed and no live
  test process or monitor left; retained source/vendor/target paths have an explicit owner and a
  task-11 final-cleanup responsibility; refuse cleanup on identity/path mismatch.
- **Superseded by:** the wave-1 cleanup pairing → the convergence-stage cleanup (new8 receipts) plus
  new10's final teardown. **This correspondence is the replan's coarsest**: `old11/15 -> 10` collapses
  two different cleanup moments into one task.
- **Evidence now:** `P3/CLEANUP-RECEIPTS.md` + `P3/CLEANUP-INVENTORY.md` (Task 8 cleanup, one residual
  disclosed: windows `source-base\src-tauri` empty skeleton, left foreign, not force-fixed);
  `E/task-8/cleanup.json`. The final pass-3 staging teardown is still owed → audit `J8e` **PARTIALLY**,
  `J10g` **NOT SATISFIED**.

### old16 — Repair baseline server compiler defects and verify remotely → **new1 (preserved baseline)**
- **Retained:** the minimal forward repair of the misplaced agent subscription/select logic and the
  missing `prepare_relay`/`replace_relay` API contract, with the frozen Task-2 diagnostic files
  preserved.
- **Superseded by:** re-execution → reuse of the completed repair as a source-bound receipt.
- **Evidence now:** preserved O receipts + `E/task-1/manifest-r3.json` selected hunks; later baseline
  wiring is covered by `cargo check --all-targets` exit 0 on three hosts. Audit `E3` = **SATISFIED**.

### oldF1–oldF4 → **new F1–F4**
- **Retained:** F1 plan-compliance audit mapping old1-16/F1-F4 and IS1-6 to evidence with a PASS only
  at zero unmapped requirements; F2 one code-quality reviewer over the complete final diff; F3
  independent audit of real native actions and viewed screenshots from 9/10; F4 ideal-state fidelity
  1:1 over the IS rows with "missing behavior is new explicit work, not a footnote".
- **Superseded by:** the evidence paths (`E/final-F1.md` → `E/final/F1.md`, etc.). F1 was delivered at
  a different path (`E/F1-PLAN-TO-ARTIFACT-AUDIT.md`).
- **Evidence now:** F1 = this audit + this mapping artifact; F2/F3/F4 = **NOT SATISFIED** (`E/final/`
  absent). Audit `K1`–`K5`, `H-7`.

---

## B. Genuinely unclear or coarse correspondences (marked, not guessed)

| # | Correspondence | Why it is marked rather than resolved |
|---|---|---|
| 1 | **old2 → 1/2/5/7/9** (one task → five) | ORIG's diagnostics work was one task with one evidence file (`E/task-2-diagnostics.json`). The replan split it by *lane* (contract in new2, native receipt in new5, adapter in new7, execution in new9) and by *preserved baseline* (new1). No single new task owns "diagnostics" as its subject; the mapping is real but the acceptance is distributed. The single-file deliverable has no successor file. |
| 2 | **old3 → 2/7/9** | The runner *contract* (old3's core) is now split between new7 (author the adapter) and new9 (execute it). Which new task "owns" the runner's acceptance is not determinable from the plan's text alone: new7's Acceptance says "actual commands/receipt validators drive every required scenario" while new9's Acceptance says "every scenario PASS". Both are needed; neither alone closes old3. |
| 3 | **old11/15 → 10** | Two different cleanup moments (wave-1 diagnostic resources vs the release-stage resources) collapse into new10. Task 8's own cleanup receipts (`P3/CLEANUP-*.md`) do not correspond to *either* old task cleanly — they are a new intermediate cleanup the original plan did not have. |
| 4 | **old12/13/14/16 → 1** | These map to new1 only as *preserved receipts*. Their acceptance is not re-proven in the replan; it is inherited. Whether inherited acceptance still satisfies ORIG's "designated remote verifier captures … nonzero counts" wording is a judgment the plan does not settle — the replan asserts it (Must-have bullet 3) and the audit accepted the reuse (`E3` SATISFIED). |
| 5 | **old9 → 8 (renumber) with a widened scope** | The renumber is clean, but new8's acceptance adds "UI build before Tauri compile" and the A/B classification discipline that ORIG did not state as a task acceptance. The extra wording is new, not retained; recorded here so it is not mistaken for inherited acceptance. |

## C. Coverage check (the F1 clause's own test)

- **old1–old16:** all sixteen rows mapped above. **Sixteen of sixteen accounted for.** Five
  correspondences are coarse or split and are disclosed in section B rather than smoothed over.
- **oldF1–oldF4:** mapped to new F1–F4; F1 delivered (this artifact and the audit), F2–F4 unstarted.
- **IS1–IS6:** audited one row at a time in the F1 audit (`D1`–`D6`): five **PARTIALLY**, none
  SATISFIED — the shortfall is entirely in the native/packaged stages (new9, new10), not in the
  mapping.
- **No original acceptance criterion is claimed as dropped.** The only deliberately changed acceptance
  is the five-field → seven-field receipt match (old7 → new3/5/6), which the audit audited as strictly
  stronger.

**Verdict for the F1 wave:** the mapping artifact now exists, so `H-25`'s "no artifact" objection is
closed. F1's PASS condition ("zero unmapped requirements") still fails for the *evidence* half, not the
mapping half: `E/task-3/`, `E/task-4/`, `E/task-5/`, `E/task-6/`, `E/task-7/`, `E/task-10/`, `E/final/`
are absent and the native matrix has never run. Those reopens are named per row above.
