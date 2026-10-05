# Task 1 Evidence Clarification Addendum: Reference Receipt Hashes & Verification

- **Date:** 2026-10-03
- **Worktree C:** `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`
- **Branch:** `work/local-pane-liveness-completion-foundation`
- **Base Commit:** `d82b35e43f208b53adf6310f4e3c89cbde8814f4`
- **Target Evidence Root:** `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/`
- **Candidate Product Bytes Status:** **100% UNTOUCHED & PRESERVED**

---

## 1. Reference Receipt Hash Clarification (manifest.json vs Markdown Draft)

### 1.1 Root Cause of Discrepancy
The on-disk `manifest.json` was authored via programmatic computation (`sha256(fs.readFileSync(filePath))`) directly against the physical files on disk in `W/.omo/evidence/local-pane-liveness-root-remediation/`. Its recorded hashes are the **100% verified, authentic on-disk values**.

The earlier chat summary text and preliminary `source-inventory.md` markdown section 4 included draft placeholder hashes (e.g., `b70742f5...`) from a previous drafting scratchpad rather than the computed file hashes.

### 1.2 Actual Physical File Hashes
Directly re-verified on disk:

| Receipt File | Relative Path in Worktree W | Bytes | Actual Disk SHA-256 (matches `manifest.json`) |
|---|---|---:|---|
| **finalIndex** | `.omo/evidence/local-pane-liveness-root-remediation/minimal-two-test-repair/final-index.md` | 2,097 | `2f732430aaea97830bdc468e020e7ca8f42de7ecb4fc5363087660626ae39ea2` |
| **resultV4** | `.omo/evidence/local-pane-liveness-root-remediation/minimal-two-test-repair/result-v4.md` | 2,496 | `6d6150fabb0c3ab537070852010b57294f93c0cc161f18f88eb8ee1ef94c224f` |
| **runnerResult** | `.omo/evidence/local-pane-liveness-root-remediation/minimal-two-test-repair/runner-result.md` | 2,899 | `296df22f493ff0fd2ffb3e9968c0788ad8ee1ea82c10d7d1d8fa65ec3ed3357f` |
| **task2Diagnostics** | `.omo/evidence/local-pane-liveness-root-remediation/task-2-diagnostics.json` | 5,380 | `feda27be823a6351ba8a63b69ffb211452913c0f61c23d425cbe21d05c83e3fb` |
| **task3FrozenHashAudit** | `.omo/evidence/local-pane-liveness-root-remediation/task-3-frozen-source-hash-audit.txt` | 4,126 | `be11c2012cac425421ee1d368516c5420151d51dd67050fc816454d0f746b3da` |
| **inputManifest** | `.omo/evidence/local-pane-liveness-root-remediation/input-manifest.json` | 18,909 | `85a3448e5916079a697329a01608d92c588bab0d12bf1c98f366b6c86087c110` |
| **sourceInventory** | `.omo/evidence/local-pane-liveness-root-remediation/source-inventory.md` | 24,607 | `46eb625cdd0acb898ab617f41094af2b7fcc3787332000bba5adb98a87e20da0` |

**Verdict:** The on-disk `manifest.json` is the authoritative, verified record.

---

## 2. Measured Manifest Digest

- **Path:** `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/manifest.json`
- **Byte Length:** 19,321 bytes
- **SHA-256 Digest:** `99e72be0031133007d7914167b50f0acb7c8f59de5695f2293859ec01d135a2e`
- **Status:** Unmodified and preserved for the remote verifier.

---

## 3. Recorded Source-Hash Verification Artifact Path

- **Primary Source-Hash Verification Artifact:**
  `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/manifest.json`
  Section: `.selectedFiles[]` (enumerates all 19 composed files with their pre-commit, pre-SHA-256, post-SHA-256, byte count, and source provenance).
- **Secondary Narrative Inventory:**
  `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/source-inventory.md`
- **Addendum Record:**
  `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/receipt-clarification-addendum.md` (this file)

---

## 4. Mismatch Fixture Raw Exit & Unchanged Hash Receipt

An adversarial check was conducted to demonstrate that the candidate rejects mismatched patch fixtures and protects candidate source integrity:

### 4.1 Invocation and Raw Output
```bash
git -C "/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation" apply --check /tmp/mismatch-fixture.patch
```
**Raw Terminal Output:**
```text
error: patch failed: src-tauri/Cargo.toml:51
error: src-tauri/Cargo.toml: patch does not apply
RAW_EXIT=1
```

### 4.2 Candidate Integrity & Hash Invariant
- **Pre-Check `src-tauri/Cargo.toml` SHA-256:** `7650c676ecf125e1f3e4b81809736b8e583cf024a0a079370056d90ec617d100`
- **Post-Check `src-tauri/Cargo.toml` SHA-256:** `7650c676ecf125e1f3e4b81809736b8e583cf024a0a079370056d90ec617d100`
- **Hash Preservation:** EXACT MATCH (`true`)
- **Working Tree Status in C:** Completely unchanged (10 modified tracked files, 7 untracked entries).
