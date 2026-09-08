# Ferryx local release pipeline: assessment and hardening proposal

Date: 2026-09-08. Status: assessment complete; implementation requires approval.

## Conclusion

The three local builders are reachable or locally available, but the repository does not enforce local-only releases. The hosted release workflow is still active, the operational guide tells operators to push a tag to trigger it, and existing artifact scripts do not enforce a complete, same-version, verified release set.

The recommended arrangement is MacBook as coordinator, macOS builder and publisher; omaki as the Linux builder; and maho-win as the Windows builder. GitHub Releases remains the download/update host. GitHub Actions must not build, sign, or publish release artifacts. Keeping ordinary PR checks and Pages deployment is a proposed boundary, not a decision to remove all CI.

This audit did not build, sign, install, launch, upload, or publish a release. Tool presence and passing unit tests are not release-readiness certification.

## Verified findings, in priority order

### 1. Local-only policy currently relies on operator behavior

`.github/workflows/release.yml:3-12` enables CalVer tag pushes and manual dispatch with `contents: write`. The hosted build matrix begins at line 23; a separate hosted MSIX job begins at line 197; the publish job begins at line 258. MSIX does not depend on the desktop build; publishing depends on both.

`docs/CROSS_PLATFORM_RELEASE_GUIDE.md:22-72` explicitly prescribes tag-triggered GitHub Actions releases. Live `gh workflow list --all --repo Indosaram/ferryx` still lists Release Ferryx as active.

Proposed fix: remove the executable release producer from `.github/workflows`, move any useful reference out of that directory, and rewrite the guide around a local entry point. Removing only the tag trigger leaves manual dispatch as a second release producer. Add a parsed workflow-policy check that rejects release build/sign/publish jobs being reintroduced. Disabling the workflow in GitHub settings is supplementary, not source enforcement.

### 2. The manifest generator accepts incomplete and ambiguous inputs

`scripts/build-latest-json.mjs:37-56` scans sorted filenames, skips artifacts without a signature sibling, accepts signature text without verifying it, and overwrites an earlier target entry when another file maps to the same platform. At line 81 it rejects only a completely empty platform set.

Consequences: one missing platform can be silently omitted; stale and fresh files can compete; a claimed version/tag can be unrelated to the payload. A `.sig` filename is not proof of valid signing.

Proposed fix: take an explicit release inventory from the current build run. Require the exact selected platform/channel set, one canonical updater artifact per target, matching source/version receipts, and cryptographic verification against the configured updater public key. Reject duplicates, missing/empty/invalid signatures, stale versions, and unexplained files before producing `latest.json`.

### 3. Broad collection can select an unintended executable or stale output

The workflow recursively collects `*.exe` under `src-tauri/target` at lines 162-180, then aliases `*setup.exe|*.exe` to `Ferryx_x64-setup.exe` at line 291. The copy at line 281 suppresses errors. Existing output directories and broad globs are not a safe inventory contract.

`scripts/build-msix.ps1:107-120` chooses the first existing executable from four candidate locations. Lines 173-176 invoke MakeAppx and check output existence without checking its native exit code; an older output can satisfy that check. SignTool at line 196 likewise has no explicit exit-code gate.

Proposed fix: builders return exact paths and hashes from a run-specific output directory. MSIX accepts the just-built executable explicitly, verifies its version, checks native exit codes, and verifies the resulting package identity/version. Store packaging uses an explicit Store mode (`-SkipSigning` in the current script), not accidental fallback from failed sideload signing.

### 4. Versions and release channels need one authoritative contract

`scripts/sync-version.mjs:25-38` maps a date tag such as `v2026.09.08.1` to application version `2026.908.1`. It rewrites Tauri config and then Cargo separately at lines 99-100; a failure on the second file can leave partial stamping. Date-shaped input is not full calendar-date validation.

The current guide also presents raw date-to-MSIX examples, while the release workflow passes the already-normalized application version. Define one mapping: tag `v2026.09.08.1`, app/updater `2026.908.1`, MSIX `2026.908.1.0`. Validate all three before writing, stamp only the isolated build source, and verify package metadata afterward.

`site/src/lib/downloads.ts:22-24,54-77` makes Microsoft Store the Windows download channel, with an interim search URL rather than a confirmed ProductId URL. Historical `docs/releases/v2026.09.06.1.md` records the final NSIS migration release. Live latest-release metadata still lists both NSIS and MSIX, and no MSI.

Recommendation: keep Store as the primary Windows channel; make NSIS migration a separate, explicit channel policy. Do not silently remove the legacy updater entry or turn NSIS back into the primary download. Preserve existing updater key continuity; local-only migration does not justify rotating the updater key.

### 5. Local machine prerequisites exist, but readiness is conditional

- **MacBook:** arm64; Bun 1.4.0; Node 22.22.3; Cargo/rustc 1.92.0; Zig 0.16.0; Tauri CLI 2.10.1; Xcode 26.6. Both Darwin Rust targets are installed. The required `Developer ID Application: Indo Yoon (5DUM8WPB4C)` identity is valid. Free disk changed from 15Gi to 11Gi during inspection: this is a substantial build-space risk, not a measured minimum requirement. Private-key usability and notarization credentials were not exercised.
- **omaki:** SSH to `indo@100.91.254.71` works. Linux x86_64; Bun 1.4.0; Cargo/rustc 1.98.0; Zig 0.16.0; Tauri CLI 2.11.4. pkg-config resolves WebKitGTK 4.1, GTK 3 and ALSA. `/home` has 677G available. `/home/indo/rel-0905` resolves to `48825781aeb071a47c99bd89078520cfbe1f1413`; `/home/indo/ferryx-src` did not resolve HEAD. Neither is established as the source for the next release.
- **maho-win:** SSH works; Windows build 26200, host `DESKTOP-1LAPJMP`; Bun 1.4.0; Cargo/rustc 1.97.0; Zig 0.16.0; Tauri CLI 2.11.4. C: reported 348,901,322,752 free bytes. Several Ferryx directories exist under the user profile. The MSVC/SDK path probes were malformed in transport, so their negative results are discarded. **MSVC linker, MakeAppx, and the exact release checkout remain unverified, not proven missing.**

Toolchain skew is observed. Select and pin a tested release toolchain rather than assuming these different versions are interchangeable. Successful packaging on current Arch does not establish compatibility with older Linux distributions; the proposal needs runtime/dependency verification on the supported baseline.

## Proposed operating contract

All items below are proposed, not implemented.

1. **Prepare:** resolve an explicitly chosen commit and tag; record source SHA, Ghostty pin, toolchain versions, lockfile hashes, application/MSIX versions and requested channels in a release plan. Use dedicated per-run source/output directories on persistent disks. Never reset an existing shared checkout or select whatever HEAD a remote machine currently has.
2. **Preflight:** verify host identity/access, tool executables and versions, MSVC/SDK discovery through valid Windows paths, disk budget, signing capability without logging secret values, and source/lockfile identity. Fail on a mismatch. Do not fall back from frozen dependency installation to a mutable install.
3. **Build:** run three platform builders against the same source identity. Mac builds universal bundles; omaki builds AppImage and DEB; Windows builds the executable and Store MSIX, with NSIS only when the migration policy requests it. Pin Linux packaging-tool inputs; do not reuse a stale AppDir or silently change packaging routes.
4. **Verify on each builder:** write a receipt containing source/version, command result, toolchain, exact filenames, sizes and SHA256. Check universal slices, Developer ID signature, notarization/stapling and updater archive layout on Mac; package/dependency and AppImage checks on Linux; executable/MSIX identity and native-tool success on Windows. Keep build success separate from runtime proof.
5. **Collect:** copy only receipt-listed artifacts to the coordinator. Recompute hashes after transfer. Require the selected matrix and reject a receipt from another SHA/tag/run.
6. **Sign and assemble:** preserve the existing updater key; verify each signature against the configured public key. Produce stable aliases from an explicit mapping. Generate the validated `latest.json`, then `SHA256SUMS.txt` over the complete publish set, excluding the checksum file itself. The current workflow computes checksums before creating the manifest, so it does not cover that manifest.
7. **Publish:** require explicit approval; confirm the tag resolves to the recorded SHA. Create/upload a draft, verify its inventory, then publish. Do not use blind `--clobber`, overwrite existing immutable version assets, or claim upload completeness from a partially successful command. External GitHub changes and Store submission remain explicit actions.
8. **Post-publish:** fetch the published aliases and manifest, check HTTP status, exact bytes/hash/signatures and platform URLs. Store submission has its own receipt; an MSIX attached to GitHub is not a Store listing.
9. **Preserve runtime:** no build step replaces the installed app or signals the existing PTY daemon. Any runtime validation must isolate its data/socket state and prove cleanup. Package installation or desktop acceptance is a separate step, not an incidental build side effect.

Suggested implementation surface: one local coordinator entry point with `prepare`, `preflight`, `build`, `verify`, and explicit `publish` stages; thin platform adapters; existing version/manifest/MSIX helpers hardened rather than duplicated; one canonical runbook linked from `AGENTS.md` and the current release guide. Exact names should be finalized in implementation, not treated as already available commands.

## Acceptance scenarios for the implementation

- Local-only policy: parse all workflow definitions; tag/manual release producers fail the policy check. PR checks and Pages remain functional. Removing a release producer must not disable those unrelated workflows.
- Source identity: feed the collector three receipts, one with a different SHA or version; it must exit nonzero without a publishable manifest. With one matching receipt per required platform it must succeed.
- Artifact completeness: remove a required `.sig`, replace its content with invalid text, or add a second artifact for the same target; each case must fail before publish. Real signed artifacts must pass verification.
- MSIX: point the packager at a stale binary or simulate MakeAppx returning nonzero while an old `.msix` exists; it must fail. A package from the exact fresh executable must expose the expected identity/version.
- Version validation: invalid calendar tags and out-of-range MSIX parts must fail without changing either input manifest; a valid tag must yield consistent app/updater/MSIX versions.
- Real build: each local builder must produce and verify its assigned packages from the same commit, with logs and receipts. Mac signing/notarization must pass; Windows SDK/linker must be exercised; Linux must be checked against the supported runtime baseline.
- Publish: after approval, verify the draft inventory before making it public, then download aliases and updater payloads and compare bytes/signatures. A missing alias or unexpected target fails acceptance.

These scenarios require failing-first regression evidence for behavior changes, then real builder/CLI/download evidence. A dry run alone is not acceptance.

## Evidence and limitations

- [Existing test evidence](local-pipeline-audit-2026-09-08/test-evidence.md): exact command, 27 tests, 26 pass, 1 pre-existing workflow-pattern failure; exit 1. No tests were weakened or changed.
- [Direct probe evidence](local-pipeline-audit-2026-09-08/probe-evidence.md): commands, observed machine results, GitHub output, malformed-probe exclusions, timeouts and cleanup.
- Source inspection covered the named workflow, version, manifest, archive and MSIX scripts, current download channels, release guide and release history. Report findings are source-backed; no current binary is certified by this audit.
- The mass-ulw DAG settled with one completed node, three failed nodes and one skipped node. Authentication failed with `refresh_token_invalidated`. Its provisional reports contained unsupported claims and were discarded; the lead rechecked the findings directly. The DAG is not reported as successful.
- Markdown diagnostics could not be obtained because the LSP daemon was unreachable. This is prose-only work, reviewed by reading; no code build is warranted for these report files.

## Decision requested

Approve implementation of this local-only contract, retaining GitHub PR checks/Pages and keeping Windows Store primary with an explicit legacy NSIS migration policy? Actual release publication and Store submission would still require a separate approval.
