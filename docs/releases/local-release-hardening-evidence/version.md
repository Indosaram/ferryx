# Ferryx Local Release Hardening: Version Stamping & Verification Evidence

Date: 2026-09-08
Component: Wave 1 - Version Stamping Engine (`scripts/sync-version.mjs`, `scripts/sync-version.test.mjs`)
Contract: `docs/releases/local-pipeline-audit-2026-09-08/IMPLEMENTATION_CONTRACT.md` (Section 3 & Section 8)

---

## 1. Exported Canonical API & Streamlined Architecture

The module `scripts/sync-version.mjs` (264 lines) exports four canonical functions with strict ESM compatibility (compatible with Node.js >= 22.22 and Bun >= 1.4, zero npm dependencies):

```typescript
export function parseReleaseTag(tag: string): { year: number; month: number; day: number; revision: number }
export function toAppVersion(tag: string): string
export function toMsixVersion(tag: string): string
export async function syncVersion(options: {
  tag: string;
  confPath?: string;
  cargoPath?: string;
  dryRun?: boolean;
}): Promise<{ version: string; msixVersion: string }>
```

### Direct Execution & Import Safety
The CLI entry point uses `realpathSync` / `fileURLToPath` comparison against `process.argv[1]` so that importing the file into tests or coordinator tooling executes zero side-effects.

Direct CLI usage:
```bash
node scripts/sync-version.mjs --tag <tag> [--conf <path>] [--cargo <path>] [--dry-run]
```
Outputs `version=<version>\n` on stdout and returns exit code 0 on success.

---

## 2. Validation & Mapping Rules

1. **Calendar Validation:**
   - CalVer input tags: `vYYYY.MM.DD` or `vYYYY.MM.DD.R` (e.g. `v2026.09.08.1` or `v2026.09.08`).
   - Year: strictly `>= 2026` and `<= 65535` (MSIX uint16 limit).
   - Month: strictly `01`..`12` (2 digits).
   - Day: strictly `01`..`31` (2 digits, validated against month lengths and leap years).
   - Leap Years: `(year % 4 === 0 && year % 100 !== 0) || year % 400 === 0`. February 29 is valid on 2028, rejected on 2026.
   - Revision: optional, defaults to `0`. If present, must be `0` or start with `1..9` (no leading zeroes like `01`), bounded by `0 <= R <= 65535`.
2. **Version Mapping & Tag Input Restriction:**
   - App / Updater SemVer: `YYYY.(MM * 100 + DD).R` (e.g. `v2026.09.08.1` -> `2026.908.1`).
   - Windows MSIX Quad: `YYYY.(MM * 100 + DD).R.0` (e.g. `v2026.09.08.1` -> `2026.908.1.0`).
   - `toMsixVersion` input contract strictly expects a release tag or legacy SemVer string; raw 4-part MSIX quad inputs (e.g. `2026.908.1.0`) are rejected.
3. **Legacy SemVer Compatibility:**
   - Generic SemVer tags (e.g. `v1.4.2` or `1.4.2`) are accepted by standalone stamping CLI and `toAppVersion`/`toMsixVersion`/`syncVersion` without requiring `year >= 2026`.
   - `toAppVersion("v1.4.2")` -> `"1.4.2"`.
   - `toMsixVersion("v1.4.2")` -> `"1.4.2.0"`.
4. **Write Safety, Atomic Rollback & Error Transparency:**
   - Both `tauri.conf.json` and `Cargo.toml` are parsed and validated in memory before any disk writes.
   - Sibling temporary files (`${target}.${randomUUID()}.tmp`) are created in the target directory to guarantee single-filesystem atomic renames (`rename(2)`) and prevent fixed `.tmp` race conditions.
   - If writing or renaming the second manifest fails synchronously, the first manifest is rolled back to its original bytes and temporary files are cleaned up.
   - **No Empty Catch Suppression:** If rollback or temporary file cleanup encounters an error, `syncVersion` throws an `AggregateError` containing both the primary failure and the rollback/cleanup errors, ensuring the caller is informed that restoration was incomplete.
   - Rewrites only the `[package]` version in `Cargo.toml`; dependency version declarations in `[dependencies]` remain byte-identical.

---

## 3. TDD Proof (Red-to-Green Progression)

### Initial TDD Red Phase (Import Side-Effects & Missing Exports)
- Command: `node --test scripts/sync-version.test.mjs`
- Exit Code: `1` (subtest exitCode 2)
- Output:
```text
TAP version 13
# usage: sync-version.mjs --tag <tag> [--conf <path>] [--cargo <path>]
# Subtest: scripts/sync-version.test.mjs
not ok 1 - scripts/sync-version.test.mjs
  ---
  duration_ms: 388.738375
  type: 'test'
  location: '/Users/indo/code/project/orca-lite/scripts/sync-version.test.mjs:1:1'
  failureType: 'testCodeFailure'
  exitCode: 2
  signal: ~
  error: 'test failed'
  code: 'ERR_TEST_FAILURE'
  ...
1..1
# tests 1
# suites 0
# pass 0
# fail 1
```

### Lead Review Hardening Red Phase (Quad Input Rejection & AggregateError Seam)
- Command: `node --test scripts/sync-version.test.mjs`
- Exit Code: `1` (3 subtests failed)
- Output:
```text
TAP version 13
...
# Subtest: toAppVersion and toMsixVersion map CalVer tags and legacy SemVer correctly
not ok 13 - toAppVersion and toMsixVersion map CalVer tags and legacy SemVer correctly
  ---
  duration_ms: 0.481084
  type: 'test'
  location: '/Users/indo/code/project/orca-lite/scripts/sync-version.test.mjs:275:1'
  failureType: 'testCodeFailure'
  error: 'Missing expected exception.'
  code: 'ERR_ASSERTION'
...
# Subtest: syncVersion rolls back first file replacement on synchronous second file failure
not ok 17 - syncVersion rolls back first file replacement on synchronous second file failure
  ---
  duration_ms: 0.874209
  type: 'test'
  location: '/Users/indo/code/project/orca-lite/scripts/sync-version.test.mjs:374:1'
  failureType: 'testCodeFailure'
  error: 'Missing expected rejection.'
  code: 'ERR_ASSERTION'
...
# Subtest: syncVersion surfaces AggregateError with original and rollback failures when rollback fails
not ok 18 - syncVersion surfaces AggregateError with original and rollback failures when rollback fails
  ---
  duration_ms: 0.7895
  type: 'test'
  location: '/Users/indo/code/project/orca-lite/scripts/sync-version.test.mjs:402:1'
  failureType: 'testCodeFailure'
  error: 'Missing expected rejection.'
  code: 'ERR_ASSERTION'
...
1..19
# tests 19
# suites 0
# pass 16
# fail 3
```

### Final Green Phase (Deterministic fs Mocks via node:test & syncBuiltinESMExports)
- Command: `node --test scripts/sync-version.test.mjs`
- Exit Code: `0`
- Output:
```text
TAP version 13
# Subtest: a date tag maps to monotonic semver and leaves dependency versions untouched
ok 1 - a date tag maps to monotonic semver and leaves dependency versions untouched
# Subtest: the JSON manifest keeps key order and a trailing newline
ok 2 - the JSON manifest keeps key order and a trailing newline
# Subtest: running twice with the same tag is idempotent
ok 3 - running twice with the same tag is idempotent
# Subtest: a semver tag is accepted
ok 4 - a semver tag is accepted
# Subtest: date mapping is strictly monotonic across a revision, day, month, and year boundary
ok 5 - date mapping is strictly monotonic across a revision, day, month, and year boundary
# Subtest: a malformed tag fails loudly and leaves both manifests unchanged
ok 6 - a malformed tag fails loudly and leaves both manifests unchanged
# Subtest: a missing tag reports usage instead of guessing
ok 7 - a missing tag reports usage instead of guessing
# Subtest: importing sync-version.mjs has no side effects and exports canonical API
ok 8 - importing sync-version.mjs has no side effects and exports canonical API
# Subtest: parseReleaseTag handles valid CalVer tags with or without revision and leading v
ok 9 - parseReleaseTag handles valid CalVer tags with or without revision and leading v
# Subtest: parseReleaseTag strictly rejects impossible calendar dates (Feb 29 non-leap, month 13, day 32)
ok 10 - parseReleaseTag strictly rejects impossible calendar dates (Feb 29 non-leap, month 13, day 32)
# Subtest: parseReleaseTag accepts leap year Feb 29 (e.g. 2028.02.29)
ok 11 - parseReleaseTag accepts leap year Feb 29 (e.g. 2028.02.29)
# Subtest: parseReleaseTag enforces boundaries and leading zero rules
ok 12 - parseReleaseTag enforces boundaries and leading zero rules
# Subtest: toAppVersion and toMsixVersion map CalVer tags and legacy SemVer correctly
ok 13 - toAppVersion and toMsixVersion map CalVer tags and legacy SemVer correctly
# Subtest: syncVersion supports dryRun and returns version pair without modifying files
ok 14 - syncVersion supports dryRun and returns version pair without modifying files
# Subtest: syncVersion writes atomically using sibling temp files and preserves dependencies
ok 15 - syncVersion writes atomically using sibling temp files and preserves dependencies
# Subtest: syncVersion validates both inputs before writing (second input invalid leaves first unchanged)
ok 16 - syncVersion validates both inputs before writing (second input invalid leaves first unchanged)
# Subtest: syncVersion rolls back first file replacement on synchronous second file failure
ok 17 - syncVersion rolls back first file replacement on synchronous second file failure
# Subtest: syncVersion surfaces AggregateError with original and rollback failures when rollback fails
ok 18 - syncVersion surfaces AggregateError with original and rollback failures when rollback fails
# Subtest: CLI rejects invalid calendar dates and impossible inputs
ok 19 - CLI rejects invalid calendar dates and impossible inputs
1..19
# tests 19
# suites 0
# pass 19
# fail 0
# cancelled 0
# skipped 0
# todo 0
# duration_ms 1136.670292
```

---

## 4. Manual QA Matrix

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
