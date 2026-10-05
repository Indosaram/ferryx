# Task 9 — Windows half: scenario verdicts, negative control and cleanup

Host: **maho-win** (`sook@100.126.171.58`, `DESKTOP-1LAPJMP`, Windows 10.0.26200.9457 x64,
rustc/cargo 1.97.0, Bun 1.4.0, Node v24.19.0). Staging root
`C:\Users\sook\ferryx-pane-completion\source-21dea3c0` (owned by this session),
evidence/log root `…\task9-c34b90fc`.

**VERDICT: all three Windows scenarios are NOT_RUN_BLOCKED, for two independent reasons.**

1. **No scenario binary exists.** `cargo build --features local-split-qa` exits **101** at `c34b90fc`
   (§ compile verdict). The three scenarios require the QA-feature delta, so they cannot be launched.
2. **`split-attach-stall` could not run even with a green binary.** The runner's own adapter
   contradicts itself before it spawns anything (below), so that scenario aborts with an uncaught
   `ASSERTION_FAILURE` and exit 1.

No scenario reached a native action, so **no screenshot was ever produced and the independent
image-reader lane has nothing to read** — the required `multimodal-looker` verdict does not exist for
any of the three, and none is claimed. Per the brief, a missing image-reader verdict is never a pass.

---

## 1. Per-scenario table

| Scenario | Exact argv | Raw native exit | Asserted line / terminal error | Selected count | Verdict |
|---|---|---|---|---|---|
| `split-happy` | `node scripts/qa/pane-liveness.mjs --scenario split-happy --binary <default ferryx.exe> --evidence-dir …\win-evidence-default --isolation-root …\win-runtime-default\split-happy` | **7** (QA build: n/a — no binary) | `BARRIER_ACK_TIMEOUT: product did not settle barrier fixture-setup receipt[0] within 9000ms` → `verdict: BLOCKED`, `cleanupGate.ok: true` | n/a | **NOT_RUN_BLOCKED** |
| `split-attach-stall` | same shape | **1** | `HarnessError: ASSERTION_FAILURE: targetRole must be 'predecessor' or 'successor', never a session ID or wildcard: got "producer"` at `common-harness.mjs:514` (`BarrierHub.prearm`, called from `pane-liveness.mjs:265`) — **before launch**; no `latest.json` written | n/a | **NOT_RUN_BLOCKED** |
| `split-cancel` | same shape | **1** — kill artifact, not the runner's own code (see the caveat below); the runner's typed exit would have been 7 | `BARRIER_ACK_TIMEOUT: product did not settle barrier fixture-setup receipt[0] within 9000ms` → `verdict: BLOCKED`, `cleanupGate.ok: true` | n/a | **NOT_RUN_BLOCKED** |

Image-reader verdict column: **absent for all three** — there is no capture to read.

### Caveat on the `split-cancel` exit code (reported rather than smoothed)

The `split-cancel` run printed its complete `BLOCKED` result (identical in shape to `split-happy`) and
then **the node process never exited** — inspected alive at ~0.2 s CPU, long after the result was on
disk: a descendant of the spawned app (`ferryx.exe` PID 8820) held the inherited stdio pipe. This
verifier therefore `taskkill /T /F`-ed the hung tree, and the wrapper then recorded
`NEG2_RUN_EXIT split-cancel = 1`.

- **Observed raw exit: 1 — a kill artifact, not a product or runner signal.**
- The runner's own mapping is `BARRIER_ACK_TIMEOUT → EXIT.barrierUnsupported = 7`, and its printed
  `result.json` verdict is `BLOCKED` with `cleanupGate.ok: true`.
- Both numbers are stated so the artifact is not mistaken for a product finding. It *is* a genuine
  runner/cleanup robustness observation: the app's own descendant survives the runner's reap and keeps
  the parent's stdio pipe open, so a failed scenario can hang its runner indefinitely
  (owning lane: QA adapter/runner scripts; `split-happy` happened not to reproduce it).

## 2. Negative control — the runner cannot be fooled into a PASS

This is the decisive experiment available *without* the QA binary, and it is what makes the
NOT_RUN_BLOCKED verdict safe rather than merely unverified: it shows the harness **fails closed**.

Method: build the **default-feature** (no `local-split-qa`) binary from the same staged `c34b90fc`
tree, then run the exact scenario argv against it.

- Binary: `src-tauri\target\debug\ferryx.exe`, sha256
  `fcab1abeea12ad184697a00d9415f1bf7341eedaa09e1e9f68f53b0ec52c98e9`, 101 938 688 bytes,
  built by `cargo build --manifest-path src-tauri/Cargo.toml` (exit 0, 55.4 s, log
  `logs/cargo-build-default-restore.log`; a prior identical build produced a *different* hash — see §4).
- Marker scan of that binary (ASCII over the file bytes; **string literals live in `.rdata`, symbol
  names do not — MSVC puts those in a `.pdb` that is absent**):

| Marker | In default binary |
|---|---|
| `FERRYX_QA_BARRIER_DIR` | **False** |
| `FERRYX_QA_RETRY_REFUSED` | **False** |
| `FERRYX_QA_STALE_BINDING_UNSERVICED` | **False** |
| `FERRYX_QA_FIXTURE_SETUP_UNSETTLED` | **False** |
| `qa_producers` / `qa_liveness` / `collect_gui_fixture_sessions` / `start_gui_boot_channel` | **False** (symbols — no `.pdb` to scan; not used as evidence) |
| `Attach requires the persisted seven-field pane binding` | **True** — candidate-lineage literal (introduced by `5464da0d`) |
| `Attach binding incarnation cannot be proven` | **True** — candidate-lineage literal (introduced by `5464da0d`) |
| `Predecessor exported no authoritative owner record for` | **False on Windows** — present on linux; the handover-adoption path is unix-only in practice, so the MSVC linker drops it. Reported, not hidden. |

Result: every QA marker is absent, two candidate-lineage literals are present, and the runner refuses
to settle `fixture-setup` without the product's private channel — **`BLOCKED` + exit 7, never a false
PASS**. `cleanupGate.ok: true` on both scenarios that got as far as the runner's `finally` block.

## 3. Second, independent blocker — the adapter contradicts itself (`split-attach-stall`)

`scripts/lib/qa-scenarios/common-harness.mjs` defines

```js
export const BARRIER_ROLES = Object.freeze({
  'backend-write': 'producer', 'presentation': 'producer',
  'attach-handshake': 'producer', 'held-rpc': 'producer',
  'predecessor-export': 'predecessor', 'successor-adopt': 'successor',
  'commit': 'predecessor', 'abort': 'successor',
});
```

while `BarrierHub.prearm()` in the same file rejects any `targetRole` that is not
`'predecessor'`/`'successor'`. `pane-liveness.mjs` pre-arms **every** barrier with
`BARRIER_ROLES[barrier]` **before launching the binary**, so `split-attach-stall` (the only one of the
three Windows scenarios with an armed barrier) throws before the product is ever spawned.

- Owning lane: **QA adapter / scripts lane** (`scripts/lib/qa-scenarios/*.mjs`, `scripts/qa/pane-liveness.mjs`).
- Provenance: present since `5464da0d` and unchanged by `c34b90fc`; `scripts/` does not exist at base
  `d82b35e4`, so the contradiction is candidate-authored, not inherited.
- Consequence for Task 9: **a green QA binary alone would not make `split-attach-stall` runnable.**

## 4. Provenance of the staged tree and the built artifact

- Windows `source-21dea3c0` was at **`39e722ce`** before staging: `handover_socket.rs a5d7e0b4e3ca77de`,
  `server.rs 163abf5260f201cc`, `ipc/terminal.rs 3946a80bb6012ba6`, `qa_barrier.rs 5391a9f4170598cc`
  (all four match the `39e722ce` column of the revision-hash matrix; the same values are recorded in the
  earlier recon's NOTEPAD).
- Delta `39e722ce → c34b90fc`: 21 paths, **0 deletions** (`git archive`, 373 157 B).
- **Full-manifest check after extraction: `checked=5086 missing=0 diff=0`** against the `c34b90fc`
  tracked-file sha256 manifest (5090 rows; `src-tauri/vendor/ghostty` is the pinned junction
  `C:\Users\sook\task2-ghostty-6a508fd5` @ `6a508fd5e34c7e222c052a6d00bb3891ff3feace`, skipped as a directory).
- **mtime trap defeated:** after extraction every extracted file **and** every `.rs`/`.toml` under
  `src-tauri\{src,tests,examples}` plus the crate-root manifests were set to the extraction instant
  (`TOUCHED_AT 2026-10-04T22:53:21+09:00`), and the stale `ferryx.exe`/`.pdb` were **deleted** before each
  of the two builds. Both build logs show a real `Compiling ferryx v2026.928.7 (…\src-tauri)` followed by
  `Finished dev profile … in 3m 05s` — the crate was genuinely recompiled, not answered `Fresh`.
- **Built-binary revision proof — what is and is not available on this host.** The dispatch suggested
  `strings`-ing the binary for a symbol that exists only in `c34b90fc` (`adopt_transferred_ownership`,
  `qa_producers`, `adopt_runner_bind`). **That check cannot be run here, and the reason is reported
  rather than papered over:**
  1. MSVC keeps function *names* in a `.pdb`, and no `.pdb` exists in the staged tree
     (`PROV_BIN_EXISTS True PDB_EXISTS False`).
  2. Every `c34b90fc`-unique *string literal* sits inside a `local-split-qa` block, so it is absent from
     the default binary by construction — verified: `activeAttachTuple` occurs only at
     `surface_host.rs:982`, inside the `#[cfg(feature = "local-split-qa")]`-gated
     `emit_stale_receipt_rejected_qa`.

  The proof used instead does not depend on the linker: **(a) source identity** — the full tracked-file
  manifest check reports `checked=5086 missing=0 diff=0` against the `c34b90fc` manifest, so the tree
  *is* that commit; **(b) observed recompilation** — `Compiling ferryx v2026.928.7` in the log, not
  `Fresh`; **(c)** the stale `ferryx.exe`/`.pdb` were deleted immediately before the build, so no pre-fix
  object could be reused; **(d) candidate-lineage literals present** — `Attach requires the persisted
  seven-field pane binding` and `Attach binding incarnation cannot be proven` are in the artifact (both
  introduced by `5464da0d`, after the `d82b35e4` base). Note (d) alone does **not** discriminate
  `c34b90fc` from `39e722ce`; (a)+(b) carry the discrimination.
- **The QA build produced no artifact at all** (`QA_BIN_MISSING`) — itself the proof that the build
  failed rather than silently reusing the previous binary.
- **Windows default builds are not byte-reproducible:** two default-feature builds of the identical tree
  produced `1b7d5a0c51bb3ee294e1ee1d36a5f3d029e6f87fd2a684878f28adfed1b44fd5` and
  `fcab1abeea12ad184697a00d9415f1bf7341eedaa09e1e9f68f53b0ec52c98e9`. **A binary sha256 is therefore not
  a revision proof on this host**; the manifest check plus string literals are used instead.

## 5. Host load recorded with every result

| Moment | Reading |
|---|---|
| staging start | `FREE_GB 29.04`, load 31 % |
| default build | load 43 %, 3m05s |
| QA build (failed) | load ~43 %, 73 s |
| negative control | `NEG2_LOAD 53`, `FREE_GB 32.4` |
| teardown | `FREE_GB 32.41` |

The QA build failed in 73 s with hard type errors, so no timeout was involved and load is not an
explanatory factor for it.

## 6. Cleanup receipts

| Resource | Teardown | Receipt |
|---|---|---|
| `ferryx.exe` PID 3136 (left by `negctl-split-happy`, the run against the missing binary) | `Stop-Process -Force` | `TEARDOWN_3136_ALIVE_AFTER False` |
| `ferryx.exe` PID 8820 (left by the hung `split-cancel` run) | `taskkill /T /F /PID 8820` | gone; final sweep shows **zero** task-owned `ferryx` |
| node PID 16964 (hung `split-cancel` runner) | `taskkill /T /F /PID 16964` | gone |
| `…\task9-c34b90fc\win-runtime-default\split-attach-stall` | **leftover** | empty directory skeleton (0 files) — the runner died before its cleanup could remove it; owned, harmless, reported rather than silently deleted |
| `…\task9-c34b90fc\win-runtime-default\{split-happy,split-cancel}` | removed by the runner | `NEG2_ISO_LEFTOVER … False` |
| foreign `cargo.exe` processes (8, other sessions' trees) | **untouched** | not this session's to kill |
| user desktop / production app / production daemon | **never addressed** | — |

## 7. What the parked mac half must still cover

Unchanged from the dispatch, and now additionally blocked on the same compile failure:
`diagnostic-classifier`, `retained-handover`, `handover-abort`, `suspension-ownership`, `stale-binding`,
plus `split-happy` / `split-attach-stall` / `split-cancel` / `split-concurrent` re-run on the
QA-feature binary once one exists. The `externally-stopped` fixture-kind gap continues to block only
`suspension-ownership`. Windows suspension remains honest-unsupported
(`terminal/suspension/windows.rs` → typed `UnsupportedPlatform`) and was never reached.
