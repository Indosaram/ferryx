# Why this took so long, and what was restructured so it cannot repeat

**Written:** 2026-10-05, by the lead session resuming `01a0ff5e`.
**Candidate:** `work/local-pane-liveness-completion-foundation`, base `d82b35e4`, HEAD **`b1f249f4`**
(43 commits). Product-code revision: **`91d447e1`** — every commit after it is `scripts/`-only. *(Corrected: this line first said
`89a363a0`; `git log --oneline -1 -- src-tauri ui` returns `91d447e1`, and `89a363a0` touched only two `scripts/` files.)*
**Numeric refresh (2026-10-05, after this report was first written):** the header, the gate figure and the F1-hole count
below were brought current. **A hand-maintained single count of "holes closed" drifted twice in one session (13 → 15), so this
report no longer asserts one: the authoritative per-hole breakdown is the table in
`.omo/ulw-execute/local-pane-liveness-remaining-work.md` §4**, and it must be re-derived rather than quoted.
**Request this answers:** *"작업 이어서 진행, 오래걸린 이유 파악하고 같은 실수 하지 않도록 구성해서 작업할 것."*

**Honesty statement.** Every number and claim below is bound to an artifact or an executed measurement cited
inline. Nothing here was re-run for this document: it is a read-only synthesis. Where a claim is an inference
rather than a measurement it says so. This document is a process finding, not a verification result — the
verification state is in §1 and is deliberately unflattering.

---

## 1. The outcome, stated without inflation

**Done and verified by execution**

| Item | Evidence |
| --- | --- |
| Task 8 — composed candidate verification | **CLOSED, zero candidate-attributed failures.** Decisive gate `daemon_handover_transfer_contract` base 5/0 (6.30 s) → candidate 1/4 (336 s) → **5/0 (6.21 s)** after repair; `cargo check --all-targets` exit 0 on 3 hosts; `pane_liveness_` 54/0·47/0·43/0; `local_split_reliability_` 15/0·15/0·14/0; linux full `--lib` 2646/31/6 with **all 31 pre-existing and zero unclassified** |
| Four real product defects found and fixed | (1) paired/ssh-remote sessions could never prove an incarnation, so attach always failed → runtime lifetime incarnation `39e722ce`; (2) the handover successor consulted its own empty registry as authority, so transfer aborted before commit → `70eefafe`; (3) `split_journal`'s temp filename contained `::`, illegal on Windows → `abd9e890`; (4) the QA barrier channel was never installed on the GUI boot path → `dd9e6813` |
| Task 9 — first native scenario verdicts | Pane is **native, not DOM**; pane binding settled from a **measured daemon inventory delta** (1→2 sessions) with the verifier's **independent probe agreeing exactly**; the split affordance click **RESOLVES** at `89a363a0` (`code: null`, `scope.origin: "window-root"`, `candidateCount: 1`) where `cfb4374b` gave `SPLIT_RIGHT_NOT_FOUND`; the scenario **reached its own assertion sequence for the first time** |
| F1 plan-to-artifact audit | Delivered: **164 requirement rows — 62 satisfied / 66 partial / 36 not satisfied, 28 holes** |
| F1 holes closed | **Most of the host-independent set** — H-1..H-5, H-8..H-11 (evidence inventory), H-16, H-17b, H-18, H-19, H-20, H-24, H-25, H-26, H-27, plus the audit itself. **The authoritative per-hole table is `remaining-work.md` §4**; do not quote a single count from here |

**Not done, and each with its reason**

| Item | State | Reason |
| --- | --- | --- |
| The frozen gate (**96** = 76 main + 20 retry) | **NOT RUN** at any revision | cannot run on this Mac (project rule); the build host is unreachable. The count moved 87 → 90 → 93 → 95 → **96** as harness commits landed; each figure belongs to the revision that produced it |
| mac nine-scenario matrix, mac gates, **Task 10 packaging/signing** | **parked** | maho-mac offline; **user instructed the park** |
| Windows full `--lib` | **no verdict; positively a HANG** (397 866 B byte-frozen for 31 min on the `ssh::bridge` test) | host-dependent |
| mac full `--lib` | **never ran to a verdict at any revision** | host-dependent; acceptance gap |
| `qa_barrier` under the QA features | **re-verified green** (`314251e0` 19/19; pass 4: 61 selected all pass) | closed — see the correction note in §4.7 |
| H-17a exact `pane_liveness_contract` filter | **RED, and now SUSPECT** | ran inside the maho-win disk-exhaustion window (§2.4) |
| H-14 / H-15 mutation RED | not started | need a build host |
| H-12, H-13, H-21, H-23 | not started | need interactive runs |
| H-6, H-22, H-28 + 6 unrun scenarios | parked | mac |
| F2 code-quality review | **in progress** (read-only lane) | running now |
| F3 / F4 | **blocked** | no screenshot exists in any of the runs, and no scenario has a PASS row to inspect |
| Any recognition/image verdict | **not claimed** | **no capture was ever produced** |

**The single most important honest number:** across **41 recorded runs** the outcome is **23 BLOCKED / 18 FAIL /
0 PASS**, only **3 of 9** scenarios were ever executed, and **no screenshot exists**. The split scenario now
clicks its own affordance and stops on the product's `split-create` barrier settlement — real progress, and not
a pass.

---

## 2. Why it took so long

Six causes, ordered by how much time each consumed. They compound: 3 and 4 are pure overhead, but 1, 2, 5 and 6
are *structural* — they made serial rediscovery the only available path.

### 2.1 The measuring instrument did not exist and had to be built before any answer was possible

Task 9 could not observe the product at all. The QA surface had to be authored from nothing —
**11 receipt producers and 9 command consumers** behind `local-split-qa` + `native-terminal` (`c34b90fc`) — and
before that a **blocking finding** showed the GUI boot path had **no QA producer surface whatsoever**
(`dd9e6813`).

Consequence, visible in the commit chain: **the first ten verifier passes are dominated by harness defects, not
product findings** — fixture kinds (`5245e6ba`), fixture construction (`1df40271`), an `E0275` codegen overflow
(`91d447e1`), reaching the interactive session (`15042b0f`), newline-joining every PowerShell builder
(`120bc965`), force-reaping the runner tree (`7a452a77`), resolving the affordance across owned windows
(`42fba06f`), restoring the suite to green (`beb80b72`), serving the frontend and binding a pane (`48b4ed93`),
warming the lazily-built UIA tree (`757f8414`), a bounded re-attach loop (`04cf8c7a`), session-dir isolation
(`5c423880`), and the app-stdio sink (`562144b3`).

**This is the largest single cost, and it was structural:** the plan asked for a native end-to-end scenario
matrix without first asking whether the product could be observed in a native run. The answer was no.

### 2.2 Serial, one-layer-at-a-time discovery — each layer hid the next

The pass sequence reads as a chain of newly-unblocked measurements: pass 5 here-string → 6 selector → 7
force-reap → 8 lazy UIA → 9 contradiction → 10 classification → 11 session-dir leak → 12 spawn link → 13 producer
tuple → 15-17 the stall → 18-19 burst shape → 20 two harness defects → 21 the click resolves and the blocker
moves to the barrier.

**The clearest proof that this was structural rather than unlucky:** pass 20 **could not have seen** the
split-scope defect until the pane step worked, and pass 21 **could not have seen** the barrier-settlement gap
until the click worked. Each fix was a *precondition* for the next measurement, so no amount of care could have
found the later defects earlier — only a different decomposition could.

### 2.3 An intermittent host stall consumed five passes and forced a budget design

`2a741884` added bounded retry for the bursty session-1 delegation stall; `cfb4374b` raised the budget to
outlast a **measured 153 s** burst. Five passes (15-19) went to it: localisation, refuting a wrong root cause,
measuring the burst's *shape*, exhausting the budget, then confirming the new one. The distribution is now
**1 / 15 / 26+ attempts** (~260 s is a **lower bound**).

The stall's cause is **still unknown** and it is **self-clearing** — so the cost is not the stall itself but the
passes spent trying to make it deterministic.

### 2.4 Environment outages, each parking already-verified work

- **The external volume went away mid-session** (ledger: *"2026-10-04 ~18:xx KST — external volume loss and
  recovery"*), and the work only resumed after a remount.
- **maho-mac went offline** and the user parked all mac-dependent work — which is why Task 10 (packaging and
  signing) and the whole mac matrix are parked, not failed.
- **maho-win's C: filled up.** A stored measurement from this host (2026-10-05, another plan's rounds) shows an
  **isolated `CARGO_TARGET_DIR` growing to 38.64 GB** and taking C: to **0 GB free**, producing exactly this
  plan's symptoms: `ENOSPC` on writes, **empty `.exit` files**, `Tests no tests`, and **`ssh` failing at the
  handshake** (`kex_exchange_identification: read: Connection reset by peer`, later `Exceeded MaxStartups`) —
  **while ping still answered with 0 % loss**. Deleting that one directory returned 31.51 GB.

  Two consequences that matter for honesty: the verifier's SSH refusal and the lead's own kex refusals are most
  likely a **full disk, not a dead host** (not yet verified); and **H-17a's `exit 101` — plus its isolated-target
  retry's `exit 101` — happened inside that window, so they are SUSPECT and must be re-measured before being
  attributed to the test or the code.** The verifier's isolated `h17a-target` was itself a ~38 GB-class artifact
  on a host already near full.

### 2.5 The pre-existing gates were RED on all three hosts, so the suite could not act as a signal

From the F1 H-24 findings, at `21dea3c0`: `bun run --cwd ui test` exits 1 on **mac (192F/6329P)**, **linux
(1224F/5297P)** and **windows (193F/6323P/5skip)**; linux full `--lib` 2646/31/6; windows' last verdict
2201/70/8. So "is my change green?" could not be answered by reading a line — it required **A/B classification
of thousands of tests**, which is what most of Task 8's cost actually was.

The durable consequence is recorded rather than smoothed over: **no green full-UI run ever existed at any
revision**, mac and Windows full `--lib` never reached a verdict, and H-24 states plainly that the accepted
dispositions **do not discharge "the gates are green"**.

### 2.6 Two product defects were only discoverable from a composed, running candidate

`39e722ce` (paired/ssh-remote sessions could never prove an incarnation, so attach always failed) and
`70eefafe` (the handover successor treated its own empty registry as authority) are invisible in any single
task's unit tests. They required the **composed** candidate to be built and exercised — which, given the plan's
dependency matrix (`1..7 → 8 → 9 → 10 → F1..F4`, a chain rather than a fan-out), meant they could not surface
until the very end. That is why the *most severe* findings arrived *last*.

---

## 3. What was restructured so it does not repeat

Each item is a mechanism that exists in the tree or the process now, with where it lives.

### 3.1 Measure a frozen revision, and name it

Every verifier pass names the revision it measured, and an invalidated revision is **discarded and re-measured**
rather than carried. Pass 21 declined to measure the old revision when the fixes had not landed — correct
behaviour, and it is why its verdict is attributable to `89a363a0` alone. The verifier also proved that
`14744c8a` touches only `scripts/` (`git diff --name-only cfb4374b 14744c8a -- ui src-tauri` is empty) so its
H-16/H-17 results are validly attributable to **both** revisions — the right way to avoid a redundant re-run.

### 3.2 Route a failure to its owner instead of patching across ownership

Harness defects went to harness lanes, product defects to product lanes. The lead stopped making cross-ownership
edits, which is what kept the candidate's product history clean: `bcb76cfd` is **`scripts/`-only (6 files,
+543/−8)**, nothing under `src-tauri/` or `ui/`.

### 3.3 Record a blocked dependency instead of re-attempting it

Host-dependent gates are recorded as **NOT RUN with their unblock condition**, not retried in a loop. This is
why H-24 can state "mac full `--lib` — no later result at any revision" as a *fact* rather than as an absence,
and it is why the remaining work is a list with owners instead of a backlog of repeats.

### 3.4 A persistent ledger plus a one-file handoff map

`.omo/ulw-execute/local-pane-liveness-completion-state.md` is a pass-by-pass record with explicit SUPERSEDED
markers, and `.omo/ulw-execute/local-pane-liveness-remaining-work.md` is the single file a new session reads to
know where to resume. The ledger now opens with a **LATEST STATE** block. This exists because context loss was
itself a cost: without it, a resuming session re-derives state instead of acting on it.

### 3.5 The citation rule — learned from a real error this session

The H-24 artifact marked the `qa_barrier` gate as *"repaired, not yet re-verified — an open verification
obligation"*, citing a ledger line reading *"Still owed: re-verification of `314251e0`"*. **That was a
forward-looking dispatch obligation, written before the runs landed.** The measured results existed in the same
file: `:2032`/`:2095` (PASS-2, at the repaired revision) record **`qa_barrier` 19/19** with all four selectors
green, and `:1855` (PASS-4) records **61 selected, all pass**.

**The ledger is not chronological** — sections were inserted at different points, so a larger line number does
not mean a later state. The rule is now written into both the ledger and the artifact: **cite the measured
result, never the obligation that preceded it; when auditing a gate, search for the latest *measurement*, not
the last *mention*.** The lane then swept every gate disposition for the same error class, which changed one
disposition (G9) and refined one state (windows full `--lib`: "not run" → **positively a hang**).

### 3.6 A durable trap list, so an environment trap is paid for once

`REPORT-PASS21-verifier.md` carries **20 traps**, each a measured environment or process fact rather than a
guess. Examples with the highest recurrence value: *a Windows scheduled task does not inherit the interactive
PATH*; *`serve-dist.mjs` takes the DIST dir — passing `ui/` serves Vite's dev shell and the app renders blank
while the HTTP check still passes (200 + `id="root"`)*; *`isAdmin=True` from `WindowsPrincipal.IsInRole` is a
false positive over SSH — prove access by the operation, not the group claim*; *a number read from a DIRTY tree
is a draft, not a decision*; **`Watching a file for CREATION is not watching it for CONTENT`** (the defect that
silently discarded a completed inner run carrying the first native verdict); **`stage from the committed blob`**
(`git archive <rev> scripts | tar -x`), never `cp -R` from a working tree a lane may be mid-flight editing, so a
foreign half-written edit is *structurally* excluded rather than merely checked; and **`a foreign process
holding your link target is a reason to move your build, not to kill`** — the verifier hit `LNK1104`, enumerated
the holder (**PID 24632, started `2026-10-04T23:20:14`**, ~7 h before its session), established it was not
its own, **reported it without killing**, and retried into an isolated `CARGO_TARGET_DIR`.

### 3.7 Separate measurement from diagnosis from fix

Pass 21 reported the `split-create` barrier gap as **a finding, not a diagnosis**, and named the three open
possibilities instead of picking one. The follow-on work then followed a **fixed measurement order**:
(1) harness — measure the session delta and preserve the barrier dir; (2) only if no session appears, apply the
QA-gated product diagnostics; (3) only if the cause still stands, align the `"leaf-default"` fallback — **kept
separate from the observability patch so the result stays interpretable.** The diagnostics were deliberately
**not committed**: a Rust change that cannot be compile-verified must not land on the candidate, so it ships as
an **apply-ready patch** with a `git apply --check` proof instead.

### 3.8 Make the instrument capable of the negative answer

`bcb76cfd` exists because pass 21 produced a number that **could not discriminate**: app stderr is **180 bytes in
both** pass 20 (click failed) and pass 21 (click resolved), so "no second log line ⇒ no split" was a
**non-sequitur** — the split flow is invisible in app stderr, because the receipt is emitted on
`cmd_terminal_spawn`'s create-only early path, which **returns before** that command's only log line, and
`cmd_terminal_spawn_operation` logs nothing at all. The harness now records a **daemon session delta** after the
click (2 → 3 means the split created; unchanged means the flow bailed before create) and **preserves the barrier
hub** so receipts can be *inspected* instead of *inferred*. It also added an **app-stdio byte delta** as a
**self-check on the lead's own claim** — and the pass-21 reading confirms it rather than falsifying it: the whole
app stderr is the pane step's single spawn line (96 B + newline) plus the harness close marker, so the split
click added **0 bytes**.

### 3.9 Make the plan auditable continuously, not at the end

The F1 plan-to-artifact audit ran as its own **read-only** lane, in parallel with execution, judging every row
from artifacts and executed evidence rather than from lane summaries. That produced the 28-hole list that now
drives the remaining work — and it is exactly the check whose absence let 36 requirements go unmet unnoticed for
most of the effort.

---

## 4. The two lessons worth carrying beyond this plan

**4.1 Build the observation instrument before promising the observation.** The dominant cost was not
implementation, verification, or even the host stalls — it was that a native end-to-end matrix was planned
without first establishing that a native run could be *observed*. Ten of 21 passes went to the instrument.
The restructured order is: prove the instrument can produce a **negative** answer, then measure.

**4.2 A green suite is an instrument too — and it was red.** With 6521 UI tests failing on all three hosts, the
cheapest signal available ("did my change break anything?") cost A/B classification instead. Restoring a green
baseline is not housekeeping; it is what makes every later verification cheap. The plan recorded the absence
honestly (H-24) rather than declaring the gates green, which is the right call and is the reason the remaining
work is legible.

---

## 5. What remains, and exactly what unblocks it

**Unblocked and running now:** the F2 code-quality review of the complete diff `d82b35e4..bcb76cfd` (read-only
lane).

**Blocked on maho-win returning (disk first):**

1. **Free space, then re-measure H-17a** — its `exit 101` is suspect (§2.4). Prune the isolated target dirs
   (`CARGO_INCREMENTAL=0` for check/test runs); **never delete other sessions' space** (`ferryx-releases`,
   `ferryx-build`, the stage's `src-tauri/target`).
2. **The frozen gate at the current HEAD** — expect **96** (76 main + 20 retry).
3. **One split scenario**, then read `split-inventory-after` against `pane-inventory-after`:
   **2 → 3 with `delta.added`** means the split created a session and only the receipt is the gap; **unchanged**
   means the flow bailed before create — which then justifies the diagnostic patch, and only then the
   `"leaf-default"` fallback fix.
4. **H-14 / H-15** mutation RED (staged revision, not the working tree).

**Blocked on maho-mac returning (user-parked):** the nine-scenario matrix, the mac gates, and **Task 10**
(packaging the verified bytes as version `2026.1003.2`, which needs codesign/stapler/spctl). Note the host's
`target/` was purged and some source mtimes were pushed to 2030-01-01, so its next build is a full cold build and
will press the disk.

**Blocked on a capture existing:** F3 and F4. **No screenshot has been produced in any of the 41 runs**, so no
recognition verdict is claimed; an image reader must preserve the `_` separator when a capture finally exists.

**Then:** the final report closes with the remaining F1 holes (see `remaining-work.md` §4 for the authoritative list), F2/F3/F4
verdicts, and the Task 9/10 results — with the gate counts and every disposition bound to the revision that produced them.

---

## Postscript (2026-10-05, after the F2 audit returned) — read this with §1

§1 above was written **before** the F2 code-quality audit reported, so it must be read with this postscript. Nothing in §1
became false, but **the candidate is not clean**, and two of §1's rows need their scope narrowed:

- **F2 returned FAIL over `d82b35e4..bcb76cfd`** — 15 findings, **0 blocker / 4 major / 6 minor / 5 note**. Four of those
  findings are **real defects in this candidate**, and one of them I escalated to **blocker** because it violates the plan's
  own text: **Windows Suspend is dead**. Base `d82b35e4` actuated through the working `TerminalSignal::Stop` →
  `windows_suspend::suspend_process` (`NtSuspendProcess` + `K32EmptyWorkingSet`); the candidate routes
  `service.rs:533` → `suspension.rs:166` → an **unconditional `Err(UnsupportedPlatform)`** stub. Plan **line 148** requires
  *"Windows uses its actual process suspend/resume and session-host mechanisms"* and **line 89** requires Windows
  suspend/resume contract tests, so the stub is a plan-requirement violation — and the candidate **locked the inverse in with
  a test** (`unsupported_backend_never_claims_or_resumes_a_stop`) that asserts all three functions fail. Blast radius, stated
  accurately: **no UI component invokes the manual suspend action today**; the reachable surface is the idle auto-suspend
  sweep, and the failure is **fail-safe** (session keeps running) — a shipped capability silently dead, not destructive.
- **F2-2**: `retain_spawn_owner()` has **no production caller**, so `spawn_owners` stays 0 and the three `HANDOVER_BUSY`
  guards can never fire. **The candidate's own compiler said so**: `all-targets.log:882` *"struct `SpawnOwnerGuard` is never
  constructed"*, `:888` *"method `retain_spawn_owner` is never used"* — the candidate shipped with its own compiler telling it
  the guard was dead.
- **Four more findings were defects in the harness commit I ordered this session** (`bcb76cfd`): the post-split measurement
  could **turn a settled scenario into a reported correctness failure** (an unguarded `snapshot()` whose budget `consume`
  throws `ASSERTION_FAILURE`, which maps to FAIL), and its instrumentation time was charged to **no** budget while sitting
  inside the measured window. **All four are now closed** across `57ef3d2d`, `0726c70c`, and `b1f249f4` — with tests that
  assert the property that was missing (`cancelReceipt.cancelAckMs === 5` — a throwing measurement cannot move the
  scenario's own settlement). **The lesson is in §3.7's spirit and worth its own line: a constraint stated as an intention is
  not a constraint.** My brief said "a measurement, never a verdict change"; the lane satisfied the letter and violated the
  intent, and I did not catch it because I wrote a goal instead of a **testable property** plus a test that fails against the
  current code.

**Consequences for §1's table.** Task 8's closure and the four product defects it lists stand unchanged. But **"Task 9 —
first native scenario verdicts"** must be read with F2-1 open: the Windows half of this plan **cannot be delivered as the
candidate stands**. And the **"Done and verified"** framing for the candidate as a whole is wrong until the F2 patches are
applied and compile-verified: **all 15 F2 findings now have a disposition** (8 closed; 6 with an apply-proven patch awaiting a
host — F2-1/F2-2 in one patch, F2-3/F2-4/F2-5/F2-7/F2-8 in a second; F2-6 documented as a deliberate narrowing needing a host
measurement; F2-15 closes only by running the gate), and **I verified the apply sequence myself** on a fresh
`git archive b1f249f4` copy: F2-1 `--check` 0 → apply 0 → F2-REMAINING `--check` 0 → apply 0.

**The honest closing state:** this session's **requested work — resume the effort, diagnose the delay, and restructure so it
cannot repeat — is complete**, and the delay diagnosis in §2/§3 is the artifact that answers it. **The pane-liveness
completion itself is not finished**, and it now has a sharper reason than "the hosts are down": **the verification gate F2
found a plan-requirement violation in the candidate that the task-level tests never could have caught**, which is precisely
the value the audit separation in §3.9 was restructured to buy.

