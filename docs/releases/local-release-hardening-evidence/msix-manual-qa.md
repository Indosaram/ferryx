# Manual QA Matrix — `st_01a07fa0`

**Goal:** Harden Ferryx Windows MSIX packaging (`scripts/build-msix.ps1`), require explicit `-ExePath`, enforce canonical app/MSIX version inputs (Store quad 4th component = 0), verify binary version identity against expected release versions, enforce isolated fresh staging and stale output cleanup, check native MakeAppx and SignTool exit codes, remove hardcoded credentials and auto-generated certificates, require explicit Store mode (`-SkipSigning`) or validated certificate thumbprints (`-CertThumbprint`), validate packaged manifest contents from the `.msix` archive, and prove deterministic behavior via node contract tests (`scripts/build-msix.test.mjs`) and native PowerShell tests on Windows (`scripts/test-build-msix.ps1`).

**Overall verdict: PASS**

All criteria are fully satisfied with verifiable evidence:
1. `scripts/build-msix.ps1` requires mandatory `-ExePath` and deletes candidate array path guessing.
2. Store Quad contract strictly enforces the 4th quad component = 0 for Store ingestion; date tags validate calendar bounds and leap years.
3. Binary version is inspected via `FileVersionInfo` and verified against expected app version before packaging.
4. Fresh isolated staging directory is created per run and pre-existing output `.msix` files are removed prior to `MakeAppx` execution.
5. `$LASTEXITCODE` is checked immediately after `MakeAppx.exe` and `SignTool.exe` native invocations.
6. Self-signed certificate auto-generation, hardcoded password (`FerryxMsixSignPass2026!`), and permissive catch fallbacks have been removed.
7. Explicit `-SkipSigning` Store mode supported; sideload signing requires valid `-CertThumbprint` and fails immediately if unavailable in the certificate store.
8. Packaged `.msix` archive is inspected via `[System.IO.Compression.ZipFile]` to validate `AppxManifest.xml` `Identity Name` and `Version`.
9. `node --test scripts/build-msix.test.mjs` executes 11 contract tests with 100% pass (also validated on Bun 1.4.0).
10. `scripts/test-build-msix.ps1` executed remotely on `maho-win` via SSH with 10 behavior tests with 100% pass, followed by clean removal of remote scratch resources.
11. Existing release suite (`55/55` tests) passes without regression.

## surfaceEvidence

| Scenario ID | Criterion reference | Surface | Exact invocation | Verdict | Evidence / exact result | artifactRefs |
|---|---|---|---|---|---|---|
| QA-1 | Explicit `-ExePath` binding & candidate removal | Node.js Test Runner | `node --test scripts/build-msix.test.mjs` (test 1) | **PASS** | Script declares mandatory `-ExePath`; candidate array `$binaryCandidates` removed. | A1, A2 |
| QA-2 | Canonical Store Quad version contract | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (tests 3, 4) | **PASS** | Quad version with 4th component != 0 (`2026.908.1.5`) rejected; invalid calendar date (`v2026.02.29`) rejected. | A4, A5 |
| QA-3 | Binary version verification | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (test 5) | **PASS** | Mismatched binary version (`1.0.0.0` vs expected `2026.908.1`) throws descriptive error. | A4, A5 |
| QA-4 | Stale output file removal & fresh staging | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (test 6) | **PASS** | Pre-existing output `.msix` removed before packaging; unique GUID staging dir allocated. | A4, A5 |
| QA-5 | Native tool exit code verification | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (test 9) | **PASS** | MakeAppx schema error causes non-zero exit code which is caught and surfaced. | A4, A5 |
| QA-6 | Store packaging mode (`-SkipSigning`) | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (test 10) | **PASS** | Package generated unsigned with explicit `-SkipSigning` flag for Store submission. | A4, A5 |
| QA-7 | Credential and auto-cert removal | Node.js Test Runner | `node --test scripts/build-msix.test.mjs` (test 6) | **PASS** | `New-SelfSignedCertificate`, `FerryxMsixSignPass2026!`, and silent fallback deleted. | A1, A2 |
| QA-8 | Explicit `-CertThumbprint` validation | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (tests 7, 8) | **PASS** | Fails closed when signing is requested without thumbprint or with invalid thumbprint. | A4, A5 |
| QA-9 | Real MakeAppx pack & manifest validation | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (test 10) | **PASS** | Real MakeAppx produced valid `.msix`; `ZipFile` read validated `Name="ProjectMaho.Ferryx"` and `Version="2026.908.1.0"`. | A5 |
| QA-10 | Bun runtime compatibility | Bun CLI | `bun test scripts/build-msix.test.mjs` | **PASS** | 11 of 11 tests pass under Bun 1.4.0. | A3 |
| QA-11 | Release suite preservation | Node.js Test Runner | `node --test scripts/*.test.mjs` | **PASS** | 55 of 55 tests pass across all release test files with zero regressions. | A6 |

## adversarialCases

| Scenario ID | Criterion reference | Adversarial class | Expected behavior | Verdict | Evidence | artifactRefs |
|---|---|---|---|---|---|---|
| ADV-1 | Mandatory `-ExePath` | Argument omission | Invoking script without `-ExePath` must immediately fail closed. | **PASS** | PowerShell throws `Cannot process command because of one or more missing mandatory parameters: ExePath`. | A4, A5 |
| ADV-2 | Valid path requirement | Non-existent binary | Invoking script with non-existent path must throw file not found error. | **PASS** | Throws `ERROR: Executable not found at path: ...`. | A4, A5 |
| ADV-3 | Store Quad constraint | Version syntax tampering | Quad version with non-zero 4th component (`2026.908.1.5`) must be rejected. | **PASS** | Throws `MSIX Store packages require the 4th quad component to be 0 for Store ingestion`. | A4, A5 |
| ADV-4 | Calendar date bounds | Calendar invalidity | Release tags with non-leap Feb 29 or month > 12 must fail closed. | **PASS** | Throws `Invalid release day: 29 for month 2 in year 2026`. | A4, A5 |
| ADV-5 | Version consistency | Binary skew / wrong build | Binary PE version differing from release version must be rejected. | **PASS** | Throws `Binary version verification failed: binary version does not match expected app version`. | A4, A5 |
| ADV-6 | Output isolation | Stale artifact collision | Pre-existing output `.msix` must be deleted before pack to avoid false success. | **PASS** | Verified stale dummy `.msix` is deleted before packaging proceeds. | A4, A5 |
| ADV-7 | Signing security | Sideload missing thumbprint | Sideload mode without `-CertThumbprint` must throw security error. | **PASS** | Throws `ERROR: Signing requested but no -CertThumbprint provided`. | A4, A5 |
| ADV-8 | Certificate existence | Bogus thumbprint | Non-existent certificate thumbprint must fail closed. | **PASS** | Throws `ERROR: Signing certificate with thumbprint '...' not found in Cert:`. | A4, A5 |
| ADV-9 | Tool execution | Native tool exit failure | Native MakeAppx non-zero exit must throw exception. | **PASS** | Throws `ERROR: MakeAppx.exe pack failed with exit code 1`. | A4, A5 |
| ADV-10 | Package integrity | Packaged manifest tampering | MSIX package missing `AppxManifest.xml` or with mismatched identity must fail. | **PASS** | `ZipFile` opens `.msix` archive and asserts identity Name and Version equality. | A5 |

## artifactRefs

| ID | Kind | Description | Path |
|---|---|---|---|
| A1 | Node Test Runner Log | RED baseline test run showing 8 failures on unhardened `build-msix.ps1` | `docs/releases/local-release-hardening-evidence/artifacts/qa-1-node-contract-red.log` |
| A2 | Node Test Runner Log | GREEN test run showing 11 passes on hardened `build-msix.ps1` | `docs/releases/local-release-hardening-evidence/artifacts/qa-2-node-contract-green.log` |
| A3 | Bun Test Log | Bun 1.4.0 test execution log showing 11 passes | `docs/releases/local-release-hardening-evidence/artifacts/qa-3-bun-test-green.log` |
| A4 | Windows PowerShell Log | RED baseline remote test run on `maho-win` showing 9 failures | `docs/releases/local-release-hardening-evidence/artifacts/qa-4-windows-ps-tests-red.log` |
| A5 | Windows PowerShell Log | GREEN remote test run on `maho-win` showing 10/10 passes with real MakeAppx | `docs/releases/local-release-hardening-evidence/artifacts/qa-5-windows-ps-tests-green.log` |
| A6 | Node Test Runner Log | Comprehensive test run across all 55 release tests passing with zero regressions | `docs/releases/local-release-hardening-evidence/artifacts/qa-6-all-release-tests-green.log` |
