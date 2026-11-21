#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import { compareVersions, readMachO } from "./lib/keeper-macho.mjs";

const BLOCK_SIZE = 512;

// The keeper payload the daemon installs a generation from, inside the app this archive carries
// (`daemon::keeper_startup::BUNDLE_KEEPER_DIR` and `keeper_launchd::BINARY_NAME`).
const KEEPER_DIR = "Ferryx.app/Contents/Resources/keeper/";
const KEEPER_BINARY = "ferryx-ptykeeper";
const KEEPER_MANIFEST = "keeper.json";
const KEEPER_FLOOR = "latest.json";
const APP_INFO_PLIST = "Ferryx.app/Contents/Info.plist";
const APP_BINARY = "Ferryx.app/Contents/MacOS/ferryx";

function field(block, offset, length) {
  return block
    .subarray(offset, offset + length)
    .toString("utf8")
    .replace(/\0.*$/, "");
}

function octal(fieldValue) {
  const value = fieldValue.trim();
  return value === "" ? 0 : Number.parseInt(value, 8);
}

function paddedSize(size) {
  return Math.ceil(size / BLOCK_SIZE) * BLOCK_SIZE;
}

function entries(bytes) {
  const archive = gunzipSync(bytes);
  const result = [];

  for (let offset = 0; offset + BLOCK_SIZE <= archive.length; ) {
    const block = archive.subarray(offset, offset + BLOCK_SIZE);
    if (block.every((byte) => byte === 0)) break;

    const prefix = field(block, 345, 155);
    const name = field(block, 0, 100);
    const path = prefix === "" ? name : `${prefix}/${name}`;
    const size = octal(field(block, 124, 12));
    const type = field(block, 156, 1);
    // The permission bits are needed for the keeper: a payload file without an execute bit cannot be
    // installed as a generation, and every other check here would pass on unreadable-but-present bytes.
    // A blank field stays `null` rather than 0 so an archive that omits the field is not read as
    // "mode 000" and refused for the wrong reason.
    const modeText = field(block, 100, 8);
    const mode = modeText.trim() === "" ? null : octal(modeText);
    // Keep the file body: the keeper manifest advertises a digest of the exact bytes that ship, and
    // only the archive's own copy of the binary can answer whether that digest is still true.
    const data =
      offset + BLOCK_SIZE + size <= archive.length
        ? archive.subarray(offset + BLOCK_SIZE, offset + BLOCK_SIZE + size)
        : null;
    result.push({ path, type, mode, data });
    offset += BLOCK_SIZE + paddedSize(size);
  }

  return result;
}

function invalidEntry(path) {
  return path.split("/").some((component) => component.startsWith("._"));
}

function regularFile(entryList, path) {
  return entryList.find((entry) => entry.path === path && (entry.type === "" || entry.type === "0")) ?? null;
}

/**
 * A string value from an XML plist, by key.
 *
 * `Info.plist` in the archive is the XML plist Tauri writes. Reading it by regex rather than through
 * a parser is deliberate: the keys this checker needs are top-level and scalar, and a full plist
 * parser would add a dependency to a script that must run with nothing but Node.
 */
function plistString(text, key) {
  const pattern = new RegExp(`<key>\\s*${key}\\s*</key>\\s*<string>([^<]*)</string>`);
  const matched = pattern.exec(String(text ?? ""));
  return matched ? matched[1] : null;
}

/** `KEEPER_PROTOCOL_MAJOR` in `scripts/lib/release-keeper.mjs`; the daemon understands major 1. */
const KEEPER_PROTOCOL_MAJOR = 1;

/**
 * Checks the bundled keeper against its own manifest, on the bytes this archive actually contains.
 *
 * The runtime compares `keeper.json`'s `sha256` with the installed binary and refuses a generation
 * that disagrees (`keeper_startup::payload_digest`), so a stale digest is not cosmetic: the install
 * pass fails and the daemon boots without a keeper. That failure is logged as a warning, so nothing
 * else in the pipeline notices.
 *
 * The digest can go stale while the bundle is being sealed — code signing appends a signature blob
 * and rewrites the Mach-O — so this is checked on the finished archive rather than trusted from the
 * step that wrote it. Returns a message describing the defect, or null.
 */
function keeperProblem(entryList) {
  const manifestEntry = regularFile(entryList, KEEPER_DIR + KEEPER_MANIFEST);
  // An archive built without a keeper payload is not this check's business; requiring the payload
  // belongs to the release build, which asserts it in the bundle before sealing.
  if (manifestEntry === null) return null;

  const binaryEntry = regularFile(entryList, KEEPER_DIR + KEEPER_BINARY);
  if (binaryEntry === null) {
    return `the archive carries ${KEEPER_MANIFEST} but no ${KEEPER_BINARY} beside it`;
  }

  let manifest;
  try {
    manifest = JSON.parse(manifestEntry.data.toString("utf8"));
  } catch (error) {
    return `${KEEPER_DIR}${KEEPER_MANIFEST} is not readable JSON: ${error.message}`;
  }

  const recorded = typeof manifest.sha256 === "string" ? manifest.sha256 : null;
  if (recorded === null) {
    return `${KEEPER_DIR}${KEEPER_MANIFEST} records no sha256, so the bundled keeper cannot be tied to the bytes that ship`;
  }
  if (binaryEntry.data === null || binaryEntry.data.length === 0) {
    return `${KEEPER_DIR}${KEEPER_BINARY} is empty`;
  }

  const actual = createHash("sha256").update(binaryEntry.data).digest("hex");
  if (recorded.toLowerCase() !== actual) {
    return (
      `${KEEPER_DIR}${KEEPER_MANIFEST} records sha256 ${recorded} but the bundled ${KEEPER_BINARY} ` +
      `hashes to ${actual}: the keeper was rewritten after its manifest was written (code signing ` +
      `rewrites the Mach-O), and the daemon refuses a generation whose recorded digest does not ` +
      `match, so this build would boot without a keeper`
    );
  }

  // The rollback floor is read from this document and nowhere else
  // (`daemon::rollback::shipped_manifest_path` checks Resources/latest.json first, then this one).
  // A payload that ships without it boots a daemon whose every rollback refuses
  // KEEPER_ROLLBACK_FLOOR_UNVERIFIED, which is the failure the floor document exists to prevent.
  const floorEntry = regularFile(entryList, KEEPER_DIR + KEEPER_FLOOR);
  if (floorEntry === null) {
    return (
      `the archive carries a keeper payload but no ${KEEPER_DIR}${KEEPER_FLOOR}: the daemon reads the ` +
      `rollback floor only from a manifest beside the executable, so every rollback on this build ` +
      `would refuse KEEPER_ROLLBACK_FLOOR_UNVERIFIED`
    );
  }
  let floor;
  try {
    floor = JSON.parse(floorEntry.data.toString("utf8"));
  } catch (error) {
    return `${KEEPER_DIR}${KEEPER_FLOOR} is not readable JSON: ${error.message}`;
  }
  const oldest = floor?.keeper?.oldestSafeRollbackVersion;
  if (typeof oldest !== "string" || oldest === "") {
    return (
      `${KEEPER_DIR}${KEEPER_FLOOR} has no keeper.oldestSafeRollbackVersion, so the shipped build states ` +
      `no rollback floor and the daemon reports the floor as unverified`
    );
  }
  if (floor.version !== manifest.version) {
    return (
      `${KEEPER_DIR}${KEEPER_FLOOR} states version ${JSON.stringify(floor.version)} but the keeper payload ` +
      `records ${JSON.stringify(manifest.version)}: the floor would describe a different generation than ` +
      `the one this archive installs`
    );
  }

  // --- the generation name must be the app version this archive ships ------------------------
  // `keeper_generation_version()` is the daemon's `CARGO_PKG_VERSION`, and the install pass places
  // the payload under `~/.ferryx/keeper/<that version>/`. If `keeper.json` names a different
  // version, the daemon installs one generation and this manifest describes another; the floor and
  // every later rollback decision then reason about a directory that does not exist.
  const infoEntry = regularFile(entryList, APP_INFO_PLIST);
  if (infoEntry === null || infoEntry.data === null) {
    return `the archive carries a keeper payload but no readable ${APP_INFO_PLIST}, so the generation name cannot be tied to the app it ships in`;
  }
  const appVersion = plistString(infoEntry.data.toString("utf8"), "CFBundleShortVersionString");
  if (typeof appVersion !== "string" || appVersion === "") {
    return `${APP_INFO_PLIST} states no CFBundleShortVersionString, so the keeper generation cannot be checked against the app version`;
  }
  if (manifest.version !== appVersion) {
    return (
      `${KEEPER_DIR}${KEEPER_MANIFEST} records generation ${JSON.stringify(manifest.version)} but the app ` +
      `bundle is version ${JSON.stringify(appVersion)}: the daemon installs its generation under its own ` +
      `CARGO_PKG_VERSION, so the payload would be installed under a name this manifest does not describe`
    );
  }

  // --- the protocol the payload speaks -------------------------------------------------------
  // The daemon must understand every keeper generation it may meet, and compatibility is stated as
  // "additive-only within a major version". A payload that declares a different major than this
  // packager and daemon speak would be installed and then fail every frame.
  if (manifest.protocolMajor !== KEEPER_PROTOCOL_MAJOR) {
    return (
      `${KEEPER_DIR}${KEEPER_MANIFEST} declares protocol major ${JSON.stringify(manifest.protocolMajor)}; ` +
      `this release's daemon speaks ${KEEPER_PROTOCOL_MAJOR}, so the generation could never be driven`
    );
  }

  // --- the payload must be installable -------------------------------------------------------
  // `keeper_install` copies the payload and requires it to be executable; a tar member without the
  // bit produces a generation directory that exists and cannot be run.
  if (binaryEntry.mode !== null && (binaryEntry.mode & 0o111) === 0) {
    return (
      `${KEEPER_DIR}${KEEPER_BINARY} is archived mode ${(binaryEntry.mode & 0o777).toString(8)}: it has no execute bit, ` +
      `so the installed generation could never be launched`
    );
  }

  // --- the minimum OS the keeper requires ----------------------------------------------------
  // The app promises `LSMinimumSystemVersion` and the keeper is exec'd beside it on the same machine.
  // A keeper slice that requires a newer macOS than the daemon's slice of the same architecture
  // means the app starts and its keeper never does; `run_keeper_install_pass` reports that only as a
  // warning, so the app would run keeper-less while looking healthy. Compared per architecture
  // because arm64 legitimately declares 11.0 while the app still supports 10.15 via x86_64.
  const daemonEntry = regularFile(entryList, APP_BINARY);
  if (daemonEntry === null || daemonEntry.data === null) {
    return `the archive carries a keeper payload but no readable ${APP_BINARY}, so the keeper's runtime floor cannot be compared with the binary it must run beside`;
  }
  let keeperFloors;
  let daemonFloors;
  try {
    keeperFloors = floorByArch(binaryEntry.data, `${KEEPER_DIR}${KEEPER_BINARY}`);
  } catch (error) {
    return error.message;
  }
  try {
    daemonFloors = floorByArch(daemonEntry.data, APP_BINARY);
  } catch (error) {
    return error.message;
  }
  for (const [arch, floor] of Object.entries(keeperFloors)) {
    const daemonFloor = daemonFloors[arch];
    if (daemonFloor === undefined) continue;
    if (compareVersions(floor, daemonFloor) > 0) {
      return (
        `the ${arch} slice of ${KEEPER_DIR}${KEEPER_BINARY} requires macOS ${floor} but the ${arch} slice of ` +
        `${APP_BINARY} requires ${daemonFloor}: on a machine between those versions the app would start and its ` +
        `keeper would not, and the install failure is only logged as a warning`
      );
    }
  }
  // A keeper that declares no minimum for a slice it ships is not verifiable at all.
  for (const arch of Object.keys(daemonFloors)) {
    if (keeperFloors[arch] === undefined) {
      return (
        `the ${arch} slice of ${KEEPER_DIR}${KEEPER_BINARY} declares no minimum macOS version, ` +
        `so its runtime floor cannot be checked against the ${arch} slice of ${APP_BINARY}`
      );
    }
  }

  return null;
}

/**
 * `{ arch: "minos" }` for a Mach-O, or a defect message for one that cannot be read.
 *
 * Thrown errors carry the message the caller returns verbatim: an unparsable payload is a refusal,
 * never a skip, because a binary this checker cannot read is one whose floor and identity ship
 * unproven.
 */
function floorByArch(bytes, label) {
  let macho;
  try {
    macho = readMachO(bytes);
  } catch (error) {
    throw new Error(`${label} is not a readable Mach-O (${error.message}), so it cannot be shipped as a payload`);
  }
  const floors = {};
  for (const slice of macho.slices) {
    if (typeof slice.minos === "string") floors[slice.arch] = slice.minos;
  }
  if (Object.keys(floors).length === 0) {
    throw new Error(`${label} declares no minimum macOS version in any slice, so its runtime floor is unstated`);
  }
  return floors;
}

function main() {
  const [archive] = process.argv.slice(2);
  if (archive === undefined) {
    process.stderr.write("usage: assert-updater-archive-layout.mjs <archive.app.tar.gz>\n");
    return 2;
  }

  const archiveEntries = entries(readFileSync(archive));
  const payloadEntries = archiveEntries.filter(({ type }) => type !== "x" && type !== "g");
  const appleDouble = payloadEntries.find(({ path }) => invalidEntry(path));
  if (appleDouble !== undefined) {
    process.stderr.write(`AppleDouble entry is invalid for Tauri updater extraction: ${appleDouble.path}\n`);
    return 1;
  }

  const invalidRoot = payloadEntries.find(({ path }) => !path.startsWith("Ferryx.app/"));
  if (invalidRoot !== undefined) {
    process.stderr.write(`Updater archive entry must be rooted in Ferryx.app: ${invalidRoot.path}\n`);
    return 1;
  }

  const keeperFailure = keeperProblem(payloadEntries);
  if (keeperFailure !== null) {
    process.stderr.write(`${keeperFailure}\n`);
    return 1;
  }

  process.stdout.write(`updater archive layout OK: ${archiveEntries.length} entries\n`);
  return 0;
}

process.exit(main());
