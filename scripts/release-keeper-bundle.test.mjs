// Coverage for the keeper bundle contract (scripts/lib/keeper-bundle.mjs).
//
// The release promise is narrow and structural: a shipped macOS app carries a runnable
// `Contents/Resources/keeper/ferryx-ptykeeper` whose recorded version, protocol and digest match
// the bytes beside them. Each case below breaks exactly one of those facts and requires the
// refusal, so none of them can regress into a warning. `cargo` and `lipo` are injected, so no real
// build runs: the negative cases need only files that are the wrong size, mode or content.
import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  KEEPER_BINARY_NAME,
  KEEPER_MANIFEST_NAME,
  KEEPER_MANIFEST_SCHEMA,
  KeeperBundleError,
  assertKeeperBundled,
  assertKeeperInApp,
  appCrateVersion,
  buildKeeperUniversal,
  installKeeperIntoApp,
  keeperCrateVersion,
  keeperExpectations,
  keeperProtocol,
  refreshKeeperDigest,
  stageKeeper,
} from "./lib/keeper-bundle.mjs";
import { buildProvenance } from "./release-provenance.mjs";

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "keeper-bundle-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

/**
 * A source tree with the files the parsers read.
 *
 * `appVersion` is the app crate's `[package] version` — what its `CARGO_PKG_VERSION` compiles to and
 * therefore the generation name the daemon installs under. It defaults to the generation name the
 * staged fixtures use, so a case that is not about version agreement keeps passing.
 */
function writeSource(root, { version = "0.1.0", major = 1, minor = 0, appVersion = "2026.1115.1" } = {}) {
  const crateDir = join(root, "crates", "ferryx-ptykeeper");
  mkdirSync(join(crateDir, "src"), { recursive: true });
  writeFileSync(
    join(crateDir, "Cargo.toml"),
    [
      "[package]",
      'name = "ferryx-ptykeeper"',
      `version = "${version}"`,
      'edition = "2021"',
      "",
      "[dependencies]",
      'serde = "1"',
      "",
    ].join("\n"),
  );
  writeFileSync(
    join(crateDir, "src", "lib.rs"),
    `pub const PROTOCOL_MAJOR: u16 = ${major};\npub const PROTOCOL_MINOR: u16 = ${minor};\n`,
  );
  const appDir = join(root, "src-tauri");
  mkdirSync(appDir, { recursive: true });
  writeFileSync(
    join(appDir, "Cargo.toml"),
    ["[package]", 'name = "ferryx"', `version = "${appVersion}"`, 'edition = "2021"', ""].join("\n"),
  );
  return root;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/** A staged keeper that passes every check, so each case can break one thing. */
function writeStaged(dir, { version = "2026.1115.1", crateVersion = "0.1.0", payload = "keeper-bytes" } = {}) {
  mkdirSync(dir, { recursive: true });
  const binary = join(dir, KEEPER_BINARY_NAME);
  writeFileSync(binary, payload);
  chmodSync(binary, 0o755);
  const bytes = readFileSync(binary);
  writeFileSync(
    join(dir, KEEPER_MANIFEST_NAME),
    `${JSON.stringify(
      {
        schema: KEEPER_MANIFEST_SCHEMA,
        version,
        crateVersion,
        binary: KEEPER_BINARY_NAME,
        protocolMajor: 1,
        protocolMinor: 0,
        sha256: sha256(bytes),
        bytes: bytes.length,
        architectures: ["aarch64", "x86_64"],
      },
      null,
      2,
    )}\n`,
  );
  return dir;
}

function refusal(code, fn) {
  assert.throws(fn, (error) => {
    assert.ok(error instanceof KeeperBundleError, `expected KeeperBundleError, got ${error?.name}: ${error?.message}`);
    assert.equal(error.code, code, `expected ${code}, got ${error.code}: ${error.message}`);
    return true;
  });
}

test("keeperCrateVersion reads the [package] version and refuses a missing crate", (t) => {
  const root = fixture(t);
  writeSource(root, { version: "2026.1115.1" });
  assert.equal(keeperCrateVersion(root), "2026.1115.1");

  // `version.workspace = true` has no literal to read; a guess would name the wrong generation
  // directory, so it is refused rather than defaulted.
  writeFileSync(join(root, "crates", "ferryx-ptykeeper", "Cargo.toml"), "[package]\nversion.workspace = true\n");
  refusal("KEEPER_VERSION_UNREADABLE", () => keeperCrateVersion(root));

  refusal("KEEPER_CRATE_MISSING", () => keeperCrateVersion(join(root, "nope")));
});

test("keeperProtocol reads both protocol numbers and refuses an unreadable library", (t) => {
  const root = fixture(t);
  writeSource(root, { major: 2, minor: 3 });
  assert.deepEqual(keeperProtocol(root), { major: 2, minor: 3 });

  writeFileSync(join(root, "crates", "ferryx-ptykeeper", "src", "lib.rs"), "// no protocol here\n");
  refusal("KEEPER_PROTOCOL_UNREADABLE", () => keeperProtocol(root));
});

test("stageKeeper lipos several slices and records the digest of the staged bytes", (t) => {
  const root = fixture(t);
  const sliceA = join(root, "arm");
  const sliceB = join(root, "x86");
  writeFileSync(sliceA, "arm-slice");
  writeFileSync(sliceB, "x86-slice");

  const calls = [];
  const exec = (command, args) => {
    calls.push([command, ...args].join(" "));
    // Model lipo writing a universal file, so the digest describes that and not a slice.
    writeFileSync(args[args.length - 1], "universal");
    return { status: 0, stdout: "", stderr: "" };
  };
  const destDir = join(root, "staged");
  const staged = stageKeeper({
    destDir,
    slices: [sliceA, sliceB],
    version: "2026.1115.1",
    crateVersion: "0.1.0",
    protocol: { major: 1, minor: 0 },
    triples: ["aarch64-apple-darwin", "x86_64-apple-darwin"],
    exec,
  });

  assert.deepEqual(calls, [`lipo -create ${sliceA} ${sliceB} -output ${join(destDir, KEEPER_BINARY_NAME)}`]);
  assert.equal(staged.manifest.sha256, sha256("universal"));
  assert.deepEqual(staged.manifest.architectures, ["aarch64", "x86_64"]);
  assert.equal(staged.manifest.protocolMajor, 1);
  assert.equal(staged.manifest.version, "2026.1115.1");
  assert.equal(staged.manifest.crateVersion, "0.1.0");
  // The staged binary must be runnable: the daemon execs it directly.
  assert.notEqual(statSync(join(destDir, KEEPER_BINARY_NAME)).mode & 0o111, 0);

  refusal("KEEPER_NO_SLICES", () =>
    stageKeeper({
      destDir: join(root, "empty"),
      slices: [],
      version: "2026.1115.1",
      protocol: { major: 1, minor: 0 },
    }),
  );

  // A name the daemon could not treat as a version is refused at packaging, not at the user's
  // first upgrade. `buildKeeperUniversal` supplies the app version, so this only catches a caller
  // that passes something else -- e.g. a label, or nothing at all.
  refusal("KEEPER_VERSION_UNREADABLE", () =>
    stageKeeper({ destDir: join(root, "bad"), slices: [sliceA], version: "keeper", protocol: { major: 1, minor: 0 } }),
  );
});

test("stageKeeper copies a single slice without invoking lipo", (t) => {
  const root = fixture(t);
  const slice = join(root, "host");
  writeFileSync(slice, "host-bytes");
  let invoked = false;
  const staged = stageKeeper({
    destDir: join(root, "staged"),
    slices: [slice],
    version: "2026.1115.1",
    protocol: { major: 1, minor: 0 },
    exec: () => {
      invoked = true;
      return { status: 0 };
    },
  });
  assert.equal(invoked, false, "a single slice must not be lipo'd");
  assert.equal(staged.manifest.sha256, sha256("host-bytes"));
});

test("buildKeeperUniversal builds each triple and stages the result", (t) => {
  const root = fixture(t);
  writeSource(root);
  const targetDir = join(root, "target");
  const commands = [];
  const exec = (command, args) => {
    commands.push([command, ...args].join(" "));
    if (command === "cargo") {
      // Model rustc writing the per-triple binary where buildKeeperUniversal looks for it.
      const triple = args[args.indexOf("--target") + 1];
      const out = join(targetDir, triple, "release", KEEPER_BINARY_NAME);
      mkdirSync(join(targetDir, triple, "release"), { recursive: true });
      writeFileSync(out, `slice-${triple}`);
      return { status: 0, stdout: "", stderr: "" };
    }
    if (command === "lipo") {
      writeFileSync(args[args.length - 1], "universal-keeper");
      return { status: 0, stdout: "", stderr: "" };
    }
    throw new Error(`unexpected command ${command}`);
  };

  const built = buildKeeperUniversal({
    sourceDir: root,
    targetDir,
    destDir: join(root, "staged"),
    triples: ["aarch64-apple-darwin", "x86_64-apple-darwin"],
    appVersion: "2026.1115.1",
    exec,
    sourceDateEpoch: 1763000000,
  });

  assert.equal(commands.filter((c) => c.startsWith("cargo build")).length, 2);
  assert.ok(commands.some((c) => c.includes("--manifest-path") && c.includes("crates/ferryx-ptykeeper/Cargo.toml")));
  assert.equal(built.manifest.version, "2026.1115.1");
  assert.equal(built.manifest.crateVersion, "0.1.0");
  assert.equal(built.manifest.sha256, sha256("universal-keeper"));
  refusal("KEEPER_NO_TARGETS", () =>
    buildKeeperUniversal({
      sourceDir: root,
      targetDir,
      destDir: join(root, "x"),
      triples: [],
      appVersion: "2026.1115.1",
      exec,
    }),
  );
  // The generation name is required: no appVersion means no installable generation.
  refusal("KEEPER_VERSION_UNREADABLE", () =>
    buildKeeperUniversal({
      sourceDir: root,
      targetDir,
      destDir: join(root, "y"),
      triples: ["aarch64-apple-darwin"],
      exec,
    }),
  );
});

test("buildKeeperUniversal refuses a silent cargo success with no artifact", (t) => {
  const root = fixture(t);
  writeSource(root);
  // cargo exits 0 but writes nothing: a target that was silently skipped, not built.
  refusal("KEEPER_BUILD_MISSING", () =>
    buildKeeperUniversal({
      sourceDir: root,
      targetDir: join(root, "target"),
      destDir: join(root, "staged"),
      triples: ["aarch64-apple-darwin"],
      appVersion: "2026.1115.1",
      exec: () => ({ status: 0, stdout: "", stderr: "" }),
    }),
  );
});

test("assertKeeperBundled accepts a matching bundle and names each broken fact", (t) => {
  const root = fixture(t);
  const good = writeStaged(join(root, "good"));
  const accepted = assertKeeperBundled({ resourceDir: good, expectedVersion: "2026.1115.1" });
  assert.equal(accepted.manifest.version, "2026.1115.1");
  assert.equal(accepted.sha256, sha256("keeper-bytes"));

  refusal("KEEPER_MISSING", () => assertKeeperBundled({ resourceDir: join(root, "absent") }));

  const notExec = writeStaged(join(root, "not-exec"));
  chmodSync(join(notExec, KEEPER_BINARY_NAME), 0o644);
  refusal("KEEPER_NOT_EXECUTABLE", () => assertKeeperBundled({ resourceDir: notExec }));

  const noManifest = writeStaged(join(root, "no-manifest"));
  rmSync(join(noManifest, KEEPER_MANIFEST_NAME));
  refusal("KEEPER_MANIFEST_MISSING", () => assertKeeperBundled({ resourceDir: noManifest }));

  const badJson = writeStaged(join(root, "bad-json"));
  writeFileSync(join(badJson, KEEPER_MANIFEST_NAME), "{ not json");
  refusal("KEEPER_MANIFEST_UNREADABLE", () => assertKeeperBundled({ resourceDir: badJson }));

  const oldSchema = writeStaged(join(root, "old-schema"));
  const oldManifestPath = join(oldSchema, KEEPER_MANIFEST_NAME);
  const oldManifest = JSON.parse(readFileSync(oldManifestPath, "utf8"));
  writeFileSync(oldManifestPath, JSON.stringify({ ...oldManifest, schema: "ferryx-keeper-bundle/v0" }));
  refusal("KEEPER_SCHEMA_UNSUPPORTED", () => assertKeeperBundled({ resourceDir: oldSchema }));

  // A version the crate did not build: the daemon would install a generation it cannot describe.
  refusal("KEEPER_VERSION_MISMATCH", () => assertKeeperBundled({ resourceDir: good, expectedVersion: "2026.1116.1" }));
  refusal("KEEPER_VERSION_MISMATCH", () =>
    assertKeeperBundled({ resourceDir: good, expectedCrateVersion: "0.2.0" }),
  );

  // The manifest describes different bytes: the digest is what makes the generation immutable.
  const tampered = writeStaged(join(root, "tampered"));
  writeFileSync(join(tampered, KEEPER_BINARY_NAME), "different-bytes");
  refusal("KEEPER_DIGEST_MISMATCH", () => assertKeeperBundled({ resourceDir: tampered }));
});

test("installKeeperIntoApp lands the keeper inside the app and assertKeeperInApp sees it", (t) => {
  const root = fixture(t);
  const staged = writeStaged(join(root, "staged"));
  const appPath = join(root, "Ferryx.app");
  mkdirSync(join(appPath, "Contents", "MacOS"), { recursive: true });

  const resourceDir = installKeeperIntoApp({ appPath, stagedDir: staged });
  assert.equal(resourceDir, join(appPath, "Contents", "Resources", "keeper"));
  assert.ok(existsSync(join(resourceDir, KEEPER_BINARY_NAME)));
  assert.notEqual(statSync(join(resourceDir, KEEPER_BINARY_NAME)).mode & 0o111, 0);

  const checked = assertKeeperInApp({ appPath, expectedVersion: "2026.1115.1" });
  assert.equal(checked.manifest.version, "2026.1115.1");

  // An app without the keeper is the state this gate exists to make impossible.
  refusal("KEEPER_MISSING", () =>
    assertKeeperInApp({ appPath: join(root, "Other.app") }),
  );
  refusal("KEEPER_MISSING", () =>
    installKeeperIntoApp({ appPath, stagedDir: join(root, "nothing-here") }),
  );
});

test("refreshKeeperDigest re-derives the digest after strip rewrites the binary", (t) => {
  const root = fixture(t);
  const staged = writeStaged(join(root, "staged"));
  // `strip` changes the bytes the manifest hashed at stage time.
  writeFileSync(join(staged, KEEPER_BINARY_NAME), "stripped-bytes");
  refusal("KEEPER_DIGEST_MISMATCH", () => assertKeeperBundled({ resourceDir: staged }));

  const refreshed = refreshKeeperDigest({ resourceDir: staged });
  assert.equal(refreshed.sha256, sha256("stripped-bytes"));
  assert.equal(refreshed.bytes, Buffer.byteLength("stripped-bytes"));
  // The refused manifest is now consistent, so the bundle validates.
  assert.equal(assertKeeperBundled({ resourceDir: staged }).sha256, sha256("stripped-bytes"));
});

test("keeperExpectations derives both versions so a call site cannot check only one", (t) => {
  const root = fixture(t);
  // The two versions differ on purpose: the generation name is the app version, the crate version
  // is what the tree builds. A helper that conflated them would pass a bundle whose crate drifted.
  writeSource(root, { version: "0.1.0" });

  assert.deepEqual(keeperExpectations({ sourceDir: root, appVersion: "2026.1115.1" }), {
    expectedVersion: "2026.1115.1",
    expectedCrateVersion: "0.1.0",
  });

  // Both keys always exist. `assertKeeperBundled` treats null as "caller does not know", so a
  // missing key would silently disable the crate-version comparison the audit found omitted.
  const expectations = keeperExpectations({ sourceDir: root, appVersion: "2026.1115.1" });
  assert.deepEqual(Object.keys(expectations).sort(), ["expectedCrateVersion", "expectedVersion"]);
  assert.notEqual(expectations.expectedCrateVersion, null);

  // A tree whose crate version disagrees with the manifest is refused once both are passed, which
  // is the check the call site was missing.
  const staged = writeStaged(join(root, "staged"), { crateVersion: "9.9.9" });
  refusal("KEEPER_VERSION_MISMATCH", () =>
    assertKeeperBundled({
      resourceDir: staged,
      ...keeperExpectations({ sourceDir: root, appVersion: "2026.1115.1" }),
    }),
  );
});

test("keeperExpectations refuses a generation name the shipped daemon does not install", (t) => {
  const root = fixture(t);
  // `keeper_generation_version()` is the app crate's CARGO_PKG_VERSION, so that — not a version
  // read from tauri.conf.json — is the name the daemon installs the generation under.
  writeSource(root, { version: "0.1.0", appVersion: "2026.1116.1" });

  // Agreement is the normal case: the generation name is the app crate's version.
  assert.deepEqual(keeperExpectations({ sourceDir: root, appVersion: "2026.1116.1" }), {
    expectedVersion: "2026.1116.1",
    expectedCrateVersion: "0.1.0",
  });

  // A caller that read the version from a file that drifted from the app crate (a bumped
  // tauri.conf.json with a stale Cargo.toml) would seal a generation no shipped daemon installs.
  // Before this check nothing compared the two, so every bundle assertion agreed with the plan
  // while disagreeing with the binary the plan describes.
  refusal("KEEPER_APP_VERSION_MISMATCH", () =>
    keeperExpectations({ sourceDir: root, appVersion: "2026.1115.1" }),
  );

  // The refusal names both versions, because the operator has to know which file to fix.
  assert.throws(
    () => keeperExpectations({ sourceDir: root, appVersion: "2026.1115.1" }),
    (error) => {
      assert.equal(error.code, "KEEPER_APP_VERSION_MISMATCH");
      assert.match(error.message, /2026\.1115\.1/);
      assert.match(error.message, /2026\.1116\.1/);
      assert.deepEqual(error.detail, { appVersion: "2026.1115.1", daemonVersion: "2026.1116.1" });
      return true;
    },
  );

  // A source tree with no app crate cannot answer which generation the daemon installs, so it is a
  // typed refusal rather than a skipped check.
  rmSync(join(root, "src-tauri"), { recursive: true, force: true });
  refusal("KEEPER_APP_CRATE_MISSING", () => appCrateVersion(root));
  refusal("KEEPER_APP_CRATE_MISSING", () =>
    keeperExpectations({ sourceDir: root, appVersion: "2026.1116.1" }),
  );
});

test("provenance records the keeper and stays null without one", () => {
  const plan = {
    repo: "ferryx/ferryx",
    tag: "v2026.1115.1",
    appVersion: "2026.1115.1",
    runId: "run-1",
    commitSha: "0".repeat(40),
    sourceDateEpoch: 1763000000,
    toolchains: { rust: "1.98.0" },
  };
  const keeper = {
    version: "0.1.0",
    protocolMajor: 1,
    protocolMinor: 0,
    binary: "Ferryx.app/Contents/Resources/keeper/ferryx-ptykeeper",
    sha256: "a".repeat(64),
  };
  const withKeeper = buildProvenance({ plan, receipts: [], sbomTools: null, keeper });
  assert.equal(withKeeper.sessionKeeper.version, "0.1.0");
  assert.equal(withKeeper.sessionKeeper.protocolMajor, 1);

  // The rollback floor is owned by scripts/lib/release-keeper.mjs (checked-in ledger); provenance
  // only echoes what it was given, so a caller that omits it records null rather than inventing one.
  const withFloor = buildProvenance({
    plan,
    receipts: [],
    sbomTools: null,
    keeper: { ...keeper, oldestSafeRollbackVersion: "2026.1115.1" },
  });
  assert.equal(withFloor.compatibility.oldestSafeRollbackVersion, "2026.1115.1");

  // Runs without a macOS build (or older run dirs) still assemble: the keys exist and are null,
  // so a consumer reads "unknown", never a crash or a missing field.
  const withoutKeeper = buildProvenance({ plan, receipts: [], sbomTools: null });
  assert.equal(withoutKeeper.sessionKeeper, null);
  assert.equal(withoutKeeper.compatibility.oldestSafeRollbackVersion, null);
});
