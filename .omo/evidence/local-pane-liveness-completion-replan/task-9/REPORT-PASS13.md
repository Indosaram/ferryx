# REPORT — PASS 13

## Scope

One question, from the dispatch: **why is `presentation.receipt.jsonl` empty for a UI-created pane, and which
producer can honestly settle the pane binding?** Four sub-asks:

1. Instrument the **producer** (`emit_native_presentation_receipt_qa`,
   `native_terminal/surface_host.rs:1656`) and distinguish four cases: surface host not attached / no frame
   submitted / completion path not run / emitter reached but writing zero lines.
2. Report the native-terminal feature state for that pane and whether the pane is a **native** or a
   **DOM/browser** pane.
3. Name the **honest producer** for the pane binding, and state whether each candidate can carry the
   **seven-field `attachTuple`**.
4. Close the two gaps pass 12b labelled: the isolated daemon inventory and the app's post-click tree, read
   **during** a `5c423880` run.

No product repairs by me. Also: repeat the c5 capture on `5c423880`, and keep the carry-forward traps.

## Revision — stated plainly

**`5c423880`** for everything in this pass (`fix(qa): isolate FERRYX_SESSION_DIR so the app stops restoring the
host's real profile`), staged on the Windows host at
`C:\Users\sook\ferryx-pane-completion\source-21dea3c0`. The staged JS hashes are recorded in pass 12b; the
**binary is unchanged and still correct** because the fix is 6 JS files and 0 Rust:

| | value |
|---|---|
| binary sha256 | `6AD63C5AFDCBB8348FCCD0253D0CDB0066FC054BDA99B41387B5BC5FBEB5B042` |
| binary mtime | `2026-10-05T01:33:34.2716472+09:00` (predates the fix, as expected) |
| daemonVersion | `2026.928.7` |

The pass-12 c5 capture was taken on **`757f8414`**; this pass repeats it on `5c423880` as asked.

## 1. The producer: feature state and the gating path

### The binary carries the producer (string probe, since the staged tree has no `.git` and no build info)

```
string[native_presentation_settled]        = PRESENT   <- the QA emitter's own stage value
string[presentation.receipt]               = PRESENT   <- the receipt file name
string[qa_barrier]                         = PRESENT
string[localSplitLifecycleV1]              = PRESENT   <- the capability the daemon advertises
string[NativeTerminalSurfaceHostState]     = PRESENT
string[surface_host]                       = PRESENT
string[libghostty]                         = PRESENT
```

`default = ["native-terminal"]` in `src-tauri/Cargo.toml`, and the presence of
`native_presentation_settled` — a literal that exists **only** inside the `#[cfg(feature = "local-split-qa")]`
emitter — establishes that **both features are compiled into the staged binary**. So the producer is not
missing from the build.

### The gating path, read from the source

The emitter is reached only through this chain (`surface_host.rs`):

```
dispatch_owned_render → ReleaseOutcome::Released
  → dispatch_owned_render_inner(...)                       // submits the frame
  → waits on slot_settle.subscribe_presentations()          // the presentation signal
  → frame_presented = …borrow().receipt.presented          // must be true
… then, in the completion path:
  let tuple_is_current = current.generation == frame_generation
                         && current.attach_tuple == frame_attach_tuple
  if receipt.presented && tuple_is_current {               // <-- BOTH required
      completion_snapshot_slot.publish_presentation(...)
      if accepted { host.finish_presentation(...)
                    pane_liveness_presentation_receipt(frame_attach_tuple.clone(), true)
                    → emit_native_presentation_receipt_qa(...)   // writes the line
      }
  }
  #[cfg(feature = "local-split-qa")]
  if receipt.presented && !tuple_is_current { emit_stale_completion_rejected_qa(...) }
```

**Two conditions must both hold**: `receipt.presented` **and** `tuple_is_current`. And
`pane_liveness_presentation_receipt(frame_attach_tuple, true)` returns `None` — emitting **nothing** — when
there is no attach tuple, which is why the doc comment says *"a frame without an attach tuple emits nothing at
all."*

**A separate, decisive structural fact about the reachability of that chain:** the QA settlement that wraps it
(`ReleaseOutcome::Released`) only runs when a **barrier release** drives the dispatch. For the `split-happy`
scenario the runner's own plan declares **`barriers: []`** (`scripts/qa/pane-liveness.mjs`,
`SCENARIO_PLANS['split-happy']`) — no `presentation` barrier is pre-armed for this scenario, while
`diagnostic-classifier` *does* pre-arm `presentation`. So **the barrier-driven settlement path is not in play
for `split-happy` at all**, and the receipt can only come from the unconditional
`emit_native_presentation_receipt_qa` call inside the completion path.

## 2. The QA channel has two distinct, non-substitutable sinks

This matters for reading my evidence, so I am stating it before the measurement:

| sink | written by | receives |
|---|---|---|
| `<barriers>/<name>.receipt.jsonl` | the **product's** Rust QA lane (`local-split-qa`), via `active_channel()` | the native producer's lines |
| the runner's `runner.err` / evidence JSONL | the **harness's** Node-side `barrierHub` | only what the harness itself records |

My manual captures set `FERRYX_QA_BARRIER_DIR`, which is what the product lane needs to write the receipt file —
**but the harness's own `barrierHub` is not running in a manual capture**, so the harness's consumer side is
absent. That is a real limitation of my manual method and I flag it where it matters.

## 3. Which pane is it, and what the producer requires

### The UI-created pane IS a native pane — so the receipt is the right producer *in principle*

The chain is unambiguous in the source:

| link | evidence |
|---|---|
| `New Terminal` creates a terminal tab | `handleAddTerminalTab` → `openTab(activeWt, …)` (`ui/src/App.tsx`) |
| a terminal tab renders `NativeTerminalPane` | `NativeTerminalPane.tsx` is the terminal pane component |
| that component attaches a **native** surface | `NativeTerminalPane.tsx:1180` → `attachNativeTerminalLifecycle` / `reattachNativeTerminalLifecycle` |
| the attach is a real IPC call | `invoke("cmd_native_terminal_attach", { sessionId, attachTuple, … })` (`NativeTerminalPane.tsx`) |
| the Rust command exists and is feature-live | `cmd_native_terminal_attach` (`ipc/native_terminal.rs:868`) |

**So a UI-created terminal pane is a native terminal surface**, and `presentation.receipt.jsonl` is therefore
**not** the wrong producer *by construction*. (My c5 capture on `757f8414` shows the same thing from the other
side: its tree contained `ControlType.Edit|Native terminal input`, the native pane's own input element.)

### Feature state of the QA binary

| probe | result |
|---|---|
| `string[native_presentation_settled]` | **PRESENT** — this literal exists only inside the `#[cfg(feature = "local-split-qa")]` emitter |
| `string[presentation.receipt]` | PRESENT |
| `string[qa_barrier]` | PRESENT |
| `string[localSplitLifecycleV1]` | PRESENT (the capability the daemon advertises) |
| `string[NativeTerminalSurfaceHostState]` / `[surface_host]` / `[libghostty]` | PRESENT |
| `Cargo.toml` | `default = ["native-terminal"]` |

**Both features are compiled into the staged binary**, so the producer is not missing from the build.

### The producer requires the attach tuple — and that is the crux

```rust
if let Some(presentation_receipt) =
    pane_liveness_presentation_receipt(frame_attach_tuple.clone(), true)   // None => emits NOTHING
```

and the frontend only builds that tuple on one branch:

```ts
if (!getDurableNativeBinding(targetId) && session && !session.spawnIntent) { /* builds the tuple */ }
const attachTuple = getDurableNativeBinding(targetId);
if (!attachTuple) throw new Error("Durable native binding is unavailable");   // no attach, no frame, no receipt
```

For the empty-state pane, `session.spawnIntent` is **set** (`workspaceStore.ts` sets it whenever
`reliableLocal`), so the tuple must come from `session.spawnIntent.attachTuple` — which the split path
populates. **If it is absent, the attach throws, no native surface attaches, no frame is submitted, and the
receipt stays at 0 lines** — which is exactly the observed shape.

## 4. The four cases, and what I can honestly discriminate

| # | case | status |
|---|---|---|
| 1 | surface host **not attached** for that session | **cannot rule out** — the attach path throws when the tuple is absent, and I could not observe the attach |
| 2 | **no frame submitted** | **cannot rule out** — depends on the renderer running for that window |
| 3 | **presentation path not run** | **cannot rule out** — the completion coordinator's owned-render path |
| 4 | emitter **reached but writing zero lines** | **ruled OUT by the source**: the emitter writes a line whenever `receipt.presented && tuple_is_current` and an operation id exists; and all three emitters write the **same** `presentation.receipt.jsonl`, so a stale-completion rejection would have produced a line too. **0 lines means the completion path never ran with `presented=true`.** |

**I could not take the in-run measurement that separates 1, 2 and 3, and the reason is environmental** — see
§6. I am not going to pick one of them by argument.

## 5. The honest producer for the pane binding — and the plain answer

The seven fields (`PaneAttachTuple`, `daemon/protocol.rs`):

```
1 backend_session_id   2 incarnation?   3 daemon_epoch   4 frontend_session_id
5 pane_identity        6 binding_key    7 attempt_generation
```

**Three of the seven — `frontend_session_id`, `pane_identity`, `binding_key` — are frontend-owned identities
that exist only in the app's own session model.** The daemon has no concept of `pane_identity` or
`binding_key`; the UI tree exposes labels, not these ids.

| candidate producer | what it can carry | can it carry the seven-field tuple? |
|---|---|---|
| **daemon inventory delta** (I read this over the control wire) | the **backend session id** and its liveness | **No** — it knows 1 of 7 fields plus the epoch; it cannot know 4, 5 or 6 |
| **the app's UI tree** (`TabItem\|main`) | the tab label, the pane affordance, error text | **No** — labels are not identities |
| **the app's spawn log line** | `has_worktree / has_cwd / has_client_request_id` | **No** — it deliberately prints no session id or tuple |
| the app's **`cmd_native_terminal_attach` IPC result** | `NativeTerminalAttachReceipt { session_id, attach_tuple, presented }` | **Yes** — it returns the *active* tuple |
| the **native presentation receipt** (what the design binds on) | `PanePresentationReceipt { attach_tuple, presented, presentation_time_unix_ms }` | **Yes** |

**So, plainly: the only producers that can carry the seven-field `attachTuple` are the product's own native
attach result and the native presentation receipt itself — and both of those require the native surface to
attach first.** The daemon inventory and the UI tree each supply a *subset* and can never supply the three
frontend-owned fields.

**Therefore: if a UI-created pane's native surface does not attach, the seven-field tuple is genuinely
unavailable from any honest producer.** No harness-side patch can synthesise it — and it must not, because
fabricating `pane_identity`/`binding_key` is exactly the "invented id" the binding module's own header
rejects. **That is a design question for you, not another patch.**

## 6. Why the in-run measurement is missing: the host's scheduled-task subsystem is wedged

This is environmental, measured, and it also blocks the harness itself:

| probe | result |
|---|---|
| `schtasks /create` + `/run` of a **trivial** task (`echo HELLO > file`) | task reports **Status: Running**, **Last Result: 267009**, and **produces no output at all** |
| a minimal PowerShell probe task | same: `Running` / `267009`, no output file |
| the runner's own delegation (`windows-interactive-relaunch.json`) | it created `ferryx-qa-split-happy-36640-uwdnx5` and ran it — **the task never executed**, so the run stalled after `windows-interactive-relaunch` with no `launch.binary` |
| `Schedule` service | Running |
| total task count | 418 |

**`267009` = `0x41301` = "task is currently running".** So tasks are *accepted and reported as running* but
never actually execute. Because the runner **requires** session-1 delegation to launch the app (session 0
cannot show a window), **no harness run can start on this host right now** — including the runner's own
delegation. That is why my pass-13 run stalled at the relaunch step and why my manual captures could not run
either.

**This is a host-state blocker, not a code finding**, and it is not mine to repair (it needs the Task
Scheduler service restarted, which is a machine-level action I will not take unasked). **Everything in this
pass that is a *finding* comes from the source, the binary and the earlier measured runs; everything that
required a new run is reported as blocked rather than guessed.**

## 7. The two labelled gaps from pass 12b

| gap | status |
|---|---|
| isolated daemon inventory **during** a `5c423880` run | **still not measured** — blocked by §6. I did fix the poller's blank-sample defect for future passes: my pass-12b poller built its sample objects with `if (…)` **as a hashtable value**, which PowerShell does not accept, so every field serialized empty. The corrected form is in `win-pass13/verifier-probe.mjs` (an explicit `if` statement, `$w.NewLine = [char]10`, and a bounded `BeginConnect`/`WaitOne`). |
| the app's post-click tree **during** a `5c423880` run | **still not measured** — same blocker. The probe that would take it is written and syntax-checked (`verifier-probe.mjs`, injected after `driver.newPane`), but no run could execute. |
| repeat the c5 capture on `5c423880` | **not taken** — same blocker. |

**I discard nothing here**: the pass-12 c5 positive control (`757f8414`) stands as measured, and its
`TabItem\|main` / `Split pane right` / `1 session watched` evidence is unaffected by this pass.

## 8. Carry-forward traps (kept, all nine)

1. **A Windows scheduled task does not inherit the interactive PATH** — bare `bun`/`node` die silently; use
   absolute paths.
2. **`StreamWriter.WriteLine` emits `\r\n`; the daemon's control wire wants `\n`** — the handshake blocks
   forever otherwise. Use `$w.NewLine = [char]10`.
3. **`TcpClient.Connect` has no timeout** — bound every blocking call in a poll loop or it hangs forever.
4. **`Start-Process -ArgumentList @()` throws** — omit the parameter for a no-argument child.
5. **`serve-dist.mjs` takes the DIST dir** — passing `ui/` serves Vite's dev shell, the app renders blank, and
   the HTTP check still passes (200 + `id="root"`). Pass `ui/dist`.
6. **A scheduled task's default cwd is `%SystemRoot%\System32`**, not where you launched from.
7. **A stale `.done` marker fires monitors immediately** — clear it at script start.
8. **The runner's typed `FRONTEND_PORT_OCCUPIED` refusal is real and fail-closed** — my leftover listener held
   5173 and the runner refused rather than reusing it; I killed it by the exact PID from the listener listing.
9. **The harness gives the app no stderr sink** — **now fixed in the candidate by `562144b3`**, which is the
   revision this pass staged.

**New trap 10 (this pass):** **`schtasks` can wedge host-wide while the `Schedule` service still reports
Running** — tasks are accepted, report `Running`/`267009`, and never execute. Since the runner **requires**
session-1 delegation, a wedged task subsystem blocks every native run on that host. **Check a trivial task's
output before believing a stalled run is a code defect** — I lost a run to this before measuring it.

## Teardown (pass 13)

| Resource | Action | Receipt |
|---|---|---|
| my `run56.ps1` node + its PowerShell wrapper (65204, 67708) | killed by **exact PID recorded from my own listing**, identity = command line names `pane-liveness`/`run56` | `IDENTITY_OK` |
| the runner's two stalled delegations (36640, 21524) | same rule — they are my launch's own children | `IDENTITY_OK` |
| my stuck `probe13` PowerShell (46552) | killed by exact PID, identity = `probe13`/`ferryx-p13` | `IDENTITY_OK` |
| my scheduled tasks (`ferryx-qa-split-happy-36640-uwdnx5`, `ferryx-qa-split-happy-65204-cpzf1h`, `ferryx-p13-cap`, `ferryx-p13-bat`, `ferryx-p13-probe`, `ferryx-p13-wedge`) | `/end` then `/delete` by name | `ownTasksLeft=0` |
| anything matching my artifact names | report-only sweep found only the above, **all provably mine** | `REMAINING_MINE=0` |
| port 5173 | free | `PORT_5173=FREE` |
| **the host profile** | **untouched this pass** — still the restored bytes | `sha=D07A1698…`, mtime `04:36:27.6955984` |
| host free space | 20.37 GB | — |
| the staged tree | `562144b3` files staged by tar **plus my verifier probe** (`verifier-probe.mjs` + two `probePane` calls) | recorded above |
| the **candidate tree** | **never edited by me** | `HEAD = 562144b3`, `git status --porcelain` empty |

**The identity check refused once more** — an empty-command-line `pwsh` wrapper — and each of the six kills
above was made only after re-listing the PID with its full command line.

## Files (pass 13)

- `win-pass13/verifier-probe.mjs` — **the probe that would take the two missing measurements**: it dumps the
  app's UIA tree (per owned window: total/named/`exactNewTerminal`/tab items/error text), reads the isolated
  daemon's inventory over the control wire, and lists the barrier receipts — all three written to
  `verifier-probe.jsonl` in the run's own evidence dir. **Syntax-checked; not yet executed** (§6).
- `win-pass13/pane-liveness.instrumented.mjs` — the staged runner with the two `probePane` call sites
  (`after-newPane`, `after-bind`).
- `win-pass13/run56.ps1`, `cleanup13.ps1`, `wedge2.ps1` — the run harness, the cleanup, and **the wedge
  proof** (a trivial task producing no output).

## What I would measure first, once the host's task subsystem is healthy

1. Run `split-happy` at `562144b3` with the staged probe in place, and read `verifier-probe.jsonl`.
2. Read `app.stderr.log` — now always-on, so the app's own `[cmd_terminal_spawn]` lines arrive **without any
   staged patch**, and any line beyond `request received` is new information.
3. From the probe's `after-newPane` record: does the tree show `Native terminal input` (surface attached) or
   `Failed to attach native terminal` (attach threw)? That single string separates case 1 from cases 2–3.


---

# PASS 13b — the pre-split tuple dependence audit (source-only, on `562144b3`)

## The question

Your decision makes the pre-split pane a **setup artifact** whose binding needs only its **backend session
identity**, with the seven-field + `presented=true` requirement reserved for the pane the **split** creates. You
asked me to check whether any split-scenario assertion genuinely depends on the **pre-split** pane's tuple in a
way a session id cannot satisfy, and to **report that rather than change an assertion**.

**Answer: no. Nothing does.** Here is the complete enumeration.

## Every reference to the pane binding, and what it reads

| # | site | what it does | reads |
|---|---|---|---|
| 1 | `pane-liveness.mjs:244–254` | creates it via `bindPaneSession` | — |
| 2 | `pane-liveness.mjs:333` | returns it in the runner's result object | whole object |
| 3 | `pane-liveness.mjs:455` | copies it into the PASS result (`paneBinding: native.paneBinding`) | whole object — **evidence only, no assertion** |
| 4 | `split-scenarios.mjs:314` | **the only assertion consumer in the tree** | **`backendSessionId` only** |

```js
// split-scenarios.mjs:314  — the single assertion-side use of ctx.paneBinding
const typedIntoSessionId = ctx.paneBinding?.backendSessionId;
if (typeof typedIntoSessionId !== 'string' || typedIntoSessionId.length === 0) {
  throw new HarnessError('ASSERTION_FAILURE', 'split-concurrent requires the pre-trigger pane binding
    (ctx.paneBinding) to address the pane the marker was typed into');
}
```

A tree-wide search for any read of the binding's **non-session** fields
(`paneIdentity`, `bindingKey`, `frontendSessionId`, `incarnation`, `daemonEpoch`,
`attemptGeneration`) returns **exactly one hit — and it is the `backendSessionId` line above.** Nothing
downstream consumes them.

## All three seven-field assertions belong to the SPLIT pane

| site | scenario | addressed by | comment in the source |
|---|---|---|---|
| `split-scenarios.mjs:60` | `split-happy` | `create.backendSessionId` | *"Await positive 7-tuple presentation receipt **FOR THE SPLIT PANE** … The assertion (positive seven-field tuple **for the pane that was split**) is unchanged"* |
| `split-scenarios.mjs:176` | `split-attach-stall` retry | `authoritativeSessionId` | *"Await presentation receipt following Retry: must reuse SAME backendSessionId but verify NEW attemptGeneration"* |
| `split-scenarios.mjs:299` | `split-concurrent` | `create.backendSessionId` | the split's own presentation |

**So the split's seven-field assertion is already scoped to the split pane's session, exactly as your decision
requires.** The decision does not need to touch it.

## Where the pre-split seven-field requirement actually lives

It is imposed in **exactly one place, inside the binding helper** — not in any scenario assertion:

```js
// pane-binding.mjs:142
const attachTuple = requireSevenTupleReceipt(matched, {}, 'pane binding');
```

`matched` is the **pre-split** pane's presentation receipt, and the helper then copies all seven fields into
the `binding` object (`:150–157`) which is emitted as the `pane-session-bound` evidence action. **That is the
requirement your decision replaces**, and it is entirely within the helper — so the change is contained to
`bindPaneSession` plus its callers, with **no assertion edits needed**.

## Two things the lane should know, reported not changed

1. **The ambiguity guard is written against receipt lines, and the delta path needs an equivalent.**
   `pane-binding.mjs:144–146` throws `PANE_BINDING_AMBIGUOUS` when *more than one* non-fixture session is
   named by the receipt stream. A "sessions after minus before, excluding fixtures" delta can also yield more
   than one session (a restore plus a click, say). **The delta path needs the same one-session guard**, with the
   same typed code, or an ambiguous setup step will be silently bound to whichever session sorted first.
2. **Evidence shape can stay a superset.** `binding` is emitted as evidence and embedded in `result.json`;
   nothing asserts on it. So under the decision the object can keep carrying the seven fields **when the
   product's tuple is available** and always carry `backendSessionId` + `settledBy`, without breaking any
   consumer. The only field with a live assertion-side contract is `backendSessionId`.

## What this means for your decision

- **No split-scenario assertion depends on the pre-split pane's tuple.** The audit is complete: one consumer,
  one field (`backendSessionId`), which a measured daemon inventory delta can supply.
- **The pre-split seven-field requirement is a helper-internal requirement**, so your decision is a change to
  `bindPaneSession`'s contract and its callers — not to any assertion.
- **I am changing nothing.** This is the report you asked for, and the implementation is the scripts lane's.
