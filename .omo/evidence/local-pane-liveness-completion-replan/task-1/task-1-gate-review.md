# Task 1 Source and Provenance Gate Review Report

- **Task:** Task 1 — Preserve prior work and compose one source-bound starting candidate
- **Plan:** `/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-completion-replan.md`
- **Candidate Worktree (C):** `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`
- **Salvage Worktree (W):** `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1` (Strictly Read-Only)
- **Base Commit:** `d82b35e43f208b53adf6310f4e3c89cbde8814f4`
- **Evidence Directory (E):** `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation/.omo/evidence/local-pane-liveness-completion-replan/task-1`
- **Reviewer:** OmO senpi-task child (`omo-native-gate-reviewer`)
- **Date:** 2026-10-03
- **Review Scope:** Independent source/provenance review, untracked closure, scope exclusion, test non-vacuity, adversarial checks, receipt hash reconciliation (zero product edits, zero builds/tests executed).

---

## 1. Executive Summary & Recommendation

- **Recommendation:** **REJECT** (Overall Confirmed withheld pending resolution of evidence placeholders, disposable mismatch re-run, and completion of remote verification gates).
- **Source Composition Status:** **PASS** (19/19 files verified bit-for-bit against authentic source provenance and git base; untracked closure complete; 16 overlay files cleanly excluded).
- **Gate Confirmation Status:** **BLOCKED** (Remote compilation and test gates are UNRUN; evidence artifact contains residual placeholder hashes; QA mismatch check targeted product file rather than disposable fixture).

---

## 2. Blockers Ledger

| Blocker ID | Violated Criterion | Observation | Evidence Pointer |
|---|---|---|---|
| **BLK-01** | `CRIT-EVIDENCE-INTEGRITY` | `source-inventory.md` Section 4 contains 5 fabricated placeholder hashes (`b70742f5...`, `a64860b7...`, `26b38c2a...`, `2c50a00e...`, `8862804b...`) carried over from a drafting scratchpad. While `manifest.json` and `receipt-clarification-addendum.md` record authentic hashes, `source-inventory.md` must be updated on disk to eliminate fabricated facts. | `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/source-inventory.md:74-85` |
| **BLK-02** | `CRIT-QA-MISMATCH-FIXTURE` | Plan Task 1 QA failure requirement mandates `git apply --check` against a deliberately mismatched *disposable fixture*. The check performed used a patch targeting candidate product file `src-tauri/Cargo.toml`. The sole remote verifier must reproduce using a disposable fixture file. | `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/receipt-clarification-addendum.md:46` |
| **BLK-03** | `CRIT-REMOTE-GATES-PENDING` | Plan Task 1 Acceptance requires remote UI build, scoped Vitest, scoped Rust diagnostics, `qa_barrier` 10/10, runner Vitest 19/19, and headless smoke to pass on `maho-win`. All 7 gates remain UNRUN. Per gate protocol, overall confirmed must not be granted until immutable remote receipts are provided. | `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/ownership.json:10-40` |

---

## 3. Original Intent & Desired Outcome

- **Original Intent:** The user and project plan (`local-pane-liveness-completion-replan.md` Task 1) require composing a pristine, source-bound candidate starting point in worktree `C` from committed base `d82b35e4`, salvaging verified diagnostics and minimal test repair hunks from `W`, excluding large unverified dirty overlays, establishing strict remote builder preflight, and proving complete source provenance and test non-vacuity before implementing subsequent tasks.
- **Desired Outcome:** A candidate repository state containing exactly the verified 19 files with 100% hash agreement, complete untracked closure, zero unrecorded files, clean exclusion of the 16 dirty overlay files, authentic reference receipts, verified non-vacuous tests, and an unambiguous handoff to the remote verifier without premature success claims.
- **User Outcome Review:** From the user's perspective, source composition fidelity has been achieved at the byte level in `C`. However, the artifact record suffered from drafting communication defects (placeholder hashes in chat and preliminary markdown), and the required remote verification gates have not yet executed. The user's requirement for verified proof rather than assumed success is upheld by withholding confirmation until the remote verifier finishes and the evidence markdown is sanitized.

---

## 4. Checked Artifact Paths & Independent Hash Verification

### 4.1 Candidate Manifest Digest
- **Path:** `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/manifest.json`
- **Measured Size:** 19,321 bytes (matches claim)
- **Measured SHA-256:** `99e72be0031133007d7914167b50f0acb7c8f59de5695f2293859ec01d135a2e` (matches claim)

### 4.2 Reference Receipts in Salvage Worktree W
All reference receipts in `W/.omo/evidence/local-pane-liveness-root-remediation/` were independently hashed on physical disk and compared against `manifest.json` and `source-inventory.md`:

| Receipt File | Bytes | Measured Disk SHA-256 | `manifest.json` Value | `source-inventory.md` Value | Status |
|---|---:|---|---|---|---|
| `minimal-two-test-repair/final-index.md` | 2,097 | `2f732430aaea97830bdc468e020e7ca8f42de7ecb4fc5363087660626ae39ea2` | `2f732430...` (Match) | `b70742f5...` (MISMATCH: Placeholder) | **NEEDS-FIX** |
| `minimal-two-test-repair/result-v4.md` | 2,496 | `6d6150fabb0c3ab537070852010b57294f93c0cc161f18f88eb8ee1ef94c224f` | `6d6150fa...` (Match) | `a64860b7...` (MISMATCH: Placeholder) | **NEEDS-FIX** |
| `minimal-two-test-repair/runner-result.md` | 2,899 | `296df22f493ff0fd2ffb3e9968c0788ad8ee1ea82c10d7d1d8fa65ec3ed3357f` | `296df22f...` (Match) | `26b38c2a...` (MISMATCH: Placeholder) | **NEEDS-FIX** |
| `task-2-diagnostics.json` | 5,380 | `feda27be823a6351ba8a63b69ffb211452913c0f61c23d425cbe21d05c83e3fb` | `feda27be...` (Match) | `2c50a00e...` (MISMATCH: Placeholder) | **NEEDS-FIX** |
| `task-3-frozen-source-hash-audit.txt` | 4,126 | `be11c2012cac425421ee1d368516c5420151d51dd67050fc816454d0f746b3da` | `be11c201...` (Match) | `8862804b...` (MISMATCH: Placeholder) | **NEEDS-FIX** |
| `input-manifest.json` | 18,909 | `85a3448e5916079a697329a01608d92c588bab0d12bf1c98f366b6c86087c110` | `85a3448e...` (Match) | N/A | **PASS** |
| `source-inventory.md` (in W) | 24,607 | `46eb625cdd0acb898ab617f41094af2b7fcc3787332000bba5adb98a87e20da0` | `46eb625c...` (Match) | N/A | **PASS** |

### 4.3 Composed Candidate Files (19 Files in C)
Independently verified against git base `d82b35e4`, disk content in `C`, and declared provenance:

| File Path | Status | Pre-Commit SHA-256 (`d82b35e4`) | Post-SHA-256 in C | Bytes | Provenance Agreement |
|---|---|---|---|---:|---|
| `src-tauri/Cargo.toml` | modified | `712b65396cd2aab5a39bc14df6517fb78c82ef886e8d65297ba02bf159891fcd` | `7650c676ecf125e1f3e4b81809736b8e583cf024a0a079370056d90ec617d100` | 6,925 | 100% byte match to `W/src-tauri/Cargo.toml` |
| `src-tauri/src/ipc/mod.rs` | modified | `7963b25a7c54eb6812ea928d0f4dd369f59db5d48e1c88f64ee0322390107177` | `7af2941ab86c5580f41c2bb22ab6caab0ae5d22789f048b6f88eb4f813b1b78a` | 1,939 | 100% byte match to `W/src-tauri/src/ipc/mod.rs` |
| `src-tauri/src/main.rs` | modified | `69e3911982c104c70bb14507098f8d4d27e4422aded38a0404ab8ddff673d8c6` | `45edd802330ae4bdca941a89629f109472c7f79a911aaefbe8ae6792b215ffb4` | 4,812 | 100% byte match to `W/src-tauri/src/main.rs` |
| `src-tauri/src/ipc/native_terminal.rs` | modified | `396c4bd334d8be5657bda690e64e1a719e70ba188f8f148ae9339141f10d9cfe` | `69b80dd05ac3b0cb687bfa54182de2f63f3df364995759452a6f47cabe1f93e2` | 146,034 | 100% byte match to `W/.../minimal-two-test-repair/native_terminal.rs` |
| `src-tauri/src/native_terminal/surface_host.rs` | modified | `4ab78ac821093b1ed42dc4211f8e32d67010cfe835d6370e6d137c9f69228f14` | `7b844f58bbb281e633b9eb2f1e30dea6c1a7fafae4de25c25a77115926f4edaf` | 364,917 | 100% byte match to `W/.../minimal-two-test-repair/surface_host.rs` |
| `src-tauri/src/ipc/debug.rs` | modified | `6a38d07779a66b8fdf02f1c0bfbdfd1f2e5061a61d7d5c18ad0a1d1b58c0e6f3` | `3ce0efe38cc8bbcd27afbee6cf9a37a99f9ca7b21edc0068d349abaac9a7967e` | 32,579 | 100% byte match to `W/src-tauri/src/ipc/debug.rs` |
| `ui/src/lib/paneDebugInfo.ts` | modified | `031602377b6b0b32f97d8b0e98b4950dadbc0b28f55def717255409544a8c47c` | `7caeb59b605589f2d5464894f2308abaf562d2a9e561b7b7f187b9b6664e804c` | 2,304 | 100% byte match to `W/ui/src/lib/paneDebugInfo.ts` |
| `ui/src/lib/paneDebugInfo.test.ts` | modified | `df110b9813b976878ec8f09e981154b7888ac8e9d530566e3254deb8088cda57` | `3772d3fbd3fb3a32cee093f3d7a7d95f032f9d1a62f60a7987182f133f1b9dcd` | 2,163 | 100% byte match to `W/ui/src/lib/paneDebugInfo.test.ts` |
| `ui/src/lib/switchDebug.ts` | modified | `e6b8089f4fdf70ad791f3047f76990cbae97c6880b5c5ae8bee427e12ffb1507` | `e9e1f742cd0a6552df6245e5a73594141093ec3610d68fb826ebb7eda27e36e1` | 3,747 | 100% byte match to `W/ui/src/lib/switchDebug.ts` |
| `ui/src/lib/switchDebug.test.ts` | modified | `4300a3b9548cfc2c5a69aff0241b5e6615e8c25b6afbfb075d35498c4777a241` | `6cd6cc7ab3348866984aa4417ffb6d8b84545d0180e49a6c30dcfad8f4ec9a53` | 3,719 | 100% byte match to `W/ui/src/lib/switchDebug.test.ts` |
| `src-tauri/src/ipc/qa_barrier.rs` | untracked | N/A (new) | `a69d9b10c6a7d0428d1176f1e2841def64a76ab2ef62fc9600c3c1d4c753fe61` | 35,740 | 100% byte match to `W/.../qa_barrier.repaired_minimal.rs` |
| `ui/src/lib/paneLiveness.ts` | untracked | N/A (new) | `070e54b8f4a35f86326103e9b0a5292aa7b8440b0c90d1842e2d0643d01bc9ee` | 8,728 | 100% byte match to `W/ui/src/lib/paneLiveness.ts` |
| `ui/src/lib/paneLiveness.test.ts` | untracked | N/A (new) | `858baf654f866cc889f5c7b0df05613e040bcce1b9340bcc49a48aba6c09d8e2` | 7,127 | 100% byte match to `W/ui/src/lib/paneLiveness.test.ts` |
| `scripts/lib/qa-scenarios/common-harness.mjs` | untracked | N/A (new) | `be5cf74ddce35fa75100ad99205a76c36e39144f42f58c7c3471e087fa299696` | 27,296 | 100% byte match to `W/.../common-harness.mjs` |
| `scripts/lib/qa-scenarios/diagnostic-classifier.mjs` | untracked | N/A (new) | `0bdb7cfa3204e39baa52e2e10b772e26db5804b141c39846d3be14c830995df9` | 7,392 | 100% byte match to `W/.../diagnostic-classifier.mjs` |
| `scripts/lib/qa-scenarios/native-driver.mjs` | untracked | N/A (new) | `60f4e23514b1c653f3a1385d3b731c8624ee2756f9e3d759094f4ecaf587f161` | 14,339 | 100% byte match to `W/scripts/.../native-driver.mjs` |
| `scripts/qa/pane-liveness.test.mjs` | untracked | N/A (new) | `0d369d8518fe9bd7ad127904c83f15c5dc51bbed018dc43d28a9882c0f5a1b32` | 28,118 | 100% byte match to `W/.../pane-liveness.test.mjs` |
| `scripts/qa/pane-liveness-vitest.config.mjs` | untracked | N/A (new) | `a8709872b371397000497aaea5b7bfea5ee19414d240c61ae4f850ec44a779a9` | 391 | 100% byte match to `W/scripts/.../pane-liveness-vitest.config.mjs` |
| `scripts/qa/pane-liveness.mjs` | untracked | N/A (new) | `2d4c0080e656610c5ee0ffbd04db75084ced68d726d0af7f99ec9037096e486c` | 31,657 | 100% byte match to `W/scripts/.../pane-liveness.mjs` |

### 4.4 Untracked Closure Audit
- Execution of `git status --porcelain=v1 -uall` reveals exactly 10 modified files and exactly 9 untracked files.
- There are ZERO extra, leftover, temporary, or unrecorded files in worktree `C`. Untracked closure is complete (100%).

### 4.5 Scope Exclusion Audit
The 16 uncompiled/unverified overlay files from `W` were independently checked via `git status` in `C`:
- `src-tauri/src/daemon/client.rs` — unmodified from `d82b35e4`
- `src-tauri/src/daemon/handover.rs` — unmodified from `d82b35e4`
- `src-tauri/src/daemon/protocol.rs` — unmodified from `d82b35e4`
- `src-tauri/src/daemon/server.rs` — unmodified from `d82b35e4`
- `src-tauri/src/daemon/session_service.rs` — unmodified from `d82b35e4`
- `src-tauri/src/ipc/terminal.rs` — unmodified from `d82b35e4`
- `src-tauri/src/ipc/file_link_tests.rs` — unmodified from `d82b35e4`
- `src-tauri/src/ipc/native_terminal_disabled.rs` — unmodified from `d82b35e4`
- `src-tauri/src/ipc/tests.rs` — unmodified from `d82b35e4`
- `src-tauri/src/lib.rs` — unmodified from `d82b35e4`
- `src-tauri/src/remote/tests.rs` — unmodified from `d82b35e4`
- `ui/src/components/NativeTerminalPane.tsx` — unmodified from `d82b35e4`
- `ui/src/components/TerminalSplitView.tsx` — unmodified from `d82b35e4`
- `ui/src/lib/nativeTerminalInputQueue.test.ts` — unmodified from `d82b35e4`
- `ui/src/lib/nativeTerminalInputQueue.ts` — unmodified from `d82b35e4`
- `ui/src/lib/nativeTerminalLifecycle.ts` — unmodified from `d82b35e4`
All 16 excluded files are cleanly absent from candidate diffs.

---

## 5. Test Selectors Non-Vacuity Inspection

1. **`qaBarrierGate` (`--features local-split-qa qa_barrier`)**:
   - Inspects 10 concrete tests:
     - 5 in `src-tauri/src/ipc/qa_barrier.rs` (prearmed scan ack, correlation validation, deadline timeout, fixture setup correlation, channel absence).
     - 3 in `src-tauri/src/ipc/native_terminal.rs` (`qa_barrier_writer_tests`: hold/release, request ID fencing, no-channel pass-through).
     - 2 in `src-tauri/src/native_terminal/surface_host.rs` (`qa_barrier_presentation_tests`: coordinator frame hold/release, deadline cancel).
   - Non-vacuous: Real barrier holds and subscriber events are asserted; start_paused clocks verify timeout bounds; no trivial assertions.
2. **`scopedRustDiagnostics` (`pane_liveness_diagnostics`)**:
   - Inspects 9 concrete tests across `debug.rs` (7), `native_terminal.rs` (1), and `surface_host.rs` (1).
   - Asserts allowlist sanitization, sensitive payload redaction, serialization codecs, and writer/reader held precedence.
3. **`exactPresentationBarrier` (`real_coordinator_frame_holds_and_recovers_through_private_barrier`)**:
   - Directly exercises `NativeTerminalSurfaceHostState::hold_presentation_barrier_qa`, awaits receipt events, and verifies `is_render_pending` transitions from true to false upon release.
4. **`canonicalRunnerVitest` (`pane-liveness.test.mjs`)**:
   - 19 automated tests covering argument parsing, scenario name validation, process supervision, barrier timeouts, PID group reaping, and negative regression assertions (H10, P2, M3, H4, H3, H6, channel v2, B1-B5).
5. **Scoped Frontend Vitest (`paneDebugInfo.test.ts`, `switchDebug.test.ts`, `paneLiveness.test.ts`)**:
   - 25 tests verifying identity triad serialization, stage threshold classifications (250ms), session identity fencing, and reader pause priority.
   - Zero tautological `expect(true).toBe(true)` or implementation-mirroring no-ops.

---

## 6. Adversarial Classes & Skills Audit

### 6.1 Adversarial Class Analysis
- **`stale_state`**:
  - The preliminary chat summary and preliminary `source-inventory.md` draft contained stale placeholder hashes (`b70742f5...`). While the author acknowledged this in `receipt-clarification-addendum.md`, the physical file `source-inventory.md` was not remediated on disk, leaving a stale artifact in `C/E`.
  - Resolution: `source-inventory.md` must be updated to reflect authentic disk hashes.
- **`dirty_worktree`**:
  - Confirmed completely clean: `git status -uall` shows only the 19 intended files. No accidental residue, compiler artifacts, or uncommitted foreign files.
  - Worktree `W` and repository `main` remained 100% read-only.
- **`misleading_success_output`**:
  - Premature pass claims: Chat initially presented scratchpad hashes as verified facts.
  - Mismatch test shortcut: `git apply --check` was executed against candidate `src-tauri/Cargo.toml` rather than a disposable fixture. This must be re-run against an isolated disposable fixture file to prevent misleading verification claims.

### 6.2 Skill Perspective Checks (`remove-ai-slops` & `programming`)
- **Overfit / Slop Pass**:
  - No deletion-only or trivial tests added.
  - No speculative abstractions or unnecessary runtime helpers introduced.
  - Production telemetry code in `debug.rs` enforces strict privacy redaction (no payload text transmitted).
  - QA harness code in `qa_barrier.rs` is strictly gated behind `#[cfg(feature = "local-split-qa")]`, having zero impact on production release binaries.

---

## 7. Prerequisites for Final Gate Approval (AdversarialVerify)

Before Task 1 gate can be marked **APPROVED**:
1. `source-inventory.md` on disk must be updated to replace the 5 placeholder hashes in Section 4 with authentic SHA-256 hashes (`2f732430...`, `6d6150fa...`, `296df22f...`, `feda27be...`, `be11c201...`).
2. The QA failure mismatch check must be re-executed against an isolated disposable fixture file, demonstrating exit code 1 and unchanged candidate hashes.
3. The sole remote verifier on `maho-win` must execute and produce immutable receipts for all 7 unrun verification gates:
   - `bun run --cwd ui build` (exit 0)
   - `bun run --cwd ui test src/lib/switchDebug.test.ts src/lib/paneDebugInfo.test.ts src/lib/paneLiveness.test.ts` (exit 0)
   - `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_diagnostics` (exit 0, 9 tests)
   - `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier` (exit 0, 10 tests)
   - `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa real_coordinator_frame_holds_and_recovers_through_private_barrier` (exit 0, 1 test)
   - `bun ui/node_modules/vitest/vitest.mjs run --config scripts/qa/pane-liveness-vitest.config.mjs scripts/qa/pane-liveness.test.mjs` (exit 0, 19 tests)
   - `node scripts/qa/pane-liveness.mjs --scenario diagnostic-classifier --headless` (exit 0, DEFERRED-NATIVE receipt)

---

## 8. Artifact Verdict

- **Report Path:** `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation/.omo/evidence/local-pane-liveness-completion-replan/task-1/task-1-gate-review.md`
- **Result:** **REJECT** (Composition bytes verified; confirmation blocked pending evidence remediation and remote gate receipts).
