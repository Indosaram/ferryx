# H-20 — audit: "no synchronous file I/O on runtime/UI/render callbacks and no block_on in synchronous teardown"

**Plan clause audited:** `/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-completion-replan.md`
line 157 (Todo 5, Work, last sentence): "QA holds before destructive fencing; **no synchronous file I/O
on runtime/UI/render callbacks and no block_on in synchronous teardown**."

**Hole closed:** F1 audit `H-20` — `E/F1-PLAN-TO-ARTIFACT-AUDIT.md`, HOLES / Holes B, row H-20 ("No
test, log or review artifact asserts this property; it is a code-review claim with no evidence";
artifact named there: `native_terminal/surface_host.rs`, `ipc/native_terminal.rs`).

**Project rule the clause restates:** `AGENTS.md` — "All synchronous disk I/O, Git subprocesses, and
OS dialogs must run via `crate::ipc::run_blocking`" / "NEVER execute synchronous disk I/O or git
commands on async Tokio runtime threads; use `run_blocking`."

**Method.** Read-only source inspection of the candidate at the audit's reference revision
`cfb4374b` (base `d82b35e4`), worktree `C`. Nothing was built, run, tested or fixed. Every call site
below was reached by reading its enclosing function and its callers; no site is listed from a guess.
`run_blocking` is defined at `src-tauri/src/ipc/mod.rs:45`.

> **POSTSCRIPT — 2026-10-05.** A concurrent lane committed `14744c8a` on top of `cfb4374b`. Its diff
> touches only six `scripts/**` files (`common-harness.mjs`, `native-driver.mjs`,
> `windows-interactive.mjs`, `pane-liveness-delegation-retry.test.mjs`, `pane-liveness-vitest.config.mjs`,
> `pane-liveness.test.mjs`) — **no `src-tauri/**` file**. Every Rust site and line number classified
> below is therefore still the current content of the tree, and none of the classifications changes.

**Classification scheme**

| Class | Meaning |
|---|---|
| COMPLIANT | the synchronous work runs inside `run_blocking` / `spawn_blocking` / a dedicated thread with its own runtime |
| ACCEPTABLE | outside the clause's reach with a stated reason (test-only, dedicated thread, sync main-thread entry) |
| VIOLATION (gated) | synchronous file I/O executed directly in an `async fn` body, i.e. on a Tokio worker thread |

---

## A. The clause's own subject — the native render / presentation path (Task 5)

**Result: COMPLIANT.** Every render-path file write in the candidate is offloaded off the render
callback.

| Site | What it does | Classification | Evidence |
|---|---|---|---|
| `native_terminal/surface_host.rs:1117` `append_qa_frame_submission` | sync `OpenOptions::append` + `write_all` of one `frame-submitted.jsonl` line | COMPLIANT — its only production caller offloads | caller `:1132 schedule_native_frame_submitted_qa` wraps it at `:1155-1157` in `tauri::async_runtime::spawn_blocking(move \|\| { let _ = append_qa_frame_submission(&dir, &record); })` |
| `native_terminal/surface_host.rs:925, :967, :1069` `channel.append_receipt(...)` | receipt append (sync open + `write_all`, `ipc/qa_barrier.rs:843`) | COMPLIANT — routed through the offloading wrapper | `:1161 schedule_native_qa_receipt` wraps `channel.append_receipt` in `tauri::async_runtime::spawn_blocking` (`:1167-1170`); the render-path emitters call the wrapper (`:1210, :1233, :1667`; frame path `:1706`) |
| `ipc/native_terminal.rs:2383-2410` | debug-only clipboard-classification log append | COMPLIANT | the `OpenOptions` write sits inside `crate::ipc::run_blocking(move \|\| { ... writeln!(file, "{entry}") ... })` (`:2408-2415`) |
| `ipc/native_terminal.rs:602` | `stdin.write_all(&bytes)` to the spawned `wl-copy`/`xclip` helper | NOT FILE I/O — a pipe write inside `std::thread::spawn` (`:598`) | `:596-608` |

`ipc/native_terminal.rs`'s other file I/O (`:2529, :3606, :3620, :3637, :3763, :3826, :3869`) is all
inside `#[cfg(test)] mod tests`, which starts at `:2488`.

## B. The rest of the candidate — the QA producer surface (`local-split-qa` + `native-terminal`)

The F1 audit's named artifacts are the native/IPC files, but the clause's literal text ("no
synchronous file I/O on runtime … callbacks") reaches the QA producer surface the candidate also
introduced (`dd9e6813`, `c34b90fc` … `cfb4374b`). That surface is where the clause is **not** met.
None of these modules exists at the base commit (`git show d82b35e4:<path>` has no such symbol), so
every site below is candidate-introduced.

### B1. VIOLATION (gated): sync file I/O directly inside an `async fn` body

| # | Sync I/O | Executed from | Why it is on a Tokio worker |
|---|---|---|---|
| 1 | `terminal/qa_liveness.rs:356` `read_frame_submission_in` — `std::fs::read_to_string` | `:449` inside `async fn await_frame_evidence` (`:429`) | the read is a plain call in an async body; the runtime poll drives it |
| 2 | `terminal/qa_liveness.rs:819` `read_correlated_command` — `std::fs::read_to_string` | `:870` inside `async fn exercise_eof_watch_in` (`:857`) | same |
| 3 | `daemon/qa_producers.rs:95` `read_command_file` — `std::fs::read_to_string` | `:390` inside `async fn run_held_rpc_watcher` (`:380`), itself started with `tokio::spawn` (`:89`) | same |
| 4 | `daemon/qa_producers.rs:108` `release_present` — `std::fs::read_to_string` | `:533` inside `async fn hold_unrelated_remote_rpc` (`:427`) | same |
| 5 | `daemon/qa_producers.rs:411` `local_split_settled` — `std::fs::read_to_string` | `:542` inside the same `async fn` | same |
| 6 | `ipc/qa_barrier.rs:419` `write_json_atomic` — `std::fs::write` + `std::fs::rename` | `:750` inside `async fn wait_for_release` (`:733`) | same |
| 7 | `ipc/qa_barrier.rs:843` receipt append (`OpenOptions::append` + `write_all`) inside `append_receipt` (`:808`) | `:1821` inside `async fn hold_backend_write_barrier` (`:1782`); and `:1913` via `append_backend_write_settlement` (`:1901`) called at `:1873` (same async fn) and `:1886` (inside the async block of `run_headless_stages`, `:1998`) | same |

**Why it is a real violation and not a stylistic one.** The project rule is unconditional about
*where* the work runs, not about how large the file is: a blocking `read_to_string`/`write` on a
Tokio worker occupies that worker for the duration of a syscall, and this surface is exactly the one
that the plan's scenario timing assertions (`attemptMs <= 15000`, `cancelAckMs <= 3000`) are measured
against. The author already knew the rule in this same file: `terminal/qa_liveness.rs:547` uses
`tokio::fs::read_to_string(&path).await` in the equivalent position, and
`ipc/qa_barrier.rs:745` likewise — so the compliant form is already present in the neighbourhood.

**Compliant form for each site** (not applied here — this lane does not edit product code):

- Sites 1-5: wrap the read in `crate::ipc::run_blocking(move || …).await`, exactly as
  `daemon/split_journal.rs` callers do, or use `tokio::fs::read_to_string(...).await` as
  `qa_liveness.rs:547` already does.
- Sites 6-7: make the writer async (`tokio::fs::write` + `tokio::fs::rename`), or move the sync
  writer behind `run_blocking`/`spawn_blocking` at the call site, exactly as
  `surface_host.rs:1155-1157` and `:1167-1170` already do for the render path.

**Mitigating facts, recorded so the finding is not overstated** (none of them is the clause's test):

- The whole surface is compiled only under `local-split-qa` + `native-terminal`
  (`daemon/mod.rs:20`, `terminal/mod.rs:39`; module docs in both files) and requires the runner's
  private env (`FERRYX_QA_BARRIER_DIR`/`FERRYX_QA_RUN_ID`) to be installed. A normal launch does
  nothing here, so **no production user path is affected**.
- The files are small and the reads are single-line JSON; the practical stall is short.
- The plan's own Task 5 clause is the reason this is filed as a violation rather than a nit.

### B2. Sync QA writers whose caller context is NOT established in this lane

These are synchronous file writers called from QA entry points. Their enclosing functions are sync;
whether the frame that calls them sits on a Tokio worker was not established here, so they are listed
as **UNESTABLISHED** rather than classified.

| Sync writer | Sync caller | Observed callers |
|---|---|---|
| `ipc/qa_barrier.rs:419` `write_json_atomic` | `:574 bind_target_session` (bound-ack write) | `qa_producers.rs:329` (`note_predecessor_export`), `qa_producers.rs:339` (`note_successor_adopt`), `ipc/terminal.rs:2163` (attach path), `qa_barrier.rs:543` (`adopt_runner_bind`) |
| `ipc/qa_barrier.rs:419` | `:660 scan_and_ack_arms` (armed-ack write) | `qa_producers.rs:76` (inside `channel()`), `terminal/qa_liveness.rs:290`, `ipc/qa_barrier.rs:1013, :1965` (+ test call sites) |
| `ipc/qa_barrier.rs:419` | `:780 write_held` (held write) | `surface_host.rs:1291, :4757`, `qa_producers.rs:512`, `ipc/terminal.rs:2173`, `qa_barrier.rs:1812` |
| `ipc/qa_barrier.rs:843` | `:896 emit_fixture_setup` / `:917 emit_fixture_setup_from_sessions` | `qa_barrier.rs:1982` (headless stages), `qa_barrier.rs:1581` |

## C. `block_on` sites

**Candidate-added `block_on` (3):**

| Site | Classification | Evidence |
|---|---|---|
| `ipc/qa_barrier.rs:1478` | ACCEPTABLE — dedicated thread with its own runtime | `:1460-1466` builds a `std::thread::Builder::name("ferryx-qa-stale-binding")`, `:1470-1476` builds a `current_thread` runtime inside that thread, then `block_on`s |
| `ipc/qa_barrier.rs:2014` | ACCEPTABLE — sync entry builds its own runtime | `run_headless_stages` (`:1998`) builds a `multi_thread` runtime (`:2003-2008`) and blocks on it; reached only from the headless CLI dispatch |
| `terminal/qa_liveness.rs:1309` | ACCEPTABLE — test-only | inside `#[cfg(test)] mod tests` (`:999`), in `fn exercise_eof_consumer_settles_from_the_real_stream_end` (`:1283`) |

**Pre-existing `block_on` inside candidate-touched files:**

| Site | Classification | Evidence |
|---|---|---|
| `terminal/pty.rs:1283, :1317` | ACCEPTABLE — test-only | both inside the file's `#[cfg(test)]` module (`shutdown_elapsed_after`, `dropping_an_exported_session_releases_its_paused_reader`) |
| `daemon/handover_socket.rs:390` | ACCEPTABLE — test-only | inside a test body (`recv_authority`/`adopt_authority` assertions) |
| `daemon/handover.rs:325` | ACCEPTABLE — test-only | inside a test body (delayed old-owner cleanup assertion) |
| `lib.rs:1183` | ACCEPTABLE — sync main-thread setup | `tauri::async_runtime::block_on(crate::ipc::file_preview::FilePreviewService::start())` inside the Tauri `setup` closure, a synchronous entry point, not a runtime callback |
| `worktree/git.rs:209` | ACCEPTABLE with a stated reason | `fn bounded_output` (`:195`) is a **sync** helper that blocks on an async git child via `tokio::runtime::Handle::current()`. It is not the clause's teardown subject. **Not established here:** the full caller set of `bounded_output` was not audited in this lane, so whether every caller reaches it from a blocking thread is unproven |

**The clause's second half — "no block_on in synchronous teardown": no instance found in the
candidate.** The only `Drop` implementations in the touched files were read: `worktree/git.rs:188`
(the `#[cfg(test)]` `Restore` guard — no I/O) and
`tests/daemon_persistence_contract.rs:182` (a test harness whose `Drop` spawns a dedicated thread
with its own `current_thread` runtime before `block_on` — ACCEPTABLE, test-only). **Not established:**
a repository-wide sweep of every `Drop` impl outside the changed files was not performed; the claim
is bounded to the candidate's touched files.

## D. Explicitly-compliant reference patterns (so the rule is demonstrably followed, not just stated)

1. `daemon/split_journal.rs` — module doc (`:37-39`): "All methods perform blocking filesystem I/O.
   Async callers must execute them through `crate::ipc::run_blocking` rather than on a Tokio runtime
   thread." Its callers in `daemon/session_service.rs` (`:377, :421, :487, :514, :524, :551, :585`)
   are all inside `crate::ipc::run_blocking(move || …)` closures.
2. `terminal/session.rs:571` — `suspension_target()` is documented "Blocking kernel identity lookup;
   callers must use `run_blocking`." All production callers do:
   `terminal/service.rs:541-543`, `:552-555`, `:566-568`; `terminal/qa_liveness.rs:687-695`.
   The `/proc` reads at `terminal/session.rs:173, :578, :583` are reachable only through it.
3. `ipc/native_terminal.rs:2408` — the debug-only log write is wrapped in `run_blocking`.
4. `native_terminal/surface_host.rs:1155-1157` and `:1167-1170` — the render path's two file writers
   are both wrapped in `spawn_blocking`.

## E. Verdict and remaining work

- **Task 5's own subject (native attach/presentation/render callbacks): COMPLIANT.** The render path
  offloads both of its writers; no synchronous file I/O executes on a render callback in the
  candidate.
- **The clause's teardown half: no instance found** in the candidate's touched files.
- **One real, gated violation class: B1, seven sites** on the QA producer surface
  (`local-split-qa` + `native-terminal`), with the compliant form named per site. It does not affect a
  production launch; it is still a violation of the clause as written.
- **Seven further sync writers (B2) are UNESTABLISHED**, with their caller lists recorded so the
  next reviewer can finish the classification without re-deriving it.

**Handoff for F2:** treat B1 as the clause's finding (fix by the named compliant form, or by an
explicit, recorded exemption in the plan). Treat B2 as an open classification task, not as a clean
bill of health.
