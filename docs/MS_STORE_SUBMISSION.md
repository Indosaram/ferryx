# Microsoft Store Submission Guide for Ferryx

This guide describes how to prepare, build, and submit Ferryx as an MSIX package to the **Microsoft Store** via **Microsoft Partner Center**.

For updates to the existing Ferryx product (`9NLHQL5JNLM4`), first follow
[resubmission verification and recovery](releases/MS_STORE_RESUBMISSION_RECOVERY.md).
Do not infer a first-ever submission from a CLI error or delete a draft before
checking its history and backing up its metadata. Package upload is not proof of
certification submission.

---

## 1. Prerequisites

1. **Microsoft Partner Center Account**: Register a developer account at [partner.microsoft.com](https://partner.microsoft.com/dashboard).
2. **Windows SDK**: Windows 10/11 SDK containing `MakeAppx.exe`, `SignTool.exe`, and `MakePri.exe` on the local Windows build host.
3. **App Name Reservation**: In Partner Center, go to **Apps and games** -> **New product** -> **MSIX or PWA app** and reserve the product name `Ferryx`.

---

## 2. App Identity Configuration

In Microsoft Partner Center under **Product management** -> **Product Identity**, the store credentials for Ferryx are:

- **Package/Identity/Name**: `ProjectMaho.Ferryx`
- **Package Family Name (PFN)**: `ProjectMaho.Ferryx_s4dtschhe0d3e`
- **Package/Identity/Publisher**: `CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36`
- **Package/Properties/PublisherDisplayName**: `Project Maho`

Configured in `src-tauri/windows/msix/AppxManifest.xml` and `scripts/build-msix.ps1`:

```xml
<Identity
  Name="ProjectMaho.Ferryx"
  Publisher="CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36"
  Version="0.1.0.0"
  ProcessorArchitecture="x64" />
```

---

## 3. Building the MSIX Package

### Local release assembly
GitHub Actions does not build or publish releases. Use the [local release runbook](releases/LOCAL_RELEASE_RUNBOOK.md) to build and assemble the Windows Store-ingestion package. Its stable release alias is:
```text
Ferryx_x64.msix
```

### Local Packaging (PowerShell on Windows)
```powershell
# 1. Build the release executable
bun install
bun run --cwd ui build
cargo build --release --manifest-path src-tauri/Cargo.toml

# 2. Package into MSIX
powershell -ExecutionPolicy Bypass -File scripts/build-msix.ps1 -ExePath "src-tauri/target/release/ferryx.exe" -Version "<release-version>" -OutputDir "dist/msix" -SkipSigning
```

---

## 4. Submitting to Microsoft Partner Center

1. Navigate to **Microsoft Partner Center** -> **Apps and games** -> **Ferryx**.
2. Click **Start your submission** (or update an existing submission).
3. **Packages Step**:
   - Drag and drop the assembled `Ferryx_x64.msix` into the package upload area.
   - The Partner Center validator will verify the `AppxManifest.xml`, architecture (`x64`), capabilities (`runFullTrust`), and assets.
4. **App Properties**:
   - Category: `Developer Tools` -> `Development Utilities` / `Productivity`
   - Privacy Policy URL: `https://ferryx.dev/privacy/`
   - Website URL: `https://ferryx.dev/`
5. **Age Ratings**:
   - Complete the IARC rating questionnaire (General developer/terminal application).
6. **Store Listings**:
   - Description: Ultra-lightweight workspace & AI agent launcher powered by Tauri v2 and Rust.
   - Feature list, search terms, and release notes.
   - Screenshots: Upload 1920x1080 desktop screenshots of Ferryx running tabs, terminal splits, and AI agent launcher.
7. **Submission Review**:
   - Review all fields and click **Submit to the Store**.
   - Certification typically completes within 24–48 hours. Once approved, Ferryx will be available for download directly in the Windows Store!
