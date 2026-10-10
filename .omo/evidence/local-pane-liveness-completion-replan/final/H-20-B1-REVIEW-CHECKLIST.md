# H-20 B1 review checklist — five remaining QA-surface sync-I/O sites

Reviewer: the lead session. Author: lane `st_01a1099e`.
Target: the commit that closes H-20 B1 sites 3, 4, 5, 6, 7 on top of `ecf80277`.

Source of truth for the finding: `H-20-SYNCHRONOUS-IO-AND-BLOCK-ON-AUDIT.md` §B1.
Source of truth for the fix pattern: `terminal/qa_liveness.rs:453-462` (already-fixed site 1) and
`terminal/qa_liveness.rs:372-380` (the `append_receipt` blocking-pool wrapper).

## A. Per-site review rows

For each site, confirm by reading the committed file (not the diff alone):

| # | Site | Check |
| --- | --- | --- |
| 3 | `daemon/qa_producers.rs` `read_command_file` call in `run_held_rpc_watcher` | the read no longer executes on the runtime thread; `dir`/`channel`/`TRIGGER_REMOTE_RPC` captured as owned/`'static`; a failed blocking read still yields `None` → `continue`, never a pass; `WATCH_TICK_MS` unchanged |
| 4 | same file, `release_present` in `hold_unrelated_remote_rpc` | as above, plus the `spec` capture choice is stated and sound; the `released` flag semantics unchanged |
| 5 | same file, `local_split_settled` in `hold_unrelated_remote_rpc` | as above; `split_settled_while_held` still computed from the same file and same non-empty-line test |
| 6 | `ipc/qa_barrier.rs` `write_json_atomic` on the rejected-control branch of `wait_for_release` | the **read** on that path was already async and must stay so; only the write moves; the `let _ =` tolerance is unchanged; the rejection branch still records the same file with the same payload |
| 7 | `ipc/qa_barrier.rs` `append_receipt` from async contexts (`:1821`, and via `append_backend_write_settlement` at `:1873`/`:1886`) | the append runs off the runtime thread; the same JSONL file gets the same line content; no line ordering change that would break a reader |

## A0. Facts pre-verified by the reviewer, to check the author's choices against

| Fact | Measured | Consequence for the review |
| --- | --- | --- |
| `ArmSpec` derives **`Clone`** | `ipc/qa_barrier.rs:198` `#[derive(Debug, Clone, serde::Deserialize)]`, fields are `String`/`u64`/`Option<String>` | cloning `spec` into the closure is sound and needs **no** field-by-field workaround; if the author added a derive or cloned fields individually, that is a deviation to question |
| `daemon/qa_producers.rs` does **not** currently use `run_blocking` | `grep` for `run_blocking|spawn_blocking` in that file → no hits | the call must be **fully qualified** as `crate::ipc::run_blocking`, exactly as `ipc/qa_barrier.rs:1668` does; a bare `run_blocking` would not resolve |
| `ipc/qa_barrier.rs` already uses the qualified form | `:1668` `crate::ipc::run_blocking(move || {` | the in-file precedent exists, so no new import is needed and none should be added |
| `qa_producers.rs` already imports `Path`/`PathBuf`/`Arc` | `:24`, `:25` | owned captures are available without new imports |
| Site 6's **read** is already async | `qa_barrier.rs:739` `tokio::fs::read_to_string(&path).await` inside `wait_for_release` | only the rejected-control **write** may move; if the author changed the read too, that is out of scope |
| The compliant `append_receipt` wrapper already exists | `terminal/qa_liveness.rs:372-380` (`spawn_blocking` + `Arc::clone` + owned `name`) | site 7 should mirror it rather than invent a new shape |
| The stage's warm `cargo check` cache survives on maho-win | `ferryx-plv-final\src-tauri\target` = 2.21 GB, 1633 deps entries, 12 `ferryx*` artifacts | the remote build after review is an **incremental** check, so a compile error would surface quickly and cheaply |

## B. Cross-cutting checks

1. **Zero behaviour change.** Diff the *contents* written, not just the call shape: same file names, same
   JSON fields, same tolerances (`let _ =`, `.ok()`, `unwrap_or(false)`).
2. **No constant moved.** `git diff` shows no change to any `*_MS`, `*_BUDGET*`, or interval value.
3. **`Send + 'static` per capture.** Every value moved into `run_blocking`/`spawn_blocking` is owned and `Send`;
   no borrow is held across an `.await`; no `&self` method is called from inside the closure.
4. **No new monomorphisation risk.** A recorded fact about this tree: `cargo build` can hit **`E0275`
   recursion-limit overflow through wgpu `Sync`** on this QA surface while `cargo check --all-targets` passes.
   So the review must confirm the change adds no new generic instantiation, and **`cargo check` is the gate
   that decides** — not `cargo build`.
5. **QA-gated only.** Every changed line sits inside
   `#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]`. Confirm no production path changed.
6. **Scope.** The commit touches only the two QA files and nothing else (`git show --name-only`).
7. **The two already-fixed sites are untouched** (site 1 and site 2 in `qa_liveness.rs`).

## C. What this review cannot establish

- That the Rust compiles: **no build may run on this Mac** and the author had no execution at all. That is
  what the remote build is for.
- Runtime behaviour: whether the blocking-pool hop changes any timing the scenarios assert. The intervals are
  unchanged, so the cadence is unchanged, but the hop adds a scheduling delay per poll — small, unmeasured here.
- Whether every capture is accepted by the compiler's `Send` analysis in full generic context.

## D. Outcome

### Review of `b5fb0610` (B1 sites 3-7) — **PASS**

Every row above was checked against the committed code, not the diff alone:

| # | Verified |
| --- | --- |
| 3 | owned captures (`dir.clone()`, `Arc::clone`), `TRIGGER_REMOTE_RPC` is `&'static str`, failure → `None` → `continue`, loop order and `WATCH_TICK_MS` unchanged |
| 4 | `dir.to_path_buf()` + `spec.clone()` — and `ArmSpec` **already derives `Clone`** (`qa_barrier.rs:198`), so the clone was the right call rather than a field-by-field workaround; failure → `false`; `released`/`break` identical |
| 5 | `dir.to_path_buf()`; failure → `false`, matching the original `.unwrap_or(false)` |
| 6 | only the **write** moved; the `let _ =` tolerance preserved; the poll's read was already `tokio::fs::read_to_string().await` and is untouched |
| 7 | `append_receipt_off_runtime` mirrors `qa_liveness.rs:372-380` as required; both `append_backend_write_settlement` call sites await; the test became `#[tokio::test]` |

**Every caller of every changed signature is updated** (the highest compile risk): `hold_backend_write_barrier` → 1 caller (`native_terminal.rs:1602`, passing `&Arc` from `active_channel()` which returns `Option<Arc<QaBarrierChannel>>`); `settle_backend_write_barrier` → caller `native_terminal.rs:1656` awaits and the test `qa_barrier.rs:2980` awaits; `append_backend_write_settlement` → both call sites await. `ipc/native_terminal.rs` being touched is the **necessary consequence** of the async signature change and was disclosed in the commit message — not scope creep.

**No constant moved** (grepped for `*_MS`, `*_BUDGET`, numeric `Duration::from_millis` in the diff → zero hits). **The surface stays QA-gated**: `ipc/mod.rs:26-27`, `daemon/mod.rs:19-20`, `terminal/mod.rs:38-39` all carry `#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]` — checked on the **attribute lines**, because a bare grep for the module name hides them and would have made this look ungated.

### Review's own finding — B2 contains the same violation class, now established

H-20 §B2 listed sync writers whose caller context was **UNESTABLISHED**. The review established it, and **five of them are reached from production `async fn`s** — the same violation as B1:

| # | Sync writer | Async call site | Enclosing `async fn` |
| --- | --- | --- | --- |
| 8 | `write_held` (held.json) | `qa_barrier.rs:1812` | `hold_backend_write_barrier` — reached from `ipc/native_terminal.rs:1602` |
| 9 | same | `qa_producers.rs:512` | `hold_unrelated_remote_rpc` |
| 10 | same | `ipc/terminal.rs:2187` | `hold_attach_handshake` — reached from `cmd_terminal_attach` (`:4883`) |
| 11 | `bind_target_session` (bound-ack) | `ipc/terminal.rs:2177` | same `hold_attach_handshake` |
| 12 | `emit_fixture_setup_from_sessions` (receipt) | `qa_barrier.rs:1585` | `emit_gui_fixture_setup` (`:1557`) |

**Deliberately NOT in scope, with the reason**: `emit_fixture_setup` at `:2013` is called from `run_diagnostic_classifier_headless`, which **builds its own runtime and `block_on`s** — H-20 §C classifies that ACCEPTABLE, so it must not change. `write_held`/`bind_target_session` are **also** called from genuinely synchronous contexts (`dispatch_owned_render` at `surface_host.rs:1291`, `note_predecessor_export`/`note_successor_adopt`, `adopt_runner_bind`), so they must **not** become `async` — they need an async wrapper beside the sync fn, used only at the async call sites. Dispatched as the second batch; the build waits for it.

### The review's complete classification of QA-surface sync I/O (so "all" is a measured claim)

Every sync filesystem operation in the QA surface was classified by caller context. This is the **measured** answer to "is
all implementation done?", and it supersedes any earlier partial list.

**Violations — sync I/O reached from an `async fn` (all must move):**

| Writer/reader | Sync site | Async call site(s) |
| --- | --- | --- |
| `write_held` | `qa_barrier.rs:780` (via `write_json_atomic :419`) | `:1812` `hold_backend_write_barrier`; `qa_producers.rs:512` `hold_unrelated_remote_rpc`; `ipc/terminal.rs:2187` `hold_attach_handshake` (reached from `cmd_terminal_attach`, `:4883`) |
| `bind_target_session` | `qa_barrier.rs:574` (via `:638`) | `ipc/terminal.rs:2177` same `hold_attach_handshake` |
| `emit_fixture_setup_from_sessions` | `qa_barrier.rs:921` (via `append_receipt :843`) | `:1585` inside `emit_gui_fixture_setup` (`:1557`) |
| `scan_and_ack_arms` — **read AND write** | read `:685`, write `:713` (via `:419`) | `qa_producers.rs:76` inside `pub fn channel()`, reached from `daemon/server.rs:2966` (also `:3022`, `:3065`, `:3106`) inside `pub async fn run_server_with_handover_and_readiness` (`server.rs:2719`) |

**ACCEPTABLE — sync, and its caller is not a runtime frame (must NOT be changed):**

| Site | Why acceptable |
| --- | --- |
| `qa_barrier.rs:2102`, `:2118`, `:2013` | inside `run_headless_stages` (`:2029`), which **builds its own runtime and `block_on`s** — H-20 §C |
| `qa_liveness.rs:1083`, `:1196`, `:1320`, `:1372`, `:1389` | inside `#[cfg(test)] mod tests` (`:1025`) |
| `scan_and_ack_arms`'s **other** callers: `qa_barrier.rs:1013` `install_for_gui_boot` (called from `start_gui_boot_channel`, `:1403`, sync) and `terminal/qa_liveness.rs:290` `start` (called from `TerminalService::new`, `terminal/service.rs:54`, sync) | sync frames throughout — so **the fn itself must stay sync** and only the async call site may switch to a wrapper |
| `write_held`/`bind_target_session`'s other callers: `surface_host.rs:1291` `dispatch_owned_render` (sync), `qa_producers.rs:329`/`:339` `note_predecessor_export`/`note_successor_adopt` (sync), `qa_barrier.rs:543` `adopt_runner_bind` (sync) | same — these are why an **async wrapper beside the sync fn** is the correct shape, not making the fn async |
| `ipc/native_terminal.rs:2383-2410` (clipboard debug log) | already inside `run_blocking` — COMPLIANT per H-20 |
| `ipc/native_terminal.rs:602` (`stdin.write_all` to `wl-copy`/`xclip`) | a pipe write inside `std::thread::spawn`, not file I/O — H-20 §A |

### Review of `e9e7e8ca` (B2 batch) — **PASS**

4 files, +181/−42. `server.rs` and `terminal.rs` being touched is the disclosed consequence of the async boot hook and the
async `channel()`. **No constant moved** (re-ran the `-U0` grep for `_MS|BUDGET|TIMEOUT|const ` → nothing). **The newly-touched
call sites are QA-gated** — checked on the attribute lines, not the module names: `server.rs:2950` gates the
`channel()`/`note_successor_adopt` site and `:3021`, `:3064`, `:3105`, `:4329` gate the others. The failure paths are the sync
fns' **own** shapes rather than invented ones (the empty scan tuple matches `:663-665`; refusing to push a
`"<scan-unavailable>:unreadable"` sentinel is right, because `{name}:unreadable` is a per-arm-file meaning). The lane also
closed **five more sites of the same class** than were asked for, and — the most useful thing it did — **corrected my "complete
set" claim** with an itemized list of what remains.

### B3 — the remaining violations, and why the risk assessment changed

Three real violations remained. The lane stopped and asked, citing blast radius; the review **removed that concern with a
measured fact: all three are QA-gated, so a production build does not contain the changed code at all.**

| # | Site | Gate evidence | Route |
| --- | --- | --- | --- |
| 13 | `terminal.rs:2487` — `handle_retry`'s sync `settle` closure holds `append_receipt` | inside `mod qa_split_producers`, gated at `:1811` | turn the closure into an async fn and await at its call sites — `.await` cannot appear in a sync closure, and edition 2021 makes `async \|…\|` the only shortcut, which the lane declined to rely on |
| 14 | `qa_producers.rs:245, 298, 337, 347` — `emit_handover_transfer`, `emit_rollback_relinquishment`, `note_predecessor_export`, `note_successor_adopt` are sync fns called from async bodies | `qa_producers` gated at `daemon/mod.rs:19-20`; **all five call sites** (`server.rs:2967, 3027, 3070, 3111, 4330`) sit under their own `#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]` block | add wrappers; **the production handover transaction is not modified** — only QA-gated call sites gain an `.await` |
| 15 | `surface_host.rs:1207` — `adopt_runner_bind` inside **sync** `dispatch_owned_render` | `#[cfg(feature = "local-split-qa")]` at `:1196` (and the file is the `native-terminal` module) | **a real violation, not §A-compliant**: `dispatch_owned_render` is reached from `tauri::async_runtime::spawn(async move {…})` at `:1682`/`:1742` via `dispatch_owned_render_inner` (`:1440`), so it runs **on a Tokio worker** — §A only inspected the two writers already offloaded at `:1155`/`:1167`. Its result **is used**, so the fire-and-forget `spawn_blocking` pattern its siblings use does not apply |

**The lesson the lane's hesitation exposed**: "this path is session-loss-sensitive" was true of the *transaction*, but the
**edits** are at QA-gated call sites that do not exist in a production build. **Checking the gate before weighing the blast
radius** is what turned a scary-sounding change into a mechanical one — and the reverse error (assuming a path is dangerous
because of the module it lives in) would have left three real violations open.

### Deliberately NOT in scope, with the reason (agreed with the lane)

| Site | Why it must not change |
| --- | --- |
| `hold_presentation_barrier_qa` (`surface_host.rs:4757/4805`) | callers are a `#[cfg(test)]` module and `run_headless_stages`, which **builds its own runtime and `block_on`s** — §C ACCEPTABLE |
| `take_command` → `correlated_control` (`qa_barrier.rs:1517`) | frame is `run_stale_binding_watcher`, which §C (`:1456-1466`) records as a **dedicated thread with its own runtime** |
| `install_for_gui_boot` (`qa_barrier.rs:1017`) | sync GUI-boot entry |
| `qa_liveness.rs:290` and the rest | inside `#[cfg(test)]` modules, or the headless self-built-runtime lane |

### Review of `987c007f` (B3 sites 1-2) — **PASS**, and the site-3 decision

3 files, +218/−41, **no constant moved**. `let settle = ` is gone and `settle_retry` exists (1 definition + 6 call sites, all in
`handle_retry`'s async body). Four `off_runtime` wrappers added. The four remaining `channel.append_receipt`/`bind_target_session`
hits in `qa_producers.rs` (`:245`, `:298`, `:337`, `:347`) are **the sync fns themselves**, keeping their signatures for the
headless lane and the unit tests — correct, not leftovers. The lane verified all five handover call sites' gating and used a
closure scanner to prove `.await` is legal at each. Accepted.

**Site 3 — the lane reported a BLOCKER rather than forcing an edit, which is the right outcome, and the resolution is that I
relaxed one of my own constraints.** Its analysis was correct: `dispatch_owned_render` is private, but making it async forces
`dispatch_scheduled_render` async, whose callers include **`pub fn set_preedit`** (`:5309`), which has a **non-QA caller**
(`cmd_native_terminal_set_preedit`, `ipc/native_terminal.rs:1228`) — so route 1 would ripple a production command's signature for
a QA-only rule. Route 2 (spawn the armed block) changes no signature and touches no production path, but defers the claim/bind/held
by one task hop.

**Approved route 2, with the reasoning recorded**: (1) the block is `#[cfg(feature = "local-split-qa")]` at `:1196`, so a
production build **does not contain the moved code**; (2) the deferral lands inside a path that **already spawns a task and
already waits on an external release**, so the hold's duration is set by the runner's release, not by the hop — and the plan's
assertions are `attemptMs <= 15000` / `cancelAckMs <= 3000`, orders of magnitude above a sub-millisecond hop; (3) route 1 is
excluded by the public-signature rule, which I will not break for a QA-only finding. **So the constraint I wrote was too absolute
for a QA-gated path — the rule ("no sync I/O on a runtime thread") is the thing that must hold.**

**Also folded in**: the two writers *inside* the already-spawned task (`:1277`) — `channel_held.write_held` (`:1291`) and the
`append_receipt` calls (`:1303`, `:1405`) — run in an async context and must use the wrappers too; `:1285` already uses
`tokio::task::spawn_blocking` for the held verdict, which is the in-file precedent.

**Required verification without a compiler** (the substitute for `cargo check`): a **whitespace-normalised diff of the moved
region against the original** (a move must read as a move, not a rewrite), brace/paren/bracket balance with `finalDepth 0` and
`minDepth 0`, a capture audit proving each moved value is `Send + 'static` and that no new future captures
`State<'_, NativeTerminalSurfaceHostState>` (the recorded `E0275` trigger), confirmation that no `pub fn` outside the gate changed,
and the deferral stated in the commit body.

### Build gate (after the third batch passes review)

- **PASS** → regenerate the archive at the new HEAD, stage on maho-win (warm `cargo check` cache preserved), run
  G1 `cargo check --all-targets`, G2 `--features local-split-qa`, G3 the frozen gate (**96**).
- **FAIL** → name the site and the exact reason; send it back rather than fixing it silently, so the review stays a real gate.
