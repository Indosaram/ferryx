#!/usr/bin/env bun
/**
 * Local release of the on-demand Chromium engine (crates/ferryx-browser-engine).
 *
 * Run by the owner on the build host of each OS; releases are local-only (see
 * scripts/release-workflow-policy.mjs), so no hosted workflow may run this.
 *
 *   bun scripts/browser-engine/release-engine.mjs keygen --out <dir>
 *       New ed25519 manifest key: <dir>/engine-manifest.key (PKCS#8 PEM, mode 0600) and the
 *       public key line to put in TRUSTED_MANIFEST_KEYS (src-tauri/src/browser/engine_install.rs).
 *   bun scripts/browser-engine/release-engine.mjs build --out <dir> [--identity <name>] [--adhoc]
 *       [--notarize --notary-profile <profile>] [--signtool-cert <thumbprint>]
 *       Builds and bundles the engine for this OS (release profile), signs it (macOS:
 *       Developer ID, hardened runtime, inside-out; Windows: Authenticode when a certificate
 *       is given, else the artifact is recorded as unsigned), zips it into
 *       <dir>/engine-<platform>.zip and writes <dir>/engine-<platform>.json (sha256, size,
 *       signing record). Notarization runs only with --notarize. --adhoc signs with "-" (local
 *       tests only; such an engine fails Gatekeeper and is recorded as not code-signed).
 *       FERRYX_ENGINE_SIGNTOOL_CERT may name the Windows certificate thumbprint instead.
 *   bun scripts/browser-engine/release-engine.mjs manifest --dir <dir> --key <file>
 *       Writes <dir>/manifest.json from every engine-<platform>.json in <dir> and signs it into
 *       <dir>/manifest.json.sig. The key path may also come from FERRYX_ENGINE_SIGNING_KEY.
 *   bun scripts/browser-engine/release-engine.mjs verify --dir <dir> --public-key <base64>
 *       Checks manifest.json.sig and every listed archive's size and sha256, as the app does.
 *   bun scripts/browser-engine/release-engine.mjs publish --dir <dir> --public-key <base64> [--publish]
 *       Uploads manifest.json, manifest.json.sig and the archives to the GitHub release
 *       browser-engine-<version> on Indosaram/ferryx (creates it as a draft, verifies the
 *       uploaded bytes, then publishes). Without --publish it prints what it would do.
 *
 * Keys never enter the repository: keygen refuses a path inside it, and the signing key is
 * read from a path given on the command line or by environment.
 */
import { spawnSync } from "node:child_process";
import { createHash, createPrivateKey, createPublicKey, generateKeyPairSync, sign, verify } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const ENGINE_CRATE = join(REPO_ROOT, "crates/ferryx-browser-engine");
export const GITHUB_REPO = "Indosaram/ferryx";
export const DEFAULT_IDENTITY = "Developer ID Application: Indo Yoon (5DUM8WPB4C)";
export const SIGNING_KEY_ENV = "FERRYX_ENGINE_SIGNING_KEY";
const ENTITLEMENTS_DIR = join(REPO_ROOT, "scripts/browser-engine/entitlements");

/** The engine version: ENGINE_VERSION in engine_install.rs, which the app pins to. */
export function engineVersion(source = readFileSync(join(REPO_ROOT, "src-tauri/src/browser/engine_install.rs"), "utf8")) {
  const match = /pub const ENGINE_VERSION: &str = "([^"]+)";/.exec(source);
  if (!match) throw new Error("ENGINE_VERSION not found in engine_install.rs");
  return match[1];
}

export function releaseTag(version) {
  return `browser-engine-${version}`;
}

export function assetUrl(version, file) {
  return `https://github.com/${GITHUB_REPO}/releases/download/${releaseTag(version)}/${file}`;
}

/** The manifest platform key for an OS/arch pair, as `platform_key()` in engine_install.rs. */
export function platformKey(os = process.platform, arch = process.arch) {
  if (os === "darwin" && arch === "arm64") return "macos-aarch64";
  if (os === "win32" && arch === "x64") return "windows-x86_64";
  return null;
}

export function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function isInside(parent, child) {
  const rel = relative(resolve(parent), resolve(child));
  return rel === "" || (!rel.startsWith("..") && !rel.startsWith("/") && !/^[A-Za-z]:/.test(rel));
}

/** Raw 32-byte ed25519 public key, base64: the format TRUSTED_MANIFEST_KEYS holds. */
export function rawPublicKeyBase64(publicKey) {
  const der = publicKey.export({ format: "der", type: "spki" });
  return Buffer.from(der.subarray(der.length - 32)).toString("base64");
}

export function publicKeyFromRaw(base64) {
  const raw = Buffer.from(base64.trim(), "base64");
  if (raw.length !== 32) throw new Error("public key must be 32 bytes of base64");
  const prefix = Buffer.from("302a300506032b6570032100", "hex");
  return createPublicKey({ key: Buffer.concat([prefix, raw]), format: "der", type: "spki" });
}

/** Writes a new key pair outside the repository; returns the public key line. */
export function keygen({ out, repoRoot = REPO_ROOT }) {
  if (!out) throw new Error("keygen needs --out <dir> (outside the repository)");
  if (isInside(repoRoot, out)) throw new Error(`refusing to write a private key inside the repository: ${out}`);
  mkdirSync(out, { recursive: true });
  const keyPath = join(out, "engine-manifest.key");
  if (existsSync(keyPath)) throw new Error(`${keyPath} exists; move it away first`);
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  writeFileSync(keyPath, privateKey.export({ format: "pem", type: "pkcs8" }), { mode: 0o600 });
  chmodSync(keyPath, 0o600);
  const publicBase64 = rawPublicKeyBase64(publicKey);
  writeFileSync(join(out, "engine-manifest.pub"), `${publicBase64}\n`);
  return { keyPath, publicKey: publicBase64 };
}

export function loadSigningKey(path) {
  if (!path) throw new Error(`no signing key: pass --key <file> or set ${SIGNING_KEY_ENV}`);
  const key = createPrivateKey(readFileSync(path));
  if (key.asymmetricKeyType !== "ed25519") throw new Error("the signing key is not ed25519");
  return key;
}

/** The manifest the app downloads, from the per-platform records `build` wrote. */
export function buildManifest({ version, records }) {
  if (records.length === 0) throw new Error("no engine-<platform>.json records found");
  const platforms = {};
  for (const record of records) {
    if (record.version !== version) {
      throw new Error(`${record.platform} was built as ${record.version}, the manifest is ${version}`);
    }
    platforms[record.platform] = { url: assetUrl(version, record.file), sha256: record.sha256, size: record.size };
  }
  return { version, platforms };
}

/** Detached signature over the exact manifest bytes, base64 (what engine_install.rs reads). */
export function signManifest(bytes, privateKey) {
  return sign(null, bytes, privateKey).toString("base64");
}

export function verifyManifestSignature(bytes, signatureBase64, publicKeyBase64) {
  const signature = Buffer.from(signatureBase64.trim(), "base64");
  if (signature.length !== 64) return false;
  return verify(null, bytes, publicKeyFromRaw(publicKeyBase64), signature);
}

function readRecords(dir) {
  return readdirSync(dir)
    .filter((name) => /^engine-[a-z0-9_-]+\.json$/.test(name))
    .sort()
    .map((name) => JSON.parse(readFileSync(join(dir, name), "utf8")));
}

export function writeManifest({ dir, keyPath, version = engineVersion() }) {
  const records = readRecords(dir);
  for (const record of records) {
    const archive = join(dir, record.file);
    if (statSync(archive).size !== record.size || sha256File(archive) !== record.sha256) {
      throw new Error(`${record.file} changed since it was built`);
    }
  }
  const manifest = buildManifest({ version, records });
  const bytes = Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`);
  const signature = signManifest(bytes, loadSigningKey(keyPath));
  writeFileSync(join(dir, "manifest.json"), bytes);
  writeFileSync(join(dir, "manifest.json.sig"), `${signature}\n`);
  return { manifest, signature, unsigned: records.filter((record) => !record.codeSigned).map((record) => record.platform) };
}

/** The checks the app makes, run over a release directory before it is uploaded. */
export function verifyReleaseDir({ dir, publicKey }) {
  const bytes = readFileSync(join(dir, "manifest.json"));
  const signaturePath = join(dir, "manifest.json.sig");
  if (!existsSync(signaturePath)) throw new Error("manifest.json.sig is missing");
  if (!verifyManifestSignature(bytes, readFileSync(signaturePath, "utf8"), publicKey)) {
    throw new Error("manifest.json.sig does not verify with the given public key");
  }
  const manifest = JSON.parse(bytes.toString("utf8"));
  for (const [platform, artifact] of Object.entries(manifest.platforms)) {
    const file = join(dir, artifact.url.split("/").pop());
    if (statSync(file).size !== artifact.size) throw new Error(`${platform}: size differs from the manifest`);
    if (sha256File(file) !== artifact.sha256) throw new Error(`${platform}: sha256 differs from the manifest`);
  }
  return manifest;
}

function run(command, args, { cwd = REPO_ROOT, env = process.env, allowFailure = false } = {}) {
  const result = spawnSync(command, args, { cwd, env, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
  if (result.error) throw result.error;
  const out = `${result.stdout ?? ""}${result.stderr ?? ""}`;
  if (result.status !== 0 && !allowFailure) {
    throw new Error(`${command} ${args.join(" ")} exited ${result.status}: ${out.trim().slice(-4000)}`);
  }
  return { status: result.status ?? -1, out };
}

/**
 * The macOS codesign order for an engine bundle: nested code first (framework libraries, the
 * framework, each helper app), the outer app last, so every seal covers signed contents.
 */
export function macSigningPlan(appPath, list = (dir) => (existsSync(dir) ? readdirSync(dir) : [])) {
  const frameworks = join(appPath, "Contents/Frameworks");
  const framework = join(frameworks, "Chromium Embedded Framework.framework");
  const steps = [];
  for (const name of list(join(framework, "Libraries")).filter((name) => name.endsWith(".dylib")).sort()) {
    steps.push({ path: join(framework, "Libraries", name), entitlements: null });
  }
  steps.push({ path: framework, entitlements: null });
  for (const name of list(frameworks).filter((name) => name.endsWith(".app")).sort()) {
    steps.push({ path: join(frameworks, name), entitlements: join(ENTITLEMENTS_DIR, "helper.plist") });
  }
  steps.push({ path: appPath, entitlements: join(ENTITLEMENTS_DIR, "engine.plist") });
  return steps;
}

export function codesignArgs(identity, step) {
  const args = ["--force", "--timestamp", "--options", "runtime", "--sign", identity];
  if (step.entitlements) args.push("--entitlements", step.entitlements);
  return [...args, step.path];
}

function signMac({ appPath, identity, log }) {
  for (const step of macSigningPlan(appPath)) {
    run("codesign", codesignArgs(identity, step));
  }
  const verifyOut = run("codesign", ["--verify", "--deep", "--strict", "--verbose=2", appPath]).out;
  const details = run("codesign", ["-dv", "--verbose=4", appPath], { allowFailure: true }).out;
  const spctl = run("spctl", ["--assess", "--type", "execute", "-vvv", appPath], { allowFailure: true });
  log(`codesign --verify: ${verifyOut.trim()}`);
  log(`spctl: exit ${spctl.status}: ${spctl.out.trim()}`);
  const authority = /Authority=(.+)/.exec(details)?.[1] ?? null;
  return { codeSigned: true, authority, spctl: spctl.out.trim(), spctlExit: spctl.status };
}

async function notarizeMac({ zipPath, appPath, notaryProfile }) {
  const { submitForNotarization, macExec } = await import("../lib/release-platforms.mjs");
  const result = submitForNotarization({ path: zipPath, notaryProfile, exec: macExec });
  macExec("xcrun", ["stapler", "staple", appPath]);
  macExec("xcrun", ["stapler", "validate", appPath]);
  return result.id ?? null;
}

function zipMac(appPath, zipPath) {
  rmSync(zipPath, { force: true });
  // ditto keeps symlinks, modes and the code signature; the archive root is ferryx_engine.app.
  run("ditto", ["-c", "-k", "--keepParent", "--norsrc", appPath, zipPath]);
}

function zipWindows(dir, zipPath) {
  rmSync(zipPath, { force: true });
  run("powershell", [
    "-NoProfile",
    "-Command",
    `Compress-Archive -Path ${JSON.stringify(join(dir, "*"))} -DestinationPath ${JSON.stringify(zipPath)} -CompressionLevel Optimal`,
  ]);
}

function signWindows({ dir, thumbprint }) {
  const targets = readdirSync(dir).filter((name) => /\.(exe|dll)$/i.test(name)).map((name) => join(dir, name));
  for (const target of targets) {
    run("signtool", ["sign", "/sha1", thumbprint, "/fd", "sha256", "/tr", "http://timestamp.digicert.com", "/td", "sha256", target]);
    run("signtool", ["verify", "/pa", target]);
  }
  return { codeSigned: true, authority: thumbprint };
}

export async function buildEngine({ out, identity = DEFAULT_IDENTITY, notarize = false, notaryProfile = null, signtoolCert = null, adhoc = false, log = console.log }) {
  const platform = platformKey();
  if (!platform) throw new Error(`no engine is built for ${process.platform}/${process.arch}`);
  if (notarize && !notaryProfile) throw new Error("--notarize needs --notary-profile <keychain profile>");
  const version = engineVersion();
  mkdirSync(out, { recursive: true });
  const bundleDir = mkdtempSync(join(tmpdir(), "ferryx-engine-bundle-"));
  log(`building ${platform} engine ${version} into ${bundleDir}`);
  // bundle-engine reads cargo metadata and runs cargo build from its cwd, so it runs in the
  // engine crate (its own workspace).
  run("cargo", ["run", "--release", "--bin", "bundle-engine", "--", bundleDir, "--release"], { cwd: ENGINE_CRATE });
  const file = `engine-${platform}.zip`;
  const zipPath = join(out, file);
  let signing;
  let notarization = null;
  if (platform.startsWith("macos")) {
    const appPath = join(bundleDir, "ferryx_engine.app");
    signing = signMac({ appPath, identity: adhoc ? "-" : identity, log });
    if (adhoc) signing = { ...signing, codeSigned: false, authority: "ad-hoc" };
    zipMac(appPath, zipPath);
    if (notarize) {
      notarization = await notarizeMac({ zipPath, appPath, notaryProfile });
      zipMac(appPath, zipPath); // re-zip so the archive carries the stapled ticket
    }
  } else {
    signing = signtoolCert ? signWindows({ dir: bundleDir, thumbprint: signtoolCert }) : { codeSigned: false, authority: null };
    if (!signtoolCert) log("WARNING: no --signtool-cert: the Windows engine is UNSIGNED (recorded in the manifest record)");
    zipWindows(bundleDir, zipPath);
  }
  const record = {
    version,
    platform,
    file,
    sha256: sha256File(zipPath),
    size: statSync(zipPath).size,
    ...signing,
    notarization,
  };
  writeFileSync(join(out, `engine-${platform}.json`), `${JSON.stringify(record, null, 2)}\n`);
  rmSync(bundleDir, { recursive: true, force: true });
  return record;
}

/** The gh commands a publish runs, in order; `publish` executes them only with --publish. */
export function publishPlan({ dir, version, files }) {
  const tag = releaseTag(version);
  const paths = files.map((file) => join(dir, file));
  return [
    ["gh", "release", "create", tag, "--repo", GITHUB_REPO, "--draft", "--title", `Browser engine ${version}`, "--notes", `Chromium engine ${version} for Ferryx (downloaded on demand from Settings > Browser).`],
    ["gh", "release", "upload", tag, "--repo", GITHUB_REPO, ...paths],
    ["gh", "release", "download", tag, "--repo", GITHUB_REPO, "--dir", "<verify-dir>"],
    ["gh", "release", "edit", tag, "--repo", GITHUB_REPO, "--draft=false", "--latest=false"],
  ];
}

export function publish({ dir, publishFlag, publicKey, env = process.env, log = console.log }) {
  if (env.CI === "true" || env.GITHUB_ACTIONS === "true") {
    throw new Error("engine releases are local-only; refusing to publish under CI");
  }
  const manifest = verifyReleaseDir({ dir, publicKey });
  const files = ["manifest.json", "manifest.json.sig", ...Object.values(manifest.platforms).map((artifact) => artifact.url.split("/").pop())];
  const plan = publishPlan({ dir, version: manifest.version, files });
  if (!publishFlag) {
    log("dry run (pass --publish to upload):");
    for (const command of plan) log(`  ${command.join(" ")}`);
    return { published: false, plan };
  }
  const verifyDir = mkdtempSync(join(tmpdir(), "ferryx-engine-verify-"));
  for (const command of plan) {
    const args = command.slice(1).map((arg) => (arg === "<verify-dir>" ? verifyDir : arg));
    if (args[1] === "edit") {
      for (const file of files) {
        if (sha256File(join(verifyDir, file)) !== sha256File(join(dir, file))) {
          throw new Error(`uploaded ${file} differs from the local file; the release stays a draft`);
        }
      }
    }
    run("gh", args);
  }
  rmSync(verifyDir, { recursive: true, force: true });
  return { published: true, plan };
}

export function parseArgs(argv) {
  const [command, ...rest] = argv;
  const options = {};
  for (let i = 0; i < rest.length; i += 1) {
    const arg = rest[i];
    if (!arg.startsWith("--")) throw new Error(`unexpected argument ${arg}`);
    const name = arg.slice(2);
    if (["publish", "notarize", "adhoc"].includes(name)) {
      options[name] = true;
    } else {
      const value = rest[i + 1];
      if (value === undefined || value.startsWith("--")) throw new Error(`--${name} needs a value`);
      options[name] = value;
      i += 1;
    }
  }
  return { command, options };
}

async function main() {
  const { command, options } = parseArgs(process.argv.slice(2));
  switch (command) {
    case "keygen": {
      const { keyPath, publicKey } = keygen({ out: options.out });
      console.log(`private key: ${keyPath} (keep it off the repository and backed up)`);
      console.log(`public key: ${publicKey}`);
      console.log(`add to TRUSTED_MANIFEST_KEYS in src-tauri/src/browser/engine_install.rs: "${publicKey}",`);
      break;
    }
    case "build":
      console.log(JSON.stringify(await buildEngine({
        out: options.out,
        identity: options.identity,
        notarize: options.notarize === true,
        notaryProfile: options["notary-profile"] ?? null,
        signtoolCert: options["signtool-cert"] ?? process.env.FERRYX_ENGINE_SIGNTOOL_CERT ?? null,
        adhoc: options.adhoc === true,
      }), null, 2));
      break;
    case "manifest": {
      const result = writeManifest({ dir: options.dir, keyPath: options.key ?? process.env[SIGNING_KEY_ENV] });
      console.log(JSON.stringify(result.manifest, null, 2));
      if (result.unsigned.length) console.log(`WARNING: code-unsigned artifacts: ${result.unsigned.join(", ")}`);
      break;
    }
    case "verify":
      verifyReleaseDir({ dir: options.dir, publicKey: options["public-key"] });
      console.log("manifest signature, sizes and sha256 OK");
      break;
    case "publish":
      publish({ dir: options.dir, publishFlag: options.publish === true, publicKey: options["public-key"] });
      break;
    default:
      throw new Error("usage: release-engine.mjs keygen|build|manifest|verify|publish (see the file header)");
  }
}

if (import.meta.main) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  });
}
