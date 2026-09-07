### Windows Window Chrome Missing Minimize, Maximize, and Close Buttons
- **ID**: L6-PACKAGING-1
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/tauri.windows.conf.json:14` - `        "titleBarStyle": "Overlay",`
- **Why it breaks**: The Windows configuration override retains `titleBarStyle: "Overlay"` and `hiddenTitle: true` from the base macOS configuration, suppressing native Windows caption controls (minimize, maximize, close). Because Ferryx's web frontend does not implement custom Windows caption controls, Windows users cannot minimize, maximize, or close the application window via chrome controls.
- **Fix**: In `src-tauri/tauri.windows.conf.json`, update `app.windows[0]` to set `"titleBarStyle": "Visible"` and `"hiddenTitle": false` to restore native Windows caption buttons matching `tauri.linux.conf.json`.
- **Status**: OPEN

### CLI Launcher Installation Symlink Logic Excludes Windows
- **ID**: L6-PACKAGING-2
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/cli_install.rs:205` - `    #[cfg(unix)]`
- **Why it breaks**: CLI launcher installation is gated behind `#[cfg(unix)]` and `resolve_launcher_status` returns `is_supported: false` on non-Unix platforms. On Windows, standard unprivileged symlinks require Developer Mode, and `install_launcher` creates no launcher file, leaving Windows users completely unable to install or invoke the `ferryx` CLI tool.
- **Fix**: In `src-tauri/src/ipc/cli_install.rs`, add a `#[cfg(windows)]` implementation to `install_launcher` that writes a batch wrapper shim `ferryx.cmd` (`@"%~dp0...\ferryx.exe" %*`) into `%LOCALAPPDATA%\Microsoft\WindowsApps` or a PATH directory, and update `resolve_launcher_status` to report `is_supported = true` on Windows.
- **Status**: OPEN

### Daemon Autostart Persistence Missing Implementation for Windows and Linux
- **ID**: L6-PACKAGING-3
- **Severity**: BLOCKER
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/daemon/launchd.rs:17` - `pub fn get_launchd_plist_path() -> Option<PathBuf> {`
- **Why it breaks**: Background daemon autostart is implemented exclusively for macOS `launchd`, with `get_launchd_plist_path()` returning `None` on all non-macOS platforms. Calling `install_launchd_agent()` on Windows or Linux immediately aborts with `"Cannot determine HOME directory"` and configures no persistent startup mechanism.
- **Fix**: Implement platform-native autostart providers: on Windows, configure a Scheduled Task via `schtasks` or write to `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; on Linux, generate and enable a `systemd --user` unit (`~/.config/systemd/user/ferryx.service`).
- **Status**: OPEN

### Shipped Debian and MSI Packages Produce No Updater Manifest Artifacts
- **ID**: L6-PACKAGING-4
- **Severity**: BLOCKER
- **Platforms affected**: Windows+Linux
- **Evidence**: `scripts/build-latest-json.mjs:5` - `const UPDATER_ARTIFACT = /(\.app\.tar\.gz|\.nsis\.zip|-setup\.exe|\.AppImage(?:\.tar\.gz)?)$/;`
- **Why it breaks**: `build-latest-json.mjs` only accepts `.app.tar.gz`, `.nsis.zip`, `-setup.exe`, and `.AppImage`, omitting `.deb` and `.msi`. The project distributes `Ferryx_amd64.deb` in GitHub releases and targets `msi` in `tauri.conf.json:54`, but users installing via these packages receive no updater payload in `latest.json`, permanently disabling in-app updates.
- **Fix**: Either remove `msi` and `deb` from `tauri.conf.json` bundle targets and distribute only `nsis` and `AppImage`, or disable in-app updater polling when running inside a package-managed (`.deb` or `.msi`) installation.
- **Status**: OPEN

### CLI Launcher Home Directory Lookup Relies on POSIX HOME Environment Variable
- **ID**: L6-PACKAGING-5
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/cli_install.rs:56` - `    let home = std::env::var_os("HOME");`
- **Why it breaks**: `get_default_launcher_path` retrieves the user home directory by querying `HOME`, which is standard on Unix but typically unset on native Windows environments where `USERPROFILE` or `LOCALAPPDATA` is used. On Windows, resolving launcher status immediately returns `CliInstallError::HomeDirNotFound`.
- **Fix**: In `src-tauri/src/ipc/cli_install.rs`, update `get_default_launcher_path` to query `std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))` or use `tauri::path::PathResolver::home_dir()`.
- **Status**: OPEN

### Updater Architecture Mapping Drops ARM64 for Windows and Linux
- **ID**: L6-PACKAGING-6
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `scripts/build-latest-json.mjs:29` - `    return ["windows-x86_64"];`
- **Why it breaks**: `targetsFor` in `scripts/build-latest-json.mjs` inspects filename architecture tokens for macOS (`darwin-aarch64` vs `darwin-x86_64`), but hardcodes static returns of `windows-x86_64` and `linux-x86_64` for all Windows and Linux artifacts. Any ARM64 Windows (`windows-aarch64`) or ARM64 Linux (`linux-aarch64`) updater packages are misclassified as x86_64 or omitted.
- **Fix**: In `scripts/build-latest-json.mjs`, update `targetsFor` to inspect architecture substrings (`aarch64|arm64`) for Windows and Linux artifacts and map them to `windows-aarch64` and `linux-aarch64`.
- **Status**: OPEN

### Debian Package Depends Omits ALSA Library Required by Sound Runtime
- **ID**: L6-PACKAGING-7
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/tauri.conf.json:92` - `          "libwebkit2gtk-4.1-0 | libwebkit2gtk-4.0-37",`
- **Why it breaks**: The Debian package configuration lists WebKitGTK, GTK3, and AppIndicator, but omits ALSA (`libasound2` or `libasound2t64`). Ferryx depends on `rodio` (`src-tauri/Cargo.toml:81`) for notification audio playback (`src-tauri/src/ipc/notifications.rs`), which dynamically binds to ALSA on Linux; omitting it causes audio initialization crashes on minimal installations.
- **Fix**: Add `"libasound2 | libasound2t64"` to the `bundle.linux.deb.depends` array in `src-tauri/tauri.conf.json`.
- **Status**: OPEN

### WiX MSI Target Included in Base Config Breaks Windows Builds Lacking WiX Toolset
- **ID**: L6-PACKAGING-8
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/tauri.conf.json:54` - `        "msi",`
- **Why it breaks**: The base configuration specifies `"msi"` under `bundle.targets`, which causes `tauri build` on Windows to attempt invoking WiX Toolset compilers (`candle.exe`, `light.exe`). While CI bypasses this with `--bundles nsis` (`release.yml:37`), developer and release builds executed locally on Windows fail unless WiX Toolset is manually installed.
- **Fix**: In `src-tauri/tauri.conf.json`, remove `"msi"` from `bundle.targets` and retain only `"nsis"` for Windows desktop packaging.
- **Status**: OPEN

### MSIX Packaging Signs with Self-Signed Untrusted Certificate Blocking Sideloading
- **ID**: L6-PACKAGING-9
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `scripts/build-msix.ps1:184` - `$cert = New-SelfSignedCertificate -Type Custom`
- **Why it breaks**: The PowerShell MSIX packaging script generates a temporary self-signed certificate with a hardcoded password to sign the MSIX artifact. Windows App Installer rejects packages signed by untrusted certificates (`0x800B0109`), making the produced MSIX artifact impossible to sideload without manually trusting the root certificate.
- **Fix**: In `scripts/build-msix.ps1`, add parameters to accept a valid Authenticode code-signing PFX certificate and disable self-signed generation by default for production releases.
- **Status**: OPEN

### MSIX Packaging Script Omits Frontend Resources Required for Embedded Remote Server
- **ID**: L6-PACKAGING-10
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `scripts/build-msix.ps1:124` - `Copy-Item $exePath -Destination "$layoutDir\ferryx.exe" -Force`
- **Why it breaks**: `build-msix.ps1` stages only `ferryx.exe` and icon images into the MSIX layout directory, omitting the `ui/dist` bundle resources configured in `src-tauri/tauri.conf.json:48`. When running from an MSIX installation, the embedded remote server (`src-tauri/src/remote/server.rs:1388`) cannot locate frontend static files and fails to serve the remote web interface.
- **Fix**: In `scripts/build-msix.ps1`, add a step copying `ui/dist` to `$layoutDir\ui\dist` (or `$layoutDir\resources\ui\dist`) matching the directory layout probed by `resolve_frontend_dist`.
- **Status**: OPEN

### Packaged Linux Resource Path Resolution Misses Standard Distribution Hierarchy
- **ID**: L6-PACKAGING-11
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/remote/server.rs:1388` - `            candidates.push(exe_dir.join("ui/dist"));`
- **Why it breaks**: `resolve_frontend_dist` only probes `exe_dir.join("ui/dist")` and `exe_dir.join("resources/ui/dist")`. On packaged Linux systems (such as `.deb` installs where the binary is installed to `/usr/bin/ferryx`), resources are placed in `/usr/lib/ferryx/resources` or `/usr/share/ferryx/resources`, so the hardcoded paths fail to locate the frontend bundle.
- **Fix**: In `src-tauri/src/remote/server.rs`, use Tauri's `app_handle.path().resource_dir()` to resolve bundle resources dynamically, or add standard Linux candidate paths `/usr/lib/ferryx/resources/ui/dist` and `/usr/share/ferryx/resources/ui/dist`.
- **Status**: OPEN

### Release CI Workflow Omits Authenticode Code-Signing for Windows Artifacts
- **ID**: L6-PACKAGING-12
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `.github/workflows/release.yml:144` - `          bunx @tauri-apps/cli build ${{ matrix.tauri_args }}`
- **Why it breaks**: The release workflow imports macOS code-signing identities and notarization credentials (`release.yml:105-132`), but provides no Authenticode certificate or signing step for Windows. The resulting Windows NSIS installer `.exe` is shipped completely unsigned, causing Windows Defender SmartScreen to flag the installer as untrusted.
- **Fix**: In `.github/workflows/release.yml`, add an Authenticode signing step via SignTool or Azure Trusted Signing using secrets (`WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD`) during or after `tauri build`.
- **Status**: OPEN

### Vendored Ghostty Build Script Hardcodes Nightly Zig 0.16.0 Toolchain Requirement
- **ID**: L6-PACKAGING-13
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/native_terminal/build_ghostty.rs:7` - `pub const REQUIRED_ZIG_VERSION: &str = "0.16.0";`
- **Why it breaks**: `build_ghostty.rs` enforces that the installed Zig compiler version must start with `0.16.0`, which is an unreleased nightly build. Standard package managers on Windows (`winget`, `choco`) and Linux (`apt`, `pacman`) only provide stable Zig versions (0.13.x or 0.14.x), causing local development and CI builds on clean environments to fail with an incompatible Zig toolchain error.
- **Fix**: In `src-tauri/native_terminal/build_ghostty.rs`, adapt `verify_zig` to accept stable supported Zig versions or provide clear instructions and automated toolchain fallback for developer environments.
- **Status**: OPEN

### CI Test Suite Gated to Linux Only Leaving Windows Untested in Pull Requests
- **ID**: L6-PACKAGING-14
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `.github/workflows/build-test.yml:127` - `        if: matrix.os_name == 'linux'`
- **Why it breaks**: Test-only defect: In `.github/workflows/build-test.yml`, the `Cargo Test` step is restricted to `matrix.os_name == 'linux'`, while Windows runners only run `cargo check` and `cargo build`. As a result, Windows-specific unit and integration tests are never executed in CI, allowing Windows-breaking packaging and IPC regressions to merge undetected.
- **Fix**: In `.github/workflows/build-test.yml`, remove the `if: matrix.os_name == 'linux'` restriction or add an explicit `if: matrix.os_name == 'windows'` test step executing `cargo test --manifest-path src-tauri/Cargo.toml`.
- **Status**: OPEN

### Dev Frontend Runner in tauri.conf.json Relies on Shell-Dependent Bun Execution
- **ID**: L6-PACKAGING-15
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/tauri.conf.json:7` - `    "beforeDevCommand": "bun scripts/dev-frontend.mjs",`
- **Why it breaks**: `beforeDevCommand` directly invokes `"bun scripts/dev-frontend.mjs"`, which assumes `bun` is available on the executable search path without `.cmd` or `.exe` resolution and assumes `scripts/` is relative to CWD. When invoked from `src-tauri/` or on standard Windows command shells without Bun registered in PATHEXT, `tauri dev` fails immediately on startup.
- **Fix**: In `src-tauri/tauri.conf.json`, use `node` or a cross-platform npm/pnpm command for `beforeDevCommand`, or wrap the invocation with `bun.cmd` / shell-aware executable resolution.
- **Status**: OPEN

### macOS Private API Enabled Globally in Shared Base Config and Cargo Features
- **ID**: L6-PACKAGING-16
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/tauri.conf.json:13` - `    "macOSPrivateApi": true,`
- **Why it breaks**: The base configuration sets `macOSPrivateApi: true` unconditionally for all targets, and `src-tauri/Cargo.toml` activates the `macos-private-api` crate feature without platform gating. On non-macOS platforms (Windows and Linux), private macOS APIs do not exist, and declaring macOS-specific permissions in the shared base configuration violates platform separation and causes rejection if submitting to platform app stores.
- **Fix**: Remove `"macOSPrivateApi": true` from `src-tauri/tauri.conf.json` and place it in `src-tauri/tauri.macos.conf.json`, and gate `features = ["macos-private-api"]` in `src-tauri/Cargo.toml` under `target.'cfg(target_os = "macos")'.dependencies.tauri`.
- **Status**: OPEN

### Daemon Persistence Hardcodes Legacy Identity and World-Writable POSIX Log Paths
- **ID**: L6-PACKAGING-17
- **Severity**: MEDIUM
- **Platforms affected**: macOS
- **Evidence**: `src-tauri/src/daemon/launchd.rs:5` - `const PLIST_LABEL: &str = "com.rorca.daemon";`
- **Why it breaks**: The launchd daemon service configuration still uses the legacy identifier `"com.rorca.daemon"` and hardcodes standard log streams to `/tmp/rorca-daemon.log` (line 39). This conflicts with the current product identity (`com.ferryx.app`) and uses world-writable `/tmp` paths vulnerable to symlink clobbering and collision.
- **Fix**: In `src-tauri/src/daemon/launchd.rs`, change `PLIST_LABEL` to `"com.ferryx.daemon"` and relocate `StandardOutPath` and `StandardErrorPath` to the user-isolated log directory `~/Library/Logs/Ferryx/`.
- **Status**: OPEN

### MSIX Manifest Identity Conflicts with Official App Identifier and Publisher
- **ID**: L6-PACKAGING-18
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/windows/msix/AppxManifest.xml:9` - `    Name="ProjectMaho.Ferryx"`
- **Why it breaks**: The MSIX manifest template hardcodes `Name="ProjectMaho.Ferryx"` and `PublisherDisplayName="Project Maho"` (line 16), which conflicts with the canonical `identifier` `"com.ferryx.app"` (line 5) and `publisher` `"Ferryx"` (line 58) declared in `tauri.conf.json`. This causes mismatched app identity, inconsistent app data folder naming, and store ingestion conflicts.
- **Fix**: In `src-tauri/windows/msix/AppxManifest.xml`, update `Identity@Name` to `"com.ferryx.app"` and `PublisherDisplayName` to `"Ferryx"`.
- **Status**: OPEN

### Updater Archive Layout Validator Strictly Requires macOS App Bundle Structure
- **ID**: L6-PACKAGING-19
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `scripts/assert-updater-archive-layout.mjs:62` - `  const invalidRoot = payloadEntries.find(({ path }) => !path.startsWith("Ferryx.app/"));`
- **Why it breaks**: The updater archive verification script hard-asserts that all payload entries start with `Ferryx.app/`, which is only valid for macOS `.app.tar.gz` bundles. Windows (`.nsis.zip`) and Linux (`.AppImage.tar.gz`) updater archives cannot be validated by this script, leaving non-macOS updater archive layouts unverified in CI release pipelines.
- **Fix**: In `scripts/assert-updater-archive-layout.mjs`, branch validation based on archive extension or target platform to verify `.nsis.zip` (root executable present) and `.AppImage.tar.gz` layouts appropriately.
- **Status**: OPEN

### Windows Linker Manifest Flags in Build Script Fail on GNU MinGW Toolchain
- **ID**: L6-PACKAGING-20
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/build.rs:30` - `    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {`
- **Why it breaks**: `src-tauri/build.rs` passes MSVC-specific linker arguments (`/MANIFEST:EMBED` and `/MANIFESTINPUT:...`) whenever `CARGO_CFG_TARGET_OS` is `"windows"`. When compiling with the GNU MinGW toolchain (`x86_64-pc-windows-gnu`), GNU `ld` does not support `/MANIFEST` flags, causing the link step to abort with unrecognized option errors.
- **Fix**: In `src-tauri/build.rs`, gate the MSVC linker arguments with `if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")`.
- **Status**: OPEN
