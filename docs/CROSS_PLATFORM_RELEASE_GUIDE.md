# Ferryx Cross-Platform Release & Distribution Guide

This document provides a comprehensive operational guide for building, packaging, verifying, and distributing **Ferryx** across macOS, Windows, and Linux platforms.

---

## 1. Overview of Release Targets

Ferryx is built on **Tauri v2** with a Rust backend and React/Vite/Bun frontend. The multi-platform release pipeline produces the following production artifacts:

| Platform | Format / Architecture | Bundler Target | Distribution Channel |
| :--- | :--- | :--- | :--- |
| **macOS** | `.dmg` (Universal / Apple Silicon / Intel) | `bundle.targets = ["dmg", "app"]` | GitHub Releases & Direct Download |
| **Windows** | `.exe` (x64 NSIS Installer) | `bundle.targets = ["nsis"]` | GitHub Releases & Direct Download |
| **Windows** | `.msi` (x64 WiX Installer) | `bundle.targets = ["msi"]` | Enterprise & Direct Download |
| **Windows** | `.msix` (x64 Windows App Package) | `scripts/build-msix.ps1` | Microsoft Store & Enterprise Sideload |
| **Linux** | `.AppImage` (x86_64 Portable) | `bundle.targets = ["appimage"]` | GitHub Releases & Direct Download |
| **Linux** | `.deb` (x86_64 Debian/Ubuntu) | `bundle.targets = ["deb"]` | Debian / Ubuntu Package Repos & Direct |

---

## 2. GitHub Actions Release Triggers

The automated release pipeline is defined in `.github/workflows/release.yml`. It can be triggered via two methods:

### Method A: Git Tag Push (Recommended for Official Releases)

Creating and pushing a version tag matching `v*` automatically triggers the complete build, packaging, checksumming, and release publication workflow.

```bash
# 1. Ensure working directory is clean and tests pass
bun run --cwd ui build
bun test --cwd site
cargo check --manifest-path src-tauri/Cargo.toml

# 2. Create an annotated semantic version tag
git tag -a v0.1.0 -m "Release v0.1.0"

# 3. Push the tag to GitHub
git push origin v0.1.0
```

### Method B: Manual Dispatch (`workflow_dispatch`)

For release dry-runs, draft releases, or pre-releases, use the GitHub Actions manual dispatch interface:

#### Via GitHub Web UI:
1. Navigate to repository **Actions** tab.
2. Select **Release Ferryx** workflow in the left sidebar.
3. Click **Run workflow**.
4. Configure inputs:
   - `release_tag`: Target release tag (e.g. `v0.1.0`; leave blank to use `v<package.json version>`).
   - `draft`: Check to publish as a private Draft Release for validation.
   - `prerelease`: Check to mark as a Pre-release (e.g. for alpha/beta milestones).

#### Via GitHub CLI (`gh`):
```bash
# Trigger a standard release
gh workflow run release.yml -f release_tag=v0.1.0 -f draft=false -f prerelease=false

# Trigger a draft release for manual QA verification
gh workflow run release.yml -f release_tag=v0.1.0-alpha -f draft=true -f prerelease=true
```

### CI Workflow Architecture

```text
[Push Tag: v* / workflow_dispatch]
        │
        ├───► [Job: build-desktop (macos-latest)] ──► .dmg / .app.tar.gz
        ├───► [Job: build-desktop (windows-latest)] ─► .exe (NSIS) / .msi (WiX)
        ├───► [Job: build-desktop (ubuntu-latest)] ──► .AppImage / .deb
        └───► [Job: build-msix (windows-latest)] ────► .msix (Store package)
                    │
                    ▼ (All builds succeed)
        [Job: publish-release (ubuntu-latest)]
              ├── Aggregate all platform artifacts
              ├── Generate SHA256SUMS.txt
              └── Publish GitHub Release with Release Notes
```

### Repository Secrets Required for Release

To enable code signing and notarization, configure the following secrets in **Settings -> Secrets and variables -> Actions**:

- `GITHUB_TOKEN`: Provided automatically by GitHub Actions with `contents: write` permissions.
- **Apple Developer ID (macOS Signing & Notarization)**:
  - `APPLE_CERTIFICATE`: Base64-encoded Developer ID Application `.p12` certificate.
  - `APPLE_CERTIFICATE_PASSWORD`: Password for the `.p12` certificate.
  - `APPLE_SIGNING_IDENTITY`: Common Name on the certificate (e.g. `Developer ID Application: Your Name (TEAM_ID)`).
  - `APPLE_ID`: Apple ID email address used for notarization.
  - `APPLE_PASSWORD`: App-specific password generated on appleid.apple.com.
  - `APPLE_TEAM_ID`: 10-character Apple Developer Team ID.

---

## 3. Windows MSIX & Microsoft Store Submission

Ferryx provides an automated PowerShell tool `scripts/build-msix.ps1` and configuration template `src-tauri/windows/msix/AppxManifest.xml` for generating MSIX packages ready for the Microsoft Store.

### Prerequisites

1. **Microsoft Partner Center Account**: Register a developer account at [partner.microsoft.com](https://partner.microsoft.com/dashboard).
2. **App Name Reservation**: In Partner Center dashboard, navigate to **Apps and games** -> **New product** -> **MSIX or PWA app** and reserve `Ferryx`.
3. **Product Identity Retrieval**: Navigate to **Product management** -> **Product Identity** to obtain:
   - **Package/Identity/Name** (e.g., `Indosaram.Ferryx`)
   - **Package/Identity/Publisher** (e.g., `CN=12345678-ABCD-EF01-2345-6789ABCDEF01`)
   - **Package/Properties/PublisherDisplayName** (e.g., `Indosaram`)

### Step-by-Step Packaging Procedure

#### 1. Configure Package Identity
Update `src-tauri/windows/msix/AppxManifest.xml` with your Partner Center identity, or supply them as arguments to `build-msix.ps1`:

```xml
<Identity
  Name="Indosaram.Ferryx"
  Publisher="CN=12345678-ABCD-EF01-2345-6789ABCDEF01"
  Version="0.1.0.0"
  ProcessorArchitecture="x64" />
```

#### 2. Build Release Executable & Package MSIX
Run PowerShell on Windows (or let GitHub Actions `build-msix` job execute it):

```powershell
# Compile frontend UI and release Rust binary
bun install --cwd ui
bun run --cwd ui build
cargo build --release --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc

# Package MSIX
powershell -ExecutionPolicy Bypass -File scripts/build-msix.ps1 -Version "0.1.0" -Publisher "CN=12345678-ABCD-EF01-2345-6789ABCDEF01" -PackageName "Indosaram.Ferryx" -OutputDir "dist/msix"
```

The script performs the following tasks:
- Normalizes version into Quad-dotted MSIX format (`0.1.0.0`).
- Discovers Windows 10/11 SDK tools (`MakeAppx.exe`, `SignTool.exe`).
- Assembles layout directory with `ferryx.exe`, store assets, and `AppxManifest.xml`.
- Packages `dist/msix/Ferryx_0.1.0_x64.msix`.
- Signs with self-signed certificate for local sideload testing (or leaves unsigned for Store ingestion).

#### 3. Upload to Microsoft Partner Center

1. Sign in to [Microsoft Partner Center](https://partner.microsoft.com/dashboard).
2. Go to **Apps and games** -> **Ferryx** -> **Start your submission**.
3. **Packages Step**:
   - Drag and drop `Ferryx_<version>_x64.msix`.
   - The Store validation engine will verify `runFullTrust` capability, `x64` architecture, and image assets.
4. **Properties**:
   - Category: `Developer Tools` -> `Development Utilities`
   - Privacy Policy URL: `https://ferryx.app`
   - Support Contact: `https://github.com/Indosaram/ferryx/issues`
5. **Age Ratings**:
   - Complete the International Age Rating Coalition (IARC) questionnaire for developer tools (typically rated All Ages / 3+).
6. **Store Listing**:
   - Title: `Ferryx`
   - Short Description: `Ultra-lightweight workspace & AI agent launcher powered by Tauri v2 and Rust`
   - Screenshots: Provide 1920x1080 desktop screenshots demonstrating multi-tab workspace, split-pane terminal, and AI companion.
7. **Review & Submit**:
   - Click **Submit to the Store**. Certification generally finishes within 24 to 48 hours.

---

## 4. Download URL Structure and Asset Naming Conventions

All direct installer endpoints follow a uniform GitHub Releases naming convention managed by `site/src/lib/downloads.ts`.

### Endpoint URL Structure

- **Latest Release Landing Page**: `https://github.com/Indosaram/ferryx/releases/latest`
- **Direct Asset Download Base**: `https://github.com/Indosaram/ferryx/releases/latest/download/<AssetFileName>`

### Asset Naming Specifications

| Platform | Target Package | Asset File Name | Architecture |
| :--- | :--- | :--- | :--- |
| **macOS** | Apple Silicon DMG | `Ferryx_aarch64.dmg` | `aarch64` (Apple Silicon M1/M2/M3/M4) |
| **macOS** | Intel 64-bit DMG | `Ferryx_x64.dmg` | `x86_64` (Intel Macs) |
| **macOS** | Universal Binary DMG | `Ferryx_universal.dmg` | `universal-apple-darwin` |
| **Windows** | Standalone Setup | `Ferryx_x64-setup.exe` | `x86_64` (NSIS Installer) |
| **Windows** | Windows MSI | `Ferryx_x64_en-US.msi` | `x86_64` (WiX Installer) |
| **Windows** | MSIX Package | `Ferryx_x64.msix` | `x86_64` (Store / Sideload) |
| **Windows** | Microsoft Store | `https://apps.microsoft.com/detail/ferryx` | Universal Windows Store link |
| **Linux** | Portable AppImage | `Ferryx_amd64.AppImage` | `x86_64` (Portable Binary) |
| **Linux** | Debian / Ubuntu DEB | `Ferryx_amd64.deb` | `amd64` (`.deb` Package) |
| **All** | SHA256 Checksums | `SHA256SUMS.txt` | Text file of SHA256 digests |

### Checksum Verification

Every GitHub Release includes `SHA256SUMS.txt` generated during the publication job. Users can verify downloaded packages using:

```bash
# Verify integrity of downloaded files
sha256sum -c SHA256SUMS.txt
```

---

## 5. Landing Page (`site`) Verification & Architecture

The landing page located in `site/` provides:
- Automatic operating system detection via `detectUserPlatform()` in `site/src/lib/downloads.ts`.
- Adaptive hero download dropdown (`DownloadMenu.tsx`) pointing directly to the detected platform's recommended binary.
- Complete multi-platform grid in `CTA.tsx` and navbar quick-download buttons.
- Full TypeScript test coverage in `site/src/lib/downloads.test.ts`.

### Verification Commands

```bash
# Run unit tests for download URLs and platform detection
bun test --cwd site

# Run static site production build
bun run --cwd site build
```

---

## 6. End-to-End Release Checklist

Before releasing a new version of Ferryx:

- [ ] **1. Bump Version**: Update version in `package.json`, `ui/package.json`, `site/package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
- [ ] **2. Verify Rust Backend**: Run `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1`.
- [ ] **3. Verify Frontend UI**: Run `bun run --cwd ui build` and `bun test --cwd ui`.
- [ ] **4. Verify Landing Page**: Run `bun test --cwd site` and `bun run --cwd site build`.
- [ ] **5. Validate GitHub Workflows**: Ensure `.github/workflows/release.yml` and `build-test.yml` syntax are valid.
- [ ] **6. Tag & Push**: Execute `git tag -a v<version> -m "Release v<version>"` and `git push origin v<version>`.
- [ ] **7. Verify Release Artifacts**: Confirm all `.dmg`, `.exe`, `.msi`, `.msix`, `.AppImage`, `.deb`, and `SHA256SUMS.txt` are published on GitHub Releases.
- [ ] **8. Submit to Microsoft Store**: Upload the generated `Ferryx_<version>_x64.msix` to Microsoft Partner Center.
