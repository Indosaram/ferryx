# Ferryx Local Release Hardening: Version Stamping Manual QA Matrix

Date: 2026-09-08
Component: Wave 1 - Version Stamping Engine (`scripts/sync-version.mjs`, `scripts/sync-version.test.mjs`)
Goal ID: version

## manualQa Matrix

### surfaceEvidence
| Scenario ID | Criterion Reference | Surface | Exact Invocation | Verdict | Artifact Refs |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `SCENARIO-01` | Section 3 / Exported API & Import Safety | Node ESM Module Import | `node -e 'import("./scripts/sync-version.mjs").then(m => console.log(JSON.stringify(Object.keys(m).sort())))'` | PASS | `scenario-01-import-api.txt` |
| `SCENARIO-02` | Section 3 / CalVer & MSIX Quad Mapping | Node Module API | `node -e 'import("./scripts/sync-version.mjs").then(m => { console.log(JSON.stringify(m.parseReleaseTag("v2026.09.08.1"))); console.log("appVersion=" + m.toAppVersion("v2026.09.08.1")); console.log("msixVersion=" + m.toMsixVersion("v2026.09.08.1")); })'` | PASS | `scenario-02-calver-mapping.txt` |
| `SCENARIO-03` | Section 3 / CLI Atomic Version Sync | Node CLI Process | `node scripts/sync-version.mjs --tag v2026.09.08.2 --conf <tmpConf> --cargo <tmpCargo>` | PASS | `scenario-03-cli-sync.txt` |
| `SCENARIO-04` | Section 3 & 8 / Legacy SemVer CLI Compatibility | Node CLI Process | `node scripts/sync-version.mjs --tag v1.4.2 --conf <tmpConf> --cargo <tmpCargo>` | PASS | `scenario-04-legacy-semver.txt` |
| `SCENARIO-05` | Section 3 / Pre-Write Validation Safety | Node CLI Process | `node scripts/sync-version.mjs --tag v2026.09.08.1 --conf <tmpConf> --cargo <tmpCargoWithoutPackage>` | PASS | `scenario-05-prewrite-validation.txt` |

### adversarialCases
| Scenario ID | Criterion Reference | Adversarial Class | Expected Behavior | Verdict | Artifact Refs |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `ADV-01` | Section 3 / Strict Calendar Dates | Invalid Leap Year (Feb 29 on non-leap year 2026) | Exit 1, reject impossible Feb 29 with calendar date error | PASS | `adv-01-invalid-feb29.txt` |
| `ADV-02` | Section 3 / Calendar Date Bounds | Impossible Month (Month 13) | Exit 1, reject month 13 outside 1..12 range | PASS | `adv-02-month-13.txt` |
| `ADV-03` | Section 3 / Calendar Date Bounds | Day Overflow in 30-day Month (April 31) | Exit 1, reject day 31 in 30-day month | PASS | `adv-03-april-31.txt` |
| `ADV-04` | Section 3 / CalVer Year Boundary | Pre-2026 CalVer Date Tag (2025.12.31) | Exit 1, reject year < 2026 | PASS | `adv-04-pre-2026.txt` |
| `ADV-05` | Section 3 / MSIX UInt16 Quad Bounds | Revision Overflow (> 65535, e.g. 65536) | Exit 1, reject revision exceeding MSIX uint16 limit | PASS | `adv-05-revision-overflow.txt` |
| `ADV-06` | Section 3 / Leading Zero Syntax Validation | Malformed Leading Zero in Revision (`v2026.09.08.01`) | Exit 1, reject leading zero in revision | PASS | `adv-06-leading-zero-revision.txt` |
| `ADV-07` | Section 3 / Synchronous Write Failure & Atomic Rollback | Second Target Rename Failure (EIO) after first target replacement | Assert first target contained new version before failure, then verify original bytes restored and temp files cleaned | PASS | `adv-07-atomic-rollback.txt` |
| `ADV-08` | Section 3 / Unsupported MSIX Quad Input | Unsupported raw 4-part quad string (`2026.908.1.0`) | Rejection, enforce release tag / SemVer input contract | PASS | `adv-08-reject-msix-quad-input.txt` |
| `ADV-09` | Section 3 / Rollback Failure Error Surfacing | Rollback write failure during synchronous recovery | AggregateError surfaced with both original EIO and rollback EACCES errors | PASS | `adv-09-aggregate-error-rollback-failure.txt` |

### artifactRefs
| ID | Kind | Description | Path |
| :--- | :--- | :--- | :--- |
| `scenario-01-import-api.txt` | cli-log | Verification of exported API keys and zero import side effects | `docs/releases/local-release-hardening-evidence/scenario-01-import-api.txt` |
| `scenario-02-calver-mapping.txt` | cli-log | Verification of `parseReleaseTag`, `toAppVersion`, and `toMsixVersion` outputs | `docs/releases/local-release-hardening-evidence/scenario-02-calver-mapping.txt` |
| `scenario-03-cli-sync.txt` | cli-log | Full CLI execution checking exitCode 0, output `version=`, and dependency preservation | `docs/releases/local-release-hardening-evidence/scenario-03-cli-sync.txt` |
| `scenario-04-legacy-semver.txt` | cli-log | CLI execution of legacy SemVer `v1.4.2` with major < 2026 | `docs/releases/local-release-hardening-evidence/scenario-04-legacy-semver.txt` |
| `scenario-05-prewrite-validation.txt` | cli-log | Corrupted `Cargo.toml` input leaves `tauri.conf.json` completely untouched | `docs/releases/local-release-hardening-evidence/scenario-05-prewrite-validation.txt` |
| `adv-01-invalid-feb29.txt` | cli-log | Error rejection output for `v2026.02.29` non-leap year | `docs/releases/local-release-hardening-evidence/adv-01-invalid-feb29.txt` |
| `adv-02-month-13.txt` | cli-log | Error rejection output for month 13 | `docs/releases/local-release-hardening-evidence/adv-02-month-13.txt` |
| `adv-03-april-31.txt` | cli-log | Error rejection output for April 31 | `docs/releases/local-release-hardening-evidence/adv-03-april-31.txt` |
| `adv-04-pre-2026.txt` | cli-log | Error rejection output for year 2025 CalVer tag | `docs/releases/local-release-hardening-evidence/adv-04-pre-2026.txt` |
| `adv-05-revision-overflow.txt` | cli-log | Error rejection output for revision 65536 exceeding MSIX bound | `docs/releases/local-release-hardening-evidence/adv-05-revision-overflow.txt` |
| `adv-06-leading-zero-revision.txt` | cli-log | Error rejection output for leading zero in revision `01` | `docs/releases/local-release-hardening-evidence/adv-06-leading-zero-revision.txt` |
| `adv-07-atomic-rollback.txt` | cli-log | Rollback verification confirming first target contained new version before EIO failure, followed by restoration and temp cleanup | `docs/releases/local-release-hardening-evidence/adv-07-atomic-rollback.txt` |
| `adv-08-reject-msix-quad-input.txt` | cli-log | Rejection output for unsupported 4-part quad input `2026.908.1.0` | `docs/releases/local-release-hardening-evidence/adv-08-reject-msix-quad-input.txt` |
| `adv-09-aggregate-error-rollback-failure.txt` | cli-log | AggregateError output containing both original EIO and rollback EACCES errors | `docs/releases/local-release-hardening-evidence/adv-09-aggregate-error-rollback-failure.txt` |
