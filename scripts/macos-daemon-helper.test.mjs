import test from "node:test";
import assert from "node:assert/strict";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  HELPER_BUNDLE_ID,
  assembleDaemonHelper,
  helperBundlePath,
  helperExec,
  readEntitlementsXml,
} from "./macos-daemon-helper.mjs";
import { finalizeMacosBundle } from "./lib/release-platforms.mjs";

// Real codesign, ad-hoc identity: only macOS has the tool chain.
function skipUnlessMacos(t) {
  if (process.platform === "darwin") return false;
  t.skip("requires macOS codesign");
  return true;
}

const ALLOW_JIT_ENTITLEMENTS = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.cs.allow-jit</key>
  <true/>
</dict>
</plist>
`;

function outerInfoPlist(version) {
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>ferryx</string>
  <key>CFBundleIdentifier</key>
  <string>com.ferryx.app.fixture</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${version}</string>
  <key>CFBundleVersion</key>
  <string>${version}</string>
</dict>
</plist>
`;
}

/** A tiny finished .app: /usr/bin/true as its executable, ad-hoc signed with one entitlement. */
function createFixtureApp(root, version) {
  const appPath = join(root, "Ferryx.app");
  mkdirSync(join(appPath, "Contents", "MacOS"), { recursive: true });
  copyFileSync("/usr/bin/true", join(appPath, "Contents", "MacOS", "ferryx"));
  writeFileSync(join(appPath, "Contents", "Info.plist"), outerInfoPlist(version));
  const buildEntitlements = join(root, "build-entitlements.plist");
  writeFileSync(buildEntitlements, ALLOW_JIT_ENTITLEMENTS);
  helperExec("codesign", ["--force", "--options", "runtime", "--sign", "-", "--entitlements", buildEntitlements, appPath]);
  return appPath;
}

/**
 * Real exec for codesign/find/file/plutil; the notarization, stapling and
 * Gatekeeper steps are faked because an ad-hoc fixture can never be notarized.
 */
function fakeNotarizationExec(calls) {
  return (command, args, opts = {}) => {
    calls.push([command, ...args].join(" "));
    if (command === "xcrun" && args[0] === "notarytool") {
      return { status: 0, stdout: JSON.stringify({ id: "fixture-submission", status: "Accepted" }), stderr: "" };
    }
    if (command === "xcrun" || command === "ditto" || command === "rm") {
      return { status: 0, stdout: "", stderr: "" };
    }
    if (command === "spctl") {
      return { status: 0, stdout: "", stderr: "accepted\nsource=Notarized Developer ID\n" };
    }
    return helperExec(command, args, opts);
  };
}

function verifyStrict(target, { deep = false } = {}) {
  const args = ["--verify", "--strict", ...(deep ? ["--deep"] : []), "--verbose=2", target];
  return helperExec("codesign", args, { allowFailure: true });
}

/**
 * File offsets of every architecture slice ([0] for a thin Mach-O), so a
 * tamper lands in whichever slice codesign checks on this machine.
 */
function sliceOffsets(bytes) {
  const magic = bytes.readUInt32BE(0);
  if (magic !== 0xcafebabe && magic !== 0xcafebabf) return [0];
  const wide = magic === 0xcafebabf;
  const entrySize = wide ? 32 : 20;
  const count = bytes.readUInt32BE(4);
  const offsets = [];
  for (let i = 0; i < count; i += 1) {
    const at = 8 + i * entrySize + 8;
    offsets.push(wide ? Number(bytes.readBigUInt64BE(at)) : bytes.readUInt32BE(at));
  }
  return offsets;
}

test("assembleDaemonHelper + finalizeMacosBundle: signed helper inside a signed app, entitlements preserved", (t) => {
  if (skipUnlessMacos(t)) return;
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-daemon-helper-"));
  try {
    const version = "2026.1018.1";
    const appPath = createFixtureApp(tmp, version);
    const before = readEntitlementsXml(appPath);
    assert.match(before, /com\.apple\.security\.cs\.allow-jit/, "fixture precondition: outer app carries one entitlement");

    const runDir = join(tmp, "run");
    const entitlements = join(runDir, "outer-entitlements.plist");
    const helper = assembleDaemonHelper({ appPath, identity: "-", entitlements });
    assert.equal(helper.helperPath, helperBundlePath(appPath));
    assert.equal(helper.helperPath, join(appPath, "Contents", "Helpers", "Ferryx Daemon.app"));
    assert.equal(helper.entitlementsPath, entitlements);
    assert.match(readFileSync(entitlements, "utf8"), /allow-jit/);

    const helperPlist = join(helper.helperPath, "Contents", "Info.plist");
    const plistValue = (key) =>
      helperExec("plutil", ["-extract", key, "raw", "-o", "-", helperPlist]).stdout.trim();
    assert.equal(plistValue("CFBundleIdentifier"), HELPER_BUNDLE_ID);
    assert.equal(plistValue("CFBundleExecutable"), "ferryx");
    assert.equal(plistValue("CFBundlePackageType"), "APPL");
    assert.equal(plistValue("LSUIElement"), "true");
    assert.equal(plistValue("CFBundleShortVersionString"), version);
    assert.deepEqual(
      readFileSync(join(helper.helperPath, "Contents", "MacOS", "ferryx")),
      readFileSync(join(appPath, "Contents", "MacOS", "ferryx")),
      "helper executable is a copy of the app executable",
    );

    const workspaceDir = join(tmp, "workspace");
    mkdirSync(workspaceDir, { recursive: true });
    const calls = [];
    finalizeMacosBundle({
      appPath,
      dmgPath: null,
      workspaceDir,
      signingIdentity: "-",
      notaryProfile: "FixtureProfile",
      approveNotarization: true,
      entitlements,
      exec: fakeNotarizationExec(calls),
    });

    const outerVerify = verifyStrict(appPath, { deep: true });
    assert.equal(outerVerify.status, 0, `outer app must verify: ${outerVerify.stderr}`);
    const helperVerify = verifyStrict(helper.helperPath, { deep: true });
    assert.equal(helperVerify.status, 0, `helper bundle must verify: ${helperVerify.stderr}`);
    assert.equal(readEntitlementsXml(appPath).replace(/\s+/g, ""), before.replace(/\s+/g, ""), "outer entitlements unchanged");
    assert.match(readEntitlementsXml(helper.helperPath), /allow-jit/, "helper carries the recorded entitlements");
    assert.ok(
      calls.every((c) => !(c.startsWith("codesign --force") && c.includes("--deep"))),
      "signing never uses --deep",
    );

    // Flip one byte inside each architecture slice of the helper executable: verification must fail.
    const helperBinary = join(helper.helperPath, "Contents", "MacOS", "ferryx");
    const bytes = readFileSync(helperBinary);
    for (const offset of sliceOffsets(bytes)) {
      const at = offset + 0x1000;
      assert.ok(at < bytes.length, "fixture precondition: flipped byte lies inside the binary");
      bytes[at] ^= 0xff;
    }
    writeFileSync(helperBinary, bytes);
    assert.notEqual(verifyStrict(helper.helperPath).status, 0, "byte-flipped helper must fail verification");
    assert.notEqual(verifyStrict(appPath, { deep: true }).status, 0, "outer app with a tampered helper must fail --deep");
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("assembleDaemonHelper: an app without entitlements records an empty file and signs without --entitlements", (t) => {
  if (skipUnlessMacos(t)) return;
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-daemon-helper-noent-"));
  try {
    const appPath = join(tmp, "Ferryx.app");
    mkdirSync(join(appPath, "Contents", "MacOS"), { recursive: true });
    copyFileSync("/usr/bin/true", join(appPath, "Contents", "MacOS", "ferryx"));
    writeFileSync(join(appPath, "Contents", "Info.plist"), outerInfoPlist("2026.1018.2"));
    helperExec("codesign", ["--force", "--options", "runtime", "--sign", "-", appPath]);

    const entitlements = join(tmp, "run", "outer-entitlements.plist");
    const calls = [];
    const exec = (command, args, opts) => {
      calls.push([command, ...args].join(" "));
      return helperExec(command, args, opts);
    };
    const helper = assembleDaemonHelper({ appPath, identity: "-", entitlements, exec });
    assert.equal(helper.entitlementsPath, null);
    assert.equal(readFileSync(entitlements, "utf8"), "");
    const helperSign = calls.find((c) => c.startsWith("codesign --force") && c.endsWith("Ferryx Daemon.app"));
    assert.ok(helperSign, "helper bundle is signed as a bundle");
    assert.ok(!helperSign.includes("--entitlements"), "no entitlements are invented");
    assert.ok(!helperSign.includes("--timestamp"), "ad-hoc identity signs without a secure timestamp");
    assert.equal(verifyStrict(helper.helperPath).status, 0);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});
