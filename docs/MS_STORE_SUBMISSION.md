# Microsoft Store Submission Guide for Ferryx

This guide describes how to prepare, build, and submit Ferryx as an MSIX package to the **Microsoft Store** via **Microsoft Partner Center**.

---

## 1. Prerequisites

1. **Microsoft Partner Center Account**: Register a developer account at [partner.microsoft.com](https://partner.microsoft.com/dashboard).
2. **Windows SDK**: Windows 10/11 SDK containing `MakeAppx.exe`, `SignTool.exe`, and `MakePri.exe` (installed automatically on GitHub Actions `windows-latest` runners).
3. **App Name Reservation**: In Partner Center, go to **Apps and games** -> **New product** -> **MSIX or PWA app** and reserve the product name `Ferryx`.

---

## 2. App Identity Configuration

In Microsoft Partner Center under **Product management** -> **Product Identity**, retrieve your store credentials:

- **Package/Identity/Name**: e.g., `YourPublisher.Ferryx`
- **Package/Identity/Publisher**: e.g., `CN=XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX`
- **Package/Properties/PublisherDisplayName**: e.g., `Ferryx Team`

Update `src-tauri/windows/msix/AppxManifest.xml` or pass them as parameters when running the build script:

```xml
<Identity
  Name="<Your-Store-Package-Name>"
  Publisher="CN=<Your-Store-Publisher-ID>"
  Version="0.1.0.0"
  ProcessorArchitecture="x64" />
```

---

## 3. Building the MSIX Package

### Automated via GitHub Actions
When a release is triggered (by pushing a tag `v*` or dispatching the `release.yml` workflow), GitHub Actions automatically compiles and packages the MSIX installer and attaches it to the release artifacts as:
```text
Ferryx_<version>_x64.msix
```

### Local Packaging (PowerShell on Windows)
```powershell
# 1. Build the release executable
bun install
bun run --cwd ui build
cargo build --release --manifest-path src-tauri/Cargo.toml

# 2. Package into MSIX
powershell -ExecutionPolicy Bypass -File scripts/build-msix.ps1 -Version "0.1.0" -OutputDir "dist/msix"
```

---

## 4. Submitting to Microsoft Partner Center

1. Navigate to **Microsoft Partner Center** -> **Apps and games** -> **Ferryx**.
2. Click **Start your submission** (or update an existing submission).
3. **Packages Step**:
   - Drag and drop `Ferryx_<version>_x64.msix` into the package upload area.
   - The Partner Center validator will verify the `AppxManifest.xml`, architecture (`x64`), capabilities (`runFullTrust`), and assets.
4. **App Properties**:
   - Category: `Developer Tools` -> `Development Utilities` / `Productivity`
   - Privacy Policy URL: `https://ferryx.app/privacy`
   - Website URL: `https://ferryx.app`
5. **Age Ratings**:
   - Complete the IARC rating questionnaire (General developer/terminal application).
6. **Store Listings**:
   - Description: Ultra-lightweight workspace & AI agent launcher powered by Tauri v2 and Rust.
   - Feature list, search terms, and release notes.
   - Screenshots: Upload 1920x1080 desktop screenshots of Ferryx running tabs, terminal splits, and AI agent launcher.
7. **Submission Review**:
   - Review all fields and click **Submit to the Store**.
   - Certification typically completes within 24–48 hours. Once approved, Ferryx will be available for download directly in the Windows Store!
