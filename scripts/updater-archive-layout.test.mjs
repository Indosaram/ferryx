import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(REPO_ROOT, "scripts", "assert-updater-archive-layout.mjs");

function archiveFixture(withAppleDouble) {
  const dir = mkdtempSync(join(tmpdir(), "updater-archive-layout-"));
  const app = join(dir, "Ferryx.app");
  mkdirSync(join(app, "Contents"), { recursive: true });
  writeFileSync(join(app, "Contents", "Info.plist"), "fixture");
  if (withAppleDouble) writeFileSync(join(dir, "._Ferryx.app"), "metadata");

  const archive = join(dir, "Ferryx.app.tar.gz");
  execFileSync("tar", ["-czf", archive, "Ferryx.app", ...(withAppleDouble ? ["._Ferryx.app"] : [])], {
    cwd: dir,
    env: withAppleDouble ? process.env : { ...process.env, COPYFILE_DISABLE: "1" },
  });
  return { archive, dir };
}

function inspect(archive) {
  return spawnSync(process.execPath, [SCRIPT, archive], { encoding: "utf8" });
}

/**
 * A minimal, real Mach-O slice carrying one `LC_BUILD_VERSION` command.
 *
 * The archive checker reads the minimum-OS floor out of the payload's own load commands (no
 * `otool`), so the fixture has to be a file that parser accepts rather than a text placeholder.
 * 64-bit little-endian header + `LC_BUILD_VERSION` (platform macOS, minos, sdk, ntools 0).
 */
function machoSlice({ cputype, minos, sdk = "14.0" }) {
  const encodeVersion = (text) => {
    const [major, minor, patch] = String(text).split(".").map((part) => Number.parseInt(part, 10) || 0);
    return ((major << 16) | (minor << 8) | patch) >>> 0;
  };
  const header = Buffer.alloc(32);
  header.writeUInt32LE(0xfeedfacf, 0);
  header.writeUInt32LE(cputype >>> 0, 4);
  header.writeUInt32LE(0, 8);
  header.writeUInt32LE(2, 12); // MH_EXECUTE
  header.writeUInt32LE(1, 16); // ncmds
  header.writeUInt32LE(24, 20); // sizeofcmds
  header.writeUInt32LE(0x00200085, 24); // NOUNDEFS | DYLDLINK | PIE
  header.writeUInt32LE(0, 28);
  const command = Buffer.alloc(24);
  command.writeUInt32LE(0x32, 0); // LC_BUILD_VERSION
  command.writeUInt32LE(24, 4);
  command.writeUInt32LE(1, 8); // PLATFORM_MACOS
  command.writeUInt32LE(encodeVersion(minos), 12);
  command.writeUInt32LE(encodeVersion(sdk), 16);
  command.writeUInt32LE(0, 20); // ntools
  return Buffer.concat([header, command]);
}

/** A universal Mach-O holding one slice per architecture, `[{arch, minos}]`. */
function universalMachO(slices) {
  const cpuType = { arm64: 0x0100000c, x86_64: 0x01000007 };
  const bodies = slices.map((entry) => machoSlice({ cputype: cpuType[entry.arch], minos: entry.minos }));
  const headerSize = 8 + 20 * bodies.length;
  const offsets = [];
  let cursor = Math.ceil(headerSize / 4096) * 4096;
  for (const body of bodies) {
    offsets.push(cursor);
    cursor = Math.ceil((cursor + body.length) / 4096) * 4096;
  }
  const out = Buffer.alloc(cursor);
  out.writeUInt32BE(0xcafebabe, 0);
  out.writeUInt32BE(bodies.length, 4);
  bodies.forEach((body, index) => {
    const at = 8 + 20 * index;
    out.writeUInt32BE(cpuType[slices[index].arch], at);
    out.writeUInt32BE(0, at + 4);
    out.writeUInt32BE(offsets[index], at + 8);
    out.writeUInt32BE(body.length, at + 12);
    out.writeUInt32BE(12, at + 16); // align 2^12
    body.copy(out, offsets[index]);
  });
  return out;
}

function infoPlist({ version, minimumSystemVersion = "10.15" }) {
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
\t<key>CFBundleIdentifier</key>
\t<string>com.ferryx.app</string>
\t<key>CFBundleShortVersionString</key>
\t<string>${version}</string>
\t<key>LSMinimumSystemVersion</key>
\t<string>${minimumSystemVersion}</string>
</dict>
</plist>
`;
}

const DAEMON_FLOORS = [
  { arch: "arm64", minos: "11.0" },
  { arch: "x86_64", minos: "10.15" },
];

/**
 * An app bundle carrying a keeper payload, archived the way the release path archives it.
 *
 * `signedAfterManifest` models what code signing does to the keeper when the bundle is sealed: the
 * signature blob is appended, so the binary's bytes stop matching the digest `keeper.json` recorded
 * a moment earlier.
 */
function keeperFixture({
  manifest = "recorded",
  signedAfterManifest = false,
  generation = "2026.1116.1",
  appVersion = "2026.1116.1",
  protocolMajor = 1,
  keeperFloors = DAEMON_FLOORS,
  keeperMode = 0o755,
} = {}) {
  const dir = mkdtempSync(join(tmpdir(), "updater-archive-keeper-"));
  const keeperDir = join(dir, "Ferryx.app", "Contents", "Resources", "keeper");
  mkdirSync(keeperDir, { recursive: true });
  mkdirSync(join(dir, "Ferryx.app", "Contents", "MacOS"), { recursive: true });
  writeFileSync(join(dir, "Ferryx.app", "Contents", "Info.plist"), infoPlist({ version: appVersion }));
  writeFileSync(join(dir, "Ferryx.app", "Contents", "MacOS", "ferryx"), universalMachO(DAEMON_FLOORS));

  const binaryPath = join(keeperDir, "ferryx-ptykeeper");
  const keeperBytes = universalMachO(keeperFloors);
  writeFileSync(binaryPath, keeperBytes);
  chmodSync(binaryPath, keeperMode);
  const digest = createHash("sha256").update(keeperBytes).digest("hex");

  const recorded = manifest === "absent" ? {} : { sha256: manifest === "stale" ? "0".repeat(64) : digest };
  writeFileSync(
    join(keeperDir, "keeper.json"),
    `${JSON.stringify({ schema: "ferryx-keeper-bundle/v1", version: generation, protocolMajor, ...recorded })}\n`,
  );
  writeFileSync(
    join(keeperDir, "latest.json"),
    `${JSON.stringify({ version: generation, keeper: { oldestSafeRollbackVersion: generation } })}\n`,
  );

  if (signedAfterManifest) writeFileSync(binaryPath, "keeper-bytesSIGNATUREBLOB");

  const archive = join(dir, "Ferryx.app.tar.gz");
  execFileSync("tar", ["-czf", archive, "Ferryx.app"], {
    cwd: dir,
    env: { ...process.env, COPYFILE_DISABLE: "1" },
  });
  return { archive, dir };
}

test("accepts a keeper whose recorded digest matches the bytes in the archive", () => {
  const { archive, dir } = keeperFixture();
  try {
    const result = inspect(archive);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("rejects a keeper rewritten after its manifest was written", () => {
  // Code signing appends a signature blob while sealing the bundle, and the daemon refuses a
  // generation whose recorded digest disagrees with the binary it installs -- then boots without a
  // keeper, logging only a warning. A release must not be able to publish those bytes.
  const { archive, dir } = keeperFixture({ signedAfterManifest: true });
  try {
    const result = inspect(archive);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /records sha256 .*but the bundled ferryx-ptykeeper hashes to/);
    assert.match(result.stderr, /boot without a keeper/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("rejects a keeper manifest with no digest, and one with no binary beside it", () => {
  const noDigest = keeperFixture({ manifest: "absent" });
  try {
    const result = inspect(noDigest.archive);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /records no sha256/);
  } finally {
    rmSync(noDigest.dir, { recursive: true, force: true });
  }

  const orphan = mkdtempSync(join(tmpdir(), "updater-archive-orphan-"));
  const keeperDir = join(orphan, "Ferryx.app", "Contents", "Resources", "keeper");
  mkdirSync(keeperDir, { recursive: true });
  writeFileSync(join(keeperDir, "keeper.json"), `${JSON.stringify({ sha256: "a".repeat(64) })}\n`);
  const archive = join(orphan, "Ferryx.app.tar.gz");
  execFileSync("tar", ["-czf", archive, "Ferryx.app"], { cwd: orphan, env: { ...process.env, COPYFILE_DISABLE: "1" } });
  try {
    const result = inspect(archive);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /no ferryx-ptykeeper beside it/);
  } finally {
    rmSync(orphan, { recursive: true, force: true });
  }
});

test("leaves an archive with no keeper payload alone", () => {
  // The keeper check is about a payload that is present: whether one must be present is asserted on
  // the bundle during the build, and an older archive without one is not this checker's failure.
  const { archive, dir } = archiveFixture(false);
  try {
    const result = inspect(archive);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("rejects a keeper generation that is not the app version in the archive", () => {
  // The daemon installs its generation under its own CARGO_PKG_VERSION. A payload naming a
  // different version is installed under a directory this manifest never describes, and the
  // rollback floor reasons about a generation that does not exist.
  const { archive, dir } = keeperFixture({ generation: "2026.1110.1", appVersion: "2026.1116.1" });
  try {
    const result = inspect(archive);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /records generation "2026\.1110\.1" but the app bundle is version "2026\.1116\.1"/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("rejects a keeper payload that speaks another protocol major", () => {
  // Compatibility within a major version is the whole compatibility story; a payload from a
  // different major would be installed and then fail every frame it is sent.
  const { archive, dir } = keeperFixture({ protocolMajor: 2 });
  try {
    const result = inspect(archive);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /declares protocol major 2; this release's daemon speaks 1/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("rejects a keeper payload archived without an execute bit", () => {
  // The install copies the payload and runs it; a member without the bit produces a generation
  // directory that exists and cannot be launched, which the boot path only logs as a warning.
  const { archive, dir } = keeperFixture({ keeperMode: 0o644 });
  try {
    const result = inspect(archive);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /is archived mode 644.*no execute bit/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("rejects a keeper slice that requires a newer macOS than the daemon it runs beside", () => {
  // arm64 legitimately differs from x86_64 (11.0 vs 10.15), so the comparison is per arch. Here the
  // keeper's x86_64 slice is built against a newer floor than the daemon's, which would make the app
  // start on 10.15 and its keeper never run there.
  const { archive, dir } = keeperFixture({
    keeperFloors: [
      { arch: "arm64", minos: "11.0" },
      { arch: "x86_64", minos: "12.0" },
    ],
  });
  try {
    const result = inspect(archive);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /x86_64 slice of Ferryx\.app\/Contents\/Resources\/keeper\/ferryx-ptykeeper requires macOS 12\.0/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("accepts a keeper whose per-arch floors match the daemon's, and one stricter than the app plist", () => {
  // The arm64 floor (11.0) is newer than LSMinimumSystemVersion (10.15) and that is correct: the app
  // supports 10.15 through its x86_64 slice. Only a keeper floor newer than the DAEMON's own slice
  // of the same architecture is a defect, so this archive must pass.
  const { archive, dir } = keeperFixture();
  try {
    const result = inspect(archive);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("rejects updater archives containing a top-level AppleDouble entry", () => {
  const { archive, dir } = archiveFixture(true);
  try {
    const result = inspect(archive);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /AppleDouble.*\._Ferryx\.app/i);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("accepts updater archives whose entries are rooted in the app bundle", () => {
  const { archive, dir } = archiveFixture(false);
  try {
    const result = inspect(archive);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
