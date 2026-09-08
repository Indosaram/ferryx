import assert from "node:assert/strict";
import test from "node:test";

import {
  parsePlan,
  parseReceipt,
  requiredKinds,
} from "./lib/release-contract.mjs";

function makeValidPlan(overrides = {}) {
  return {
    schemaVersion: 1,
    runId: "rel-20260908-1",
    repo: "Indosaram/ferryx",
    commitSha: "5d5499806a1b207849778f488b4e3e7b821a751b",
    tag: "v2026.09.08.1",
    appVersion: "2026.908.1",
    msixVersion: "2026.908.1.0",
    channels: {
      store: true,
      nsisMigration: false,
    },
    requiredTargets: [
      "darwin-aarch64",
      "darwin-x86_64",
      "linux-x86_64",
    ],
    toolchains: {
      node: ">=22.0.0",
      bun: ">=1.4.0",
      zig: "0.16.0",
    },
    sourceDateEpoch: 1788868800,
    createdAt: "2026-09-08T12:00:00.000Z",
    ...overrides,
  };
}

function makeValidReceipt(host = "macbook", plan = makeValidPlan(), overrides = {}) {
  let artifacts = [];
  if (host === "macbook") {
    artifacts = [
      {
        kind: "macos-updater",
        name: "Ferryx.app.tar.gz",
        relPath: "darwin/Ferryx.app.tar.gz",
        bytes: 28491024,
        sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: "darwin/Ferryx.app.tar.gz.sig",
        targets: ["darwin-aarch64", "darwin-x86_64"],
      },
      {
        kind: "dmg",
        name: "Ferryx.dmg",
        relPath: "darwin/Ferryx.dmg",
        bytes: 35123456,
        sha256: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
        signatureRelPath: null,
        targets: [],
      },
    ];
  } else if (host === "omaki") {
    artifacts = [
      {
        kind: "appimage",
        name: "Ferryx_amd64.AppImage",
        relPath: "linux/Ferryx_amd64.AppImage",
        bytes: 45123456,
        sha256: "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: "linux/Ferryx_amd64.AppImage.sig",
        targets: ["linux-x86_64"],
      },
      {
        kind: "deb",
        name: "Ferryx_amd64.deb",
        relPath: "linux/Ferryx_amd64.deb",
        bytes: 15123456,
        sha256: "2223456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: null,
        targets: [],
      },
    ];
  } else if (host === "maho-win") {
    artifacts = [
      {
        kind: "msix",
        name: "Ferryx_x64.msix",
        relPath: "windows/Ferryx_x64.msix",
        bytes: 55123456,
        sha256: "3323456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: null,
        targets: [],
      },
    ];
  }

  return {
    schemaVersion: 1,
    runId: plan.runId,
    host,
    commitSha: plan.commitSha,
    appVersion: plan.appVersion,
    completedAt: "2026-09-08T12:30:00.000Z",
    exitCode: 0,
    artifacts,
    ...overrides,
  };
}

// ------------------- parsePlan tests -------------------

test("parsePlan accepts valid standard plan without NSIS migration", () => {
  const plan = makeValidPlan();
  const parsed = parsePlan(plan);
  assert.equal(parsed.runId, plan.runId);
  assert.equal(parsed.schemaVersion, 1);
  assert.equal(parsed.appVersion, "2026.908.1");
  assert.equal(parsed.msixVersion, "2026.908.1.0");
  assert.deepEqual(parsed.requiredTargets, [
    "darwin-aarch64",
    "darwin-x86_64",
    "linux-x86_64",
  ]);
});

test("parsePlan accepts valid plan with NSIS migration and windows-x86_64 target", () => {
  const plan = makeValidPlan({
    channels: { store: true, nsisMigration: true },
    requiredTargets: [
      "darwin-aarch64",
      "darwin-x86_64",
      "linux-x86_64",
      "windows-x86_64",
    ],
    toolchains: {
      node: ">=22.0.0",
      bun: ">=1.4.0",
      zig: "0.16.0",
      rust: "1.85.0",
      tauri: "2.3.0",
    },
  });
  const parsed = parsePlan(plan);
  assert.equal(parsed.channels.nsisMigration, true);
  assert.ok(parsed.requiredTargets.includes("windows-x86_64"));
});

test("parsePlan rejects unknown top-level properties and schema URLs", () => {
  assert.throws(
    () => parsePlan(makeValidPlan({ $schema: "https://ferryx.dev/schemas/release-plan-v1.json" })),
    /unexpected property: \$schema/i,
  );
  assert.throws(
    () => parsePlan(makeValidPlan({ credentials: "secret-token" })),
    /unexpected property: credentials/i,
  );
  assert.throws(
    () => parsePlan(makeValidPlan({ unknownField: true })),
    /unexpected property: unknownField/i,
  );
});

test("parsePlan rejects missing required plan properties", () => {
  const plan = makeValidPlan();
  delete plan.sourceDateEpoch;
  assert.throws(() => parsePlan(plan), /missing required property: sourceDateEpoch/i);
});

test("parsePlan rejects schemaVersion other than 1", () => {
  assert.throws(() => parsePlan(makeValidPlan({ schemaVersion: 2 })), /schemaVersion must be 1/i);
  assert.throws(() => parsePlan(makeValidPlan({ schemaVersion: "1" })), /schemaVersion must be 1/i);
});

test("parsePlan rejects non-canonical or invalid calendar tags", () => {
  assert.throws(() => parsePlan(makeValidPlan({ tag: "v2026.02.29" })), /invalid calendar date|leap year/i);
  assert.throws(() => parsePlan(makeValidPlan({ tag: "v2026.13.01" })), /month must be 1\.\.12/i);
  assert.throws(() => parsePlan(makeValidPlan({ tag: "v2025.12.31" })), /year must be >= 2026/i);
});

test("parsePlan rejects appVersion or msixVersion mismatch with tag", () => {
  assert.throws(
    () => parsePlan(makeValidPlan({ appVersion: "2026.908.2" })),
    /appVersion does not match tag/i,
  );
  assert.throws(
    () => parsePlan(makeValidPlan({ msixVersion: "2026.908.1.5" })),
    /msixVersion does not match tag/i,
  );
});

test("parsePlan rejects target/channel inconsistency", () => {
  // NSIS migration is false, but windows target included
  assert.throws(
    () =>
      parsePlan(
        makeValidPlan({
          channels: { store: true, nsisMigration: false },
          requiredTargets: [
            "darwin-aarch64",
            "darwin-x86_64",
            "linux-x86_64",
            "windows-x86_64",
          ],
        }),
      ),
    /target 'windows-x86_64' requires channels\.nsisMigration to be true/i,
  );

  // NSIS migration is true, but windows target missing
  assert.throws(
    () =>
      parsePlan(
        makeValidPlan({
          channels: { store: true, nsisMigration: true },
          requiredTargets: ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"],
        }),
      ),
    /channels\.nsisMigration requires 'windows-x86_64' in requiredTargets/i,
  );
});

test("parsePlan rejects duplicate targets in requiredTargets", () => {
  assert.throws(
    () =>
      parsePlan(
        makeValidPlan({
          requiredTargets: [
            "darwin-aarch64",
            "darwin-aarch64",
            "darwin-x86_64",
            "linux-x86_64",
          ],
        }),
      ),
    /duplicate target/i,
  );
});

test("parsePlan validates toolchains strict keys", () => {
  // Missing required zig
  assert.throws(
    () =>
      parsePlan(
        makeValidPlan({
          toolchains: { node: ">=22.0.0", bun: ">=1.4.0" },
        }),
      ),
    /missing required toolchain: zig/i,
  );

  // Unknown toolchain key
  assert.throws(
    () =>
      parsePlan(
        makeValidPlan({
          toolchains: {
            node: ">=22.0.0",
            bun: ">=1.4.0",
            zig: "0.16.0",
            python: "3.12",
          },
        }),
      ),
    /unexpected toolchain property: python/i,
  );
});

test("parsePlan validates sourceDateEpoch and createdAt", () => {
  assert.throws(
    () => parsePlan(makeValidPlan({ sourceDateEpoch: -1 })),
    /sourceDateEpoch must be a positive integer/i,
  );
  assert.throws(
    () => parsePlan(makeValidPlan({ sourceDateEpoch: "1788868800" })),
    /sourceDateEpoch must be a positive integer/i,
  );
  assert.throws(
    () => parsePlan(makeValidPlan({ createdAt: "not-a-date" })),
    /createdAt must be a valid ISO 8601 date string/i,
  );
});

// ------------------- requiredKinds tests -------------------

test("requiredKinds returns standard 5 kinds without NSIS migration", () => {
  const plan = makeValidPlan();
  const kinds = requiredKinds(plan);
  assert.deepEqual(kinds, ["macos-updater", "dmg", "appimage", "deb", "msix"]);
});

test("requiredKinds returns 6 kinds when nsisMigration is true", () => {
  const plan = makeValidPlan({
    channels: { store: true, nsisMigration: true },
    requiredTargets: [
      "darwin-aarch64",
      "darwin-x86_64",
      "linux-x86_64",
      "windows-x86_64",
    ],
  });
  const kinds = requiredKinds(plan);
  assert.deepEqual(kinds, ["macos-updater", "dmg", "appimage", "deb", "msix", "nsis"]);
});

// ------------------- parseReceipt tests -------------------

test("parseReceipt accepts valid host receipts bound to plan", () => {
  const plan = makeValidPlan();

  const macReceipt = parseReceipt(makeValidReceipt("macbook", plan), plan);
  assert.equal(macReceipt.host, "macbook");
  assert.equal(macReceipt.exitCode, 0);
  assert.equal(macReceipt.artifacts.length, 2);

  const linuxReceipt = parseReceipt(makeValidReceipt("omaki", plan), plan);
  assert.equal(linuxReceipt.host, "omaki");

  const winReceipt = parseReceipt(makeValidReceipt("maho-win", plan), plan);
  assert.equal(winReceipt.host, "maho-win");
});

test("parseReceipt rejects unknown receipt properties or schema URLs", () => {
  const plan = makeValidPlan();
  assert.throws(
    () =>
      parseReceipt(
        makeValidReceipt("macbook", plan, { $schema: "https://ferryx.dev/schemas/build-receipt-v1.json" }),
        plan,
      ),
    /unexpected property: \$schema/i,
  );
  assert.throws(
    () =>
      parseReceipt(
        makeValidReceipt("macbook", plan, { extraProperty: 123 }),
        plan,
      ),
    /unexpected property: extraProperty/i,
  );
});

test("parseReceipt rejects non-zero exitCode", () => {
  const plan = makeValidPlan();
  assert.throws(
    () => parseReceipt(makeValidReceipt("macbook", plan, { exitCode: 1 }), plan),
    /exitCode must be 0/i,
  );
});

test("parseReceipt rejects mismatched runId, commitSha, or appVersion against plan", () => {
  const plan = makeValidPlan();
  assert.throws(
    () => parseReceipt(makeValidReceipt("macbook", plan, { runId: "mismatch" }), plan),
    /runId mismatch/i,
  );
  assert.throws(
    () =>
      parseReceipt(
        makeValidReceipt("macbook", plan, { commitSha: "0000000000000000000000000000000000000000" }),
        plan,
      ),
    /commitSha mismatch/i,
  );
  assert.throws(
    () => parseReceipt(makeValidReceipt("macbook", plan, { appVersion: "2026.908.9" }), plan),
    /appVersion mismatch/i,
  );
});

test("parseReceipt enforces host-to-kind bindings", () => {
  const plan = makeValidPlan();

  // macbook producing linux artifact
  const invalidMacReceipt = makeValidReceipt("macbook", plan, {
    artifacts: [
      {
        kind: "appimage",
        name: "Ferryx_amd64.AppImage",
        relPath: "linux/Ferryx_amd64.AppImage",
        bytes: 1234,
        sha256: "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: "linux/Ferryx_amd64.AppImage.sig",
        targets: ["linux-x86_64"],
      },
    ],
  });
  assert.throws(() => parseReceipt(invalidMacReceipt, plan), /kind 'appimage' not permitted for host 'macbook'/i);

  // omaki producing macos-updater
  const invalidOmakiReceipt = makeValidReceipt("omaki", plan, {
    artifacts: [
      {
        kind: "macos-updater",
        name: "Ferryx.app.tar.gz",
        relPath: "darwin/Ferryx.app.tar.gz",
        bytes: 1234,
        sha256: "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: "darwin/Ferryx.app.tar.gz.sig",
        targets: ["darwin-aarch64"],
      },
    ],
  });
  assert.throws(() => parseReceipt(invalidOmakiReceipt, plan), /kind 'macos-updater' not permitted for host 'omaki'/i);
});

test("parseReceipt validates targets against artifact kind", () => {
  const plan = makeValidPlan();

  // dmg with targets declared (must be empty)
  const invalidDmg = makeValidReceipt("macbook", plan, {
    artifacts: [
      {
        kind: "dmg",
        name: "Ferryx.dmg",
        relPath: "darwin/Ferryx.dmg",
        bytes: 1234,
        sha256: "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: null,
        targets: ["darwin-aarch64"],
      },
    ],
  });
  assert.throws(() => parseReceipt(invalidDmg, plan), /targets for kind 'dmg' must be empty/i);

  // updater kind with invalid target
  const invalidUpdater = makeValidReceipt("macbook", plan, {
    artifacts: [
      {
        kind: "macos-updater",
        name: "Ferryx.app.tar.gz",
        relPath: "darwin/Ferryx.app.tar.gz",
        bytes: 1234,
        sha256: "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: "darwin/Ferryx.app.tar.gz.sig",
        targets: ["windows-x86_64"],
      },
    ],
  });
  assert.throws(() => parseReceipt(invalidUpdater, plan), /invalid target 'windows-x86_64' for kind 'macos-updater'/i);
});

test("parseReceipt rejects artifact names not matching allowed extensions (e.g. .AppImage.tar.gz and .nsis.zip)", () => {
  const plan = makeValidPlan();

  // omaki appimage with .AppImage.tar.gz instead of .AppImage
  const invalidAppImageReceipt = makeValidReceipt("omaki", plan, {
    artifacts: [
      {
        kind: "appimage",
        name: "Ferryx_amd64.AppImage.tar.gz",
        relPath: "linux/Ferryx_amd64.AppImage.tar.gz",
        bytes: 45123456,
        sha256: "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: "linux/Ferryx_amd64.AppImage.tar.gz.sig",
        targets: ["linux-x86_64"],
      },
      {
        kind: "deb",
        name: "Ferryx_amd64.deb",
        relPath: "linux/Ferryx_amd64.deb",
        bytes: 40123456,
        sha256: "2223456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: null,
        targets: [],
      },
    ],
  });
  assert.throws(
    () => parseReceipt(invalidAppImageReceipt, plan),
    /does not match allowed extensions/i,
  );

  // maho-win nsis with .nsis.zip instead of -setup.exe
  const planWithNsis = makeValidPlan({ channels: { store: true, nsisMigration: true } });
  const invalidNsisReceipt = makeValidReceipt("maho-win", planWithNsis, {
    artifacts: [
      {
        kind: "msix",
        name: "Ferryx_x64.msix",
        relPath: "windows/Ferryx_x64.msix",
        bytes: 55123456,
        sha256: "3323456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: null,
        targets: [],
      },
      {
        kind: "nsis",
        name: "Ferryx_x64.nsis.zip",
        relPath: "windows/Ferryx_x64.nsis.zip",
        bytes: 48123456,
        sha256: "4423456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: "windows/Ferryx_x64.nsis.zip.sig",
        targets: ["windows-x86_64"],
      },
    ],
  });
  assert.throws(
    () => parseReceipt(invalidNsisReceipt, planWithNsis),
    /does not match allowed extensions/i,
  );
});

test("parseReceipt rejects unsafe relative paths and path traversal", () => {
  const plan = makeValidPlan();

  // Traversal in relPath
  const traversalReceipt = makeValidReceipt("macbook", plan, {
    artifacts: [
      {
        kind: "macos-updater",
        name: "Ferryx.app.tar.gz",
        relPath: "../etc/passwd",
        bytes: 1234,
        sha256: "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: "darwin/Ferryx.app.tar.gz.sig",
        targets: ["darwin-aarch64", "darwin-x86_64"],
      },
    ],
  });
  assert.throws(() => parseReceipt(traversalReceipt, plan), /relPath must be a safe relative path/i);

  // Absolute path
  const absoluteReceipt = makeValidReceipt("macbook", plan, {
    artifacts: [
      {
        kind: "macos-updater",
        name: "Ferryx.app.tar.gz",
        relPath: "/tmp/Ferryx.app.tar.gz",
        bytes: 1234,
        sha256: "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: "darwin/Ferryx.app.tar.gz.sig",
        targets: ["darwin-aarch64", "darwin-x86_64"],
      },
    ],
  });
  assert.throws(() => parseReceipt(absoluteReceipt, plan), /relPath must be a safe relative path/i);
});

test("parseReceipt requires signatureRelPath for updater kinds and permits null for installers", () => {
  const plan = makeValidPlan();

  // updater kind missing signatureRelPath
  const missingSigReceipt = makeValidReceipt("macbook", plan, {
    artifacts: [
      {
        kind: "macos-updater",
        name: "Ferryx.app.tar.gz",
        relPath: "darwin/Ferryx.app.tar.gz",
        bytes: 1234,
        sha256: "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: null,
        targets: ["darwin-aarch64", "darwin-x86_64"],
      },
    ],
  });
  assert.throws(() => parseReceipt(missingSigReceipt, plan), /signatureRelPath is required for updater kind 'macos-updater'/i);
});

test("parseReceipt rejects duplicate artifact kinds within a receipt", () => {
  const plan = makeValidPlan();
  const dupReceipt = makeValidReceipt("macbook", plan, {
    artifacts: [
      {
        kind: "macos-updater",
        name: "Ferryx.app.tar.gz",
        relPath: "darwin/Ferryx.app.tar.gz",
        bytes: 1234,
        sha256: "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: "darwin/Ferryx.app.tar.gz.sig",
        targets: ["darwin-aarch64", "darwin-x86_64"],
      },
      {
        kind: "macos-updater",
        name: "Ferryx2.app.tar.gz",
        relPath: "darwin/Ferryx2.app.tar.gz",
        bytes: 1234,
        sha256: "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        signatureRelPath: "darwin/Ferryx2.app.tar.gz.sig",
        targets: ["darwin-aarch64", "darwin-x86_64"],
      },
    ],
  });
  assert.throws(() => parseReceipt(dupReceipt, plan), /duplicate artifact kind 'macos-updater'/i);
});

test("parseReceipt accepts optional valid toolchains in receipt", () => {
  const plan = makeValidPlan();
  const receipt = parseReceipt(
    makeValidReceipt("macbook", plan, {
      toolchains: {
        node: "22.22.0",
        bun: "1.4.0",
        zig: "0.14.0",
        rust: "1.82.0",
        tauri: "2.3.0",
      },
    }),
    plan,
  );
  assert.equal(receipt.toolchains.node, "22.22.0");
  assert.equal(receipt.toolchains.rust, "1.82.0");
});

test("parseReceipt rejects unknown toolchains property or credentials in receipt", () => {
  const plan = makeValidPlan();
  assert.throws(
    () =>
      parseReceipt(
        makeValidReceipt("macbook", plan, {
          toolchains: {
            node: "22.22.0",
            apiToken: "secret_12345",
          },
        }),
        plan,
      ),
    /unexpected toolchain property: apiToken/i,
  );
});

test("parseReceipt rejects invalid toolchain field types in receipt", () => {
  const plan = makeValidPlan();
  assert.throws(
    () =>
      parseReceipt(
        makeValidReceipt("macbook", plan, {
          toolchains: {
            node: 1234,
          },
        }),
        plan,
      ),
    /toolchains\.node must be a non-empty string/i,
  );
});

test("parseReceipt rejects legacy compressed wrappers (.AppImage.tar.gz, .nsis.zip)", () => {
  const plan = makeValidPlan({
    channels: { store: true, nsisMigration: true },
    requiredTargets: [
      "darwin-aarch64",
      "darwin-x86_64",
      "linux-x86_64",
      "windows-x86_64",
    ],
  });

  // 1. appimage named .AppImage.tar.gz
  const invalidAppImageReceipt = makeValidReceipt("omaki", plan);
  invalidAppImageReceipt.artifacts[0].name = "Ferryx_amd64.AppImage.tar.gz";
  invalidAppImageReceipt.artifacts[0].relPath = "linux/Ferryx_amd64.AppImage.tar.gz";
  assert.throws(
    () => parseReceipt(invalidAppImageReceipt, plan),
    /does not match allowed extensions/i,
  );

  // 2. nsis named .nsis.zip
  const invalidNsisZipReceipt = makeValidReceipt("maho-win", plan);
  invalidNsisZipReceipt.artifacts.push({
    kind: "nsis",
    name: "Ferryx_x64.nsis.zip",
    relPath: "windows/Ferryx_x64.nsis.zip",
    bytes: 55123456,
    sha256: "3323456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    signatureRelPath: "windows/Ferryx_x64.nsis.zip.sig",
    targets: ["windows-x86_64"],
  });
  assert.throws(
    () => parseReceipt(invalidNsisZipReceipt, plan),
    /does not match allowed extensions/i,
  );

  // 3. nsis named without -setup.exe (e.g. Ferryx_x64.exe)
  const invalidNsisExeReceipt = makeValidReceipt("maho-win", plan);
  invalidNsisExeReceipt.artifacts.push({
    kind: "nsis",
    name: "Ferryx_x64.exe",
    relPath: "windows/Ferryx_x64.exe",
    bytes: 55123456,
    sha256: "3323456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    signatureRelPath: "windows/Ferryx_x64.exe.sig",
    targets: ["windows-x86_64"],
  });
  assert.throws(
    () => parseReceipt(invalidNsisExeReceipt, plan),
    /does not match allowed extensions/i,
  );
});

