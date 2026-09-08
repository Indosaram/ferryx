# Direct release-readiness evidence

Captured by the lead on 2026-09-08 after the inspection DAG failed. Provisional child reports were discarded, not treated as proof.

## MacBook

Command (read-only):

```sh
uname -m; df -h .; bun --version; node --version; cargo --version; rustc --version; rustup target list --installed; zig version; cargo tauri --version; xcodebuild -version; security find-identity -v -p codesigning
```

Observed stdout: arm64; disk 926Gi total, 862Gi used, 11Gi available; Bun 1.4.0; Node v22.22.3; Cargo/rustc 1.92.0; both aarch64-apple-darwin and x86_64-apple-darwin targets; Zig 0.16.0; tauri-cli 2.10.1; Xcode 26.6 build 17F113; four valid signing identities including Developer ID Application: Indo Yoon (5DUM8WPB4C). The chained command returned successfully; it was not a build or signing test. Earlier df reported 15Gi available.

## Omaki

Monitor mon_JCTV7XM41033FJE9; terminal bash_3. Exact command:

```sh
ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 indo@100.91.254.71 'export PATH="$HOME/.bun/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"; uname -sm; df -h /home; for c in "bun --version" "cargo --version" "rustc --version" "zig version" "cargo tauri --version" "pkg-config --modversion webkit2gtk-4.1 gtk+-3.0 alsa"; do printf "PROBE %s\n" "$c"; sh -c "$c"; printf "exit=%s\n" "$?"; done; for d in "$HOME/ferryx-src" "$HOME/rel-0905"; do if test -d "$d"; then printf "REPO %s\n" "$d"; git -C "$d" rev-parse HEAD; fi; done'
```

Captured output:

```text
Linux x86_64
/dev/mapper/root 930G 251G 677G 27% /home
bun 1.4.0; exit=0
cargo 1.98.0 (797e8a9bc 2026-08-05); exit=0
rustc 1.98.0 (88d9e12ae 2026-08-18); exit=0
zig 0.16.0; exit=0
tauri-cli 2.11.4; exit=0
pkg-config webkit2gtk-4.1 gtk+-3.0 alsa:
2.52.6
3.24.52
1.2.16.1
exit=0
/home/indo/ferryx-src: HEAD lookup failed (ambiguous argument HEAD).
/home/indo/rel-0905: 48825781aeb071a47c99bd89078520cfbe1f1413
watcher completed (exit code 0)
```

The overall shell exit 0 does not erase the first checkout's HEAD failure. No source synchronization occurred.

## Windows

Monitors mon_1TFREQYEDWMH3F0N / bash_4 and mon_G9QVAHS36YPV3CXC / bash_5 both completed with exit 0. Commands used SSH BatchMode, ConnectTimeout=10, ServerAliveInterval=5 and ServerAliveCountMax=2, then powershell -NoProfile -NonInteractive -EncodedCommand with UTF-16LE/base64 payloads.

The exact first payload as a JSON-escaped string (escapes expose the path transport defect rather than hiding it):

```json
"$ErrorActionPreference='Continue'; [Environment]::OSVersion.VersionString; $env:COMPUTERNAME; Get-PSDrive C | Select-Object Name,Used,Free; foreach($tool in @('bun','cargo','rustc','zig','cargo-tauri')) { Write-Output ('PROBE '+$tool); & $tool --version; Write-Output ('exit='+$LASTEXITCODE) }; $vs='C:Program Files (x86)Microsoft Visual StudioInstaller\u000bswhere.exe'; if(Test-Path $vs){ & $vs -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath }; Get-ChildItem 'C:Program Files (x86)Windows Kits\b\bin' -Filter MakeAppx.exe -Recurse -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName; foreach($d in @('C:Userssook\ferryx-winbuildorca-lite','C:Userssook\rel-0905')) {if(Test-Path $d){ Write-Output ('REPO '+$d); git -C $d rev-parse HEAD }}"
```

The exact corrective payload as a JSON-escaped string:

```json
"$ProgressPreference='SilentlyContinue'; $ErrorActionPreference='Continue'; zig version; Write-Output ('zigExit='+$LASTEXITCODE); Get-ChildItem $env:USERPROFILE -Directory -Filter '*ferryx*' | ForEach-Object { $_.FullName }; foreach($p in @('C:Program Files (x86)Microsoft Visual StudioInstaller\u000bswhere.exe','C:Program FilesMicrosoft Visual StudioInstaller\u000bswhere.exe','C:BuildTools')) { Write-Output ($p+' exists='+(Test-Path $p)) }; foreach($p in @('C:Program Files (x86)Windows Kits\b\bin','C:Program FilesWindows Kits\b\bin')){ Write-Output ($p+' exists='+(Test-Path $p)); if(Test-Path $p){ Get-ChildItem $p -Recurse -Filter MakeAppx.exe | ForEach-Object { $_.FullName } }}"
```

To reconstruct either actual invocation, let script be the decoded JSON string and encoded be Buffer.from(script, "utf16le").toString("base64"); the command was:

```text
ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 maho-win powershell -NoProfile -NonInteractive -EncodedCommand <encoded>
```

Accepted observations: Windows NT 10.0.26200.0; DESKTOP-1LAPJMP; C: used=650321346560 and free=348901322752 bytes; Bun 1.4.0, Cargo 1.97.0, rustc 1.97.0, tauri-cli 2.11.4 (each printed exit=0). Initial zig --version was a probe mistake and returned exit=1; corrected zig version returned 0.16.0 and zigExit=0.

The corrected profile-directory listing returned ferryx-build-773aa61, ferryx-build-d158e09, ferryx-permqa, ferryx-qa, ferryx-ulw-01a04fcf, ferryx-win-build and ferryx-winbuild under C:/Users/sook. These names are candidates, not certified release checkouts.

Discarded observations: hardcoded backslash paths lost separators/control characters before PowerShell execution. False Test-Path results do NOT prove missing Visual Studio, SDK, MakeAppx or source checkouts. Those prerequisites remain unverified. Future probes should use forward slashes or Join-Path with environment-derived roots. PowerShell CLIXML progress noise also appeared; it is not a build error.

## GitHub state

Monitor mon_451B3Z54YSSQNCCT / bash_2 ran:

```sh
gh workflow list --all --repo Indosaram/ferryx && gh run list --repo Indosaram/ferryx --workflow release.yml --limit 3 --json databaseId,status,conclusion,createdAt,event,headBranch && gh release view --repo Indosaram/ferryx --json tagName,assets
```

Observed workflow-list output: CI Build & Check active (340976178); Deploy Ferryx Web and Docs to GitHub Pages active (340101629); Release Ferryx active (340976179). The monitor timed out with exit 1 before run/release output arrived. The chained release-view stage was not established as executed; no run status or CI release provenance is inferred.

Direct alternative, monitor mon_ESQC5MXJRZX4JGYH / bash_6:

```sh
gh api repos/Indosaram/ferryx/releases/latest --jq '{tag: .tag_name, assets: [.assets[].name]}'
```

It emitted JSON with tag v2026.09.06.1 and these 17 asset names, then the monitor timed out with exit 1. This is observed response data, not a clean command success or verification of asset bytes:

- Ferryx.app.tar.gz
- Ferryx.app.tar.gz.sig
- Ferryx_2026.906.1_amd64.AppImage
- Ferryx_2026.906.1_amd64.AppImage.sig
- Ferryx_2026.906.1_amd64.deb
- Ferryx_2026.906.1_amd64.deb.sig
- Ferryx_2026.906.1_universal.dmg
- Ferryx_2026.906.1_x64-setup.exe
- Ferryx_2026.906.1_x64-setup.exe.sig
- Ferryx_2026.906.1_x64.msix
- Ferryx_amd64.AppImage
- Ferryx_amd64.deb
- Ferryx_universal.dmg
- Ferryx_x64-setup.exe
- Ferryx_x64.msix
- latest.json
- SHA256SUMS.txt

## Cleanup and scope

All lead SSH monitors completed; all three GitHub monitors reached their timeout and exited. No background server, browser, build, app or PTY daemon was launched. No remote filesystem changes, package installation, key operations, tag push or release publication were performed. The failed DAG is terminal, not an active resumption channel. Test cleanup is documented separately in test-evidence.md. The durable notepad and these evidence files are intentional retained audit artifacts, not runtime scratch state.

The exact standalone `gh release view --repo Indosaram/ferryx --json tagName,assets` was also run under mon_JZDCGH0K5HWF9PEA / bash_7. It timed out after 30 seconds with exit 1 and no captured response. The direct API response above is the available current metadata evidence; this command is not marked successful.

The failed remote lane left six local probe scripts. After inspecting them and attributing them to this audit, the lead deleted `/tmp/probe_omaki.sh`, `/tmp/probe_omaki2.sh`, `/tmp/probe_maho.sh`, `/tmp/probe_maho2.sh`, `/tmp/probe_maho3.sh`, and `/tmp/probe_maho4.sh`. No remote temporary file was created by these probes.
