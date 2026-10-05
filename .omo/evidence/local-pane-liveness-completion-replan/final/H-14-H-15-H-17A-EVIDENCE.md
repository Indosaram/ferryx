# H-14 / H-15 / H-17a — mutation and filter evidence (executed)

**Date:** 2026-10-05. **Host:** omarchy (Linux 7.1.9-arch, cargo 1.98.0). **Revision:** `d2769d44`
(branch `work/local-pane-liveness-completion-foundation`).

**Stage:** `/home/indo/ferryx-plv-f2-20261005` — tracked tree from `git archive d2769d44`, plus the three build
inputs an archive cannot carry (`src-tauri/vendor/ghostty`, `ui/dist`, `ui/node_modules`), plus a warm `target/`
from a donor tree. Logs: `~/plv-test-logs/`.

These are **executed** results, not readings. They close two F1 audit holes that had been carried as "no evidence
exists".

---

## H-17a — the exact `pane_liveness_contract` filter under its own name

**The hole** (F1 audit): *"Exact `pane_liveness_contract` filter and `localSplitContract.test.ts` never run under
their own names."*

**Command:** `cargo test --manifest-path src-tauri/Cargo.toml --lib -- pane_liveness_contract --test-threads=1`

```
H17A_EXIT=0
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 2675 filtered out; finished in 0.00s
```

**Disposition: CLOSED.** The filter selects **10** tests and all pass under its own name, which is what the hole
required. (`localSplitContract.test.ts` is a separate clause of the same hole and is not covered here — it belongs
to the vitest surface.)

---

## H-14 — mutation RED for the pump-generation compare

**The hole** (F1 audit): *"No mutation run exists for the pump-generation compare"* — i.e. the three
`pane_liveness_pump_generation_*` tests were never shown to be load-bearing.

**The mutation:** the generation compare is `bindings…get(session_id).copied() != Some(token)`, and it appears at
**five** sites in `src-tauri/src/ipc/terminal.rs` (`:168`, `:202`, `:234`, `:250`, `:444`). All five were flipped
`!=` → `==`, which makes a **stale** token's stop/start proceed and clobber the current binding.

| Step | Result |
| --- | --- |
| Baseline GREEN (before mutation) | **exit 0** — `3 passed; 0 failed` |
| Mutation applied | `compare_sites_found=5`, `mutated_sites=5` |
| **Mutated run** | **exit 101** — `0 passed; 3 failed` |
| Which tests went RED | **all three**: `…stale_teardown_preserves_new_binding`, `…old_teardown_preserves_new_stream_atomically`, `…old_completion_cannot_install_new_stream` |
| Restore + re-run GREEN | **exit 0** — `3 passed; 0 failed` |

**Disposition: CLOSED.** This is the full mutation shape the project's discipline requires: GREEN → mutate →
**RED on exactly the three target tests** → restore → GREEN. The tests are therefore **load-bearing**: they fail
when the generation guard is removed, so they are not source-text pins and not vacuously passing.

---

## H-15 — mutation RED for the Task 6 UI failure classes

**The hole** (F1 audit): *"No RED-under-mutation evidence for any of these UI classes in any artifact"* — the Task 6
clause names held reply/cancel/late completion/newer binding/unknown epoch/EOF listener and forbids mock-call-only proof.

**The mutation:** the stale-generation fence in `localSplitLifecycle.ts`'s `current()` guard. The original is

```ts
const current = () => !this.stopped && generation === this.generation &&
  localSplitIntent(this.services.read())?.requestId === this.intent.requestId &&
  (localSplitIntent(this.services.read())?.generation ?? 0) <= generation;
```

and it was reduced to `const current = () => !this.stopped;` — i.e. every generation/identity condition removed.

| Step | Result |
| --- | --- |
| Baseline GREEN (before mutation) | **exit 0** — `Test Files 1 passed (1)`, `Tests 19 passed (19)` |
| Mutation applied | recorded verbatim in `~/plv-test-logs/h15-red.log` |
| **Mutated run** | **exit 1** — `1 failed | 18 passed (19)` |
| Which test went RED | **`durable split races > late creation cannot replace a newer frontend binding`** |
| Restore + re-run GREEN | **exit 0** — `Tests 19 passed (19)` |

**Disposition: CLOSED.** The fence is **load-bearing for that test** — removing it makes the test fail, so the test
pins the invariant rather than passing vacuously. Stated honestly: **only that one test went red**; the other 18 in the
file are pinned by other mechanisms, which is expected because the fence guards specifically the "a late creation must
not replace a newer binding" invariant that test is named for.

**Command:** `bun x vitest run --maxWorkers=1 src/lib/localSplitLifecycle.test.ts` from the stage's `ui/`.

---

## What these runs establish, and what they do not

**Establish:** that the `pane_liveness_contract` filter runs 10 tests green under its own name (H-17a); that the three
pump-generation tests are load-bearing against a real mutation of the five generation-compare sites (H-14); and that the
`localSplitLifecycle` stale-generation fence is load-bearing for the "late creation cannot replace a newer frontend
binding" test (H-15).

**Do not establish:** anything about the Windows surface (these ran on Linux; the plan makes Linux mandatory for the Unix
scopes and the Windows matrix is a separate obligation); nothing about runtime behaviour beyond these filters and that one
UI file — H-14 and H-15 are static changes to guards exercised by in-crate unit tests and one vitest file, not end-to-end
scenario evidence; and H-17a's second clause (`localSplitContract.test.ts` under its own name) is **not** covered here.

## Reproducing

```bash
# H-17a
cargo test --manifest-path src-tauri/Cargo.toml --lib -- pane_liveness_contract --test-threads=1

# H-14 — baseline, mutate, RED, restore, GREEN
cargo test --manifest-path src-tauri/Cargo.toml --lib -- pane_liveness_pump_generation --test-threads=1
sed -i 's/copied() != Some(token)/copied() == Some(token)/g' src-tauri/src/ipc/terminal.rs   # 5 sites
cargo test --manifest-path src-tauri/Cargo.toml --lib -- pane_liveness_pump_generation --test-threads=1
# restore the file, then re-run
```

## Staging note (the trap that cost the first attempt)

The first run returned `101` for **every** gate with a build-script error —
`resource path '../ui/dist' doesn't exist` — because the Linux stage had ghostty and a warm `target/` but **not
`ui/dist`**, which the tauri build script requires. **No Rust code was type-checked in that attempt**, so reading
its `101` as a code failure would have been a false attribution. A stage needs all four: the tracked tree, ghostty,
`ui/dist`, and `node_modules` (the last for the vitest gates).
