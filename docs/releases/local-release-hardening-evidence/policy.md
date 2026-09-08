# Local-Release Hardening: Source-Enforced Workflow Policy Evidence

**Date:** 2026-09-08
**Task ID:** `st_01a07f9e`
**Status:** COMPLETE / VERIFIED GREEN
**Deliverable Files:**
- `scripts/release-workflow-policy.mjs` (Policy regression guard parsed with `Bun.YAML`)
- `scripts/release-workflow.test.mjs` (Behavioral parsed-policy test suite)
- `.github/workflows/release.yml` (Removed / retired hosted release producer)
- `.github/workflows/build-test.yml` (Wired policy check in PR test suite)
- `docs/releases/local-release-hardening-evidence/policy.md` (This document)

---

## 1. Executive Summary & Policy Scope

In accordance with the Ferryx Local-Release Hardening Contract (`docs/releases/local-pipeline-audit-2026-09-08/IMPLEMENTATION_CONTRACT.md`), all hosted release building, signing, and publishing workflows have been retired. Releases are strictly coordinated locally across dedicated builders (`macbook`, `omaki`, `maho-win`) using credential-free plans and cryptographic Minisign verification.

To prevent accidental reintroduction or workflow-level regressions of hosted release producers, this change installs a source-enforced policy regression guard (`scripts/release-workflow-policy.mjs`) that executes on every pull request within `.github/workflows/build-test.yml`.

### Boundary Notice
This tool is a **policy regression guard** against known workflow patterns, direct commands, actions, and configurations. It does **not** claim universal malicious backdoor prevention across arbitrary shell indirection (which cannot be statically proved by pattern matching).

### Enforced Workflow Policies:
1. **Forbidden Workflow File Names:** `.github/workflows/release.yml` and `.github/workflows/release.yaml` are strictly disallowed.
2. **Forbidden Trigger Classes:** `push.tags` is disallowed.
3. **Forbidden Permissions:** `contents: write` or `write-all` permissions at either workflow or job level are disallowed. Existing PR checks and GitHub Pages permissions (`contents: read`, `pages: write`, `id-token: write`) remain permitted.
4. **Forbidden Secret References:** Release signing and credential secrets (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_CONTENT`, `KEYCHAIN_PASSWORD`, `APPLE_SIGNING_IDENTITY`) are rejected anywhere in workflow files via a single recursive scan covering mapping keys, values, and environment variables.
5. **Forbidden Release Build Entry Points:**
   - `tauri build`, `@tauri-apps/cli build`, `cargo tauri build`, and `bun tauri build`.
   - Root `bun run build` / `npm run build` delegation (where root `package.json` delegates `"build": "cargo tauri build"`), while preserving scoped frontend builds (`bun run --cwd ui build` and `working-directory: ./site`).
   - `build-msix.ps1` (Windows MSIX packaging script).
   - Codesigning and notarization commands (`security import`, `security create-keychain`, `codesign`, `notarytool`, `stapler`, `assert-updater-archive-layout.mjs`).
   - Local coordinator invocations within CI (`scripts/release-local.mjs (build|assemble|publish)`).
   - PR check builds (`cargo check`, debug `cargo build` for Cargo Link Windows, `bun run --cwd ui build`) are permitted.
6. **Forbidden Release Publishing Actions & Workflows:**
   - Actions: `tauri-apps/tauri-action`, `softprops/action-gh-release`, `actions/create-release`, `ncipollo/release-action`.
   - Reusable workflow calls or jobs invoking release workflows or using `secrets: inherit` where they reintroduce producer jobs.
   - Manifest commands: `build-latest-json.mjs`, `gh release`.
7. **Artifact Safety Preservation:** Broad `*.exe` collection (an artifact of the obsolete hosted producer) has been eliminated along with the producer, while artifact integrity and signing are guaranteed by the local assembly lane.
8. **Simplicity and Line Budget:** Redundant per-block env scanning was removed in favor of unified recursive AST traversal; the validator contains 232 noncomment lines (strictly <= 250 limit).

---

## 2. Public API and CLI Contract

### CLI Command
```bash
# Validate live repository workflows (defaults to .github/workflows)
bun scripts/release-workflow-policy.mjs

# Validate specific workflow file or directory
bun scripts/release-workflow-policy.mjs .github/workflows/build-test.yml

# Output machine-readable JSON results
bun scripts/release-workflow-policy.mjs --json .github/workflows
```
- **Exit Code 0:** All evaluated workflows comply with local-release-only policy.
- **Exit Code 1:** One or more policy violations or missing/invalid files detected.

### Programmatic API (`scripts/release-workflow-policy.mjs`)
- `validateWorkflow(workflow: object, filePath?: string): { valid: boolean, violations: string[] }`
- `validateWorkflowYaml(yamlContent: string, filePath?: string): { valid: boolean, violations: string[] }`
- `validateWorkflowFile(filePath: string): { filePath: string, valid: boolean, violations: string[] }`
- `validateWorkflowsDir(dirPath: string): { dirPath: string, valid: boolean, violations: string[], results: Array<{ filePath: string, valid: boolean, violations: string[] }> }`
- `runCli(argv?: string[]): number`

### Test Invocation
```bash
# Run release workflow policy tests (16 tests)
node --test scripts/release-workflow.test.mjs
```

---

## 3. TDD Regression Progression

### Step 1: Baseline Pre-existing Failure Audit
Prior to implementation, running the release test suite exhibited 1 pre-existing regex failure:
```
not ok 6 - signatures and updater bundles are collected as release artifacts
  The input did not match the regular expression /-name "\*-setup\.exe"/. Input had -name "*.exe".
```
All 10 other tests relied on regex searches within the retired hosted producer `release.yml`.

### Step 2: RED Phase — Replacing Positive Regex Tests with Policy Enforcement Tests
`scripts/release-workflow.test.mjs` was rewritten to test behavioral parsed-policy enforcement.
Initial test run before `release-workflow-policy.mjs` implementation confirmed 10 failing tests and 1 pass (preserving Windows Cargo Link).

### Step 3: Implementation and Actual Producer RED Verification
`scripts/release-workflow-policy.mjs` was created using `Bun.YAML.parse`.
Running the validator against `.github/workflows/release.yml` before deletion produced a deterministic rejection across all policy axes:
- File name `release.yml` forbidden
- `push.tags` forbidden
- `contents: write` forbidden
- macOS codesigning and certificate imports forbidden
- Tauri bundle builds forbidden
- `build-msix.ps1` forbidden
- `softprops/action-gh-release@v2` forbidden
- `build-latest-json.mjs` forbidden

### Step 4: Lead Review Delta — Expanding Exact Entry Points (RED -> GREEN)
Following lead review, fixtures for:
1. `bun tauri build`
2. Root `bun run build` delegation (package.json build invokes cargo tauri build)
3. `tauri-apps/tauri-action`
4. `scripts/release-local.mjs` coordinator invocation
5. Reusable workflow jobs with `secrets: inherit`
were added to `scripts/release-workflow.test.mjs`. They were verified RED (5 failing), then supported in `scripts/release-workflow-policy.mjs`, turning GREEN (16 passed).

---

## 4. Manual QA Matrix

### surfaceEvidence

| Scenario ID | Criterion Reference | Surface | Exact Invocation | Verdict | Evidence / Exact Result | artifactRefs |
|---|---|---|---|---|---|---|
| QA-1 | Actual retired producer rejection | Bun CLI | `bun scripts/release-workflow-policy.mjs docs/releases/local-release-hardening-evidence/temp-retired.yml` | **PASS** | Exit code 1; correctly flags `push.tags`, `contents: write`, `@tauri-apps/cli build`, `TAURI_SIGNING_PRIVATE_KEY`, and `action-gh-release`. | A1 |
| QA-2 | Manual/tag/callable/reusable fixture rejection | Node.js Test Runner | `node --test --test-name-pattern="fixture\|build\|action\|coordinator\|reusable" scripts/release-workflow.test.mjs` | **PASS** | 10 subtests pass; confirms rejection of `workflow_dispatch`, `push.tags`, `workflow_call`, `bun tauri build`, root `bun run build`, `tauri-action`, `release-local.mjs`, and `secrets: inherit`. | A2 |
| QA-3 | PR check workflow permitted | Bun CLI | `bun scripts/release-workflow-policy.mjs .github/workflows/build-test.yml` | **PASS** | Exit code 0; permits `cargo check`, debug `cargo build` (Windows Cargo Link), and `bun run --cwd ui build`. | A3 |
| QA-4 | Pages deployment workflow permitted | Bun CLI | `bun scripts/release-workflow-policy.mjs .github/workflows/deploy-pages.yml` | **PASS** | Exit code 0; permits `contents: read`, `pages: write`, `id-token: write` and Pages build/deploy actions. | A4 |
| QA-5 | Live workflows directory compliance | Bun CLI | `bun scripts/release-workflow-policy.mjs .github/workflows` | **PASS** | Exit code 0; `.github/workflows` complies fully; `release.yml` is absent. | A5 |
| QA-6 | Policy test suite verification | Node.js Test Runner | `node --test scripts/release-workflow.test.mjs` | **PASS** | 16 subtests pass; 0 fail. | A6 |
| QA-7 | Full release test suite regression closure | Node.js Test Runner | `node --test scripts/sync-version.test.mjs scripts/build-latest-json.test.mjs scripts/release-workflow.test.mjs scripts/updater-archive-layout.test.mjs` | **PASS** | 38 subtests pass; 0 fail. Resolves pre-existing regex failure. | A7 |
| QA-8 | Windows Cargo Link step preserved | Node.js Test Runner | `node --test --test-name-pattern="Cargo Link" scripts/release-workflow.test.mjs` | **PASS** | Passes; asserts `cargo build --manifest-path src-tauri/Cargo.toml` is retained for Windows PR checks. | A8 |

### adversarialCases

| Scenario ID | Criterion Reference | Adversarial Class | Expected Behavior | Verdict | Evidence | artifactRefs |
|---|---|---|---|---|---|---|
| ADV-1 | Permission boundaries | Unauthorized write permissions | Rejects `contents: write` and `write-all` at both workflow and job levels. | **PASS** | Exit code 1; violations reported for `write-all` and job `contents: write`. | A9 |
| ADV-2 | Credential leak prevention | Reintroduced release secrets | Rejects any workflow referencing signing secrets (`TAURI_SIGNING_PRIVATE_KEY`, `APPLE_CERTIFICATE`, etc.) in env, steps, or secrets mappings. | **PASS** | Exit code 1; detects secrets in mapping keys and variable references. | A10 |
| ADV-3 | Syntax error resilience | Malformed YAML documents | Malformed YAML syntax is caught and reported gracefully without uncaught process crash. | **PASS** | Exit code 1; reports `YAML parsing error: Unexpected token`. | A11 |
| ADV-4 | Release backdoor evasion | Reintroduced `release.yml` file | Rejects directory scan if `.github/workflows/release.yml` or `release.yaml` exists, regardless of content. | **PASS** | Exit code 1; reports `Hosted release producer workflow "release.yml" is forbidden`. | A12 |
| ADV-5 | Stealth build commands | Alternative release packaging invocations | Rejects scripts or commands invoking `build-msix.ps1` or `tauri build` disguised in arbitrary steps. | **PASS** | Exit code 1; flags `build-msix.ps1` in arbitrary PowerShell invocation. | A13 |
| ADV-6 | Legitimate vs Release Actions | Action discrimination | Permitted deployment actions (`actions/deploy-pages`) pass; release publication actions (`softprops/action-gh-release`) fail. | **PASS** | Pages fixture exits 0; Release action fixture exits 1. | A14 |

### artifactRefs

| ID | Kind | Description | Path |
|---|---|---|---|
| A1 | Subprocess transcript | Output of validator rejecting retired release producer fixture | `docs/releases/local-release-hardening-evidence/qa-1-retired-producer-red.log` |
| A2 | Test transcript | Output of Node test runner verifying fixture rejections (manual, tag, callable, tauri build, root build, tauri-action, release-local, secrets) | `docs/releases/local-release-hardening-evidence/qa-2-fixtures-rejection.log` |
| A3 | CLI transcript | Output of validator confirming `build-test.yml` compliance | `docs/releases/local-release-hardening-evidence/qa-3-pr-check-permitted.log` |
| A4 | CLI transcript | Output of validator confirming `deploy-pages.yml` compliance | `docs/releases/local-release-hardening-evidence/qa-4-pages-permitted.log` |
| A5 | CLI transcript | Output of validator confirming `.github/workflows` live directory compliance | `docs/releases/local-release-hardening-evidence/qa-5-live-workflows-green.log` |
| A6 | Test transcript | Output of `node --test scripts/release-workflow.test.mjs` (16 tests pass) | `docs/releases/local-release-hardening-evidence/qa-6-test-suite-green.log` |
| A7 | Test transcript | Output of full release test suite (38 tests pass) | `docs/releases/local-release-hardening-evidence/qa-7-release-suite-green.log` |
| A8 | Test transcript | Output of test verifying Windows Cargo Link step preservation | `docs/releases/local-release-hardening-evidence/qa-8-windows-link-preserved.log` |
| A9 | CLI transcript | Adversarial test: permissions `write-all` and `contents: write` rejection | `docs/releases/local-release-hardening-evidence/adv-1-permissions.log` |
| A10 | CLI transcript | Adversarial test: forbidden release secret leak detection | `docs/releases/local-release-hardening-evidence/adv-2-secrets.log` |
| A11 | CLI transcript | Adversarial test: graceful handling of malformed YAML syntax | `docs/releases/local-release-hardening-evidence/adv-3-malformed-yaml.log` |
| A12 | CLI transcript | Adversarial test: detection and rejection of reintroduced `release.yml` file | `docs/releases/local-release-hardening-evidence/adv-4-file-forbidden.log` |
| A13 | CLI transcript | Adversarial test: detection of stealth release packaging commands (`build-msix.ps1`) | `docs/releases/local-release-hardening-evidence/adv-5-stealth-build.log` |
| A14 | CLI transcript | Adversarial test: action discrimination between Pages deployment and GitHub Release | `docs/releases/local-release-hardening-evidence/adv-6-actions-discrimination.log` |
