# Task 1 Source Inventory: Candidate Baseline Composition

- **Worktree C:** `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`
- **Branch:** `work/local-pane-liveness-completion-foundation`
- **Base Commit:** `d82b35e43f208b53adf6310f4e3c89cbde8814f4`
- **Plan:** `.omo/plans/local-pane-liveness-completion-replan.md` (Task 1)
- **Status:** **SOURCE COMPOSITION COMPLETE — READY FOR REMOTE VERIFICATION**
- **Date:** 2026-10-03

---

## 1. Executive Summary

This inventory documents the precise source composition for Task 1 of the local pane liveness completion replan. In strict adherence to project boundaries:
- Worktree `C` was created cleanly from committed base `d82b35e4`.
- Shared `main` and salvage worktree `W` (`/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1`) were preserved as **strictly read-only** (zero uncommitted edits, resets, or stash operations).
- No local or remote builds/tests, no GUI/daemon actions, and no git commits were executed in this session.
- Exactly 19 files (10 modified, 9 untracked) were selected from verified immutable receipts and applied to `C` using `git apply` and direct untracked staging.
- All 19 files match their expected pre/post SHA-256 hashes bit-for-bit (100% agreement).
- Exactly 16 uncompiled/unverified overlay files present in `W` were deliberately **excluded** to prevent unproven regression drift.

---

## 2. Selected Files Manifest (19 Files)

| Path | Status | Category | SHA-256 | Bytes | Role |
|---|---|---|---|---:|---|
| `src-tauri/Cargo.toml` | modified | minimal-two-test-repair | `7650c676ecf125e1f3e4b81809736b8e583cf024a0a079370056d90ec617d100` | 6,925 | Adds `local-split-qa` feature |
| `src-tauri/src/ipc/mod.rs` | modified | minimal-two-test-repair | `7af2941ab86c5580f41c2bb22ab6caab0ae5d22789f048b6f88eb4f813b1b78a` | 1,939 | Declares `pub mod qa_barrier` under feature |
| `src-tauri/src/main.rs` | modified | minimal-two-test-repair | `45edd802330ae4bdca941a89629f109472c7f79a911aaefbe8ae6792b215ffb4` | 4,812 | Registers qa_barrier IPC commands |
| `src-tauri/src/ipc/qa_barrier.rs` | untracked | minimal-two-test-repair | `a69d9b10c6a7d0428d1176f1e2841def64a76ab2ef62fc9600c3c1d4c753fe61` | 35,740 | Isolated test barrier coordination |
| `src-tauri/src/ipc/native_terminal.rs` | modified | minimal-two-test-repair | `69b80dd05ac3b0cb687bfa54182de2f63f3df364995759452a6f47cabe1f93e2` | 146,034 | Repaired writer/barrier test harness |
| `src-tauri/src/native_terminal/surface_host.rs` | modified | minimal-two-test-repair | `7b844f58bbb281e633b9eb2f1e30dea6c1a7fafae4de25c25a77115926f4edaf` | 364,917 | Repaired presentation barrier test harness |
| `src-tauri/src/ipc/debug.rs` | modified | frozen-diagnostics | `3ce0efe38cc8bbcd27afbee6cf9a37a99f9ca7b21edc0068d349abaac9a7967e` | 32,579 | Pane liveness telemetry & allowlist sanitization |
| `ui/src/lib/paneLiveness.ts` | untracked | frozen-diagnostics | `070e54b8f4a35f86326103e9b0a5292aa7b8440b0c90d1842e2d0643d01bc9ee` | 8,728 | Frontend pane liveness observation bridge |
| `ui/src/lib/paneLiveness.test.ts` | untracked | frozen-diagnostics | `858baf654f866cc889f5c7b0df05613e040bcce1b9340bcc49a48aba6c09d8e2` | 7,127 | Unit tests for pane liveness observation |
| `ui/src/lib/paneDebugInfo.ts` | modified | frozen-diagnostics | `7caeb59b605589f2d5464894f2308abaf562d2a9e561b7b7f187b9b6664e804c` | 2,304 | Formats pane debug JSON with liveness state |
| `ui/src/lib/paneDebugInfo.test.ts` | modified | frozen-diagnostics | `3772d3fbd3fb3a32cee093f3d7a7d95f032f9d1a62f60a7987182f133f1b9dcd` | 2,163 | Tests for formatPaneDebugInfo |
| `ui/src/lib/switchDebug.ts` | modified | frozen-diagnostics | `e9e1f742cd0a6552df6245e5a73594141093ec3610d68fb826ebb7eda27e36e1` | 3,747 | Structured switch debug logging |
| `ui/src/lib/switchDebug.test.ts` | modified | frozen-diagnostics | `6cd6cc7ab3348866984aa4417ffb6d8b84545d0180e49a6c30dcfad8f4ec9a53` | 3,719 | Tests for switchDebug |
| `scripts/lib/qa-scenarios/common-harness.mjs` | untracked | minimal-two-test-repair | `be5cf74ddce35fa75100ad99205a76c36e39144f42f58c7c3471e087fa299696` | 27,296 | Process supervision, barrier hub client |
| `scripts/lib/qa-scenarios/diagnostic-classifier.mjs` | untracked | minimal-two-test-repair | `0bdb7cfa3204e39baa52e2e10b772e26db5804b141c39846d3be14c830995df9` | 7,392 | Repaired headless/native diagnostic classifier |
| `scripts/lib/qa-scenarios/native-driver.mjs` | untracked | minimal-two-test-repair | `60f4e23514b1c653f3a1385d3b731c8624ee2756f9e3d759094f4ecaf587f161` | 14,339 | Desktop native automation driver |
| `scripts/qa/pane-liveness.test.mjs` | untracked | minimal-two-test-repair | `0d369d8518fe9bd7ad127904c83f15c5dc51bbed018dc43d28a9882c0f5a1b32` | 28,118 | Repaired 19-test canonical runner suite |
| `scripts/qa/pane-liveness-vitest.config.mjs` | untracked | minimal-two-test-repair | `a8709872b371397000497aaea5b7bfea5ee19414d240c61ae4f850ec44a779a9` | 391 | Runner Vitest configuration |
| `scripts/qa/pane-liveness.mjs` | untracked | minimal-two-test-repair | `2d4c0080e656610c5ee0ffbd04db75084ced68d726d0af7f99ec9037096e486c` | 31,657 | CLI runner entrypoint |

---

## 3. Deliberately Excluded Overlay Files (16 Files in W)

| Path | In W Status | Exclusion Justification |
|---|---|---|
| `src-tauri/src/daemon/client.rs` | `+68 / -4` | Uncompiled Task 3 daemon client split operation draft. Belongs to Task 3. |
| `src-tauri/src/daemon/handover.rs` | `+532 / -0` | Unverified Task 4 handover state transfer draft. Belongs to Task 4. |
| `src-tauri/src/daemon/protocol.rs` | `+50 / -0` | Unfrozen protocol extensions. Belongs to Task 2 contract freeze. |
| `src-tauri/src/daemon/server.rs` | `+140 / -10` | Uncompiled daemon server split handling. Belongs to Task 3. |
| `src-tauri/src/daemon/session_service.rs` | `+32 / -0` | Uncompiled daemon session service draft. Belongs to Task 3. |
| `src-tauri/src/ipc/terminal.rs` | `+443 / -0` | Uncompiled cmd_terminal_spawn_operation draft. Belongs to Task 3. |
| `src-tauri/src/ipc/file_link_tests.rs` | `+4 / -0` | Incidental test fixture noise in dirty overlay. |
| `src-tauri/src/ipc/native_terminal_disabled.rs` | `+5 / -0` | Incidental disabled native stub noise. |
| `src-tauri/src/ipc/tests.rs` | `+4 / -0` | Incidental IPC test noise in dirty overlay. |
| `src-tauri/src/lib.rs` | `+1 / -0` | Incidental export noise in dirty overlay. |
| `src-tauri/src/remote/tests.rs` | `+8 / -0` | Incidental remote test noise in dirty overlay. |
| `ui/src/components/NativeTerminalPane.tsx` | `+54 / -10` | Uncompiled frontend presentation draft. Belongs to Task 6. |
| `ui/src/components/TerminalSplitView.tsx` | `+15 / -4` | Uncompiled frontend split retry/cancel draft. Belongs to Task 6. |
| `ui/src/lib/nativeTerminalInputQueue.test.ts` | `+52 / -0` | Uncompiled queue test draft in dirty overlay. |
| `ui/src/lib/nativeTerminalInputQueue.ts` | `+61 / -0` | Uncompiled input queue extension draft. Belongs to Task 6. |
| `ui/src/lib/nativeTerminalLifecycle.ts` | `+12 / -2` | Uncompiled frontend presentation lifecycle draft. Belongs to Task 6. |

---

## 4. Immutable Reference Receipts

All reference receipts were independently measured on physical disk in `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1/.omo/evidence/local-pane-liveness-root-remediation/` and match `manifest.json` exactly:

- `final-index.md`: `.omo/evidence/local-pane-liveness-root-remediation/minimal-two-test-repair/final-index.md`
  - Measured SHA-256: `2f732430aaea97830bdc468e020e7ca8f42de7ecb4fc5363087660626ae39ea2` (2,097 bytes)
  - Prior Invalid Value: `b70742f5fa979f429bc0fa69db4d1dbf41ecb25ff0e719544bc02814aa27f8eb` (invalid drafting placeholder; corrected 2026-10-03 per gate review BLK-01)
- `result-v4.md`: `.omo/evidence/local-pane-liveness-root-remediation/minimal-two-test-repair/result-v4.md`
  - Measured SHA-256: `6d6150fabb0c3ab537070852010b57294f93c0cc161f18f88eb8ee1ef94c224f` (2,496 bytes)
  - Prior Invalid Value: `a64860b7952a1b94e09f5fa44b3c4f2bb7f55152d194cf2fe6264e1c5188e00e` (invalid drafting placeholder; corrected 2026-10-03 per gate review BLK-01)
- `runner-result.md`: `.omo/evidence/local-pane-liveness-root-remediation/minimal-two-test-repair/runner-result.md`
  - Measured SHA-256: `296df22f493ff0fd2ffb3e9968c0788ad8ee1ea82c10d7d1d8fa65ec3ed3357f` (2,899 bytes)
  - Prior Invalid Value: `26b38c2a93fdf6c6e7f223f03b879ecaa167098e94fa0076241b7dd79ef53da2` (invalid drafting placeholder; corrected 2026-10-03 per gate review BLK-01)
- `task-2-diagnostics.json`: `.omo/evidence/local-pane-liveness-root-remediation/task-2-diagnostics.json`
  - Measured SHA-256: `feda27be823a6351ba8a63b69ffb211452913c0f61c23d425cbe21d05c83e3fb` (5,380 bytes)
  - Prior Invalid Value: `2c50a00e57ba5ee37004fcfd7463f28328bf35a42fe12260ff0dbe26798e46fe` (invalid drafting placeholder; corrected 2026-10-03 per gate review BLK-01)
- `task-3-frozen-source-hash-audit.txt`: `.omo/evidence/local-pane-liveness-root-remediation/task-3-frozen-source-hash-audit.txt`
  - Measured SHA-256: `be11c2012cac425421ee1d368516c5420151d51dd67050fc816454d0f746b3da` (4,126 bytes)
  - Prior Invalid Value: `8862804b08709405f69747519965d1d6e191986dd033501a403ff255ef483e58` (invalid drafting placeholder; corrected 2026-10-03 per gate review BLK-01)
- `input-manifest.json`: `.omo/evidence/local-pane-liveness-root-remediation/input-manifest.json`
  - Measured SHA-256: `85a3448e5916079a697329a01608d92c588bab0d12bf1c98f366b6c86087c110` (18,909 bytes)
- `source-inventory.md` (in W): `.omo/evidence/local-pane-liveness-root-remediation/source-inventory.md`
  - Measured SHA-256: `46eb625cdd0acb898ab617f41094af2b7fcc3787332000bba5adb98a87e20da0` (24,607 bytes)

### 4.1 Provenance of Discrepancy & Correction History
- **Mistake Provenance:** The preliminary `source-inventory.md` authoring draft and initial turn summary copied five speculative placeholder hashes from an unexecuted drafting template before programmatic measurement completed.
- **Audit Discovery:** Gate reviewer `omo-native-gate-reviewer` cited `BLK-01` (`CRIT-EVIDENCE-INTEGRITY`) in `task-1-gate-review.md`, confirming that while `manifest.json` recorded authentic disk hashes, `source-inventory.md` had preserved the unverified strings.
- **Correction Action:** Replaced placeholder strings with exact values matching physical files on disk and on-disk `manifest.json`. Prior placeholder hashes are explicitly recorded above as invalid rather than silently erased.

---

## 5. Adversarial Check Receipt

- **Check:** `git apply --check` against deliberately mismatched patch fixture.
- **Fixture:** `/tmp/task1-mismatch.patch` (context line tampered with `_TAMPERED_MISMATCH`).
- **Result:** Rejected with `error: patch failed: src-tauri/Cargo.toml:51`, `error: src-tauri/Cargo.toml: patch does not apply`, exit code 1. Candidate worktree hash and working files remained completely unchanged.


---

## 6. Revision 2 Source Closure: NativeTerminalInputQueue Telemetry Integration

- **Date:** 2026-10-03
- **Defect Remediation:** `defect-report-source-omission.md` (TS2339 `getQueuedHeadAgeMs` missing on `NativeTerminalInputQueueManager`).
- **Inclusion Rationale:** `ui/src/lib/paneLiveness.ts` (lines 176, 216), `ui/src/lib/paneDebugInfo.ts`, and `ui/src/lib/paneLiveness.test.ts` directly query `terminalInputQueue.getQueuedHeadAgeMs(sessionId)` to distinguish in-flight execution latency from queue waiting backlog. Omitting the queue telemetry extensions broke TypeScript compiler checks during remote UI build (`ui-build.log`).
- **Files Promoted from Excluded to Selected:**
  1. `ui/src/lib/nativeTerminalInputQueue.ts`
     - Pre-SHA-256 (`d82b35e4`): `34c082b1d53d14dec5b905313cd14626e5bc683a0dee774f099cb6705aca2969`
     - Post-SHA-256: `4445db58a7474b7ae0fb9256fa7adca8fd7869e0c22006afb8eff8d99469e10c` (16,601 bytes)
     - Added: `InputIdentityMeta` interface, `getQueuedHeadAgeMs`, `getHeadQueuedRequestId`, `terminal.surface.input.accepted` and `dispatch` switchDebug calls.
  2. `ui/src/lib/nativeTerminalInputQueue.test.ts`
     - Pre-SHA-256 (`d82b35e4`): `9d62c3253080b3ad1beb99b18b1cd1fca3394483b6314cb861ef7740cc22be0e`
     - Post-SHA-256: `7488c2d1311e4313af1d9d5b3b7b3e51f47a1cd0a5fff0bf98d72af0c4636554` (21,783 bytes)
     - Added: Unit test asserting distinction between in-flight execution age and waiting queued head age.
- **Updated Composed Files Total:** 21 files (12 modified, 9 untracked).
- **Updated Excluded Files Total:** 14 files.
- **Active Manifest:** `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/manifest-r2.json` (SHA-256: `63f7f3deb55b57b7fe1b6c769e3cae7328ec6eef809d39e9fe41f15e80fc1cc7`).


---

## 7. Revision 3 Source Closure: DaemonSessionDetails Protocol & Constructor Closure

- **Date:** 2026-10-03
- **Defect Remediation:** `defect-report-daemon-protocol-omission.md` (E0609 `reader_paused`, `kernel_stopped`, `registry_suspended`, `suspension_source` missing on `DaemonSessionDetails`, and E0599 missing `DaemonSessionDetails::new`).
- **Inclusion Rationale:** `src-tauri/src/ipc/debug.rs` (lines 839-873) directly references the 4 optional fields and calls `DaemonSessionDetails::new(...)`. Adding these fields to `DaemonSessionDetails` in `protocol.rs` required adding the 4 optional fields initialized to `None` across all 9 struct literal sites in the crate (1 in `protocol.rs` test fixture, 1 in `terminal.rs`, 3 in `session_service.rs`, 1 in `file_link_tests.rs`, 1 in `ipc/tests.rs`, 2 in `remote/tests.rs`).
- **Files Promoted from Excluded to Selected (6 files):**
  1. `src-tauri/src/daemon/protocol.rs` (SHA-256: `8bc25c3db517d3df6c43a77986e26d5a82a08a6e337256515159395f476ea342`, 73433 bytes)
  2. `src-tauri/src/ipc/terminal.rs` (SHA-256: `9284c5539b8c970aa902bc0c0c3b35f9e4b5e8c66871a6bf0a7188acbf7c5ab6`, 130639 bytes)
  3. `src-tauri/src/daemon/session_service.rs` (SHA-256: `c057f6626129bb31e476722c09a57560887406fd6fabb1378f96edef0feba8d4`, 102312 bytes)
  4. `src-tauri/src/ipc/file_link_tests.rs` (SHA-256: `dfc35c15f3fde2db508ca052d4a7291af5fd5fb06b068c0bb9397d0a493f87f8`, 20647 bytes)
  5. `src-tauri/src/ipc/tests.rs` (SHA-256: `0b0a2b1196dabb27feba7a4027d0091dd1c8fa073c32d57396d3ae04c58f830d`, 156334 bytes)
  6. `src-tauri/src/remote/tests.rs` (SHA-256: `37dc4477f5578982d09512156f49e9658a404be97c1d73d24853ef72c8ef7dfa`, 160290 bytes)
- **Reconciled Site Count:** Exactly 9 struct literal sites (8 callsite file sites + 1 protocol test fixture site).
- **Updated Composed Files Total:** 27 files (18 modified, 9 untracked).
- **Updated Excluded Files Total:** 8 files.
- **Active Manifest:** `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/manifest-r3.json` (SHA-256: `987e27388d1cc0ddbdcf64b57610b157217ff68907d59f292f64b1ae5dbc633e`, 23970 bytes).
