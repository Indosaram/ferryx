# Resumed DAG verification and pipeline audit completion report

Date: 2026-09-08. Run ID: `dag_22f0b1a8-ff89-4751-8640-3903e9543fd4`. Status: lead-verified audit recovery complete (prior event: 5 completed, 0 failed; final correction is follow-up in same run); implementation pending approval.

## Executive summary

Following the mass-ulw DAG failure (refresh_token_invalidated) and discard of unverified provisional child reports, the interrupted audit was recovered in the same run (run id `dag_22f0b1a8-ff89-4751-8640-3903e9543fd4`, prior 5-completed/0-failed event, noting final correction is follow-up in same run) via mahoquot/gemini-3.8-flash-high through direct source, receipt, and artifact verification. Source findings and main conclusions otherwise already verified by lead. All original DAG scopes were verified directly against source files, live system probes, and repository artifacts. Cited source, workflow, test, script, and evidence files exist on disk. Final audit sign-off remains subject to lead verification.

## DAG scope verdicts: pass and limitation status

- CI policy and release trigger: PASS on vulnerability discovery; LIMITATION on current repository state. .github/workflows/release.yml lines 3-12 retains tag and workflow_dispatch triggers with write permissions; actual workflow still produces hosted releases. Build-test.yml (PR checks) and deploy-pages.yml (docs) are intact.
- Mac coordinator readiness: PASS on prerequisites; LIMITATION on build headroom and execution. Apple Silicon arm64 host has required tools (Bun 1.4.0, Rustc/Cargo 1.92.0, Zig 0.16.0, Tauri CLI 2.10.1, Xcode 26.6) and valid Developer ID Application certificate. Critical limits: disk snapshots (15Gi initial -> 11Gi lead inspection -> 10Gi producer -> 9.0Gi lead check) are time-varying observations, not readiness thresholds; private signing keys, keychain authorization prompts, and notarytool credentials were not exercised; no compilation or codesign run occurred.
- Linux builder (omaki) readiness: PASS on remote reachability; LIMITATION on tool skew and checkout. Host 100.91.254.71 is reachable over SSH with Bun 1.4.0, Rustc 1.98.0, Zig 0.16.0, Tauri CLI 2.11.4, WebKitGTK 4.1, GTK3, ALSA, and 677G disk. Critical limits: Rust toolchain skew (1.98 vs 1.92 Mac); checkout /home/indo/ferryx-src failed HEAD lookup while /home/indo/rel-0905 is at 48825781a; glibc minimum baseline unverified; zero build executions.
- Windows builder (maho-win) readiness: PASS on recovered prerequisites; LIMITATION on execution and source currency. Direct probe via Join-Path corrected earlier backslash-transport corruption. Critical limits: tools were not invoked to build or package; checkout HEAD e2a19066f is stale; no runtime verification.
- Artifact contract and packaging: PASS on audit coverage; LIMITATION on code state. Source audits verified permissive updater manifest generation (scripts/build-latest-json.mjs:37-56, 81-86), non-atomic version stamping (scripts/sync-version.mjs:7, 27-30, 98-103), unverified MSIX packaging and error swallowing (scripts/build-msix.ps1:107-124, 171-200), and unhashed updater manifest in SHA256SUMS.txt (.github/workflows/release.yml:300).
- Tooling test suite: PASS on test execution preservation; PRE-EXISTING 1 FAIL. Verified via existing captured test-evidence.md (node --test runner across all 4 release test scripts, not newly executed by recovery): exactly 27 tests, 26 pass, 1 fail (scripts/release-workflow.test.mjs:56 asserting *-setup.exe against workflow *.exe).

## Recovered evidence and changed conclusions versus main assessment

- Main assessment Section 5 marked maho-win MSVC linker, MakeAppx, and release checkout as unverified due to malformed path probes.
- Recovered direct probe (remote-readiness.md, monitor mon_MC3N0JE1MHN7N17T) definitively proves:
  1. Visual Studio 2022 Build Tools exists at C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools.
  2. MSVC link.exe exists at VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64/link.exe (exit 0).
  3. Windows SDK 10.0.26100.0 provides MakeAppx.exe for x64, x86, and arm64 (exit 0).
  4. Verified git checkout exists at C:/Users/sook/ferryx-winbuild/orca-lite with HEAD e2a19066fe36f126d62ffecc952f3dc0b5f3258a.
- Changed conclusion: Windows tooling prerequisites are confirmed present on maho-win, removing the unverified-tools blocker from the main assessment.

## Preserved boundaries and unresolved limits

- Evidence labeling: Tool versions, keychain identity, and omaki packages are historic lead probe observations (probe-evidence.md). Windows MSVC/SDK/checkout paths are recovered probe observations (remote-readiness.md). Test suite result (26/27 pass) is from existing captured test-evidence.md, not newly executed by recovery; coordinator git HEAD (5d5499806a1b207849778f488b4e3e7b821a751b, verified by lead) is a newly executed check.
- Disk capacity boundary: Mac free disk space was measured at 15Gi, 11Gi, 10Gi, and most recently 9.0Gi by the lead. All disk values are time-varying snapshots, not certified readiness thresholds.
- Read-only boundary: Zero binaries built, packages assembled, tags pushed, releases published, or production files edited.
- Credential search boundary: Developer ID certificate presence does not prove private key usability or notary credentials. Minisign private keys were not touched.
- Host checkouts: Inspected candidate HEADs differ and other checkouts/cleanliness not assessed.

## Concrete next implementation decision

- Decision pending operator approval: Approve transition from read-only audit to local release hardening.
- Concrete next steps once approved:
  1. Retire hosted release jobs from .github/workflows/release.yml while keeping build-test.yml and deploy-pages.yml.
  2. Implement coordinator pipeline script orchestrating prepare, preflight, build, verify, and publish stages with per-run directories and explicit commit pinning.
  3. Harden scripts/build-latest-json.mjs to require full platform inventory and cryptographic minisign validation.
  4. Harden scripts/sync-version.mjs (atomic writes, calendar check) and scripts/build-msix.ps1 ($LASTEXITCODE checks, explicit Store mode).
- Neither hardening nor release execution is claimed as implemented.

## Cleanup receipt

Report-only changes; Windows probe monitor completed without temp script; file watches fired; no build/server/app resources.
