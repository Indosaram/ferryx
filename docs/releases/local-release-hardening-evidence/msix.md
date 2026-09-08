# Ferryx Local-Release Hardening Evidence: Windows MSIX Packaging

**Date:** 2026-09-08
**Scope:** Production hardening and verification proof for `scripts/build-msix.ps1`, `scripts/build-msix.test.mjs`, and `scripts/test-build-msix.ps1`.
**Host Environment:** Remote Windows builder `maho-win` (Windows NT 10.0.26200.0, PowerShell 5.1 & PowerShell 7.6.5, Windows SDK MakeAppx 10.0.26100.0) and macOS local coordinator.

---

## 1. Executive Summary

The Windows MSIX packaging script (`scripts/build-msix.ps1`) has been hardened to eliminate candidate guessing, enforce Microsoft Store Quad version constraints, verify binary version identity against expected release versions, check native tool exit codes, eliminate hardcoded credentials/auto-generated certificates, enforce isolated fresh staging, and validate packaged manifest metadata directly from the resulting `.msix` zip archive.

Deterministic node test suites (`scripts/build-msix.test.mjs`) and native PowerShell test fixtures (`scripts/test-build-msix.ps1`) were written and verified via TDD: observing full RED regression failures against the legacy script, followed by clean GREEN validation across all 11 static contract tests and 10 remote Windows behavior tests.

---

## 2. Hardened Architecture & Security Controls

### 2.1 Explicit `-ExePath` Binding (No Candidate Guessing)
- The `-ExePath` parameter is declared as `[Parameter(Mandatory = $true)] [string]$ExePath`.
- All candidate array guessing (`$binaryCandidates` searching `src-tauri/target/release/ferryx.exe`, etc.) was completely deleted.
- Fails closed immediately if the path is empty, non-existent, or not a leaf file.

### 2.2 Canonical Version Normalization & Store Quad Contract
- Accepts release tags (`vYYYY.MM.DD[.R]`), SemVer (`Major.Minor.Patch`), or Quad (`Major.Minor.Patch.0`).
- Validates calendar dates: year >= 2026, month 1..12, valid days per month accounting for leap years (`[DateTime]::DaysInMonth`).
- Monotonically maps date tags to App SemVer (`YYYY.(MM * 100 + DD).R`) and MSIX Quad (`YYYY.(MM * 100 + DD).R.0`).
- **Store Quad Rule:** Explicitly rejects any 4-part quad version where the 4th component is non-zero (e.g., `2026.908.1.5` or `1.0.0.1`), throwing:
  `MSIX Store packages require the 4th quad component to be 0 for Store ingestion (got revision <rev> in '<version>').`

### 2.3 Binary Version Verification
- Reads `[System.Diagnostics.FileVersionInfo]::GetVersionInfo($resolvedExePath)`.
- Verifies that binary `ProductVersion`, `FileVersion`, or composite parts (`ProductMajorPart.ProductMinorPart.ProductBuildPart`) match the expected app version or MSIX quad version.
- Rejects binaries with version mismatch or missing version info, preventing accidental packaging of wrong or debug binaries.

### 2.4 Isolated Fresh Staging & Stale File Removal
- Creates a dedicated unique temporary layout directory per packaging run using `Join-Path ([System.IO.Path]::GetTempPath()) ("ferryx-msix-staging-" + [System.Guid]::NewGuid().ToString("N"))`.
- Guarantees staging directory removal in a `finally` block.
- Pre-existing `.msix` files at the destination path are actively unlinked before `MakeAppx` executes, eliminating false successes from stale artifacts.

### 2.5 Native Tool Exit Code Checking
- Immediately checks `$LASTEXITCODE -ne 0` following `MakeAppx.exe pack` and `SignTool.exe sign`.
- Throws fatal exceptions with the exact exit code if native Windows tools exit non-zero.

### 2.6 Credential & Certificate Security
- **Eliminated:** `New-SelfSignedCertificate` auto-generation.
- **Eliminated:** Hardcoded PFX password `FerryxMsixSignPass2026!`.
- **Eliminated:** Permissive `try/catch` block that logged a warning and silently continued with an unsigned package when signing was requested.
- **Store Mode:** Explicit `-SkipSigning` switch marks package for unsigned Microsoft Store submission.
- **Sideload Mode:** When `-SkipSigning` is not passed, requires explicit `-CertThumbprint <thumbprint>`. Verifies existence of the certificate in `Cert:\CurrentUser\My` or `Cert:\LocalMachine\My`; fails immediately if not found.

### 2.7 Packaged Manifest Verification
- Uses `[System.IO.Compression.ZipFile]` to inspect `AppxManifest.xml` directly within the produced `.msix` archive.
- Confirms `Identity.Name == $PackageName` and `Identity.Version == $msixVersion`.

---

## 3. Public API Contract (`scripts/build-msix.ps1`)

```powershell
param (
    [Parameter(Mandatory = $true)]
    [string]$ExePath,

    [Parameter(Mandatory = $true)]
    [string]$Version,

    [string]$Publisher = "CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36",
    [string]$PackageName = "ProjectMaho.Ferryx",
    [string]$OutputDir = "dist/msix",
    [switch]$SkipSigning = $false,
    [string]$CertThumbprint = $null,
    [string]$ManifestTemplate = $null,
    [string]$IconsDir = $null
)
```

### Standard Store Invocation:
```powershell
.\scripts\build-msix.ps1 -ExePath "target\release\ferryx.exe" -Version "v2026.09.08.1" -OutputDir "dist\msix" -SkipSigning
```

### Standard Sideload Invocation:
```powershell
.\scripts\build-msix.ps1 -ExePath "target\release\ferryx.exe" -Version "2026.908.1" -OutputDir "dist\msix" -CertThumbprint "0123456789ABCDEF0123456789ABCDEF01234567"
```

---

## 4. TDD Verification Evidence

### 4.1 Node Test Runner Static Contract (`scripts/build-msix.test.mjs`)
- **RED Baseline:** 8 failed, 3 passed (prior to hardening).
- **GREEN Verification:** 11 passed, 0 failed.
- **Command:** `node --test scripts/build-msix.test.mjs`
```
TAP version 13
# Subtest: build-msix.ps1: requires explicit -ExePath parameter and removes candidate guessing
ok 1 - build-msix.ps1: requires explicit -ExePath parameter and removes candidate guessing
# Subtest: build-msix.ps1: enforces canonical Store quad version contract (4th component = 0)
ok 2 - build-msix.ps1: enforces canonical Store quad version contract (4th component = 0)
# Subtest: build-msix.ps1: verifies binary version against expected app version
ok 3 - build-msix.ps1: verifies binary version against expected app version
# Subtest: build-msix.ps1: enforces isolated fresh staging and removes stale output before packing
ok 4 - build-msix.ps1: enforces isolated fresh staging and removes stale output before packing
# Subtest: build-msix.ps1: checks native tool exit codes after MakeAppx and SignTool
ok 5 - build-msix.ps1: checks native tool exit codes after MakeAppx and SignTool
# Subtest: build-msix.ps1: removes auto-generated cert, hardcoded password, and silent fallback
ok 6 - build-msix.ps1: removes auto-generated cert, hardcoded password, and silent fallback
# Subtest: build-msix.ps1: supports explicit -SkipSigning and requires valid -CertThumbprint for sideload
ok 7 - build-msix.ps1: supports explicit -SkipSigning and requires valid -CertThumbprint for sideload
# Subtest: build-msix.ps1: validates packaged MSIX manifest version and name after pack
ok 8 - build-msix.ps1: validates packaged MSIX manifest version and name after pack
# Subtest: version contract: maps date tags, semver, and quad versions correctly
ok 9 - version contract: maps date tags, semver, and quad versions correctly
# Subtest: version contract: rejects non-zero 4th quad component for Store submission
ok 10 - version contract: rejects non-zero 4th quad component for Store submission
# Subtest: version contract: rejects invalid calendar dates
ok 11 - version contract: rejects invalid calendar dates
1..11
# tests 11
# pass 11
# fail 0
```

### 4.2 Remote Windows Behavior Suite (`scripts/test-build-msix.ps1` on `maho-win`)
- **Execution surface:** Remotely executed on `maho-win` via OpenSSH within dedicated isolated scratch directory `C:\Users\sook\AppData\Local\Temp\ferryx-test-<guid>`. Tiny C# native executables compiled dynamically with `csc.exe` to supply real PE version resources without building the full app.
- **RED Baseline:** 9 failed, 1 passed (unhardened script failed to require `-ExePath`, accepted invalid quad revisions, ignored binary versions, left stale output intact, lacked thumbprint verification, and failed MakeAppx).
- **GREEN Verification:** 10 passed, 0 failed.
- **Command:** `ssh maho-win powershell -NoProfile -File C:\Users\sook\AppData\Local\Temp\ferryx-test-<guid>\test-build-msix.ps1`
```
--- Test 1: Missing mandatory -ExePath parameter fails ---
RESULT: PASS

--- Test 2: Non-existent -ExePath fails ---
RESULT: PASS

--- Test 3: Version quad with non-zero 4th component is rejected ---
RESULT: PASS

--- Test 4: Invalid calendar date tag is rejected ---
RESULT: PASS

--- Test 5: Binary version mismatch against expected app version fails ---
RESULT: PASS

--- Test 6: Stale output file is cleaned before packaging ---
RESULT: PASS

--- Test 7: Omitted -SkipSigning without -CertThumbprint fails ---
RESULT: PASS

--- Test 8: Unavailable certificate thumbprint fails ---
RESULT: PASS

--- Test 9: MakeAppx failure exit code is detected and reported ---
RESULT: PASS

--- Test 10: Small valid fixture packs MSIX and validates packaged manifest ---
[1/6] App Version: 2026.908.1, MSIX Quad Version: 2026.908.1.0
[2/6] Binary version verified successfully against 2026.908.1
[3/6] Found Windows SDK MakeAppx: C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\makeappx.exe
Store packaging mode (-SkipSigning): package created unsigned for Microsoft Store ingestion.
[4/6] Generated AppxManifest.xml with version 2026.908.1.0
[5/6] Packing MSIX package to: ...\pack_out\Ferryx_2026.908.1_x64.msix
Microsoft (R) MakeAppx Tool
Package creation succeeded.
[6/6] Validating packaged MSIX manifest from archive...
Packaged manifest verified: Name='ProjectMaho.Ferryx', Version='2026.908.1.0'
=======================================================
 MSIX BUILD SUCCESSFUL: ...\pack_out\Ferryx_2026.908.1_x64.msix
=======================================================
MSIX successfully validated: ProjectMaho.Ferryx 2026.908.1.0
RESULT: PASS

=======================================================
Test Run Summary: Total=10, Passed=10, Failed=0
=======================================================
All tests passed!
```

### 4.3 Scratch & Resource Cleanup Verification
- The remote test execution script deleted its internal scratch directory `ferryx-test-scratch-<guid>` in a `finally` block.
- The remote runner cleaned up the SSH test directory `ferryx-test-<guid>` on completion.
- Remote directory audit of `$env:TEMP\ferryx*` confirmed zero residual test folders or temporary files.
