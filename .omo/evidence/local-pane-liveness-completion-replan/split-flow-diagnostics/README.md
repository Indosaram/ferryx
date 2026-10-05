# Split-flow diagnostics patch — ready to apply at `89a363a0`

**Status: authored, statically reviewed, NOT compiled, NOT committed.**
Nothing in this directory was built, tested, or run. The patch is an artifact only; it must be
applied and compile-verified on a build host before it is trusted. `maho-win` was unreachable
(SSH refused at kex) while this was written, so no compiler has seen these lines.

| | |
|---|---|
| Patch | `SPLIT-FLOW-DIAGNOSTICS.patch` — 10,809 bytes (unified diff, `git apply -p1`) |
| Base revision | `89a363a002cf696a52724ff652c2d6ef2179b5e1` (worktree `ferryx-wt/local-pane-liveness-completion-foundation`) |
| Files touched | `src-tauri/src/ipc/terminal.rs` (5 hunks, +56/−0), `ui/src/App.tsx` (1 hunk, +22/−3), `ui/src/state/workspaceStore.ts` (2 hunks, +21/−3) — total +99/−6 |
| Shape | diagnostics only — no control-flow, no error code, no return value, no message text changed |

Identical copies of both files are mirrored into the campaign evidence root of each checkout used
here (`.omo/evidence/local-pane-liveness-completion-replan/split-flow-diagnostics/` under both the
main checkout and the `local-pane-liveness-completion-foundation` worktree). They are byte-identical;
if you edit one, update the other.

## 1. Why this patch exists

PASS-21 at `89a363a0` measured that the split affordance click now **resolves** (the accessibility
gate is gone) and the run stops one stage later:

```
BLOCKED / BARRIER_ACK_TIMEOUT — "product did not settle barrier split-create receipt[0] within 9000ms"
```

The run's captured app stderr was, in full, one line:

```
[cmd_terminal_spawn] request received has_worktree=false has_cwd=true has_client_request_id=true
```

(`task-9/win-pass21/inner-app.stderr.log`, 97 bytes, drained complete.)

That line proves two things at once: **app stderr is captured** by the harness, and **the split flow
is invisible in it**. Source reading explains why:

- The `split-create` receipt is emitted only on `cmd_terminal_spawn`'s create-only early path
  (`src-tauri/src/ipc/terminal.rs:3974-3993` at `89a363a0`:
  `request.create_only == Some(true) || request.prepared_local_split.is_some()` →
  `create_local_split_until(...)` → `qa_split_producers::record_and_emit_split_create(...)`), and that
  path **returns before** the only `[cmd_terminal_spawn] request received` log at `:3998-4000`.
- `cmd_terminal_spawn_operation` (`src-tauri/src/ipc/terminal.rs:3909-3967`), the flow's **first** step
  (the `Prepare` variant), logs nothing at all.
- The frontend side is equally silent: `splitPane` has three silent `return`s and `handleSplitActive`
  has three more plus a fallback (see §5).

So the receipt's absence cannot currently be attributed: the split may never have created a session,
or it may have created one and failed to settle the barrier. This patch makes every step of that
sequence observable.

## 2. Rust diagnostics added (all behind `#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]`)

Naming mirrors the existing QA producers (`FERRYX_QA_RETRY_REFUSED`, `FERRYX_QA_BATCH_UNSERVICEABLE`,
`FERRYX_QA_CANCEL_UNSETTLED`, …) and the existing `key=value` field style of
`[cmd_terminal_spawn] request received has_worktree=…`. Every line is `eprintln!` — same sink the
harness already drains.

| # | Literal prefix | Site | Meaning |
|---|---|---|---|
| 1 | `FERRYX_QA_SPLIT_OP_RECEIVED: variant=prepare request_id=… workspace_id=…` | `cmd_terminal_spawn_operation`, `Prepare` arm, first statement | The frontend's split **step 1** reached Rust. |
| 2 | `FERRYX_QA_SPLIT_OP_RECEIVED: variant=status request_id=… origin_epoch=…` | `Status` arm, first statement | The flow polled the operation instead of creating (a retry/status loop is running). |
| 3 | `FERRYX_QA_SPLIT_OP_RECEIVED: variant=cancel request_id=… origin_epoch=…` | `Cancel` arm, first statement | The flow cancelled the operation. |
| 4 | `FERRYX_QA_SPLIT_OP_REFUSED: request_id=… workspace_id=… blank_request_id=… startup_present=… remote_workspace=… paired_workspace=…` | inside the `Prepare` guard, immediately before the unchanged `return Err(…"Prepare requires a local shell workspace and request identity")` | Which of the **four** guard conditions tripped. The returned error and its code are untouched; the log only names the cause. |
| 5 | `FERRYX_QA_SPLIT_OP_PREPARED: request_id=… origin_epoch=… workspace_id=… cwd=…` | `Prepare` arm, immediately before `Ok(SplitOperationResponse::Prepare { … })` | Preparation **completed** (identity minted, cwd resolved, workspace registered). |
| 6 | `FERRYX_QA_SPLIT_CREATE_RECEIVED: create_only=… prepared_local_split=… request_id=… workspace_id=…` | `cmd_terminal_spawn`, first statement of the create-only branch | The frontend's split **step 2** reached Rust, with both discriminators. `request_id=-` when no prepared split was carried. |
| 7 | `FERRYX_QA_SPLIT_CREATE_REFUSED: request_id=… create_only_not_true=… workspace_mismatch=… worktree_mismatch=… startup_present=…` | inside the identity guard, immediately before the unchanged `return Err(…"Prepared split identity does not match create request")` | Which of the **four** mismatch conditions tripped. |
| 8 | `FERRYX_QA_SPLIT_CREATE_READY: request_id=… session_id=… daemon_epoch=…` | after `record_and_emit_split_create(...)`, immediately before `return Ok(SpawnTerminalResponse { … })` | The create **succeeded**; this session id exists. (The receipt is emitted one statement above it.) |

Notes:

- #6 also disambiguates the third exit on this path — the `ok_or_else(...)` that returns
  `"Create-only requires preparedLocalSplit"` (which fires before any other log). If stderr shows
  `create_only=true prepared_local_split=false`, that is the exit that fired.
- #8 is deliberately adjacent to `record_and_emit_split_create`. **`FERRYX_QA_SPLIT_CREATE_READY`
  without a `split-create` receipt is the decisive result**: it proves the session was created and
  the receipt was lost at the barrier channel (`channel()` / `operation_id()` absent or unarmed),
  which is a completely different defect from "the split never created anything".

## 3. How to read the result

The sequence is monotonic; read stderr top-down and stop at the first line that does not have a
successor. Run `grep FERRYX_QA_SPLIT_` over `app.stderr.log`.

| stderr shows | Conclusion |
|---|---|
| no `FERRYX_QA_SPLIT_*` at all, only `[cmd_terminal_spawn] request received …` | Neither Rust split entry point was reached. The exit is in the frontend (§5) or the click never dispatched. |
| `_OP_RECEIVED: variant=prepare` then `_OP_REFUSED` | The four-condition `Prepare` guard fired; the flags name the condition (e.g. `startup_present=true`). |
| `_OP_RECEIVED: variant=prepare` and nothing after | `prepare_local_split_until` / `run_blocking` cwd resolution / `register_workspace` did not complete — the failure is inside the Prepare stage, not the create stage. |
| `_OP_PREPARED` then no `_CREATE_RECEIVED` | Preparation succeeded and the frontend never sent the create-only spawn. The break is between the two IPC calls. |
| `_CREATE_RECEIVED: create_only=true prepared_local_split=false` | The `ok_or_else` "Create-only requires preparedLocalSplit" exit fired. |
| `_CREATE_RECEIVED` then `_CREATE_REFUSED` | Identity mismatch; the flags name it. |
| `_CREATE_READY: session_id=…` | A session really was created. The missing `split-create` receipt is a producer/barrier problem (see the note on #8), not a creation failure. |
| `_CREATE_READY` then no `_CREATE_READY` successor and no receipt | Same as above — creation succeeded, the receipt did not settle. |

## 4. Frontend observability — a capture path **does** exist

**Answer: yes.** The project already has a WebView→disk channel the harness can read, and it is
already used throughout these two files.

Evidence (all at `89a363a0`):

- `ui/src/lib/switchDebug.ts` — `switchDebug(event, details)` → `createSwitchDebugLogger` →
  `sink(entry)` → `invoke<void>("cmd_switch_debug_log", { entry })` (`switchDebug.ts:113`).
- `src-tauri/src/ipc/debug.rs:376` — `cmd_switch_debug_log` → `append_switch_debug_entry(&std::env::temp_dir(), &entry)`.
- `src-tauri/src/ipc/debug.rs:150-152` — the file is **`<temp_dir>/ferryx-switch-debug.jsonl`**
  (rotated to `ferryx-switch-debug.jsonl.1` at 5 MiB, `MAX_LOG_FILE_BYTES`). On the Windows lane that
  is `%TEMP%\ferryx-switch-debug.jsonl`; on macOS/Linux `$TMPDIR/ferryx-switch-debug.jsonl`.
  A precedent reader exists in-tree: `scripts/verify-terminal-typing.mjs:6`
  (`process.env.FERRYX_TRACE_PATH ?? "/tmp/ferryx-switch-debug.jsonl"`).
- **Nothing forwards webview console output to the process.** `console.info("[ferryx:switch]", entry)`
  stays inside the WebView; a grep for `on_webview_console` / `console_message` handlers under
  `src-tauri/src/` returns nothing. Only the explicit `invoke` reaches Rust. So a bare `console.log`
  is *not* a usable diagnostic, which is why this patch uses `switchDebug` and not `console.log`.

### The gate that must be enabled (read this before interpreting an empty jsonl)

`switchDebug` is compile-time gated in the UI: `resolveSwitchDebugEnabled` returns
`env.DEV || env.VITE_SWITCH_DEBUG === "1"` and is forced `false` when `MODE === "test"`
(`ui/src/lib/switchDebug.ts`). The harness serves the **pre-built `ui/dist`** statically on
`127.0.0.1:5173` (`scripts/lib/qa-scenarios/frontend-server.mjs`, route decision "static `ui/dist`
(option B)") and never builds anything, so a default production `ui/dist` has `import.meta.env.DEV
=== false` and the TS diagnostics are inert. To make them land on disk the UI must be rebuilt with:

```
VITE_SWITCH_DEBUG=1 bun run --cwd ui build        # bash
$env:VITE_SWITCH_DEBUG='1'; bun run --cwd ui build   # PowerShell
```

The Rust-side sink itself is already on for this lane: it is `cfg!(debug_assertions) ||
FERRYX_SWITCH_DEBUG == "1"` (`src-tauri/src/ipc/debug.rs:135-147`) and the harness launches
`src-tauri\target\debug\ferryx.exe` (a debug build), so no extra app env var is needed. A release
binary would additionally need `FERRYX_SWITCH_DEBUG=1` in the app's environment.

### One caveat that shapes how the details are written

`sanitize_switch_debug_details` (`src-tauri/src/ipc/debug.rs:234`) keeps **only** allowlisted detail
keys; everything else is dropped from the persisted entry. `reason` is allowlisted, so every TS
diagnostic in this patch carries its discriminating text in `reason`. The **event name itself always
survives**, which is why the exits get distinct event names rather than one event with a flag.

| Event name (survives verbatim) | Site | Meaning |
|---|---|---|
| `split.active.requested` | `App.tsx` `handleSplitActive`, before `splitPane(...)` | The click reached the handler and it is about to call `splitPane`. `reason` carries `direction` and the resolved `targetLeafId`. |
| `split.active.refused.remote-host` | `handleSplitActive` guard 1 | A remote host is active; the split was refused before any local logic. |
| `split.active.refused.paired-feature-gated` | `handleSplitActive` guard 2 | Paired-daemon project with `machineFeaturesEnabled !== true`. |
| `split.active.refused.no-terminal-tab` | `handleSplitActive` guard 3 | No active tab, or the active tab is a browser. |
| `split.active.fallback.leaf-default.layout-missing` | `handleSplitActive`, **fallback (a)** | `layoutsByTabId[activeTab.id]` is absent → `activeLayout?.activeLeafId` is `undefined` → the `?? "leaf-default"` literal is passed to `splitPane`. |
| `split.active.fallback.leaf-default.active-leaf-missing` | `handleSplitActive`, **fallback (b)** | The layout exists but `activeLeafId` is `null` → same literal. |
| `split.pane.refused.non-terminal-tab` | `workspaceStore.ts` `splitPane` guard 1 | `tabId` is not in the layout, or the tab is not a terminal tab. |
| `split.pane.refused.target-leaf-not-in-layout` | `splitPane` guard 2 | `targetLeafId` is not among `collectLeafIds(layout.root)`. `reason` carries the requested leaf id **and the actual leaf ids**, which is what distinguishes the `"leaf-default"` hypothesis from any other cause. |
| `split.pane.refused.source-session-missing` | `splitPane` guard 3 | `sessionIdsByLeafId[targetLeafId] ?? tab.sessionId` has no session in the store. |
| `split.pane.dispatched` | `splitPane`, immediately after `dispatch({ type: "SPLIT_PANE", … })` | All three guards passed and the pane tree was updated — the flow is about to drive the backend. |

**What the next pass can conclude if the UI is *not* rebuilt with `VITE_SWITCH_DEBUG=1`:** it can
still prove whether the Rust split entry points were reached at all (§2/§3 — app stderr is always
captured), and therefore whether the break is in Rust or in the frontend. It **cannot** tell *which*
frontend exit fired — `handleSplitActive`'s guards, the `"leaf-default"` fallback, or one of
`splitPane`'s three returns — because none of those emit anything the process can see. That is the
one question this patch leaves open in a default run, and it is why the UI rebuild above matters.

## 5. The `"leaf-default"` fallback — leading hypothesis, deliberately **not** fixed

`App.tsx:2328` is `const targetLeafId = activeLayout?.activeLeafId ?? "leaf-default";`. The literal
`"leaf-default"` cannot match a real leaf id:

- `createLayoutId(prefix)` = `` `${prefix}:${randomPart}` `` — `ui/src/state/layout.ts:1148-1151`.
- `getTabPaneLayout`'s fallback is `` `leaf-default-${tab.id}` `` — `ui/src/state/layout.ts:810`.
- `layout.ts:926` mints a *prefixed* id via `createLayoutId("leaf-default")`, i.e. `leaf-default:<uuid>`
  — still never the bare literal.
- `collectLeafIds(root)` returns `node.leafId` verbatim (`ui/src/state/paneTree.ts:114-124`), so
  `splitPane`'s second guard can never accept `"leaf-default"`.

So whenever `activeLeafId` is unresolvable, `handleSplitActive` passes a leaf id that `splitPane`
rejects → **silent no-op**, which is exactly the symptom class PASS-21 measured. The sibling code
paths use working fallbacks — `App.tsx:2369` (`collectLeafIds(activeLayout.root)[0]`) and the
`?? leafIds[0]` at `App.tsx:2355` — so this is an inconsistency, not a convention. The same
literal also appears at `App.tsx:2341` (`handleUnsplitActive`, a different flow).

**It is not fixed here.** An observability patch and a behaviour change in the same diff make the
next run uninterpretable: if the fallback is repaired at the same time, a passing run cannot say
whether the guard was firing before. Measure first — the two new `split.active.fallback.leaf-default.*`
events tell you whether fallback (a) or (b) fires, and `split.pane.refused.target-leaf-not-in-layout`
carries the actual leaf ids — then fix it as its own change with its own verification.

## 6. Apply, build, run

Run from the checkout root, at `89a363a0`.

```bash
# 0. the patch must apply to the exact revision it was authored against
git rev-parse HEAD                       # expect 89a363a002cf696a52724ff652c2d6ef2179b5e1

# 1. apply (check first; the patch is -p1 with repo-relative paths)
git apply --check -p1 .omo/evidence/local-pane-liveness-completion-replan/split-flow-diagnostics/SPLIT-FLOW-DIAGNOSTICS.patch
git apply -p1       .omo/evidence/local-pane-liveness-completion-replan/split-flow-diagnostics/SPLIT-FLOW-DIAGNOSTICS.patch

# 2. Rust, with the QA feature. `native-terminal` is a default feature, so the
#    cfg(all(feature = "local-split-qa", feature = "native-terminal")) gate is satisfied.
cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa

# 3. UI, with the WebView->disk channel enabled (required for §4 to produce anything)
VITE_SWITCH_DEBUG=1 bun run --cwd ui build          # bash
# $env:VITE_SWITCH_DEBUG='1'; bun run --cwd ui build   # PowerShell

# 4. the PASS-21 scenario, unchanged
node scripts/qa/pane-liveness.mjs --scenario split-happy \
  --binary <checkout>/src-tauri/target/debug/ferryx.exe \
  --evidence-dir <ev> --isolation-root <iso>
```

The harness fails closed with `FRONTEND_DIST_MISSING` if `ui/dist` is absent and with
`FRONTEND_PORT_OCCUPIED` if 5173 is held by a foreign listener; it never builds and never kills a
foreign listener.

### Reading the result

```powershell
# the always-captured Rust sequence (works with or without step 3)
Select-String -Path <ev>\task-3-harness\split-happy\run-*\app.stderr.log -Pattern 'FERRYX_QA_SPLIT_'
# the WebView->disk channel (only populated when the UI was built in step 3)
Select-String -Path $env:TEMP\ferryx-switch-debug.jsonl -Pattern '"event":"split\.'
```

The jsonl is append-only and **shared across runs** (single global file, rotated at 5 MiB). Record
its length before the run, or filter by the new event names and ignore earlier entries.

## 7. Zero behaviour change

- Every Rust line is inside `#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]`,
  so a normal build contains none of it. With the feature on, each addition is an `eprintln!`
  statement inserted next to — never inside — the existing control flow: no condition, no error code,
  no error message, no return value and no ordering of side effects was changed. The two refusal
  logs re-evaluate the same guard expressions purely to name the tripped condition; the `if`
  conditions themselves are byte-identical.
- The three `splitPane` guards keep their original conditions verbatim; only the `return` statements
  grew a block around them.
- The TS diagnostics are no-ops wherever the existing logger is disabled, which includes the whole
  test runner (`resolveSwitchDebugEnabled` returns `false` for `MODE === "test"`). No test enumerates
  the switch-debug event set (`ui/src/lib/switchDebug.test.ts` asserts specific events only), so
  adding event names cannot break one.

## 8. Verification performed here (and its limits)

- The three source files were copied out of the worktree and confirmed **byte-identical to the
  committed blobs** at `89a363a0` (SHA-256 of working file == SHA-256 of `git show 89a363a0:<path>`),
  so the patch's line numbers and context are those of the stated revision. They were re-checked
  after the patch was written and are still identical — `git status --porcelain` for those three
  paths is empty. (Unrelated `scripts/**` files carry another lane's in-flight edits; they were not
  read for content, not modified, and are not touched by this patch.)
- `git apply --check -p1` against pristine copies of all three files: **exit 0**.
- `git apply -p1` against those copies, then SHA-256 comparison of the result against the authored
  files: **identical for all three** — the patch reproduces exactly the reviewed content.
- Every `file:line` cited above was read in this session at that revision.

**Nothing was compiled.** No `cargo`, no `bun`, no `tsc`, no `vitest`, no `vite`, no GUI, no daemon,
no remote host was invoked. Rust type-correctness (borrow/move order in particular) is argued from
reading, not from a compiler — the build in §6 step 2 is the first real check. **Nothing was
committed**, and the worktree this patch was authored against was not modified: the diff was produced
from copies under `/tmp/ferryx-split-diag/`.
