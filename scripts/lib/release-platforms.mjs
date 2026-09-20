import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { tmpdir } from "node:os";

import { runHostScript, runProcess, quoteSh, quotePowerShell } from "./release-hosts.mjs";
import { verifyMinisign } from "./minisign-verify.mjs";
import {
  parseReceipt,
  parsePlan,
  validateToolchains,
  KIND_DEFINITIONS,
  requiredKinds,
  VALID_HOSTS,
} from "./release-contract.mjs";

/**
 * Probes the local macOS host (macbook) for reachability, disk, toolchains, and signing.
 */
export async function probeMacbook(hostConfig, options = {}) {
  const failures = [];
  let reachable = false;
  let disk = { requiredBytes: hostConfig.minFreeBytes, availableBytes: 0, ok: false };
  let os = { platform: "darwin", arch: process.arch, ok: false };
  const tools = {};
  const signing = { hasSigningIdentity: false, hasNotaryProfile: false };

  try {
    // Reachability and OS/arch check
    const osOut = spawnSync("uname", ["-sm"], { encoding: "utf8" });
    if (osOut.status === 0) {
      reachable = true;
      const [sysName, machine] = osOut.stdout.trim().split(/\s+/);
      os = {
        platform: sysName.toLowerCase() === "darwin" ? "darwin" : sysName,
        arch: machine,
        ok: sysName.toLowerCase() === "darwin",
      };
      if (!os.ok) failures.push(`Expected darwin OS, got ${sysName}`);
    } else {
      failures.push("Failed to query uname on macbook");
    }

    // Disk budget probe (nearest existing parent)
    let checkPath = resolve(hostConfig.root);
    while (!existsSync(checkPath) && checkPath !== "/" && checkPath !== ".") {
      checkPath = dirname(checkPath);
    }
    const dfOut = spawnSync("df", ["-k", "-P", checkPath], { encoding: "utf8" });
    if (dfOut.status === 0) {
      const lines = dfOut.stdout.trim().split("\n");
      if (lines.length >= 2) {
        const parts = lines[1].trim().split(/\s+/);
        const availKb = Number(parts[3]);
        if (!Number.isNaN(availKb)) {
          const availBytes = availKb * 1024;
          const ok = availBytes >= hostConfig.minFreeBytes;
          disk = { requiredBytes: hostConfig.minFreeBytes, availableBytes: availBytes, ok };
          if (!ok) {
            failures.push(
              `Insufficient disk space on macbook: required ${hostConfig.minFreeBytes} bytes, available ${availBytes} bytes`,
            );
          }
        }
      }
    } else {
      failures.push("Failed to check disk space via df on macbook");
    }

    // Tools probe
    const toolQueries = [
      { name: "bun", cmd: "bun", args: ["--version"], regex: /^(\d+\.\d+\.\d+)/ },
      { name: "zig", cmd: "zig", args: ["version"], regex: /^(\d+\.\d+\.\d+)/ },
      { name: "rust", cmd: "rustc", args: ["--version"], regex: /^rustc (\d+\.\d+\.\d+)/ },
      { name: "cargo", cmd: "cargo", args: ["--version"], regex: /^cargo (\d+\.\d+\.\d+)/ },
      { name: "tauri", cmd: "bun", args: ["tauri", "--version"], regex: /tauri-cli (\d+\.\d+\.\d+)/ },
      { name: "node", cmd: "node", args: ["--version"], regex: /^v?(\d+\.\d+\.\d+)/ },
    ];

    for (const tq of toolQueries) {
      try {
        const res = spawnSync(tq.cmd, tq.args, { encoding: "utf8" });
        if (res.status === 0) {
          const out = (res.stdout || "") + (res.stderr || "");
          const match = out.match(tq.regex);
          const version = match ? match[1] : out.trim().split(/\s+/)[0];
          tools[tq.name] = { version, ok: true };
        } else {
          tools[tq.name] = { version: null, ok: false, error: "nonzero exit" };
          failures.push(`Tool ${tq.name} returned nonzero exit`);
        }
      } catch (err) {
        tools[tq.name] = { version: null, ok: false, error: err.message };
        failures.push(`Tool ${tq.name} not found: ${err.message}`);
      }
    }

    // Check expectedTools if specified
    if (hostConfig.expectedTools) {
      for (const [tool, expVer] of Object.entries(hostConfig.expectedTools)) {
        if (!tools[tool] || !tools[tool].ok) {
          failures.push(`Expected tool ${tool} version ${expVer} is missing`);
        }
      }
    }

    // Signing environment probe (booleans only, never emit secrets)
    if (hostConfig.signingIdentity) {
      const idOut = spawnSync("security", ["find-identity", "-v", "-p", "codesigning"], { encoding: "utf8" });
      if (idOut.status === 0 && idOut.stdout.includes(hostConfig.signingIdentity)) {
        signing.hasSigningIdentity = true;
      } else {
        failures.push(`Signing identity not found in keychain: '${hostConfig.signingIdentity}'`);
      }
    }

    if (hostConfig.notaryProfile) {
      const notOut = spawnSync(
        "xcrun",
        ["notarytool", "history", "--keychain-profile", hostConfig.notaryProfile, "--output-format", "json"],
        { encoding: "utf8" },
      );
      if (notOut.status === 0) {
        signing.hasNotaryProfile = true;
      } else {
        failures.push(
          `Notary profile '${hostConfig.notaryProfile}' invalid or missing via xcrun notarytool: ${(notOut.stderr || notOut.stdout).trim()}`,
        );
      }
    }
  } catch (err) {
    failures.push(`Macbook probe fatal error: ${err.message}`);
  }

  return {
    host: "macbook",
    platform: "darwin",
    reachable,
    disk,
    os,
    tools,
    signing,
    ok: failures.length === 0,
    failures,
  };
}

/**
 * Probes the remote Linux host (omaki) via SSH.
 */
export async function probeOmaki(hostConfig, options = {}) {
  const failures = [];
  let reachable = false;
  let disk = { requiredBytes: hostConfig.minFreeBytes, availableBytes: 0, ok: false };
  let os = { platform: "linux", arch: "x86_64", ok: false };
  let tools = {};
  const signing = {};
  const packages = {};

  const pathPrefix = hostConfig.path ? `export PATH="${hostConfig.path}:$PATH"\n` : "";
  const script = `
set -u
${pathPrefix}
# OS and Arch
OS_SYS="$(uname -s)"
OS_ARCH="$(uname -m)"

# Disk check
CHECK_PATH="${hostConfig.root}"
while [ ! -d "$CHECK_PATH" ] && [ "$CHECK_PATH" != "/" ]; do
  CHECK_PATH="$(dirname "$CHECK_PATH")"
done
DISK_LINE="$(df -k -P "$CHECK_PATH" 2>/dev/null | tail -1)"
AVAIL_KB="$(echo "$DISK_LINE" | awk '{print $4}')"

# Tools
BUN_V="$(bun --version 2>/dev/null || true)"
ZIG_V="$(zig version 2>/dev/null || true)"
RUST_V="$(rustc --version 2>/dev/null | awk '{print $2}' || true)"
CARGO_V="$(cargo --version 2>/dev/null | awk '{print $2}' || true)"
TAURI_V="$(cargo tauri --version 2>/dev/null | awk '{print $2}' || true)"
NODE_V="$(node --version 2>/dev/null || true)"

# Linux Packages
PKG_WEBKIT="$(pkg-config --modversion webkit2gtk-4.1 2>/dev/null || pkg-config --modversion webkit2gtk-4.0 2>/dev/null || true)"
PKG_GTK="$(pkg-config --modversion gtk+-3.0 2>/dev/null || true)"
PKG_ALSA="$(pkg-config --modversion alsa 2>/dev/null || true)"

cat <<EOF
---PROBE_START---
{
  "os": { "sys": "$OS_SYS", "arch": "$OS_ARCH" },
  "availKb": "$AVAIL_KB",
  "tools": {
    "bun": "$BUN_V",
    "zig": "$ZIG_V",
    "rust": "$RUST_V",
    "cargo": "$CARGO_V",
    "tauri": "$TAURI_V",
    "node": "$NODE_V"
  },
  "packages": {
    "webkit2gtk": "$PKG_WEBKIT",
    "gtk3": "$PKG_GTK",
    "alsa": "$PKG_ALSA"
  }
}
---PROBE_END---
EOF
`;

  try {
    const res = await runHostScript(hostConfig, { posix: script }, options);
    reachable = true;
    const match = res.stdout.match(/---PROBE_START---\s*([\s\S]*?)\s*---PROBE_END---/);
    if (!match) {
      throw new Error(`Invalid probe response from omaki: ${res.stdout}`);
    }
    const data = JSON.parse(match[1]);

    // OS check
    os = {
      platform: data.os.sys.toLowerCase() === "linux" ? "linux" : data.os.sys,
      arch: data.os.arch,
      ok: data.os.sys.toLowerCase() === "linux" && data.os.arch === "x86_64",
    };
    if (!os.ok) failures.push(`Expected linux x86_64 on omaki, got ${data.os.sys} ${data.os.arch}`);

    // Disk check
    const availKb = Number(data.availKb);
    if (!Number.isNaN(availKb) && availKb > 0) {
      const availBytes = availKb * 1024;
      const ok = availBytes >= hostConfig.minFreeBytes;
      disk = { requiredBytes: hostConfig.minFreeBytes, availableBytes: availBytes, ok };
      if (!ok) {
        failures.push(
          `Insufficient disk space on omaki: required ${hostConfig.minFreeBytes} bytes, available ${availBytes} bytes`,
        );
      }
    } else {
      failures.push("Failed to parse disk availability on omaki");
    }

    // Tools check
    for (const [tool, ver] of Object.entries(data.tools)) {
      const ok = typeof ver === "string" && ver.trim() !== "";
      tools[tool] = { version: ver ? ver.trim() : null, ok };
      if (!ok) {
        failures.push(`Required tool '${tool}' is missing on omaki`);
      }
    }

    // Packages check
    for (const [pkg, ver] of Object.entries(data.packages)) {
      const ok = typeof ver === "string" && ver.trim() !== "";
      packages[pkg] = { version: ver ? ver.trim() : null, ok };
      if (!ok) {
        failures.push(`Required package '${pkg}' not found via pkg-config on omaki`);
      }
    }
  } catch (err) {
    failures.push(`omaki probe error: ${err.message}`);
  }

  return {
    host: "omaki",
    platform: "linux",
    reachable,
    disk,
    os,
    tools,
    packages,
    signing,
    ok: failures.length === 0,
    failures,
  };
}

/**
 * Probes the remote Windows host (maho-win) via SSH and PowerShell.
 */
export async function probeMahoWin(hostConfig, options = {}) {
  const failures = [];
  let reachable = false;
  let disk = { requiredBytes: hostConfig.minFreeBytes, availableBytes: 0, ok: false };
  let os = { platform: "win32", arch: "x64", ok: false };
  let tools = {};
  const signing = {};
  const windowsTools = {};

  const script = `
$root = "${hostConfig.root}"
$driveLetter = $root.Substring(0, 1)
$drive = Get-PSDrive -Name $driveLetter -ErrorAction SilentlyContinue
$freeBytes = if ($drive) { $drive.Free } else { 0 }

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()

# Tools
$bunV = (bun --version 2>$null)
$zigV = (zig version 2>$null)
$rustV = ((rustc --version 2>$null) -replace '^rustc\\s+', '' -replace '\\s+.*$', '')
$cargoV = ((cargo --version 2>$null) -replace '^cargo\\s+', '' -replace '\\s+.*$', '')

# vswhere & link.exe
$vswhere = Join-Path \${env:ProgramFiles(x86)} "Microsoft Visual Studio/Installer/vswhere.exe"
$hasVswhere = Test-Path $vswhere
$linker = $null
if ($hasVswhere) {
    $inst = & $vswhere -latest -products * -property installationPath
    if ($inst) {
        $toolsDir = Join-Path $inst "VC/Tools/MSVC"
        if (Test-Path $toolsDir) {
            $latest = Get-ChildItem -Path $toolsDir -Directory | Sort-Object Name -Descending | Select-Object -First 1
            if ($latest) {
                $lp = Join-Path $latest.FullName "bin/Hostx64/x64/link.exe"
                if (Test-Path $lp) { $linker = $lp }
            }
        }
    }
}

# MakeAppx.exe
$sdkRoots = @("C:/Program Files (x86)/Windows Kits/10/bin", "C:/Program Files/Windows Kits/10/bin")
$makeAppx = $null
foreach ($r in $sdkRoots) {
    if (Test-Path $r) {
        $f = Get-ChildItem -Path $r -Filter "MakeAppx.exe" -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.FullName -like "*x64*" } | Select-Object -First 1
        if ($f) { $makeAppx = $f.FullName; break }
    }
}

$probe = [PSCustomObject]@{
    arch = $arch
    freeBytes = $freeBytes
    tools = [PSCustomObject]@{
        bun = $bunV
        zig = $zigV
        rust = $rustV
        cargo = $cargoV
    }
    windows = [PSCustomObject]@{
        hasVswhere = $hasVswhere
        hasLinker = ($linker -ne $null)
        hasMakeAppx = ($makeAppx -ne $null)
    }
}

Write-Output "---PROBE_START---"
Write-Output ($probe | ConvertTo-Json -Compress)
Write-Output "---PROBE_END---"
`;

  try {
    const res = await runHostScript(hostConfig, { powershell: script }, options);
    reachable = true;
    const match = res.stdout.match(/---PROBE_START---\s*([\s\S]*?)\s*---PROBE_END---/);
    if (!match) {
      throw new Error(`Invalid probe response from maho-win: ${res.stdout}`);
    }
    const data = JSON.parse(match[1]);

    // OS check
    os = {
      platform: "win32",
      arch: data.arch.toLowerCase() === "x64" ? "x64" : data.arch,
      ok: data.arch.toLowerCase() === "x64",
    };
    if (!os.ok) failures.push(`Expected win32 x64 on maho-win, got ${data.arch}`);

    // Disk check
    const freeBytes = Number(data.freeBytes);
    const ok = freeBytes >= hostConfig.minFreeBytes;
    disk = { requiredBytes: hostConfig.minFreeBytes, availableBytes: freeBytes, ok };
    if (!ok) {
      failures.push(
        `Insufficient disk space on maho-win: required ${hostConfig.minFreeBytes} bytes, available ${freeBytes} bytes`,
      );
    }

    // Tools check
    for (const [tool, ver] of Object.entries(data.tools)) {
      const tOk = typeof ver === "string" && ver.trim() !== "";
      tools[tool] = { version: ver ? ver.trim() : null, ok: tOk };
      if (!tOk) {
        failures.push(`Required tool '${tool}' is missing on maho-win`);
      }
    }

    // Windows build tools check
    windowsTools.hasVswhere = Boolean(data.windows.hasVswhere);
    windowsTools.hasLinker = Boolean(data.windows.hasLinker);
    windowsTools.hasMakeAppx = Boolean(data.windows.hasMakeAppx);

    if (!windowsTools.hasVswhere) failures.push("vswhere.exe not found on maho-win");
    if (!windowsTools.hasLinker) failures.push("MSVC link.exe not found on maho-win");
    if (!windowsTools.hasMakeAppx) failures.push("Windows SDK MakeAppx.exe not found on maho-win");
  } catch (err) {
    failures.push(`maho-win probe error: ${err.message}`);
  }

  return {
    host: "maho-win",
    platform: "win32",
    reachable,
    disk,
    os,
    tools,
    windowsTools,
    signing,
    ok: failures.length === 0,
    failures,
  };
}

/**
 * Runs preflight probes across all configured hosts.
 */
export async function preflightAll(config, options = {}) {
  const hosts = config.hosts;
  const [macbook, omaki, mahoWin] = await Promise.all([
    probeMacbook(hosts.macbook, options),
    probeOmaki(hosts.omaki, options),
    probeMahoWin(hosts["maho-win"], options),
  ]);

  const allHosts = {
    macbook,
    omaki,
    "maho-win": mahoWin,
  };

  const ok = macbook.ok && omaki.ok && mahoWin.ok;
  return { ok, hosts: allHosts };
}

/**
 * Creates git bundle files for source and ghostty repositories into the target directory.
 */
export function createGitBundles({ repoDir, ghosttyRepoDir, commitSha, ghosttyPin, outDir }) {
  if (!existsSync(repoDir)) throw new Error(`Source repo directory not found: ${repoDir}`);
  if (!existsSync(ghosttyRepoDir)) throw new Error(`Ghostty repo directory not found: ${ghosttyRepoDir}`);

  const sourceBundlePath = join(outDir, "source.bundle");
  const ghosttyBundlePath = join(outDir, "ghostty.bundle");

  const tempRefSource = `refs/heads/bundle-release-${commitSha.slice(0, 8)}`;
  try {
    execFileSync("git", ["update-ref", tempRefSource, commitSha], { cwd: repoDir, stdio: "pipe" });
    execFileSync("git", ["bundle", "create", sourceBundlePath, "HEAD", tempRefSource], {
      cwd: repoDir,
      stdio: "pipe",
    });
  } finally {
    try {
      execFileSync("git", ["update-ref", "-d", tempRefSource], { cwd: repoDir, stdio: "pipe" });
    } catch {}
  }

  const tempRefGhostty = `refs/heads/bundle-ghostty-${ghosttyPin.slice(0, 8)}`;
  try {
    execFileSync("git", ["update-ref", tempRefGhostty, ghosttyPin], { cwd: ghosttyRepoDir, stdio: "pipe" });
    execFileSync("git", ["bundle", "create", ghosttyBundlePath, "HEAD", tempRefGhostty], {
      cwd: ghosttyRepoDir,
      stdio: "pipe",
    });
  } finally {
    try {
      execFileSync("git", ["update-ref", "-d", tempRefGhostty], { cwd: ghosttyRepoDir, stdio: "pipe" });
    } catch {}
  }

  const sourceBytes = readFileSync(sourceBundlePath);
  const ghosttyBytes = readFileSync(ghosttyBundlePath);

  const sourceBundleSha256 = createHash("sha256").update(sourceBytes).digest("hex");
  const ghosttyBundleSha256 = createHash("sha256").update(ghosttyBytes).digest("hex");

  return {
    sourceBundlePath,
    ghosttyBundlePath,
    sourceBundleSha256,
    ghosttyBundleSha256,
  };
}

/**
 * Creates a build receipt conforming to the release contract.
 */
export function createBuildReceipt({ host, runId, commitSha, appVersion, exitCode = 0, artifacts, toolchains }) {
  const receipt = {
    schemaVersion: 1,
    runId,
    host,
    commitSha,
    appVersion,
    completedAt: new Date().toISOString(),
    exitCode,
    artifacts,
    ...(toolchains ? { toolchains } : {}),
  };

  return receipt;
}

/**
 * Identifies the artifact kind from a filename and host.
 */
function identifyKind(filename, hostName) {
  for (const [kind, def] of Object.entries(KIND_DEFINITIONS)) {
    if (def.permittedHost === hostName) {
      for (const ext of def.allowedExtensions) {
        if (filename.endsWith(ext) && !filename.endsWith(".sig")) {
          return kind;
        }
      }
    }
  }
  return null;
}

/**
 * Scans artifacts staged in artifactsOutDir, computes sha256 checksums, and constructs receipt artifact entries.
 */
function scanAndVerifyArtifacts(artifactsOutDir, hostName, plan) {
  const files = readdirSync(artifactsOutDir);
  const artifacts = [];

  for (const file of files) {
    if (file.endsWith(".sig")) continue;
    const kind = identifyKind(file, hostName);
    if (!kind) continue;

    const kindDef = KIND_DEFINITIONS[kind];
    const fullPath = join(artifactsOutDir, file);
    const stat = statSync(fullPath);
    const bytes = stat.size;
    const content = readFileSync(fullPath);
    const sha256 = createHash("sha256").update(content).digest("hex");

    const platformSubdir = hostName === "macbook" ? "darwin" : hostName === "omaki" ? "linux" : "windows";
    const relPath = `${platformSubdir}/${file}`;

    let signatureRelPath = null;
    if (kindDef.isUpdater) {
      const sigFile = `${file}.sig`;
      const fullSigPath = join(artifactsOutDir, sigFile);
      if (existsSync(fullSigPath)) {
        signatureRelPath = `${platformSubdir}/${sigFile}`;
      } else {
        // Look for alternate naming if archive has .tar.gz
        const baseSigFile = file.replace(/\.app\.tar\.gz$/, ".app.tar.gz.sig");
        if (existsSync(join(artifactsOutDir, baseSigFile))) {
          signatureRelPath = `${platformSubdir}/${baseSigFile}`;
        }
      }
    }

    artifacts.push({
      kind,
      name: file,
      relPath,
      bytes,
      sha256,
      signatureRelPath,
      targets: kindDef.expectedTargets,
    });
  }

  return artifacts;
}

function parseBuildResult(stdout) {
  const match = stdout.match(/---BUILD_RESULT---\s*({[^\r\n]+})/);
  if (!match) throw new Error("Remote builder did not return a build result");
  return validateToolchains(JSON.parse(match[1]), { required: ["node", "bun", "zig", "rust"] });
}

export function createLinuxBuildScript({ workspaceDir, plan, hostConfig }) {
  const signingKey = process.env.TAURI_SIGNING_PRIVATE_KEY;
  const signingPassword = process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;
  const signingEnv = signingKey
    ? `export TAURI_SIGNING_PRIVATE_KEY=${quoteSh(signingKey)}\nexport TAURI_SIGNING_PRIVATE_KEY_PASSWORD=${quoteSh(signingPassword ?? "")}`
    : "";
  const pathPrefix = hostConfig.path ? `export PATH=${quoteSh(hostConfig.path)}:$PATH\n` : "";
  const targetDir = hostConfig.root ? `${hostConfig.root}/cargo-target` : `"$workspace/cargo-target"`;
  const targetDirCode = hostConfig.root
    ? `export CARGO_TARGET_DIR=${quoteSh(targetDir)}\nmkdir -p "$CARGO_TARGET_DIR"`
    : `export CARGO_TARGET_DIR="$workspace/cargo-target"`;
  return `set -euo pipefail
umask 077
${pathPrefix}workspace=${quoteSh(workspaceDir)}
${signingEnv}
source_dir="$workspace/source"
ghostty_dir="$workspace/ghostty"
out_dir="$workspace/out"
test ! -e "$source_dir" && test ! -e "$ghostty_dir" && test ! -e "$out_dir"
verify_repo="$workspace/.verify-repo"
git init --bare --quiet "$verify_repo"
GIT_DIR="$verify_repo" git bundle verify "$workspace/source.bundle"
GIT_DIR="$verify_repo" git bundle verify "$workspace/ghostty.bundle"
rm -rf "$verify_repo"
git clone "$workspace/source.bundle" "$source_dir"
git -C "$source_dir" checkout --detach ${quoteSh(plan.commitSha)}
test "$(git -C "$source_dir" rev-parse HEAD)" = ${quoteSh(plan.commitSha)}
git clone "$workspace/ghostty.bundle" "$ghostty_dir"
git -C "$ghostty_dir" checkout --detach ${quoteSh(plan.ghosttyPin ?? "HEAD")}
actual_ghostty="$(git -C "$ghostty_dir" rev-parse HEAD)"
expected_ghostty="$(sed -n 's/.*EXPECTED_GHOSTTY_SHA:.*"\\([0-9a-f]\\{40\\}\\)".*/\\1/p' "$source_dir/src-tauri/native_terminal/build_ghostty.rs")"
test "$actual_ghostty" = "$expected_ghostty"
rm -rf "$source_dir/src-tauri/vendor/ghostty"
mkdir -p "$source_dir/src-tauri/vendor"
cp -R "$ghostty_dir" "$source_dir/src-tauri/vendor/ghostty"
node "$source_dir/scripts/sync-version.mjs" --tag ${quoteSh(plan.tag)}
bun install --cwd "$source_dir/ui" --frozen-lockfile
mkdir "$out_dir"
${targetDirCode}
export SOURCE_DATE_EPOCH=${quoteSh(String(plan.sourceDateEpoch))}
export NO_STRIP=true
rm -rf "$CARGO_TARGET_DIR/release/bundle"
cd "$source_dir"
bun tauri build --bundles appimage,deb
appimages=()
while IFS= read -r f; do
  [ -n "$f" ] && appimages+=("$f")
done < <(find "$CARGO_TARGET_DIR/release/bundle/appimage" -maxdepth 1 -type f -name '*.AppImage')
debs=()
while IFS= read -r f; do
  [ -n "$f" ] && debs+=("$f")
done < <(find "$CARGO_TARGET_DIR/release/bundle/deb" -maxdepth 1 -type f -name '*.deb')
test "${"${#appimages[@]}"}" -eq 1 && test "${"${#debs[@]}"}" -eq 1
test -s "${"${appimages[0]}"}" && test -s "${"${debs[0]}"}"
cp "${"${appimages[0]}"}" "$out_dir/Ferryx_amd64.AppImage"
cp "${"${debs[0]}"}" "$out_dir/Ferryx_amd64.deb"
test -s "$out_dir/Ferryx_amd64.AppImage" && test -s "$out_dir/Ferryx_amd64.deb"
printf '%s\\n' '---BUILD_RESULT---' "{\\"node\\":\\"$(node --version | sed 's/^v//')\\",\\"bun\\":\\"$(bun --version)\\",\\"zig\\":\\"$(zig version)\\",\\"rust\\":\\"$(rustc --version | awk '{print $2}')\\"}"
`;
}

export function createWindowsBuildScript({ workspaceDir, plan, hostConfig, signingSecretFile = null }) {
  const pathPrefix = hostConfig.path
    ? `$env:PATH = ${quotePowerShell(`${hostConfig.path};`)} + $env:PATH\n`
    : "";
  const targetDirCode = hostConfig.root
    ? `$cargoTarget = Join-Path ${quotePowerShell(hostConfig.root)} 'cargo-target'\nif (-not (Test-Path $cargoTarget)) { New-Item -ItemType Directory -Path $cargoTarget -Force | Out-Null }\n$env:CARGO_TARGET_DIR = $cargoTarget`
    : `$cargoTarget = Join-Path $workspace 'cargo-target'\n$env:CARGO_TARGET_DIR = $cargoTarget`;
  const nsisBuild = plan.channels.nsisMigration
    ? `Remove-Item -Path (Join-Path $cargoTarget 'release\\bundle') -Recurse -Force -ErrorAction SilentlyContinue
bun tauri build --bundles nsis
$nsis = @(Get-ChildItem -Path $cargoTarget -Filter '*-setup.exe' -File -Recurse)
if ($nsis.Count -ne 1) { throw "Expected exactly one NSIS installer, found $($nsis.Count)" }
Copy-Item -LiteralPath $nsis[0].FullName -Destination (Join-Path $outDir 'Ferryx_x64-setup.exe')
`
    : "";
  return `$ErrorActionPreference = 'Stop'
${pathPrefix}$workspace = ${quotePowerShell(workspaceDir)}
$signingSecretFile = ${signingSecretFile ? `Join-Path $workspace ${quotePowerShell(signingSecretFile)}` : "$null"}
try {
if ($signingSecretFile -and (Test-Path $signingSecretFile)) {
  $signingSecret = Get-Content -LiteralPath $signingSecretFile -Raw | ConvertFrom-Json
  $env:TAURI_SIGNING_PRIVATE_KEY = $signingSecret.key
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $signingSecret.password
}
$sourceDir = Join-Path $workspace 'source'
$ghosttyDir = Join-Path $workspace 'ghostty'
$outDir = Join-Path $workspace 'out'
if ((Test-Path $sourceDir) -or (Test-Path $ghosttyDir) -or (Test-Path $outDir)) { throw 'Remote output collision' }
$verifyRepo = Join-Path $workspace '.verify-repo'
git init --bare --quiet $verifyRepo; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$env:GIT_DIR = $verifyRepo
git bundle verify (Join-Path $workspace 'source.bundle'); if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
git bundle verify (Join-Path $workspace 'ghostty.bundle'); if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Remove-Item env:GIT_DIR -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $verifyRepo -Recurse -Force
git clone (Join-Path $workspace 'source.bundle') $sourceDir; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
git -C $sourceDir checkout --detach ${quotePowerShell(plan.commitSha)}; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ((git -C $sourceDir rev-parse HEAD).Trim() -ne ${quotePowerShell(plan.commitSha)}) { throw 'Source checkout SHA mismatch' }
git clone (Join-Path $workspace 'ghostty.bundle') $ghosttyDir; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$pinLine = Select-String -Path (Join-Path $sourceDir 'src-tauri/native_terminal/build_ghostty.rs') -Pattern 'EXPECTED_GHOSTTY_SHA:.*"([0-9a-f]{40})"'
$ghosttyPin = $pinLine.Matches[0].Groups[1].Value
git -C $ghosttyDir checkout --detach $ghosttyPin; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ((git -C $ghosttyDir rev-parse HEAD).Trim() -ne $ghosttyPin) { throw 'Ghostty checkout SHA mismatch' }
Remove-Item -LiteralPath (Join-Path $sourceDir 'src-tauri/vendor/ghostty') -Recurse -Force
Copy-Item -LiteralPath $ghosttyDir -Destination (Join-Path $sourceDir 'src-tauri/vendor/ghostty') -Recurse
node (Join-Path $sourceDir 'scripts/sync-version.mjs') --tag ${quotePowerShell(plan.tag)}; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
bun install --cwd (Join-Path $sourceDir 'ui') --frozen-lockfile; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
New-Item -ItemType Directory -Path $outDir | Out-Null
${targetDirCode}
$env:SOURCE_DATE_EPOCH = ${quotePowerShell(String(plan.sourceDateEpoch))}
Push-Location $sourceDir
bun tauri build --no-bundle; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$executables = @(Get-ChildItem -Path (Join-Path $cargoTarget 'release') -Filter 'ferryx.exe' -File)
if ($executables.Count -ne 1) { throw "Expected exactly one Ferryx executable, found $($executables.Count)" }
& (Join-Path $sourceDir 'scripts/build-msix.ps1') -ExePath $executables[0].FullName -Version ${quotePowerShell(plan.msixVersion)} -OutputDir $outDir -SkipSigning
if ($LASTEXITCODE -ne $null -and $LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
${nsisBuild}Pop-Location
$msix = @(Get-ChildItem -Path $outDir -Filter '*.msix' -File)
if ($msix.Count -ne 1 -or $msix[0].Length -le 0) { throw 'Expected exactly one non-empty MSIX' }
if ($msix[0].Name -ne 'Ferryx_x64.msix') { Move-Item $msix[0].FullName (Join-Path $outDir 'Ferryx_x64.msix') }
$versions = [ordered]@{ node = ((node --version) -replace '^v',''); bun = (bun --version); zig = (zig version); rust = ((rustc --version) -split ' ')[1] }
Write-Output '---BUILD_RESULT---'
Write-Output ($versions | ConvertTo-Json -Compress)
} finally {
  Remove-Item -LiteralPath $signingSecretFile -Force -ErrorAction SilentlyContinue
  Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
  Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
}
`;
}

/**
 * Signs an updater artifact using the local tauri signer and verifies the signature
 * against the repository's updater public key.
 */
export async function signUpdaterArtifact({ artifactPath, repoDir }) {
  const privKey = process.env.TAURI_SIGNING_PRIVATE_KEY;
  const privKeyPath = process.env.TAURI_SIGNING_PRIVATE_KEY_PATH;
  if (!privKey && !privKeyPath) {
    throw new Error(
      `Updater signing failed: TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH required to sign updater artifact '${basename(artifactPath)}'`,
    );
  }

  const tauriBin = process.env.FERRYX_TAURI_BIN || "cargo";
  const tauriArgs = ["tauri", "signer", "sign", artifactPath];
  await runProcess(tauriBin, tauriArgs, { cwd: repoDir, timeoutMs: 60000 });

  const sigPath = `${artifactPath}.sig`;
  if (!existsSync(sigPath) || statSync(sigPath).size === 0) {
    throw new Error(`Signature file was not created or is empty: ${sigPath}`);
  }

  const tauriConfPath = join(repoDir, "src-tauri", "tauri.conf.json");
  if (existsSync(tauriConfPath)) {
    const tauriConf = JSON.parse(readFileSync(tauriConfPath, "utf8"));
    const pubkey = tauriConf?.plugins?.updater?.pubkey;
    if (pubkey) {
      const data = readFileSync(artifactPath);
      const signature = readFileSync(sigPath, "utf8");
      verifyMinisign({ data, signature, publicKey: pubkey });
    }
  }
}

/**
 * Default command executor used by the macOS finalization phase.
 * Returns captured stdio; throws on nonzero exit unless `allowFailure` is set.
 */
export function macExec(command, args, { allowFailure = false } = {}) {
  const res = spawnSync(command, args, { encoding: "utf8" });
  if (res.error) throw res.error;
  const stdout = res.stdout || "";
  const stderr = res.stderr || "";
  if (res.status !== 0 && !allowFailure) {
    throw new Error(
      `Command failed (exit ${res.status}): ${command} ${args.join(" ")}\n${(stderr || stdout).trim()}`,
    );
  }
  return { status: res.status ?? 0, stdout, stderr };
}

/**
 * Submits a path to Apple's notary service and fails closed unless the returned
 * status is "Accepted". `notarytool submit --wait` exits 0 even when the
 * submission comes back Invalid, so the JSON result must be inspected.
 */
export function submitForNotarization({ path, notaryProfile, exec = macExec }) {
  const { stdout, stderr } = exec(
    "xcrun",
    ["notarytool", "submit", path, "--keychain-profile", notaryProfile, "--wait", "--output-format", "json"],
    { allowFailure: true },
  );
  const raw = `${stdout}\n${stderr}`;
  const jsonMatch = raw.match(/\{[\s\S]*\}/);
  if (!jsonMatch) {
    throw new Error(`Notarization returned no JSON result for '${basename(path)}': ${raw.trim()}`);
  }
  let result;
  try {
    result = JSON.parse(jsonMatch[0]);
  } catch (err) {
    throw new Error(`Notarization returned unparsable result for '${basename(path)}': ${err.message}`);
  }
  if (result.status !== "Accepted") {
    throw new Error(
      `Notarization status '${result.status ?? "unknown"}' for '${basename(path)}' (submission ${result.id ?? "unknown"}): ${result.message ?? raw.trim()}`,
    );
  }
  return result;
}

/**
 * Rebuilds the disk image from a specific .app bundle, replacing any image the
 * Tauri bundler produced before the app was re-signed, notarized, and stapled.
 */
export function recreateDmgFromApp({ appPath, dmgPath, stagingDir, exec = macExec }) {
  const appName = basename(appPath);
  const volumeName = appName.replace(/\.app$/, "");
  exec("rm", ["-rf", stagingDir]);
  exec("mkdir", ["-p", stagingDir]);
  exec("ditto", [appPath, join(stagingDir, appName)]);
  exec("ln", ["-s", "/Applications", join(stagingDir, "Applications")]);
  exec("rm", ["-f", dmgPath]);
  exec("hdiutil", [
    "create",
    "-volname",
    volumeName,
    "-srcfolder",
    stagingDir,
    "-fs",
    "HFS+",
    "-format",
    "UDZO",
    "-ov",
    dmgPath,
  ]);
  exec("rm", ["-rf", stagingDir]);
  return { dmgPath, volumeName };
}

/**
 * Re-signs, notarizes, and staples the macOS app bundle, then rebuilds and
 * notarizes the DMG from that final app so the image never ships the bundler's
 * pre-signing copy of the embedded helpers.
 */
export function finalizeMacosBundle({
  appPath,
  dmgPath = null,
  workspaceDir,
  signingIdentity = null,
  notaryProfile = null,
  approveNotarization = false,
  exec = macExec,
}) {
  // Re-sign every Mach-O binary (including embedded helpers) with hardened
  // runtime and a secure timestamp, then seal the bundle.
  if (signingIdentity) {
    const findOut = exec("find", [appPath, "-type", "f"]).stdout;
    for (const itemPath of findOut.trim().split("\n")) {
      if (!itemPath) continue;
      const fileType = exec("file", ["-b", itemPath]).stdout;
      if (fileType.includes("Mach-O")) {
        exec("codesign", ["--force", "--options", "runtime", "--timestamp", "--sign", signingIdentity, itemPath]);
      }
    }
    exec("codesign", ["--force", "--options", "runtime", "--timestamp", "--sign", signingIdentity, appPath]);
  }

  exec("codesign", ["--verify", "--deep", "--strict", "--verbose=2", appPath]);
  if (signingIdentity) {
    const signInfo = exec("codesign", ["-dv", "--verbose=4", appPath], { allowFailure: true });
    const combined = `${signInfo.stdout}\n${signInfo.stderr}`;
    if (!combined.includes(signingIdentity)) {
      throw new Error(`App bundle is not signed with expected identity '${signingIdentity}'`);
    }
  }

  const notarizations = [];
  if (approveNotarization && notaryProfile) {
    const appZipPath = join(workspaceDir, "Ferryx-notary.zip");
    exec("rm", ["-f", appZipPath]);
    exec("ditto", ["-c", "-k", "--keepParent", appPath, appZipPath]);
    notarizations.push({
      target: "app",
      ...submitForNotarization({ path: appZipPath, notaryProfile, exec }),
    });
    exec("rm", ["-f", appZipPath]);

    exec("xcrun", ["stapler", "staple", appPath]);
    exec("xcrun", ["stapler", "validate", appPath]);

    if (dmgPath) {
      // The bundler built this DMG from the app as it existed before the
      // helper re-sign/notarize/staple pass, so rebuild it from the final app.
      recreateDmgFromApp({ appPath, dmgPath, stagingDir: join(workspaceDir, "dmg-staging"), exec });
      if (signingIdentity) {
        exec("codesign", ["--force", "--timestamp", "--sign", signingIdentity, dmgPath]);
      }
      notarizations.push({
        target: "dmg",
        ...submitForNotarization({ path: dmgPath, notaryProfile, exec }),
      });
      exec("xcrun", ["stapler", "staple", dmgPath]);
      exec("xcrun", ["stapler", "validate", dmgPath]);
    }

    // Validate Gatekeeper assessment with spctl
    const spctlApp = exec("spctl", ["-a", "-vvv", "-t", "install", appPath], { allowFailure: true });
    const spctlAppOut = `${spctlApp.stdout}\n${spctlApp.stderr}`;
    if (spctlApp.status !== 0 || !spctlAppOut.includes("Notarized Developer ID")) {
      throw new Error(`Gatekeeper spctl validation failed for ${appPath}: ${spctlAppOut.trim()}`);
    }

    if (dmgPath) {
      const spctlDmg = exec("spctl", ["-a", "-vvv", "-t", "install", dmgPath], { allowFailure: true });
      const spctlDmgOut = `${spctlDmg.stdout}\n${spctlDmg.stderr}`;
      if (spctlDmg.status !== 0 || !spctlDmgOut.includes("Notarized Developer ID")) {
        throw new Error(`Gatekeeper spctl validation failed for ${dmgPath}: ${spctlDmgOut.trim()}`);
      }
    }
  }

  return { appPath, dmgPath, notarizations };
}

/**
 * Orchestrates an isolated platform build for a specific host.
 */
export async function buildHost({
  hostName,
  config,
  runDir,
  approveNotarization = false,
  runner = null,
  timeoutMs = Number(process.env.FERRYX_BUILD_TIMEOUT_MS) || 1800000,
}) {
  if (!VALID_HOSTS.includes(hostName)) {
    throw new Error(`Unknown host: '${hostName}'. Valid hosts: ${VALID_HOSTS.join(", ")}`);
  }

  const hostConfig = config.hosts[hostName];
  if (!hostConfig) {
    throw new Error(`Host '${hostName}' not defined in config`);
  }

  const planPath = join(runDir, "plan.json");
  if (!existsSync(planPath)) {
    throw new Error(`Plan file not found in run directory: ${planPath}`);
  }
  const plan = parsePlan(readFileSync(planPath, "utf8"));

  const platformSubdir = hostName === "macbook" ? "darwin" : hostName === "omaki" ? "linux" : "windows";
  const artifactsOutDir = join(runDir, "artifacts", platformSubdir);
  const receiptsDir = join(runDir, "receipts");
  mkdirSync(artifactsOutDir, { recursive: true });
  mkdirSync(receiptsDir, { recursive: true });
  const receiptPath = join(receiptsDir, `build-receipt-${hostName}.json`);
  if (existsSync(receiptPath) || readdirSync(artifactsOutDir).length !== 0) {
    throw new Error(`Build output already exists for host '${hostName}'; refusing to overwrite`);
  }

  const workspaceDir = join(hostConfig.root, plan.runId);

  // Output collision check on host workspace
  if (hostName === "macbook") {
    if (existsSync(workspaceDir)) {
      throw new Error(`Host workspace already exists: refusing to overwrite (${workspaceDir})`);
    }
    mkdirSync(hostConfig.root, { recursive: true });
  } else if (hostConfig.platform === "linux") {
    const checkRes = await runHostScript(
      hostConfig,
      { posix: `if [ -e "${workspaceDir}" ]; then echo "EXISTS"; fi` },
      { timeoutMs: 10000 },
    );
    if (checkRes.stdout.trim() === "EXISTS") {
      throw new Error(`Host workspace already exists: refusing to overwrite (${workspaceDir})`);
    }
  } else if (hostConfig.platform === "win32") {
    const checkRes = await runHostScript(
      hostConfig,
      { powershell: `if (Test-Path "${workspaceDir}") { Write-Output "EXISTS" }` },
      { timeoutMs: 10000 },
    );
    if (checkRes.stdout.trim() === "EXISTS") {
      throw new Error(`Host workspace already exists: refusing to overwrite (${workspaceDir})`);
    }
  }

  // Ensure bundles exist in runDir/bundles
  const bundlesDir = join(runDir, "bundles");
  mkdirSync(bundlesDir, { recursive: true });
  const sourceBundlePath = join(bundlesDir, "source.bundle");
  const ghosttyBundlePath = join(bundlesDir, "ghostty.bundle");
  if (!existsSync(sourceBundlePath) || !existsSync(ghosttyBundlePath)) {
    // Read ghostty pin from build_ghostty.rs in source repo
    const rsContent = execFileSync("git", ["show", `${plan.commitSha}:src-tauri/native_terminal/build_ghostty.rs`], {
      cwd: config.repository,
      encoding: "utf8",
    });
    const pinMatch = rsContent.match(/EXPECTED_GHOSTTY_SHA:\s*&str\s*=\s*"([0-9a-f]{40})"/);
    if (!pinMatch) {
      throw new Error("Could not extract EXPECTED_GHOSTTY_SHA from build_ghostty.rs");
    }
    const ghosttyPin = pinMatch[1];
    createGitBundles({
      repoDir: config.repository,
      ghosttyRepoDir: config.ghosttyRepository,
      commitSha: plan.commitSha,
      ghosttyPin,
      outDir: bundlesDir,
    });
  }

  let toolchains = null;
  let exitCode = 0;

  if (runner) {
    mkdirSync(workspaceDir, { recursive: true });
    const runResult = await runner({
      hostName,
      hostConfig,
      workspaceDir,
      artifactsOutDir,
      plan,
      approveNotarization,
    });
    if (!runResult || typeof runResult.exitCode !== "number") {
      throw new Error(`Build runner returned no result for host '${hostName}'`);
    }
    exitCode = runResult.exitCode;
    toolchains = runResult.toolchains ?? null;
  } else {
    // Real build dispatch
    if (hostName === "macbook") {
      mkdirSync(workspaceDir, { recursive: true });
      const isolatedSource = join(workspaceDir, "source");
      const isolatedGhostty = join(workspaceDir, "ghostty");

      // Clone source & ghostty
      execFileSync("git", ["clone", sourceBundlePath, isolatedSource], { stdio: "pipe" });
      execFileSync("git", ["checkout", plan.commitSha], { cwd: isolatedSource, stdio: "pipe" });
      execFileSync("git", ["clone", ghosttyBundlePath, isolatedGhostty], { stdio: "pipe" });

      const ghosttyTargetDir = join(isolatedSource, "src-tauri", "vendor", "ghostty");
      rmSync(ghosttyTargetDir, { recursive: true, force: true });
      mkdirSync(dirname(ghosttyTargetDir), { recursive: true });
      execFileSync("cp", ["-R", isolatedGhostty, ghosttyTargetDir]);

      // Stamp versions
      execFileSync("node", ["scripts/sync-version.mjs", "--tag", plan.tag], {
        cwd: isolatedSource,
        stdio: "pipe",
      });

      // Frozen UI install
      execFileSync("bun", ["install", "--cwd", "ui", "--frozen-lockfile"], {
        cwd: isolatedSource,
        stdio: "pipe",
      });

      // Build Darwin universal
      const cargoTargetDir = hostConfig.root
        ? join(hostConfig.root, "cargo-target")
        : join(workspaceDir, "cargo-target");
      mkdirSync(cargoTargetDir, { recursive: true });

      const buildEnv = {
        ...process.env,
        CARGO_TARGET_DIR: cargoTargetDir,
        SOURCE_DATE_EPOCH: String(plan.sourceDateEpoch),
      };
      if (hostConfig.signingIdentity) {
        buildEnv.APPLE_SIGNING_IDENTITY = hostConfig.signingIdentity;
      }
      if (!approveNotarization) {
        delete buildEnv.APPLE_API_KEY;
        delete buildEnv.APPLE_API_KEY_PATH;
        delete buildEnv.APPLE_API_ISSUER;
        delete buildEnv.APPLE_ID;
        delete buildEnv.APPLE_PASSWORD;
      }

      // Build Darwin universal binaries first (skip bundle)
      const aarch64Rel = join(cargoTargetDir, "aarch64-apple-darwin", "release");
      const x86Rel = join(cargoTargetDir, "x86_64-apple-darwin", "release");
      const universalRel = join(cargoTargetDir, "universal-apple-darwin", "release");

      execFileSync(
        "bun",
        ["tauri", "build", "--target", "universal-apple-darwin", "--no-bundle"],
        {
          cwd: isolatedSource,
          env: buildEnv,
          stdio: "pipe",
          timeout: timeoutMs,
        },
      );

      // Ensure all workspace binaries (e.g. ferryx-cli, ferryx-relay) are universal lipo'd
      mkdirSync(universalRel, { recursive: true });
      for (const binName of ["ferryx-cli", "ferryx-relay"]) {
        const aarch64Bin = join(aarch64Rel, binName);
        const x86Bin = join(x86Rel, binName);
        const universalBin = join(universalRel, binName);
        if (existsSync(aarch64Bin) && existsSync(x86Bin)) {
          execFileSync("lipo", ["-create", aarch64Bin, x86Bin, "-output", universalBin], { stdio: "pipe" });
        }
      }

      // Pre-sign universal binaries with hardened runtime so Tauri bundler packages valid signed components
      if (hostConfig.signingIdentity) {
        for (const binName of ["ferryx", "ferryx-cli", "ferryx-relay"]) {
          const targetBin = join(universalRel, binName);
          if (existsSync(targetBin)) {
            execFileSync("codesign", ["--force", "--options", "runtime", "--sign", hostConfig.signingIdentity, targetBin], {
              stdio: "pipe",
            });
          }
        }
      }

      // Run Tauri bundling now that all universal binaries exist
      execFileSync(
        "bun",
        ["tauri", "build", "--target", "universal-apple-darwin", "--bundles", "app,dmg", "-c", '{"build":{"beforeBuildCommand":""}}'],
        {
          cwd: isolatedSource,
          env: buildEnv,
          stdio: "pipe",
          timeout: timeoutMs,
        },
      );

      // Collect and verify macOS bundle artifacts
      const bundleDir = join(
        cargoTargetDir,
        "universal-apple-darwin",
        "release",
        "bundle",
      );
      const macosDir = join(bundleDir, "macos");
      const appPath = join(macosDir, "Ferryx.app");
      if (!existsSync(appPath)) {
        throw new Error(`Expected macOS app bundle not found at ${appPath}`);
      }

      // 1. Verify universal binary architectures with lipo
      const binaryPath = join(appPath, "Contents", "MacOS", "ferryx");
      if (!existsSync(binaryPath)) {
        throw new Error(`Executable binary not found at ${binaryPath}`);
      }
      const lipoOut = execFileSync("lipo", ["-info", binaryPath], { encoding: "utf8" });
      if (!lipoOut.includes("x86_64") || (!lipoOut.includes("arm64") && !lipoOut.includes("aarch64"))) {
        throw new Error(`macOS binary is not a universal binary (lipo output: ${lipoOut.trim()})`);
      }

      // 2. Verify Info.plist
      const plistPath = join(appPath, "Contents", "Info.plist");
      if (!existsSync(plistPath)) {
        throw new Error(`Info.plist not found at ${plistPath}`);
      }
      const plistText = readFileSync(plistPath, "utf8");
      if (!plistText.includes("<string>com.ferryx.app</string>")) {
        throw new Error("Info.plist bundle identifier is not 'com.ferryx.app'");
      }
      if (!plistText.includes(`<string>${plan.appVersion}</string>`)) {
        throw new Error(`Info.plist version does not match plan version '${plan.appVersion}'`);
      }

      // 3-4. Re-sign all Mach-O binaries, notarize/staple the app, then rebuild
      // and notarize the DMG from that final signed + stapled app.
      const dmgDir = join(bundleDir, "dmg");
      let dmgPath = null;
      if (existsSync(dmgDir)) {
        for (const file of readdirSync(dmgDir)) {
          if (file.endsWith(".dmg")) {
            dmgPath = join(dmgDir, file);
            break;
          }
        }
      }

      finalizeMacosBundle({
        appPath,
        dmgPath,
        workspaceDir,
        signingIdentity: hostConfig.signingIdentity ?? null,
        notaryProfile: hostConfig.notaryProfile ?? null,
        approveNotarization,
      });

      // 5. Create updater tar from verified (and stapled) .app, excluding AppleDouble
      const updaterTarPath = join(artifactsOutDir, "Ferryx.app.tar.gz");
      execFileSync(
        "tar",
        ["--no-xattrs", "-czf", updaterTarPath, "-C", macosDir, "Ferryx.app"],
        {
          env: { ...process.env, COPYFILE_DISABLE: "1" },
          stdio: "pipe",
        },
      );
      if (dmgPath && existsSync(dmgPath)) {
        execFileSync("cp", [dmgPath, join(artifactsOutDir, "Ferryx_universal.dmg")]);
      }
    } else {
      const scpCommand = process.env.FERRYX_SCP_COMMAND || "scp";
      // Bundle transfer and artifact retrieval can be silent for minutes on a
      // busy remote host. Keep the SSH transport alive rather than letting a
      // quiet build look like a dead connection.
      const sshArgs = [
        "-o", "BatchMode=yes",
        "-o", "ConnectTimeout=30",
        "-o", "ServerAliveInterval=15",
        "-o", "ServerAliveCountMax=6",
      ];
      const bundleTarget = hostConfig.platform === "win32"
        ? `${hostConfig.ssh}:${workspaceDir.replaceAll("\\", "/")}/`
        : `${hostConfig.ssh}:${workspaceDir}/`;

      if (hostConfig.platform === "linux") {
        await runHostScript(hostConfig, {
          posix: `set -eu\numask 077\nmkdir -p ${quoteSh(hostConfig.root)}\nmkdir ${quoteSh(workspaceDir)}\n`,
        }, { timeoutMs: 10000 });
      } else {
        await runHostScript(hostConfig, {
          powershell: `$ErrorActionPreference = 'Stop'\nif (-not (Test-Path ${quotePowerShell(hostConfig.root)})) { New-Item -ItemType Directory -Path ${quotePowerShell(hostConfig.root)} -Force | Out-Null }\nNew-Item -ItemType Directory -Path ${quotePowerShell(workspaceDir)} | Out-Null`,
        }, { timeoutMs: 10000 });
      }
      await runProcess(scpCommand, [...sshArgs, sourceBundlePath, ghosttyBundlePath, bundleTarget], { timeoutMs });

      if (hostConfig.platform === "linux") {
        const script = createLinuxBuildScript({ workspaceDir, plan, hostConfig });
        const result = await runHostScript(hostConfig, { posix: script }, { timeoutMs });
        toolchains = parseBuildResult(result.stdout);
        for (const name of ["Ferryx_amd64.AppImage", "Ferryx_amd64.deb"]) {
          await runProcess(scpCommand, [...sshArgs, `${hostConfig.ssh}:${workspaceDir}/out/${name}`, artifactsOutDir], { timeoutMs });
        }
      } else {
        const signingKey = process.env.TAURI_SIGNING_PRIVATE_KEY;
        const signingPassword = process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;
        const secretDir = mkdtempSync(join(tmpdir(), "ferryx-release-secret-"));
        const secretPath = join(secretDir, "tauri-signing-secret.json");
        try {
          if (signingKey) {
            writeFileSync(secretPath, JSON.stringify({ key: signingKey, password: signingPassword ?? "" }), {
              mode: 0o600,
            });
            await runProcess(scpCommand, [...sshArgs, secretPath, `${bundleTarget}tauri-signing-secret.json`], { timeoutMs });
          }
          const script = createWindowsBuildScript({
            workspaceDir,
            plan,
            hostConfig,
            signingSecretFile: signingKey ? "tauri-signing-secret.json" : null,
          });
          const result = await runHostScript(hostConfig, { powershell: script }, { timeoutMs });
          toolchains = parseBuildResult(result.stdout);
          const names = ["Ferryx_x64.msix"];
          if (plan.channels.nsisMigration) names.push("Ferryx_x64-setup.exe");
          for (const name of names) {
            await runProcess(scpCommand, [...sshArgs, `${hostConfig.ssh}:${workspaceDir.replaceAll("\\", "/")}/out/${name}`, artifactsOutDir], { timeoutMs });
          }
        } finally {
          rmSync(secretDir, { recursive: true, force: true });
        }
      }
    }
  }

  if (exitCode !== 0) {
    throw new Error(`Build on host '${hostName}' failed with exit code ${exitCode}`);
  }

  // Sign any updater artifacts that are missing signatures using the coordinator's local key
  for (const file of readdirSync(artifactsOutDir)) {
    if (file.endsWith(".sig")) continue;
    const kind = identifyKind(file, hostName);
    if (!kind) continue;
    const kindDef = KIND_DEFINITIONS[kind];
    if (!kindDef.isUpdater) continue;

    const sigFile = join(artifactsOutDir, `${file}.sig`);
    if (!existsSync(sigFile)) {
      await signUpdaterArtifact({
        artifactPath: join(artifactsOutDir, file),
        repoDir: config.repository,
      });
    }
  }

  // Scan and verify fresh artifacts staged in artifactsOutDir
  const artifacts = scanAndVerifyArtifacts(artifactsOutDir, hostName, plan);
  if (artifacts.length === 0) {
    throw new Error(`No valid build artifacts found in ${artifactsOutDir} for host '${hostName}'`);
  }
  const expectedKinds = requiredKinds(plan).filter((kind) => KIND_DEFINITIONS[kind].permittedHost === hostName);
  const actualKinds = artifacts.map((artifact) => artifact.kind).sort();
  if (actualKinds.join("\n") !== expectedKinds.sort().join("\n")) {
    throw new Error(`Host '${hostName}' produced [${actualKinds.join(", ")}], expected [${expectedKinds.join(", ")}]`);
  }
  for (const artifact of artifacts) {
    if (KIND_DEFINITIONS[artifact.kind].isUpdater && !artifact.signatureRelPath) {
      throw new Error(`Required signature missing for updater artifact '${artifact.name}'`);
    }
  }

  const receipt = createBuildReceipt({
    host: hostName,
    runId: plan.runId,
    commitSha: plan.commitSha,
    appVersion: plan.appVersion,
    exitCode: 0,
    artifacts,
    toolchains,
  });

  // Strict validation against release contract
  const validatedReceipt = parseReceipt(receipt, plan);

  writeFileSync(receiptPath, JSON.stringify(validatedReceipt, null, 2), { flag: "wx" });

  return validatedReceipt;
}
