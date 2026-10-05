# F2-2 - `retain_spawn_owner()` has no production caller (fail-open invariant)

Candidate worktree: `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`
HEAD: `bcb76cfd` (verified, tree clean). Base: `d82b35e4`.
Method: static reading and `git` inspection only. No build, no test, no execution.

**Verdict: CONFIRMED.** The invariant is tested but not enforced. `spawn_owners` is always 0 in every
production daemon, so the three guards that read it can never fire. The candidate's own recorded
compiler output proves it.

---

## 1. The guard, the counter, and the three read sites

Added by the candidate's first commit `5464da0d` (all three line numbers verified in `bcb76cfd`):

| Element | Location |
|---|---|
| `pub(crate) struct SpawnOwnerGuard` | `src-tauri/src/daemon/handover.rs:450` |
| `impl Drop for SpawnOwnerGuard` -> `fetch_sub(1)` | `:454-459` |
| `spawn_owners: std::sync::atomic::AtomicUsize` field | `:516`, initialised `AtomicUsize::new(0)` at `:587` |
| `pub(crate) fn retain_spawn_owner(self: &Arc<Self>) -> Result<SpawnOwnerGuard, String>` | `:574-581`; `fetch_add(1)` at `:579`; returns `Err("HANDOVER_BUSY")` unless status is `Active` |
| guard read - `prepare_handover` (unix) | `:656` `if self.spawn_owners.load(Ordering::Relaxed) != 0 { return Err("HANDOVER_BUSY".into()); }` |
| guard read - `commit_handover_v4` | `:708` |
| guard read - `commit_handover_v5` | `:762` |

The intent is sound: a handover must not prepare or commit while a spawn is mid-flight, because the
spawn may be attaching to a PTY whose ownership the handover is about to transfer.

## 2. No production caller - exhaustive check

```
$ grep -rn "retain_spawn_owner\|spawn_owners\|SpawnOwnerGuard\|retain_spawn" src-tauri/src --include=*.rs
src-tauri/src/daemon/handover.rs:450  pub(crate) struct SpawnOwnerGuard {
src-tauri/src/daemon/handover.rs:454  impl Drop for SpawnOwnerGuard {
src-tauri/src/daemon/handover.rs:457      self.manager.spawn_owners.fetch_sub(1, Ordering::Relaxed);
src-tauri/src/daemon/handover.rs:462  mod spawn_owner_tests {
src-tauri/src/daemon/handover.rs:472      let owner = manager.retain_spawn_owner().unwrap();      // #[cfg(test)]
src-tauri/src/daemon/handover.rs:492      assert!(manager.retain_spawn_owner().is_err());          // #[cfg(test)]
src-tauri/src/daemon/handover.rs:496      assert!(manager.retain_spawn_owner().is_ok());           // #[cfg(test)]
src-tauri/src/daemon/handover.rs:503      let owner = manager.retain_spawn_owner().unwrap();      // #[cfg(test)]
src-tauri/src/daemon/handover.rs:508      assert!(manager.retain_spawn_owner().is_err());          // #[cfg(test)]
src-tauri/src/daemon/handover.rs:510      assert!(manager.retain_spawn_owner().is_ok());           // #[cfg(test)]
src-tauri/src/daemon/handover.rs:516      spawn_owners: std::sync::atomic::AtomicUsize,
src-tauri/src/daemon/handover.rs:574  pub(crate) fn retain_spawn_owner(...)
src-tauri/src/daemon/handover.rs:579      self.spawn_owners.fetch_add(1, Ordering::Relaxed);
src-tauri/src/daemon/handover.rs:587      spawn_owners: std::sync::atomic::AtomicUsize::new(0),
src-tauri/src/daemon/handover.rs:656      if self.spawn_owners.load(Ordering::Relaxed) != 0 {   // prepare_handover
src-tauri/src/daemon/handover.rs:708      if self.spawn_owners.load(Ordering::Relaxed) != 0 {   // commit_handover_v4
src-tauri/src/daemon/handover.rs:762      if self.spawn_owners.load(Ordering::Relaxed) != 0 {   // commit_handover_v5
```

The only six call sites of `retain_spawn_owner` are inside `#[cfg(test)] mod spawn_owner_tests`
(`handover.rs:462-512`). The unmatched hits are the definition, the field, the initialiser, the
`fetch_add`/`fetch_sub` inside the definition and its `Drop`, and the three reads. **Nothing in
production code ever increments the counter.**

Corroborating evidence from the candidate's own build logs (produced by its verification runs, not by
me):

`.omo/evidence/local-pane-liveness-completion-replan/task-8/pass3/mac/logs/all-targets.log:882`

```
warning: struct `SpawnOwnerGuard` is never constructed
   --> src/daemon/handover.rs:450:19

warning: method `retain_spawn_owner` is never used
   --> src/daemon/handover.rs:574:19
```

The same two warnings appear in `transfer.log:362/368` and in the Linux logs
(`zero_config_gen4_audit-list.log:336/342`, `target-app_menu_contract.log:336/342`). The compiler
independently confirms the grep.

## 3. Ownership is not tracked by a different mechanism now

Checked the alternative the brief asked about - `prune_dead_spawn_ownership` - and it is unrelated:

- `src-tauri/src/daemon/session_service.rs:2483` `prune_dead_spawn_ownership(now)`, called from
  `handle_spawn` at `:1838`.
- Its body prunes `session_metadata` entries whose session is no longer live (via
  `release_session_ownership`), expires `spawn_idempotency_cache` entries past `SPAWN_REQUEST_TTL`, and
  drops stale `provider_session_claims`.
- It never touches `HandoverManager`, `spawn_owners`, or `retain_spawn_owner`.

The daemon does hold a separate, *working* in-flight gate: `HandoverManager::retain_request`
(`handover.rs:835-855`, `in_flight: Mutex<usize>` at `:521`), reached from
`DaemonSessionService::retain_machine_request` (`session_service.rs:1098-1105`). But that guard is for
*request* admission on the machine API, not for the spawn path, and the three `spawn_owners` reads do
not consult it.

So the invariant is genuinely inert. `AGENTS.md`'s "no weakened test, no simulated receipt" and the
plan's own spirit ("test decision: new tests require demonstrated baseline or targeted mutation
failure") are violated in the usual way: the test passes because the code path it tests is only
reachable from the test.

## 4. Consequence

The guards are **fail-open**: a handover can prepare and commit while a spawn is in flight, and the
`HANDOVER_BUSY` protection the guard was written to provide does not exist. This is exactly the
condition behind the historical handover session-loss class (see
`notes/incidents/2026-09-26-handover-lost-26-of-37-sessions.md` and
`2026-09-26-handover-lost-35-of-55-sessions.md` in the memory store: a handover overlapping in-flight
session work). The candidate added the guard and the test but not the wiring, so it reads as protection
in review while providing none.

## 5. Options

### Option 1 - Wire it into the production spawn path

Claim a `SpawnOwnerGuard` at the top of the real spawn entry and hold it for the whole registered-session
lifecycle. `DaemonSessionService::handle_spawn` (`session_service.rs:1714`) is that entry: it is reached
from the local-split create path (`:529`, `LOCAL_SPLIT_SPAWN.scope(deadline, self.handle_spawn(...))`)
and from `spawn_machine` (`:858`).

- Pros: makes the invariant real; closes the exact overlap the guard documents; converts two compiler
  warnings into used code; is what the plan's "generation fencing"/ownership language asks for.
- Cons: a spawn attempted while a handover is *prepared* now fails with a new error string instead of
  proceeding. That is the intended behavior, but it is a behavior change and needs its own test.

### Option 2 - Withdraw the guards and the claim

Delete `spawn_owners`, `SpawnOwnerGuard`, `retain_spawn_owner`, the three reads and the test.

- Pros: removes dead code and an honest-looking but inert invariant; no behavior change.
- Cons: abandons a real protection that the handover path should have. The plan's todo 4 asks to "run
  existing handover contract first, fix only observed missing guarantees" and todo 2 asks for "checked
  live-arm binding; ACK only after installed" - withdrawing the guard without a replacement leaves the
  overlap unprotected and would have to be reported as a known gap.

### Recommendation: Option 1

Withdrawing a protection the plan's handover task depends on is the wrong direction, and the wiring is
small and local. It is included in `F2-1-suspend-windows.patch` (its `daemon/session_service.rs`,
`daemon/handover.rs` and `daemon/server.rs` hunks; the rest of that patch is F2-1). What it does:

- **below every spawn replay in `handle_spawn`** (the machine `previous`-record replay, the
  `spawn_idempotency_cache` hit, and the live-session `client_request_id` scan) and immediately above
  the `spawn_remote` early return, claim the guard:
  `self.handover_manager.upgrade().map(|manager| manager.retain_spawn_owner()).transpose()`, mapping
  `Err("HANDOVER_BUSY")` to `SpawnError::Other("Daemon handover is in progress and does not accept new
  sessions")`. That placement is load-bearing and was moved down by the pre-apply audit's F1 - see §6.1,
  and the code comment in `handle_spawn`;
- pass it into `spawn_remote` as a new first parameter `_spawn_owner: Option<&SpawnOwnerGuard>`, so the
  remote path holds the claim for its own lifecycle too (the parameter is named with a leading
  underscore because it is held deliberately, not read);
- leave an absent `Weak` manager permissive, which preserves the fixtures that build a
  `DaemonSessionService` without a handover owner. I checked the handover integration tests
  (`tests/machine_owner_handover.rs`, `tests/support/session_metadata_handover.rs`): they create the
  session *before* `prepare_handover` and spawn nothing between prepare and commit, so no existing test
  starts failing.

**Not verified:** that no production path spawns a session while a handover is prepared in a way the
product *needs* to keep working. If a legitimate flow exists (for example a GUI reconnect that spawns
during the successor's prepared window), this change would surface it as a new error rather than
silently allowing the overlap - which is the honest outcome, but it needs a Windows/macOS compile and
the handover contract suite to confirm before landing. That is why this is a patch and not a commit.

---

## 6. Post-audit update (2026-10-05) - activation cost and retry contract

`PATCH-PRE-APPLY-AUDIT.md` attacked the artifact that carries this wiring and recorded two `major`
findings against it. Both are about the claim described above, and both are now fixed in the
regenerated `F2-1-suspend-windows.patch`
(sha256 `99c3d711ce0555fdfabbe7f3431fac89d1e6bef9c93b314dd0d436616c89351a`, 35 103 B, 8 files).

### 6.1 F1 - the claim sat above the spawn replays and broke idempotency

The claim was placed immediately after the draining check, which is **above** the idempotency-cache
replay and the machine `previous`-record replay. During the `prepare`->`commit` handover window a
*retry of an already-created spawn* therefore failed with `SpawnError::Other("Daemon handover is in
progress and does not accept new sessions")` instead of returning the session it had already created.
The wiring is now placed **below every replay and immediately above the `spawn_remote` early return**,
so only a request that is about to create a session takes the claim, and both the local and the remote
create paths stay gated. `F2-1-SUSPEND-WINDOWS-REGRESSION.md §0.1` carries the full reasoning.

### 6.2 F2 - this wiring is what first activates the three gates

The finding, in the audit's own terms: `retain_spawn_owner()` had **zero** production callers before
this patch (only `handover.rs`'s `#[cfg(test)] mod spawn_owner_tests`), so this wiring is the first
thing that ever increments `spawn_owners`. From the first production spawn onward, any long spawn -
`spawn_remote` waiting on a relay tunnel is the named case - delays `prepare_handover`,
`commit_handover_v4` and `commit_handover_v5` for its whole duration.

**Decision: keep the guard, and make the block typed, observable and retryable (step 3 of the brief,
not step 2's "already bounded").** Read from the source:

- **It cannot hang.** Each gate returns its refusal immediately; there is no internal wait, and no
  deadline to add, in `prepare_handover` / `commit_handover_v4` / `commit_handover_v5`. The failure mode
  was never a hang inside the daemon.
- **But it was not typed.** `return Err("HANDOVER_BUSY".into())` carried no guard identity, and the
  three call sites (`daemon/server.rs:4233` `PrepareHandover`, `:4357` `CommitHandover`, `:4774`
  `handle_upgrade_binary`) turned it into an untyped `DaemonResponse::Error`. The one caller that
  **polls** - `scripts/install-macos-app.mjs` driving `upgradeBinary` - could not distinguish
  "a spawn is in flight, retry" from "this handover is structurally broken, stop".

What the regenerated patch changes:

| Where | Change |
|---|---|
| `daemon/handover.rs` | `pub(crate) const HANDOVER_BUSY` and `pub(crate) fn is_spawn_gate_busy` - the token has one home, and the check cannot be re-implemented ad hoc at a call site |
| `daemon/handover.rs` | private `HandoverManager::spawn_gate_refusal()` builds the refusal from the live guard count: `HANDOVER_BUSY: {n} in-flight spawn(s) hold the handover gate`; the three gates use it, and `retain_spawn_owner` names the handover status instead |
| `daemon/server.rs` | `handover_failure_response(context, message)` maps a spawn-gate refusal to `DaemonResponse::HandoverRejected { reason }` and leaves every other handover failure the generic `Error` it already was, message text unchanged; all three call sites use it |
| `daemon/handover.rs` tests | new unix-gated `a_blocked_handover_reports_busy_and_succeeds_on_retry_after_the_spawn_completes` |

**The retry contract.** A blocked handover is retryable, not terminal. The gate reopens the moment the
last in-flight spawn's `SpawnOwnerGuard` drops, so the delay is bounded by that spawn's own work, never
by an unbounded wait. The caller's obligation: on
`{"type":"handoverRejected","reason":"HANDOVER_BUSY: ..."}`, wait and re-issue `upgradeBinary` (or
`prepareHandover` / `commitHandover`) rather than treating the daemon as unable to hand over. The
installer script itself is unchanged - its retry policy is the host operator's call, and this patch's
job is to make the outcome distinguishable and to state what is expected of it.

**Residual, accepted and named.** (1) The delay itself is the intended cost of this invariant: a
handover must not commit over a spawn that is still creating its session. (2) The guard covers the
**spawn**, not the later attach handshake (`ipc/terminal.rs::cmd_terminal_spawn_operation` is a separate
request), which is audit F3 - unchanged here, and the reason §5's "for the whole registered-session
lifecycle" wording was corrected. (3) The new test pins the retry contract but was not compiled or run:
no build or test was permitted in this session, so a Windows/macOS `cargo check` plus the handover
contract suite remain mandatory before landing, exactly as the paragraph above already says.
