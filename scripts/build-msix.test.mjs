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

test("build-msix.ps1: enforces isolated fresh staging and removes stale output before packing", () => {
  assert.match(
    scriptContent,
    /NewGuid\(\)|ferryx-msix-staging/i,
    "Script must create a fresh isolated staging directory per build",
  );
  assert.match(
    scriptContent,
    /Remove-Item\s+(-Path\s+)?\$msixOutputFile/i,
    "Script must remove any pre-existing output MSIX before packing to prevent stale success",
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

test("build-msix.ps1: validates packaged MSIX manifest version and name after pack", () => {
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
    "Script must validate Identity Name and Version in the packaged manifest",
  );
});

// Canonical version normalization contract test suite
function canonicalizeVersion(raw) {
  if (!raw || typeof raw !== "string" || raw.trim().length === 0) {
    throw new Error("Version string cannot be empty.");
  }
  let v = raw.trim();
  if (v.startsWith("v") || v.startsWith("V")) {
    v = v.slice(1);
  }

  const dateMatch = v.match(/^(\d{4})\.(\d{1,2})\.(\d{1,2})(?:\.(\d+))?$/);
  if (dateMatch) {
    const year = Number(dateMatch[1]);
    const month = Number(dateMatch[2]);
    const day = Number(dateMatch[3]);
    const rev = dateMatch[4] !== undefined ? Number(dateMatch[4]) : 0;

    if (year < 2026) {
      throw new Error(`Invalid release year: ${year}. Year must be >= 2026.`);
    }
    if (month < 1 || month > 12) {
      throw new Error(`Invalid release month: ${month}.`);
    }
    const daysInMonth = new Date(Date.UTC(year, month, 0)).getUTCDate();
    if (day < 1 || day > daysInMonth) {
      throw new Error(`Invalid release day: ${day} for month ${month} in year ${year}.`);
    }
    if (rev < 0 || rev > 65535) {
      throw new Error(`Revision ${rev} out of range (0..65535).`);
    }
    const appMinor = month * 100 + day;
    return {
      appVersion: `${year}.${appMinor}.${rev}`,
      msixVersion: `${year}.${appMinor}.${rev}.0`,
    };
  }

  const parts = v.split(".");
  if (parts.length === 4) {
    for (const p of parts) {
      if (!/^\d+$/.test(p)) throw new Error(`Version part '${p}' is not numeric.`);
      const num = Number(p);
      if (num < 0 || num > 65535) throw new Error(`Version part ${num} out of range (0..65535).`);
    }
    const [p0, p1, p2, p3] = parts.map(Number);
    if (p3 !== 0) {
      throw new Error(`MSIX Store packages require the 4th quad component to be 0 for Store ingestion (got revision ${p3}).`);
    }
    return {
      appVersion: `${p0}.${p1}.${p2}`,
      msixVersion: `${p0}.${p1}.${p2}.0`,
    };
  }

  if (parts.length === 3) {
    for (const p of parts) {
      if (!/^\d+$/.test(p)) throw new Error(`Version part '${p}' is not numeric.`);
      const num = Number(p);
      if (num < 0 || num > 65535) throw new Error(`Version part ${num} out of range (0..65535).`);
    }
    const [p0, p1, p2] = parts.map(Number);
    return {
      appVersion: `${p0}.${p1}.${p2}`,
      msixVersion: `${p0}.${p1}.${p2}.0`,
    };
  }

  throw new Error(`Invalid version format '${raw}'.`);
}

test("version contract: maps date tags, semver, and quad versions correctly", () => {
  const t1 = canonicalizeVersion("v2026.09.08.1");
  assert.equal(t1.appVersion, "2026.908.1");
  assert.equal(t1.msixVersion, "2026.908.1.0");

  const t2 = canonicalizeVersion("v2026.09.08");
  assert.equal(t2.appVersion, "2026.908.0");
  assert.equal(t2.msixVersion, "2026.908.0.0");

  const t3 = canonicalizeVersion("2026.908.1");
  assert.equal(t3.appVersion, "2026.908.1");
  assert.equal(t3.msixVersion, "2026.908.1.0");

  const t4 = canonicalizeVersion("2026.908.1.0");
  assert.equal(t4.appVersion, "2026.908.1");
  assert.equal(t4.msixVersion, "2026.908.1.0");

  const t5 = canonicalizeVersion("v2028.02.29");
  assert.equal(t5.appVersion, "2028.229.0");
  assert.equal(t5.msixVersion, "2028.229.0.0");
});

test("version contract: rejects non-zero 4th quad component for Store submission", () => {
  assert.throws(
    () => canonicalizeVersion("2026.908.1.5"),
    /MSIX Store packages require the 4th quad component to be 0/,
  );
  assert.throws(
    () => canonicalizeVersion("1.0.0.1"),
    /MSIX Store packages require the 4th quad component to be 0/,
  );
});

test("version contract: rejects invalid calendar dates", () => {
  assert.throws(() => canonicalizeVersion("v2026.02.29"), /Invalid release day/);
  assert.throws(() => canonicalizeVersion("v2026.04.31"), /Invalid release day/);
  assert.throws(() => canonicalizeVersion("v2026.13.01"), /Invalid release month/);
  assert.throws(() => canonicalizeVersion("v2025.12.01"), /Year must be >= 2026/);
});
