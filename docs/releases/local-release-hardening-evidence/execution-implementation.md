# Local release execution implementation

Date: 2026-09-08  
Task: `st_01a080fd`

## Outcome

The local release coordinator now has production remote execution paths for Linux (`omaki`) and Windows (`maho-win`) instead of the previous unconditional throw.

Implemented behavior:

- A prepared plan digest is computed over the exact bytes written to `plan.json`.
- Build, assemble, verify, and publish bind their mutations/verifications to the prepared plan and host-config digests.
- Assembly writes `assembly-state.json`, binding plan, receipt tree, artifact tree, and publish tree digests. Verify and publish reject changes after assembly.
- Remote workspaces are collision-checked, created before SCP, and populated with source and Ghostty bundles.
- Generated Linux and Windows scripts run `git bundle verify`, clone into fresh directories, detach at the exact source SHA, derive and verify the Ghostty pin, replace the vendor checkout, stamp the version, and use `bun install --frozen-lockfile`.
- Linux builds AppImage and DEB, requires exactly one non-empty output of each class, archives the updater payload, and invokes the Tauri signer to generate its required signature.
- Windows builds the exact executable, invokes `build-msix.ps1` with explicit `-ExePath`, release quad version, output directory, and Store `-SkipSigning` mode. The approval-dependent NSIS channel builds and signs its updater only when requested by the plan.
- SCP retrieves an exact expected filename set. Receipt generation requires the exact host artifact kinds and updater signatures, and refuses pre-existing artifact or receipt output.
- A custom runner cannot imply success by returning `undefined`; it must return an explicit numeric exit code.
- Publication requires both approvals, validates assembly state before any GitHub write, distinguishes an existing release from an indeterminate `gh release view` failure, verifies an exact downloaded draft inventory and bytes, and only then undrafts.
- Process failure text goes through the shared `redactProcessOutput`. No signer help command is run, and signing secret values are neither read nor printed by these scripts.

## TDD and verification evidence

A focused red run was captured at `/tmp/st_01a080fd-red.log`. It demonstrated both original defects under test:

- untouched prepare/build handoff failed before reaching the runner because the plan digest did not cover the newline written to disk;
- an absent runner result fell through to artifact scanning instead of failing at the runner boundary.

Green verification artifacts:

- `artifacts/st_01a080fd-focused-tests.log`: `node --test scripts/release-local.test.mjs scripts/release-platforms.test.mjs` - 21 passed, 0 failed.
- `artifacts/st_01a080fd-cli-check.log`: syntax checks for both production modules and real CLI help execution.

No public release, notarization submission, hardware build, daemon/app change, global configuration mutation, or commit was performed.

## Review notes

The two production files predate this task and remain substantially above the 250 pure-LOC guideline (`release-local.mjs` and `release-platforms.mjs`). Splitting them safely is outside this focused completion task and would require the dedicated refactor protocol before the next feature edit.

Architectural review:

1. Responsibility: the scoped files own release coordination and platform execution respectively, though both are oversized inherited modules.
2. Boundary purity: JSON plans/configs/receipts continue through strict existing parsers; generated remote scripts quote all newly introduced path/version values.
3. Variant discrimination: platform dispatch uses the existing host/platform boundary; no new open tagged union was introduced.
4. Escape hatches: no `any`, type suppression, or non-null assertion was introduced.
5. Defensive layers: digest checks are boundary integrity controls, not redundant setter/readback checks.
6. Helpers: script generators are exported and directly exercised as the testable production seam.
7. Tests: the new digest handoff, absent-runner failure, and generated remote script contracts are covered.
8. Parameter bloat: inherited object-parameter APIs were retained; no positional parameter expansion was introduced.
9. Redundant verification: checkout and artifact verification validate untrusted remote/build outputs and are required by contract.
10. Naming: new names use positive concepts (`validatePreparedRun`, `validateAssemblyState`).
11. Logging: no logger setup was added; subprocess errors use the project redaction seam.
