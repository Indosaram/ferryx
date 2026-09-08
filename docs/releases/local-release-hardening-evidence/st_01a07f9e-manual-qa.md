# Manual QA Matrix — `st_01a07f9e`

**Goal:** Replace hosted release producer with source-enforced local-only policy regression guard, remove `.github/workflows/release.yml`, implement `Bun.YAML`-based policy validator, minimally wire policy check into `build-test.yml`, and replace obsolete positive regex tests with behavioral parsed-policy tests while preserving existing PR checks, Pages deployment, and Windows Cargo Link verification.

**Overall verdict: PASS**

All contract specifications and lead review criteria have been satisfied:
1. Deliverable files written: `scripts/release-workflow-policy.mjs`, `scripts/release-workflow.test.mjs`, `.github/workflows/build-test.yml`, with `.github/workflows/release.yml` cleanly removed.
2. The policy validator parses workflow YAML using `Bun.YAML.parse` and rejects:
   - File name `release.yml` / `release.yaml`
   - `push.tags`
   - `contents: write` or `write-all` permissions
   - Release signing secrets via recursive AST scan
   - Release build entry points: `tauri build`, `@tauri-apps/cli build`, `cargo tauri build`, `bun tauri build`, root `bun run build` (package.json build delegation), `build-msix.ps1`, codesigning commands, and local coordinator invocations `scripts/release-local.mjs (build|assemble|publish)`
   - Actions and workflows: `tauri-apps/tauri-action`, `softprops/action-gh-release`, `actions/create-release`, `ncipollo/release-action`, reusable release workflows, and `secrets: inherit` where reintroducing producer jobs
   - Manifest commands: `build-latest-json.mjs`, `gh release`
3. PR check builds (`cargo check`, debug `cargo build` for Windows Cargo Link, `bun run --cwd ui build`) and Pages permissions (`contents: read`, `pages: write`, `id-token: write`) remain permitted and functional.
4. Minimal policy check wired directly into `ui-check` in `.github/workflows/build-test.yml`.
5. Pre-existing regex failure on broad `*.exe` collection is retired; all tests across the release test suite pass GREEN.
6. Behavioral fixtures covering `bun tauri build`, root `bun run build`, `tauri-action`, `release-local.mjs`, and `secrets: inherit` were added, verified RED, and brought GREEN.
7. Policy regression guard scope explicitly noted: guards against known workflow regression patterns without claiming universal malicious backdoor defense against arbitrary shell indirection.
8. Line budget respected: 232 noncomment lines (strictly <= 250 limit).

The ULW status command returned `ULW_LOOP_PLAN_MISSING`; therefore evidence is stored in caller evidence directory `docs/releases/local-release-hardening-evidence/` and mirrored in `.omo/evidence/qa/st_01a07f9e/` and `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07f9e/`.

## surfaceEvidence

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

## adversarialCases

| Scenario ID | Criterion Reference | Adversarial Class | Expected Behavior | Verdict | Evidence | artifactRefs |
|---|---|---|---|---|---|---|
| ADV-1 | Permission boundaries | Unauthorized write permissions | Rejects `contents: write` and `write-all` at both workflow and job levels. | **PASS** | Exit code 1; violations reported for `write-all` and job `contents: write`. | A9 |
| ADV-2 | Credential leak prevention | Reintroduced release secrets | Rejects any workflow referencing signing secrets (`TAURI_SIGNING_PRIVATE_KEY`, `APPLE_CERTIFICATE`, etc.) in env, steps, or secrets mappings. | **PASS** | Exit code 1; detects secrets in mapping keys and variable references. | A10 |
| ADV-3 | Syntax error resilience | Malformed YAML documents | Malformed YAML syntax is caught and reported gracefully without uncaught process crash. | **PASS** | Exit code 1; reports `YAML parsing error: Unexpected token`. | A11 |
| ADV-4 | Release backdoor evasion | Reintroduced `release.yml` file | Rejects directory scan if `.github/workflows/release.yml` or `release.yaml` exists, regardless of content. | **PASS** | Exit code 1; reports `Hosted release producer workflow "release.yml" is forbidden`. | A12 |
| ADV-5 | Stealth build commands | Alternative release packaging invocations | Rejects scripts or commands invoking `build-msix.ps1` or `tauri build` disguised in arbitrary steps. | **PASS** | Exit code 1; flags `build-msix.ps1` in arbitrary PowerShell invocation. | A13 |
| ADV-6 | Legitimate vs Release Actions | Action discrimination | Permitted deployment actions (`actions/deploy-pages`) pass; release publication actions (`softprops/action-gh-release`) fail. | **PASS** | Pages fixture exits 0; Release action fixture exits 1. | A14 |

## artifactRefs

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
