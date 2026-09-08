# Coordinator and Platform Adapter Execution Verification

Date: 2026-09-08  
Independent QA task: `st_01a080ec`  
Contract: `docs/releases/local-pipeline-audit-2026-09-08/IMPLEMENTATION_CONTRACT.md`  
Manual QA matrix: `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a080ec/st_01a080ec-manual-qa.md`

## Verdict

**FAIL. The coordinator and platform adapters do not satisfy the execution scope.** Previous completion signals and green test counts overstate implementation.

The strongest blockers are:

1. A fresh `prepare` output cannot enter `build`: the stored plan digest excludes the newline written to `plan.json`.
2. Remote Linux and Windows production builds unconditionally throw. There is no transport staging, clone, checkout, build, artifact retrieval, or receipt collection path.
3. Remote preflight embeds configuration in shell source without quoting, suppresses tool failures, and does not enforce configured or plan toolchain versions.
4. Receipt generation accepts an incomplete host artifact set and overwrites existing receipts/artifacts.
5. Publish does not revalidate plan/receipts/files before writing, treats every failed `gh release view` as “not found,” and verifies only a local-file subset of downloaded draft assets.
6. Public `verify-remote` checks two HTTP responses and manifest version only; it does not verify public asset inventory, checksums, or bytes.

No source changes or redundant full test suites were performed. Each execution claim below is backed by a new artifact from this turn.

## Status by required surface

| Required surface | Classification | Evidence-based result |
|---|---|---|
| CLI command routing/help | Implemented | Seven commands are routed, but existence is not execution certification. |
| Prepare immutable source identity and Ghostty pin | Partially implemented, build-blocking defect | `prepare` resolves commit and Ghostty HEAD, but stores SHA256 of JSON without newline then writes the newline. Untouched build fails as tampered. |
| Source/Ghostty transport integrity | Missing | Bundles can be generated, but existing bundle files are trusted by presence only. No stored expected digest, `git bundle verify`, or transport content binding exists. |
| Local macOS isolated build | Environment/implementation blocked | Dedicated root and bundle clone code exist, but fresh CLI cannot reach it due to digest mismatch. Required `lipo`, strict codesign verification, notary submit/staple, and updater archive validation are absent from the adapter. |
| Remote Linux build | Missing | Production branch unconditionally throws at `scripts/lib/release-platforms.mjs:742`; exact CLI invocation reproduced it. |
| Remote Windows build | Missing | Same unconditional throw; no remote PowerShell MSIX path is wired. |
| Process argv boundary | Implemented for process launch | `runProcess` uses `spawn(command,args)` and SSH options are discrete argv entries. |
| Script/data injection boundary | Defective | `hostConfig.root`, `hostConfig.path`, and workspace paths are inserted into shell/PowerShell source. A validated root executed a marker command in the faithful POSIX transport scenario. Imported `quoteSh`/`quotePowerShell` are unused here. |
| Preflight reachability/disk/OS | Partially implemented | Real commands are run and disk/OS data is parsed. This is a preflight only, not build certification. |
| Preflight tool executability and pinning | Defective | Linux uses `|| true`; Windows redirects errors without explicit per-tool status checks; host expected versions and parsed plan requirements are not compared. Five tools printing versions and exiting 9 were accepted. |
| Isolated target roots | Partially implemented | Workspace path is `host.root/runId` and an initial collision check exists. However, failed builds leave that path, receipts/artifacts are shared under the run directory, and those outputs are overwritten on retry. |
| Exact artifacts, paths, and SHA | Partially implemented | Staged files receive computed byte counts/SHA and assembly performs stronger jail/hash checks. Builder scanning does not require exact expected kinds and trusts pre-existing staged files. |
| Strict receipts/signatures | Defective at builder boundary; stronger at assembly | Parser rejects unknown fields and updater entries without signature paths, but requires only one artifact, not the exact host set. A DMG-only macOS receipt was accepted. Signature cryptography is deferred to assembly, so a receipt’s mere signature path is not proof. |
| No daemon/installed-app/cache effects | Verified by source audit for these modules | No daemon, installed-app, or shared-cache mutation commands were found; executed scenarios used temporary roots. This does not cure shared run-output overwrites. |
| Publish approvals before write | Gate implemented, integrity prerequisite missing | Flag and environment approval are checked before remote calls. However, approved publish does not rerun verification or bind current plan/receipts/files to assembly before `gh release create`. |
| Draft exact byte/inventory verification | Defective | It byte-compares each local file found after upload, but ignores extra downloaded assets and does not validate local checksums first. A stale-checksum release with an extra remote asset was undrafted. |
| Public post-publish verification | Missing/incomplete | Only `latest.json` version and existence of `SHA256SUMS.txt` are checked. Checksums and assets are not downloaded/verified. |

## Concrete blockers with file references

### 1. Fresh build is dead on arrival

- `scripts/release-local.mjs:186-187` hashes `planContent`.
- `scripts/release-local.mjs:210` writes `planContent + "\n"`.
- `scripts/release-local.mjs:262-264` hashes file bytes including the newline and rejects the mismatch.

Real result: prepare exited 0; `PLAN_FILE_SHA=dae64a47...`, while `STATE_PLAN_SHA=c4b51485...`; untouched build exited 1. This blocks every host before platform logic. Evidence: `01-prepare-build-handoff.log`.

### 2. Remote builders are not implemented

- `scripts/release-local.mjs:267-274` delegates directly to `buildHost` with no production runner.
- `scripts/lib/release-platforms.mjs:742` throws for every non-macbook host.
- `scripts/release-platforms.test.mjs:249-325` supplies fake bundle strings and a `mockRunner` that directly writes fake artifact values. It does not exercise transport staging, bundle validation, remote clone/checkout, platform command construction, or retrieval.

The exact coordinator CLI was exercised for both hosts after aligning the fixture digest only to reach dispatch. Both exited 1 with the unconditional throw. Evidence: `02-remote-omaki.log`, `02-remote-maho-win.log`.

### 3. Remote shell injection and ignored tool exits

- `scripts/lib/release-platforms.mjs:137-150` interpolates `path` and `root` into a POSIX script.
- `scripts/lib/release-platforms.mjs:153-161` suppresses nonzero tool/package exits with `|| true`.
- `scripts/lib/release-platforms.mjs:229-236` treats any non-empty captured output as success.
- `scripts/lib/release-platforms.mjs:276-287` similarly interpolates Windows root and captures tools without retaining each native command’s status.
- `scripts/release-local.mjs:224-230` parses `--plan` and then discards `plan`.
- `validateToolchains` is imported in the adapter but never called.

A config accepted by `loadHostConfig` caused command execution through `hostConfig.root`. Tools printing `999.999.999` and exiting 9 were all `ok:true`; expected Bun `1.4.0` was ignored. Evidence: `03-preflight-boundaries.log`.

### 4. Partial receipts and unsafe overwrite

- `scripts/lib/release-platforms.mjs:527-570` scans whatever recognized files happen to exist.
- `scripts/lib/release-platforms.mjs:752-755` rejects only zero artifacts.
- `scripts/lib/release-contract.mjs:421-422` likewise requires only a non-empty array.
- `requiredKinds()` exists at `scripts/lib/release-contract.mjs:331-338` but is not used by `parseReceipt` or `buildHost`.
- `scripts/lib/release-platforms.mjs:769-770` writes the receipt without collision protection.
- Artifact copies at `scripts/lib/release-platforms.mjs:727-739` also overwrite destination names.
- `scripts/lib/release-platforms.mjs:682` uses `cp -R isolatedGhostty ghosttyTargetDir`; because the source checkout may already contain that directory, BSD `cp` can nest rather than create the intended exact vendor tree.

A DMG-only runner produced a valid macOS receipt. A second invocation changed both the existing artifact and receipt SHA without rejection. Evidence: `04-partial-receipt-overwrite.log`.

### 5. Publish can write unverified content and accept an inexact draft

- `scripts/release-local.mjs:403-415` checks approvals first, which is correct.
- It does not call `verifyReleaseRun`, validate `SHA256SUMS.txt`, or bind receipts/plan/files to prior assembly before write.
- `scripts/release-local.mjs:456-460` interprets every nonzero `gh release view` as absence, including auth/network/tool failures.
- `scripts/release-local.mjs:485-488` uploads the current directory with `--clobber`.
- `scripts/release-local.mjs:507-519` checks every local file exists and matches remotely, but never rejects extra downloaded assets.

With both approvals present, stale local checksum metadata, `release view` exit 2, and an extra downloaded asset, the function created, uploaded, and undrafted successfully. Evidence: `05-publish-integrity.log`.

### 6. `verify-remote` is presence/version-only

- `scripts/release-local.mjs:548-566` checks HTTP success and manifest version, then only checks that the sums URL responds successfully.

It does not parse `SHA256SUMS.txt`, enforce exact inventory, fetch artifacts, or verify remote bytes. The command therefore cannot support the contract’s public latest/download verification claim.

## What is genuinely implemented and verified

- Process spawning uses command plus argv rather than shell command concatenation in `runProcess`.
- SSH has `BatchMode=yes`, `ConnectTimeout=10`, and bounded server-alive options.
- Local/remote workspaces are named under configured per-host roots and initially collision-checked.
- Receipt parsing enforces strict root/artifact keys, host/kind binding, positive byte counts, lowercase SHA256 syntax, safe relative path syntax, and updater signature path presence.
- Assembly is wired to the receipt-driven `assembleRelease` implementation, where stronger real-file hash, jail, completeness, and Minisign checks live. This turn did not rerun that previously tested domain and does not treat old logs as current execution proof.
- Publication requires both `--approve-publish` and `FERRYX_APPROVE_PUBLISH=1` before remote access.
- No daemon, installed application, or shared-cache mutations were found in the reviewed coordinator/adapter code.

## Environment-blocked versus implementation-blocked

- **Environment-blocked:** a real universal macOS build may also be constrained by configured disk/signing/notary prerequisites, but this turn cannot reach those checks because the digest mismatch fails first.
- **Implementation-blocked:** all fresh CLI builds; both remote builds; platform verification commands; exact builder receipt completeness; pinned/version-aware preflight; injection-safe script composition; pre-publish immutable-input verification; exact draft inventory; public byte verification.

## Final required remediation

Release completion must remain blocked until all of the following are observable:

1. Fresh `prepare -> build` reaches the platform adapter without rewriting state.
2. Production `omaki` and `maho-win` CLI builds stage immutable source/Ghostty inputs through real transports, verify bundle/checkout SHAs, use dedicated roots, retrieve artifacts, and emit receipts.
3. Tests exercise those transport scripts and staging/clone/path behavior, not a callback that writes the final artifacts.
4. Preflight fails on nonzero tool execution and mismatched configured/plan versions; every script interpolation uses safe quoting or sends values as data.
5. Builders require the exact host artifact set and signatures, start from empty per-host outputs, and fail on existing receipt/artifact destinations.
6. macOS/Linux/Windows platform-specific binary/package verification from contract section 5 is wired and status-checked.
7. Publish revalidates unchanged plan/receipts/files before `gh release create`, distinguishes release-not-found from command failure, rejects extra/missing draft assets, and only then undrafts.
8. Public verification downloads and validates exact inventory and bytes against checksums.

## Artifact index

All artifacts are non-empty under:

`.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a080ec/`

- `00-attempt-status.log` - ULW lookup and evidence-directory selection.
- `01-prepare-build-handoff.log` - real fresh prepare/build failure and byte hashes.
- `02-remote-omaki.log` - exact Linux coordinator CLI failure.
- `02-remote-maho-win.log` - exact Windows coordinator CLI failure.
- `03-preflight-boundaries.log` - POSIX injection and ignored exit/version evidence.
- `04-partial-receipt-overwrite.log` - incomplete receipt and overwrite SHA evidence.
- `05-publish-integrity.log` - stateful process-level draft workflow accepting stale/inexact content.
- `06-source-audit.log` - line-numbered contract/source/test evidence.
- `st_01a080ec-manual-qa.md` - required `manualQa` matrix.
