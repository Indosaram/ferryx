# `replace_relay` contract — decision record

**Date:** 2026-10-05. **Basis:** code read at `d2769d44` (this work stream) and `origin/main` = `af95b9a7`
(the pane-liveness merge), plus the uncommitted working tree of `/Volumes/T9-Mac/project/ferryx`.

**Why this needs a decision:** two versions of one function exist, they are **mutually exclusive**, and whichever lands
second silently removes the other's capability.

---

## The two versions

| | This work stream (landed on `main` by the merge) | The peer's in-flight work |
| --- | --- | --- |
| Location | `src-tauri/src/remote/server.rs` (`:4721` in the merged tree) | same file, **staged** (`:4960`) |
| Signature | `pub fn replace_relay(&mut self, state, client) -> Option<tokio::task::JoinHandle<()>>` | `pub(crate) fn replace_relay(&mut self, state, client)` — returns **`()`** |
| Behaviour | takes the previous `relay_task`, aborts it, **returns it** so the caller can join and confirm cancellation | takes the previous `relay_task` and aborts it, discarding the handle |

## The evidence that decides it

**1. This work stream's version has an executable contract test that requires the return value.**
`src-tauri/src/remote/server.rs:8285` and `:8323`:

```rust
let prev = handle.replace_relay(Arc::clone(&state), client_1.clone());
assert!(prev.is_none(), "first replace has no previous task");
...
let prev_task = handle.replace_relay(Arc::clone(&state), client_2);
assert!(prev_task.is_some(), "second replace returns previous task");
let join_res = tokio::time::timeout(Duration::from_secs(5), prev_task.unwrap()).await;
assert!(join_res.is_ok(), "previous task join timed out");
let join_err = join_res.unwrap().expect_err("previous task must be aborted");
assert!(join_err.is_cancelled(), "previous task was cancelled");
```

That test is named `test_remote_server_handle_prepare_and_replace_relay_contract` and it is part of the
baseline relay repair this stream delivered (`d82b35e4`). **It cannot compile against the `()` version.**

**2. The peer's version has no use of the return value at all.** In their staged `remote/server.rs`,
`replace_relay` occurs **once** — the definition. There is no `let prev`, no `prev_task`, no contract test.

**3. Therefore the two are mutually exclusive, and the loss is silent.** If the peer's `()` version commits on top of
`main`, the candidate's contract test stops compiling (or, if the test is also dropped, the *capability* disappears
without any test noticing): the caller can no longer prove the previous relay task was cancelled. The daemon's own
caller (`src-tauri/src/daemon/server.rs:4950`) discards the value today, so **only the test would catch the loss** —
which is exactly the kind of silent regression the project's rules exist to prevent.

## Recommendation

**Keep the returning contract** (`Option<JoinHandle<()>>`), for two reasons that are checkable rather than stylistic:

1. It is the only version with an **executable** contract that proves the abort happened (`join_err.is_cancelled()`).
   The `()` version asserts nothing.
2. It is what the baseline relay repair was written against, so reverting it re-opens the repair's own question.

**If the peer's `()` version is the intended one**, then the change must be explicit, not incidental: delete
`test_remote_server_handle_prepare_and_replace_relay_contract`, and state where the "previous task was cancelled"
guarantee now lives. Otherwise the guarantee is simply lost.

## What this record does not establish

- Which version the peer *intends* — their intent is not in the repo; the `pub(crate)` narrowing suggests deliberate
  encapsulation, and the dropped return may be deliberate or may be an artifact of writing the definition fresh.
- Whether any production caller needs the handle. Today the daemon discards it, so production behaviour is identical
  either way; the difference is in what can be **proven**.

## Timing

The peer's file is **uncommitted**, so the collision has not happened yet. **Deciding before they commit is cheaper than
resolving it afterwards**, because after the fact the losing side's test failure looks like a fresh regression rather
than a contract choice.
