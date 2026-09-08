import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const SCRIPT_PATH = resolve(__dirname, "build-msix.ps1");
const scriptContent = readFileSync(SCRIPT_PATH, "utf8");

test("build-msix.ps1: requires explicit -ExePath parameter and removes candidate guessing", () => {
  assert.match(
    scriptContent,
    /\[Parameter\s*\([^)]*Mandatory\s*=\s*\$true[^)]*\)\]\s*\[string\]\$ExePath/i,
    "build-msix.ps1 must declare [Parameter(Mandatory = $true)] [string]$ExePath",
  );
  assert.doesNotMatch(
    scriptContent,
    /\$binaryCandidates/i,
    "Candidate guessing array ($binaryCandidates) must be removed",
  );
  assert.doesNotMatch(
    scriptContent,
    /src-tauri\/target\/release\/ferryx\.exe/i,
    "Hardcoded candidate binary paths must be removed",
  );
});

test("build-msix.ps1: enforces canonical Store quad version contract (4th component = 0)", () => {
  assert.match(
    scriptContent,
    /MSIX Store packages require the 4th quad component to be 0|Store packages require.*4th.*0/i,
    "Script must explicitly enforce that the 4th quad component is 0 for Store packages",
  );
  assert.match(
    scriptContent,
    /DaysInMonth/i,
    "Script must validate calendar day limits (including leap years) for date tags",
  );
});

test("build-msix.ps1: verifies binary version against expected app version", () => {
  assert.match(
    scriptContent,
    /FileVersionInfo/i,
    "Script must inspect binary FileVersionInfo",
  );
  assert.match(
    scriptContent,
    /ProductVersion|FileVersion/i,
    "Script must compare binary ProductVersion or FileVersion against expected app version",
  );
  assert.match(
    scriptContent,
    /Binary version.*does not match/i,
    "Script must throw when binary version does not match expected app version",
  );
});

test("build-msix.ps1: enforces isolated fresh staging and fails closed if output already exists", () => {
  assert.match(
    scriptContent,
    /NewGuid\(\)|ferryx-msix-staging/i,
    "Script must create a fresh isolated staging directory per build",
  );
  assert.match(
    scriptContent,
    /Test-Path\s+(-Path\s+)?\$msixOutputFile[\s\S]*?(throw|Output package already exists|Refusing to overwrite)/i,
    "Script must check if output MSIX exists and throw/fail closed instead of deleting it",
  );
  assert.doesNotMatch(
    scriptContent,
    /Remove-Item\s+(-Path\s+)?\$msixOutputFile/i,
    "Script must not delete pre-existing output MSIX",
  );
});

test("build-msix.ps1: checks native tool exit codes after MakeAppx and SignTool", () => {
  assert.match(
    scriptContent,
    /& \$makeAppx[\s\S]*?\$LASTEXITCODE\s*-ne\s*0/i,
    "Script must check $LASTEXITCODE immediately after MakeAppx execution",
  );
  assert.match(
    scriptContent,
    /& \$signTool[\s\S]*?\$LASTEXITCODE\s*-ne\s*0/i,
    "Script must check $LASTEXITCODE immediately after SignTool execution",
  );
});

test("build-msix.ps1: removes auto-generated cert, hardcoded password, and silent fallback", () => {
  assert.doesNotMatch(
    scriptContent,
    /New-SelfSignedCertificate/i,
    "Script must not auto-generate self-signed certificates",
  );
  assert.doesNotMatch(
    scriptContent,
    /FerryxMsixSignPass2026!/i,
    "Script must not contain hardcoded signing passwords",
  );
  assert.doesNotMatch(
    scriptContent,
    /Package remains unsigned/i,
    "Script must not silently fallback to unsigned when signing was requested",
  );
});

test("build-msix.ps1: supports explicit -SkipSigning and requires valid -CertThumbprint for sideload", () => {
  assert.match(
    scriptContent,
    /\[switch\]\$SkipSigning/i,
    "Script must support -SkipSigning switch",
  );
  assert.match(
    scriptContent,
    /\[string\]\$CertThumbprint/i,
    "Script must accept optional -CertThumbprint parameter",
  );
  assert.match(
    scriptContent,
    /Cert:\\CurrentUser\\My|Cert:\\LocalMachine\\My/i,
    "Script must look up certificate by thumbprint in Windows certificate store",
  );
});

test("build-msix.ps1: validates packaged MSIX manifest identity (Name, Version, Publisher, ProcessorArchitecture=x64)", () => {
  assert.match(
    scriptContent,
    /System\.IO\.Compression\.ZipFile|OpenRead/i,
    "Script must inspect packaged MSIX archive directly",
  );
  assert.match(
    scriptContent,
    /AppxManifest\.xml/i,
    "Script must inspect AppxManifest.xml inside the packaged MSIX",
  );
  assert.match(
    scriptContent,
    /\.Identity|\.Package\.Identity/i,
    "Script must inspect Identity node in the packaged manifest",
  );
  assert.match(
    scriptContent,
    /Publisher/i,
    "Script must validate Identity Publisher in the packaged manifest",
  );
  assert.match(
    scriptContent,
    /ProcessorArchitecture.*x64|x64.*ProcessorArchitecture/i,
    "Script must validate Identity ProcessorArchitecture is x64 in the packaged manifest",
  );
});
