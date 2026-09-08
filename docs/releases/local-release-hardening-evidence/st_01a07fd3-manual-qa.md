# Manual QA Matrix — `st_01a07fd3`

**Task ID:** `st_01a07fd3`  
**Parent Session:** `01a07f49-03be-7242-be4f-68a7e66c2166`  
**Date:** 2026-09-08  
**Role:** Manual QA Executor (`omo-senpi-qa-executor`)  
**Scope:** Verification of the MSIX Identity & Output-Preservation Hardening Delta  
**Target Files Audited:**
- `scripts/build-msix.ps1`
- `scripts/build-msix.test.mjs`
- `scripts/test-build-msix.ps1`
- `docs/releases/local-release-hardening-evidence/msix-identity-delta.md`

**Overall Verdict:** **PASS**

All verification criteria for the MSIX identity and output-preservation delta are satisfied with authentic, non-empty, reproducible evidence:
1. **Real Windows RED/GREEN Reproduction on `maho-win`:**
   - **Test 6 (Output Collision Preservation):** Unhardened script fails RED (unconditionally unlinks pre-existing package). Hardened script passes GREEN (immediately fails closed, preserves exact pre-existing file bytes).
   - **Test 11 (Unauthorized Publisher Rejection):** Unhardened script fails RED (blindly overwrote or ignored Publisher, exiting 0). Hardened script passes GREEN (packaged manifest validation detects Publisher mismatch and fails closed with non-zero exit).
   - **Test 12 (Foreign ProcessorArchitecture Rejection):** Unhardened script fails RED (arm64 manifest packaged by MakeAppx /nv exited 0 without error). Hardened script passes GREEN (in-archive manifest validation detects arm64 vs expected x64 and throws non-zero exit).
2. **Zero Skipped or Suppressed Tests:** No skipped (`test.skip`, `it.skip`), suppressed, or ignored tests exist in `scripts/build-msix.test.mjs` or `scripts/test-build-msix.ps1`. The previously empty `catch {}` block in Test 6 has been eliminated and replaced with explicit process capture asserting non-zero exit, error class, and byte preservation.
3. **No JavaScript Mirror of PowerShell Behavior:** The ~110-line JavaScript reimplementation of `ConvertTo-CanonicalVersion` (`canonicalizeVersion`) was completely removed from `scripts/build-msix.test.mjs`. All 8 Node.js/Bun tests assert contract patterns directly on `build-msix.ps1` source, leaving PowerShell execution on Windows as the single source of truth.
4. **Exact Owned Scope & Remote Cleanup:** Changes are strictly confined to the MSIX delta files. Remote scratch directories on `maho-win` were created with isolated unique paths and recursively cleaned in all scenarios, verified by explicit cleanup receipts (`QA_GREEN_CLEANUP_OK`, `QA_RED_CLEANUP_OK`).
5. **Release Suite Non-Regression:** The complete release foundation suite passes 62/62 tests (0 fail, 0 skipped, 0 cancelled).

---

## surfaceEvidence

| Scenario ID | Criterion reference | Surface | Exact invocation | Verdict | Evidence / exact result | artifactRefs |
|---|---|---|---|---|---|---|
| SURF-MSIX-01 | Output collision fail-closed contract | Node.js Test Runner (macOS arm64) | `node --test scripts/build-msix.test.mjs` (test 4) | **PASS** | Script matches `Test-Path $msixOutputFile` fail-closed throw and does not match `Remove-Item $msixOutputFile`. | ART-NODE-01 |
| SURF-MSIX-02 | Packaged manifest 4-way identity contract | Node.js Test Runner (macOS arm64) | `node --test scripts/build-msix.test.mjs` (test 8) | **PASS** | Script inspects packaged `AppxManifest.xml` for `Name`, `Version`, `Publisher`, and `ProcessorArchitecture=x64`. | ART-NODE-01 |
| SURF-MSIX-03 | Bun 1.4.0 runtime compatibility | Bun Test Runner (macOS arm64) | `bun test scripts/build-msix.test.mjs` | **PASS** | All 8 contract tests pass in 953ms with 0 failures under Bun 1.4.0. | ART-BUN-01 |
| SURF-MSIX-04 | Full Windows test suite execution & cleanup | Windows Remote PowerShell (`maho-win` NT 10.0.26200.0) | `ssh -o BatchMode=yes -o ConnectTimeout=60 maho-win "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:/Users/sook/AppData/Local/Temp/ferryx-msix-qa-green-st01a07fd3/test-build-msix.ps1"` | **PASS** | All 12 behavior tests pass (including real MakeAppx packaging). Scratch directory removed; emitted `QA_GREEN_CLEANUP_OK`. Remote exit code 0. | ART-WIN-GREEN |
| SURF-MSIX-05 | Release suite non-regression | Node.js Test Runner (macOS arm64) | `node --test scripts/sync-version.test.mjs scripts/release-workflow.test.mjs scripts/minisign-verify.test.mjs scripts/build-msix.test.mjs` | **PASS** | All 62 foundation release tests pass with zero regressions, zero failures, and zero skipped tests. | ART-FOUNDATION |

---

## adversarialCases

| Scenario ID | Criterion reference | Adversarial class | Expected behavior | Verdict | Evidence | artifactRefs |
|---|---|---|---|---|---|---|
| ADV-MSIX-01 | Pre-existing package preservation | Output collision / overwrite attempt | Pre-existing `.msix` must not be deleted, truncated, or overwritten; packaging must fail closed and preserve exact bytes. | **PASS** | Test 6 in `test-build-msix.ps1`: Script catches existing file, throws `ERROR: Output package already exists at ... Refusing to overwrite existing package.`; byte length (63 bytes) and byte-for-byte content verified identical. | ART-WIN-GREEN |
| ADV-MSIX-02 | Output collision baseline failure | Pre-existing package deletion regression (RED check) | Unhardened script containing `Remove-Item` must fail the preservation assertion. | **PASS** | Test 6 against unhardened `build-msix.ps1`: Script fails with `RESULT: FAIL - Expected failure class matching 'Output package already exists', but caught: 'ERROR: MakeAppx.exe pack failed with exit code 1'`. | ART-WIN-RED |
| ADV-MSIX-03 | Publisher identity enforcement | Unauthorized Publisher injection | Manifest template declaring unauthorized Publisher (`CN=WRONG-UNAUTHORIZED-PUBLISHER`) must be rejected after packaging. | **PASS** | Test 11 against hardened `build-msix.ps1`: MakeAppx creates package, in-archive validator detects mismatch, script exits non-zero with `Packaged manifest Identity Publisher.*does not match expected`. | ART-WIN-GREEN |
| ADV-MSIX-04 | Publisher baseline gap detection | Incomplete Publisher validation regression (RED check) | Unhardened script without Publisher validation must fail the assertion. | **PASS** | Test 11 against unhardened `build-msix.ps1`: Exits 0 silently; harness catches unexpected success: `RESULT: FAIL - Expected error matching 'Packaged manifest Identity Publisher.*does not match expected', but caught: 'Expected non-zero exit code when packaged manifest has wrong Publisher'`. | ART-WIN-RED |
| ADV-MSIX-05 | ProcessorArchitecture enforcement | Foreign architecture template injection (`arm64`) | Manifest template declaring `ProcessorArchitecture="arm64"` must be rejected after packaging. | **PASS** | Test 12 against hardened `build-msix.ps1`: MakeAppx creates package, in-archive validator detects mismatch, script exits non-zero with `Packaged manifest Identity ProcessorArchitecture.*does not match expected 'x64'`. | ART-WIN-GREEN |
| ADV-MSIX-06 | Architecture baseline gap detection | Incomplete Architecture validation regression (RED check) | Unhardened script without ProcessorArchitecture validation must fail the assertion. | **PASS** | Test 12 against unhardened `build-msix.ps1`: Exits 0 silently; harness catches unexpected success: `RESULT: FAIL - Expected error matching 'Packaged manifest Identity ProcessorArchitecture.*does not match expected 'x64'', but caught: 'Expected non-zero exit code when packaged manifest has arm64 ProcessorArchitecture'`. | ART-WIN-RED |

---

## artifactRefs

| ID | Kind | Description | Path |
|---|---|---|---|
| ART-NODE-01 | Node.js Test Log | TAP test execution log for `scripts/build-msix.test.mjs` (8/8 pass, 0 fail) | `docs/releases/local-release-hardening-evidence/artifacts/msix-delta-qa-node-contract.log` |
| ART-BUN-01 | Bun Test Log | Bun 1.4.0 test execution log for `scripts/build-msix.test.mjs` (8/8 pass, 0 fail) | `docs/releases/local-release-hardening-evidence/artifacts/msix-delta-qa-bun-contract.log` |
| ART-WIN-GREEN | Windows PowerShell Log | Live remote execution on `maho-win` running all 12 tests on hardened `build-msix.ps1`, showing 12/12 pass, exit code 0, and cleanup receipt `QA_GREEN_CLEANUP_OK` | `docs/releases/local-release-hardening-evidence/artifacts/msix-delta-qa-windows-green.log` |
| ART-WIN-RED | Windows PowerShell Log | Live remote execution on `maho-win` running all 12 tests on unhardened `build-msix.ps1` (commit `3a19bdf`), showing 3 failures on Tests 6, 11, 12, exit code 1, and cleanup receipt `QA_RED_CLEANUP_OK` | `docs/releases/local-release-hardening-evidence/artifacts/msix-delta-qa-windows-red.log` |
| ART-FOUNDATION | Node.js Test Log | TAP test execution log for all 62 foundation release tests (`sync-version`, `release-workflow`, `minisign-verify`, `build-msix`) passing with 0 failures and 0 skips | `docs/releases/local-release-hardening-evidence/artifacts/msix-delta-qa-release-foundation.log` |
