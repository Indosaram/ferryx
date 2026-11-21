// Keeper payload for release bundles. The plan ships the supervisor and the keeper in ONE release:
// "the build asserts the keeper binary is present in the bundle", and there must be no build in
// which supervised restart can run keeper-less.
//
// The keeper binary is built from its own workspace (`crates/ferryx-ptykeeper`, which declares an
// empty `[workspace]` so it never joins the app's), lipo'd once, and placed inside the app bundle
// at `Contents/Resources/keeper/`. It is copied in by the release scripts rather than declared in
// `tauri.conf.json`'s `bundle.resources` on purpose: a missing resource path makes every
// developer `tauri build` fail, which would force a keeper compile before any app could bundle.
// The release path is what must be impossible to run keeper-less, so the assertion lives there and
// is checked against the bytes that ship.
//
// `keeper.json` beside the binary is the single source the release manifest and `provenance.json`
// quote: version (the install directory name, `~/.ferryx/keeper/<version>/`), protocol major, and
// the sha256 of exactly these bytes. A manifest that disagrees with the binary on disk is refused,
// because the daemon installs a generation under that version and never replaces it.
//
// `version` is the APP version, not the keeper crate's own `0.1.0`
// (`src-tauri/src/daemon/keeper_launchd.rs::keeper_generation_version`). The generation name is an
// identity that must change per release; a constant crate version would collide with the previous
// release's directory and trip the daemon's immutable-bytes refusal on every upgrade. The crate
// version is still recorded, as `crateVersion`, for diagnosis only.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, resolve } from "node:path";

/** The keeper crate, relative to the repository root. */
export const KEEPER_CRATE_REL = "crates/ferryx-ptykeeper";
/** The app crate, relative to the repository root: the value its `CARGO_PKG_VERSION` compiles to. */
export const APP_CRATE_REL = "src-tauri";
/** Binary name inside a generation directory (`ferryx_ptykeeper::install::BINARY_NAME`). */
export const KEEPER_BINARY_NAME = "ferryx-ptykeeper";
/** Manifest written beside the binary; its schema changes only incompatibly. */
export const KEEPER_MANIFEST_NAME = "keeper.json";
export const KEEPER_MANIFEST_SCHEMA = "ferryx-keeper-bundle/v1";
/** Where the keeper payload lives inside a macOS app bundle (`Contents/Resources/keeper`). */
export const KEEPER_RESOURCE_DIR = "keeper";

/** Typed refusal: `code` leads the message so a build failure names what is missing. */
export class KeeperBundleError extends Error {
  constructor(code, message, detail = {}) {
    super(`${code}: ${message}`);
    this.name = "KeeperBundleError";
    this.code = code;
    this.detail = detail;
  }
}

function sha256(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}

/**
 * The generation name must be a three-component version (`looks_like_version` in the keeper crate).
 *
 * A name that is not a version is not retained or ordered like one, and the daemon refuses it, so a
 * bundle that recorded one could never be installed. Caught here, at packaging, not at the user's
 * first upgrade.
 */
function requireGenerationVersion(version) {
  if (typeof version !== "string" || !/^\d+\.\d+\.\d+/.test(version)) {
    throw new KeeperBundleError(
      "KEEPER_VERSION_UNREADABLE",
      `generation name ${JSON.stringify(version ?? null)} is not a three-component version, so it would be refused at install time`,
    );
  }
}

/**
 * `version = "x.y.z"` from the crate's `[package]` table.
 *
 * Read as text rather than through a TOML parser: the release scripts already depend on the build
 * toolchain alone, and the crate file's `[package]` table sits above its first dependency table.
 */
function packageVersion(manifestPath, missingCode) {
  if (!existsSync(manifestPath)) {
    throw new KeeperBundleError(missingCode, `no crate manifest at ${manifestPath}`);
  }
  const text = readFileSync(manifestPath, "utf8");
  const start = text.indexOf("[package]");
  if (start < 0) throw new KeeperBundleError("KEEPER_VERSION_UNREADABLE", `${manifestPath} has no [package] table`);
  const section = text.slice(start + "[package]".length).split(/\n\[/)[0];
  const version = /^\s*version\s*=\s*"([^"]+)"/m.exec(section)?.[1];
  if (!version) throw new KeeperBundleError("KEEPER_VERSION_UNREADABLE", `${manifestPath} has no package version`);
  return version;
}

export function keeperCrateVersion(sourceDir) {
  return packageVersion(join(sourceDir, KEEPER_CRATE_REL, "Cargo.toml"), "KEEPER_CRATE_MISSING");
}

/**
 * `version = "x.y.z"` from the app crate's `[package]` table.
 *
 * This is the string `env!("CARGO_PKG_VERSION")` expands to, which is what
 * `daemon::keeper_launchd::keeper_generation_version()` installs the generation under. The bundled
 * `keeper.json` names the same generation, so the two must be the same value or the daemon installs
 * under one name while the manifest describes another.
 */
export function appCrateVersion(sourceDir) {
  return packageVersion(join(sourceDir, APP_CRATE_REL, "Cargo.toml"), "KEEPER_APP_CRATE_MISSING");
}

/**
 * `PROTOCOL_MAJOR` / `PROTOCOL_MINOR` from the keeper library.
 *
 * The daemon is the side that must understand every keeper generation it may meet, so a bundle that
 * cannot state the protocol it speaks cannot be checked for compatibility by an installer.
 */
export function keeperProtocol(sourceDir) {
  const libPath = join(sourceDir, KEEPER_CRATE_REL, "src", "lib.rs");
  if (!existsSync(libPath)) {
    throw new KeeperBundleError("KEEPER_PROTOCOL_UNREADABLE", `no keeper library at ${libPath}`);
  }
  const text = readFileSync(libPath, "utf8");
  const major = /pub const PROTOCOL_MAJOR:\s*u16\s*=\s*(\d+);/.exec(text)?.[1];
  const minor = /pub const PROTOCOL_MINOR:\s*u16\s*=\s*(\d+);/.exec(text)?.[1];
  if (major === undefined) {
    throw new KeeperBundleError("KEEPER_PROTOCOL_UNREADABLE", `${libPath} declares no PROTOCOL_MAJOR`);
  }
  return { major: Number(major), minor: minor === undefined ? 0 : Number(minor) };
}

/**
 * Builds the keeper for each target triple, lipo's the result into `destDir`, and writes
 * `keeper.json`.
 *
 * Each triple is a separate `cargo build` of the keeper's own workspace. `--target-dir` points at
 * the release's cargo target directory, so the keeper's artifacts neither land in the crate
 * directory inside the isolated source tree nor collide with the app's build output.
 */
export function buildKeeperUniversal({
  sourceDir,
  targetDir,
  destDir,
  triples,
  appVersion,
  env = process.env,
  exec = execFileSync,
  sourceDateEpoch = null,
}) {
  if (!triples || triples.length === 0) {
    throw new KeeperBundleError("KEEPER_NO_TARGETS", "no target triple was given, so no keeper could be built");
  }
  // The generation name is the app version; see the header for why the crate version cannot be it.
  requireGenerationVersion(appVersion);
  const crateVersion = keeperCrateVersion(sourceDir);
  const protocol = keeperProtocol(sourceDir);
  const buildEnv = { ...env, CARGO_TARGET_DIR: targetDir };
  if (sourceDateEpoch !== null) buildEnv.SOURCE_DATE_EPOCH = String(sourceDateEpoch);
  const slices = [];
  for (const triple of triples) {
    exec(
      "cargo",
      [
        "build", "--manifest-path", join(sourceDir, KEEPER_CRATE_REL, "Cargo.toml"),
        "--release", "--target", triple, "--target-dir", targetDir,
        "--bin", KEEPER_BINARY_NAME,
      ],
      { cwd: sourceDir, env: buildEnv, stdio: "pipe" },
    );
    const slice = join(targetDir, triple, "release", KEEPER_BINARY_NAME);
    if (!existsSync(slice)) {
      throw new KeeperBundleError(
        "KEEPER_BUILD_MISSING",
        `cargo reported success but ${slice} does not exist; the keeper was not built for ${triple}`,
      );
    }
    slices.push(slice);
  }
  const staged = stageKeeper({ destDir, slices, version: appVersion, crateVersion, protocol, triples, exec });
  return { ...staged, slices };
}

/**
 * Places `slices` (one per architecture) at `destDir/ferryx-ptykeeper` and records `keeper.json`.
 *
 * One slice is a plain copy; several are `lipo`'d into the universal binary the app bundle ships.
 * The digest is taken from the final file, not from a slice, so it describes the bytes that are
 * installed into `~/.ferryx/keeper/<version>/`.
 */
export function stageKeeper({ destDir, slices, version, crateVersion, protocol, triples = [], exec = execFileSync }) {
  if (!slices || slices.length === 0) {
    throw new KeeperBundleError("KEEPER_NO_SLICES", "no keeper slice to stage");
  }
  requireGenerationVersion(version);
  mkdirSync(destDir, { recursive: true });
  const binary = join(destDir, KEEPER_BINARY_NAME);
  if (slices.length === 1) {
    copyFileSync(slices[0], binary);
  } else {
    exec("lipo", ["-create", ...slices, "-output", binary], { stdio: "pipe" });
  }
  chmodSync(binary, 0o755);
  const bytes = readFileSync(binary);
  const manifest = {
    schema: KEEPER_MANIFEST_SCHEMA,
    version,
    crateVersion: crateVersion ?? null,
    binary: KEEPER_BINARY_NAME,
    protocolMajor: protocol.major,
    protocolMinor: protocol.minor,
    sha256: sha256(bytes),
    bytes: bytes.length,
    architectures: triples.map((triple) => triple.split("-")[0]),
  };
  writeFileSync(join(destDir, KEEPER_MANIFEST_NAME), `${JSON.stringify(manifest, null, 2)}\n`);
  return { dir: destDir, binary, manifest };
}

/**
 * Asserts a staged keeper directory is exactly what it claims: a runnable binary, and a manifest
 * whose version and digest match these bytes.
 *
 * Every failure is typed. `expectedVersion` is the crate version of the source tree the release was
 * built from; a bundle carrying a different version would plant a generation the daemon cannot
 * describe, and a digest that disagrees with the binary means the manifest describes something else.
 */
export function assertKeeperBundled({ resourceDir, expectedVersion = null, expectedCrateVersion = null }) {
  const binary = join(resourceDir, KEEPER_BINARY_NAME);
  const manifestPath = join(resourceDir, KEEPER_MANIFEST_NAME);
  if (!existsSync(binary)) {
    throw new KeeperBundleError(
      "KEEPER_MISSING",
      `no keeper binary at ${binary}; the release would ship without the process that owns PTY masters`,
      { resourceDir },
    );
  }
  const stat = statSync(binary);
  if (!stat.isFile()) throw new KeeperBundleError("KEEPER_NOT_A_FILE", `${binary} is not a regular file`);
  if ((stat.mode & 0o111) === 0) {
    throw new KeeperBundleError(
      "KEEPER_NOT_EXECUTABLE",
      `${binary} is not executable (mode ${(stat.mode & 0o777).toString(8)})`,
    );
  }
  if (!existsSync(manifestPath)) {
    throw new KeeperBundleError("KEEPER_MANIFEST_MISSING", `no ${KEEPER_MANIFEST_NAME} beside ${binary}`);
  }
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  } catch (error) {
    throw new KeeperBundleError("KEEPER_MANIFEST_UNREADABLE", `${manifestPath} is not JSON: ${error.message}`);
  }
  if (manifest.schema !== KEEPER_MANIFEST_SCHEMA) {
    throw new KeeperBundleError(
      "KEEPER_SCHEMA_UNSUPPORTED",
      `${manifestPath} declares schema ${JSON.stringify(manifest.schema)}, expected ${KEEPER_MANIFEST_SCHEMA}`,
    );
  }
  if (typeof manifest.version !== "string" || !/^\d+\.\d+\.\d+/.test(manifest.version)) {
    throw new KeeperBundleError(
      "KEEPER_VERSION_UNREADABLE",
      `${manifestPath} has no version a ~/.ferryx/keeper/<version>/ generation directory accepts`,
    );
  }
  // `manifest.version` is the app version (the generation name); the crate version is separate.
  // Both are checked when the caller knows them, so neither can drift from the build it describes.
  if (expectedVersion !== null && manifest.version !== expectedVersion) {
    throw new KeeperBundleError(
      "KEEPER_VERSION_MISMATCH",
      `${manifestPath} records generation ${manifest.version} but this release is ${expectedVersion}`,
      { expectedVersion, recorded: manifest.version },
    );
  }
  if (expectedCrateVersion !== null && manifest.crateVersion !== expectedCrateVersion) {
    throw new KeeperBundleError(
      "KEEPER_VERSION_MISMATCH",
      `${manifestPath} records crate version ${manifest.crateVersion} but the source tree builds ${expectedCrateVersion}`,
      { expectedCrateVersion, recorded: manifest.crateVersion },
    );
  }
  const actual = sha256(readFileSync(binary));
  if (manifest.sha256 !== actual) {
    throw new KeeperBundleError(
      "KEEPER_DIGEST_MISMATCH",
      `${manifestPath} records sha256 ${manifest.sha256} but ${binary} hashes to ${actual}`,
      { recorded: manifest.sha256, actual },
    );
  }
  return { binary, manifestPath, manifest, sha256: actual };
}

/**
 * The directory name the daemon requires of its own executable's parent.
 *
 * `keeper_startup::bundle_keeper_dir` takes `current_exe().parent()`, refuses anything not named
 * `MacOS`, and only then looks at `../Resources/keeper`. The name is therefore load-bearing: a
 * harness that stages a daemon one directory up gets `PayloadAbsent` from a layout that looks
 * correct to a human.
 */
export const MACOS_DIR = "MacOS";
/** The bundle container directory (`Ferryx.app`). */
export const BUNDLE_CONTENTS_DIR = "Contents";
/** The app executable's name inside `Contents/MacOS` (`src-tauri/Cargo.toml`'s `[[bin]]` name). */
export const DAEMON_BINARY_NAME = "ferryx";

/**
 * The keeper payload directory a daemon at `daemonPath` will read, or `null` when its parent is not
 * `MacOS`.
 *
 * This is the one place the `Contents/MacOS/<exe>` -> `Contents/Resources/keeper` relationship is
 * written down on the JavaScript side, so a harness that stages a bundle and a harness that reports
 * why a payload could not be found cannot disagree about where it should have been.
 */
export function keeperPayloadDirFor(daemonPath) {
  if (typeof daemonPath !== "string" || daemonPath.length === 0) return null;
  const macosDir = dirname(resolve(daemonPath));
  if (basename(macosDir) !== MACOS_DIR) return null;
  return join(dirname(macosDir), "Resources", KEEPER_RESOURCE_DIR);
}

/**
 * Whether `daemonPath` is already laid out the way `bundle_keeper_dir` requires, with the payload
 * present.
 *
 * Used to short-circuit staging: a caller that hands in an already-bundled daemon (the release
 * bundle itself, or a second call in the same run) must not have it copied somewhere else, because
 * the copy would then be the executable under test while the caller's own path is not.
 */
export function isBundledDaemon({ daemonPath }) {
  const payloadDir = keeperPayloadDirFor(daemonPath);
  if (payloadDir === null) return false;
  return existsSync(join(payloadDir, KEEPER_BINARY_NAME));
}

/**
 * Builds the minimal macOS app bundle a daemon needs in order to install its keeper payload.
 *
 * `ensure_keeper` -> `bundle_keeper_dir(current_exe())` reads the *executable's own path* and has no
 * environment override, so a `cargo` output at `target/debug/ferryx` can never activate keeper
 * ownership however it is configured: it answers `PayloadAbsent` and `start_production` aborts before
 * the install, before the routing flag is written, and before any session could be supervised. The
 * only way to exercise the real code path is to put the real bytes where the code looks.
 *
 * Nothing is rebuilt or patched. The daemon and the keeper are byte copies of the builds under test,
 * which keeps the gate's daemon-digest binding honest (`write_chain_evidence` hashes the executable it
 * was handed) and leaves a copied Mach-O's embedded signature intact. What the layout changes is only
 * *where* the daemon looks.
 *
 * `keeper.json` is written by `stageKeeper` from the bytes it just copied, so the digest and version
 * describe the file on disk rather than the file the caller believed it passed.
 */
export function stageAppBundle({
  appPath,
  daemonBinary,
  keeperBinary,
  executableName = DAEMON_BINARY_NAME,
  sourceDir = null,
  version = null,
  crateVersion = null,
  protocol = null,
  triples = [],
}) {
  if (typeof appPath !== "string" || appPath.length === 0) {
    throw new KeeperBundleError("KEEPER_BUNDLE_NO_PATH", "no app bundle path was given");
  }
  for (const [label, value] of [
    ["daemon", daemonBinary],
    ["keeper", keeperBinary],
  ]) {
    if (typeof value !== "string" || value.length === 0) {
      throw new KeeperBundleError("KEEPER_BUNDLE_NO_SOURCE", `no ${label} binary was given to stage`);
    }
    if (!existsSync(value)) {
      throw new KeeperBundleError("KEEPER_BUNDLE_SOURCE_MISSING", `the ${label} binary ${value} does not exist`);
    }
  }
  // The generation name must come from the daemon under test, never from a caller's belief: a bundle
  // labelled with another version installs a generation no shipped daemon ever writes.
  const generation = version ?? (sourceDir !== null ? appCrateVersion(sourceDir) : null);
  requireGenerationVersion(generation);
  const stagedCrateVersion = crateVersion ?? (sourceDir !== null ? keeperCrateVersion(sourceDir) : null);
  const stagedProtocol = protocol ?? (sourceDir !== null ? keeperProtocol(sourceDir) : null);
  if (stagedProtocol === null) {
    throw new KeeperBundleError(
      "KEEPER_PROTOCOL_UNREADABLE",
      "no protocol was given and no source directory was named to read it from",
    );
  }

  const contents = join(appPath, BUNDLE_CONTENTS_DIR);
  const macosDir = join(contents, MACOS_DIR);
  const payloadDir = join(contents, "Resources", KEEPER_RESOURCE_DIR);
  mkdirSync(macosDir, { recursive: true });
  mkdirSync(payloadDir, { recursive: true });

  const daemon = join(macosDir, executableName);
  copyFileSync(daemonBinary, daemon);
  // A copy keeps the bytes but not necessarily the mode; an unexecutable daemon would fail with a
  // message about a missing file, hiding the real cause.
  chmodSync(daemon, 0o755);

  const staged = stageKeeper({
    destDir: payloadDir,
    slices: [keeperBinary],
    version: generation,
    crateVersion: stagedCrateVersion,
    protocol: stagedProtocol,
    triples,
  });

  return {
    appPath,
    contents,
    macosDir,
    daemon,
    payloadDir,
    keeper: staged.binary,
    manifest: staged.manifest,
    manifestPath: join(payloadDir, KEEPER_MANIFEST_NAME),
    daemonSha256: sha256(readFileSync(daemon)),
    keeperSha256: staged.manifest.sha256,
  };
}

/** Copies a staged keeper directory into `Contents/Resources/keeper` of `appPath`. */
export function installKeeperIntoApp({ appPath, stagedDir }) {
  const target = join(appPath, "Contents", "Resources", KEEPER_RESOURCE_DIR);
  mkdirSync(target, { recursive: true });
  for (const name of [KEEPER_BINARY_NAME, KEEPER_MANIFEST_NAME]) {
    const source = join(stagedDir, name);
    if (!existsSync(source)) throw new KeeperBundleError("KEEPER_MISSING", `no staged ${name} in ${stagedDir}`);
    copyFileSync(source, join(target, name));
  }
  chmodSync(join(target, KEEPER_BINARY_NAME), 0o755);
  return target;
}

/**
 * Rewrites `keeper.json`'s digest after the binary changed on disk.
 *
 * `strip` rewrites the Mach-O after the dSYM is archived, so the digest recorded at stage time
 * describes bytes that no longer ship. Nothing compares against this digest today, but a manifest
 * describing a different file is exactly the stale metadata that turns a future integrity check
 * into a false alarm, so it is re-derived from the final bytes.
 */
export function refreshKeeperDigest({ resourceDir }) {
  const binary = join(resourceDir, KEEPER_BINARY_NAME);
  const manifestPath = join(resourceDir, KEEPER_MANIFEST_NAME);
  if (!existsSync(binary) || !existsSync(manifestPath)) {
    throw new KeeperBundleError("KEEPER_MISSING", `no keeper binary and manifest under ${resourceDir}`);
  }
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const bytes = readFileSync(binary);
  manifest.sha256 = sha256(bytes);
  manifest.bytes = bytes.length;
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

/**
 * Asserts the built app bundle really carries the keeper.
 *
 * Runs on the bundle Tauri just produced and before anything is sealed, signed or notarized, so a
 * release cannot reach a dSYM archive or a DMG without the keeper in it.
 */
export function assertKeeperInApp({ appPath, expectedVersion = null, expectedCrateVersion = null }) {
  const resourceDir = join(appPath, "Contents", "Resources", KEEPER_RESOURCE_DIR);
  const checked = assertKeeperBundled({ resourceDir, expectedVersion, expectedCrateVersion });
  return { ...checked, resourceDir };
}

/**
 * The two version expectations a bundle assertion needs, derived in ONE place.
 *
 * They answer different questions and a call site that supplies only one silently loses a check:
 * `expectedVersion` is the generation directory name (the app version), while
 * `expectedCrateVersion` is what the source tree actually builds. Omitting the latter makes
 * `assertKeeperBundled` skip the crate-version comparison entirely, so a bundle whose keeper crate
 * drifted from the daemon it ships beside would still pass.
 *
 * Returning both from one function means a call site cannot pass half the check by accident; it
 * spreads the result and gets the crate version read from the tree that was bundled.
 *
 * The generation name is checked against the app crate's own version here, because that is the
 * value the daemon compiles into `CARGO_PKG_VERSION` and installs the generation under. Nothing
 * else compares them: a caller that read `appVersion` from a different file (a `tauri.conf.json`
 * bumped without its `Cargo.toml`) would otherwise name a generation the shipped daemon does not
 * install, and every bundle assertion would agree with the plan while disagreeing with the binary.
 */
export function keeperExpectations({ sourceDir, appVersion }) {
  const daemonVersion = appCrateVersion(sourceDir);
  if (appVersion !== daemonVersion) {
    throw new KeeperBundleError(
      "KEEPER_APP_VERSION_MISMATCH",
      `the bundle would name keeper generation ${JSON.stringify(appVersion)} but the daemon built ` +
        `from ${join(sourceDir, APP_CRATE_REL)} reports CARGO_PKG_VERSION ${JSON.stringify(daemonVersion)}; ` +
        `the daemon installs its generation under that version, so \`keeper.json\` would describe a ` +
        `generation no shipped daemon ever installs`,
      { appVersion, daemonVersion },
    );
  }
  return { expectedVersion: appVersion, expectedCrateVersion: keeperCrateVersion(sourceDir) };
}

