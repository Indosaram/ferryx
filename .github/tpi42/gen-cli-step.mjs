// Cuts the cli-windows-amd64 build step out of the GENERATED release build script
// (createWindowsBuildScript) and prints it as a standalone PowerShell script that runs in this
// checkout. Only the lines of the generated CLI step are kept, unmodified: the
// CARGO_ENCODED_RUSTFLAGS guard and the block from the "+crt-static" comment through the
// Copy-Item of the shipped copy. The dumpbin lookup and Get-FerryxDependents come from
// createWindowsFloorScanScript, also unmodified.
// usage: bun gen-cli-step.mjs <sourceDir> <cargoTarget> <outDir>
import { createWindowsBuildScript, createWindowsFloorScanScript } from "../../scripts/lib/release-platforms.mjs";

const [sourceDir, cargoTarget, outDir] = process.argv.slice(2);
const plan = {
  schemaVersion: 1,
  runId: "tpi42",
  repo: "Indosaram/ferryx",
  commitSha: "94c4a3f5".padEnd(40, "0"),
  tag: "v2026.10.27.5",
  appVersion: "2026.1027.5",
  msixVersion: "2026.1027.5.0",
  channels: { store: true, nsisMigration: false },
  sourceDateEpoch: 1793059200,
};
const full = createWindowsBuildScript({ workspaceDir: "C:\\tpi42-unused", plan, hostConfig: { root: null } });
const lines = full.split("\n");
const guardAt = lines.findIndex((l) => l.startsWith("if (Test-Path Env:CARGO_ENCODED_RUSTFLAGS)"));
const startAt = lines.findIndex((l) => l.startsWith("# The desktop-free CLI skips tauri-build"));
const endAt = lines.findIndex((l) => l.startsWith("Copy-Item -LiteralPath $cliExe"));
if (guardAt < 0 || startAt < 0 || endAt < startAt) throw new Error("generated script layout changed; cannot cut the CLI step");
const cliStep = [lines[guardAt], ...lines.slice(startAt, endAt + 1)];
const body = cliStep.join("\n");
for (const needle of ["-C target-feature=+crt-static", "--release --no-default-features --bin ferryx-cli"]) {
  if (!body.includes(needle)) throw new Error(`generated CLI step lacks ${needle}`);
}
const floor = createWindowsFloorScanScript({ nsisMigration: false }).split("\n");
const depsEnd = floor.findIndex((l) => l.startsWith("$appDependents"));
const q = (s) => `'${s.replaceAll("'", "''")}'`;
process.stdout.write(`$ErrorActionPreference = 'Stop'
$sourceDir = ${q(sourceDir)}
$cargoTarget = ${q(cargoTarget)}
$env:CARGO_TARGET_DIR = $cargoTarget
$outDir = ${q(outDir)}
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
# ---- generated CLI step (createWindowsBuildScript, lines ${guardAt + 1} and ${startAt + 1}-${endAt + 1}) ----
${body}
# ---- generated floor-scan helpers (createWindowsFloorScanScript, lines 1-${depsEnd}) ----
${floor.slice(0, depsEnd).join("\n")}
$cliDependents = Get-FerryxDependents (Join-Path $outDir 'ferryx-cli-windows-amd64.exe')
Write-Output '---CLI_DEPENDENTS---'
Write-Output (ConvertTo-Json -Compress -InputObject @($cliDependents))
`);
