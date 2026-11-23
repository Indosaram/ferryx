import { isAbsolute, normalize } from "node:path";
import { parseReleaseTag, toAppVersion, toMsixVersion } from "../sync-version.mjs";

export const ALLOWED_PLAN_KEYS = Object.freeze([
  "schemaVersion",
  "runId",
  "repo",
  "commitSha",
  "tag",
  "appVersion",
  "msixVersion",
  "channels",
  "requiredTargets",
  "toolchains",
  "sourceDateEpoch",
  "createdAt",
]);

export const ALLOWED_CHANNELS_KEYS = Object.freeze(["store", "nsisMigration"]);

export const ALLOWED_TOOLCHAIN_KEYS = Object.freeze([
  "node",
  "bun",
  "zig",
  "rust",
  "tauri",
]);

export const REQUIRED_TOOLCHAIN_KEYS = Object.freeze(["node", "bun", "zig"]);

export const VALID_HOSTS = Object.freeze(["macbook", "omaki", "maho-win"]);

export const KIND_DEFINITIONS = Object.freeze({
  "macos-updater": {
    permittedHost: "macbook",
    isUpdater: true,
    expectedTargets: ["darwin-aarch64"],
    stableAlias: "Ferryx_universal.app.tar.gz",
    allowedExtensions: [".app.tar.gz"],
  },
  dmg: {
    permittedHost: "macbook",
    isUpdater: false,
    expectedTargets: [],
    stableAlias: "Ferryx_universal.dmg",
    allowedExtensions: [".dmg"],
  },
  appimage: {
    permittedHost: "omaki",
    isUpdater: true,
    expectedTargets: ["linux-x86_64"],
    stableAlias: "Ferryx_amd64.AppImage",
    allowedExtensions: [".AppImage"],
  },
  deb: {
    permittedHost: "omaki",
    isUpdater: false,
    expectedTargets: [],
    stableAlias: "Ferryx_amd64.deb",
    allowedExtensions: [".deb"],
  },
  msix: {
    permittedHost: "maho-win",
    isUpdater: false,
    expectedTargets: [],
    stableAlias: "Ferryx_x64.msix",
    allowedExtensions: [".msix"],
  },
  nsis: {
    permittedHost: "maho-win",
    isUpdater: true,
    expectedTargets: ["windows-x86_64"],
    stableAlias: "Ferryx_x64-setup.exe",
    allowedExtensions: ["-setup.exe"],
  },
  "cli-linux-amd64": {
    permittedHost: "omaki",
    isUpdater: false,
    expectedTargets: [],
    stableAlias: "ferryx-cli-linux-amd64",
    allowedExtensions: ["ferryx-cli-linux-amd64", "ferryx-cli"],
  },
  "cli-darwin-universal": {
    permittedHost: "macbook",
    isUpdater: false,
    expectedTargets: [],
    stableAlias: "ferryx-cli-darwin-universal",
    allowedExtensions: ["ferryx-cli-darwin-universal", "ferryx-cli"],
  },
  // The remote-desktop client: remote hosts need it to drive a GUI over the paired tunnel.
  "rd-client-linux-amd64": {
    permittedHost: "omaki",
    isUpdater: false,
    expectedTargets: [],
    stableAlias: "ferryx-rd-client-linux-amd64",
    allowedExtensions: ["ferryx-rd-client-linux-amd64", "ferryx-rd-client"],
  },
  "rd-client-darwin-universal": {
    permittedHost: "macbook",
    isUpdater: false,
    expectedTargets: [],
    stableAlias: "ferryx-rd-client-darwin-universal",
    allowedExtensions: ["ferryx-rd-client-darwin-universal", "ferryx-rd-client"],
  },
  // Windows needs the client and its FFmpeg DLL sidecars together: the client links FFmpeg
  // dynamically, so a bare .exe exits 0xC0000135 (STATUS_DLL_NOT_FOUND) on a host without a
  // system FFmpeg (measured 2026-11-10). One release asset cannot carry several files, so the
  // artifact is a zip holding ferryx-rd-client.exe plus exactly the DLLs it loads.
  "rd-client-windows-amd64": {
    permittedHost: "maho-win",
    isUpdater: false,
    expectedTargets: [],
    stableAlias: "ferryx-rd-client-windows-amd64.zip",
    allowedExtensions: ["-windows-amd64.zip"],
    // Members a Windows client needs beside the exe to start at all. The client loads avcodec
    // and avutil directly and swscale/swresample through the FFmpeg link closure.
    zipEntries: ["ferryx-rd-client.exe", "avcodec-*.dll", "avutil-*.dll", "swscale-*.dll", "swresample-*.dll"],
  },
});

export const ALLOWED_RECEIPT_KEYS = Object.freeze([
  "schemaVersion",
  "runId",
  "host",
  "commitSha",
  "appVersion",
  "completedAt",
  "exitCode",
  "artifacts",
  "toolchains",
]);

export const REQUIRED_RECEIPT_KEYS = Object.freeze([
  "schemaVersion",
  "runId",
  "host",
  "commitSha",
  "appVersion",
  "completedAt",
  "exitCode",
  "artifacts",
]);

export const ALLOWED_ARTIFACT_KEYS = Object.freeze([
  "kind",
  "name",
  "relPath",
  "bytes",
  "sha256",
  "signatureRelPath",
  "targets",
]);

const SHA256_REGEX = /^[0-9a-f]{64}$/;
const COMMIT_SHA_REGEX = /^[0-9a-f]{40}$/;
const REPO_REGEX = /^[a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+$/;

/**
 * Members a multi-file artifact must carry, as exact names or `*` globs (e.g. `avcodec-*.dll`).
 * `[]` for a single-file artifact.
 *
 * @param {string} kind
 * @returns {string[]}
 */
export function zipEntriesForKind(kind) {
  return KIND_DEFINITIONS[kind]?.zipEntries ?? [];
}

/**
 * Matches one `zipEntries` pattern against a zip member name. `*` matches any run of characters
 * except a path separator, so a nested entry never satisfies a top-level requirement.
 */
function matchesZipPattern(pattern, name) {
  const source = pattern
    .split("*")
    .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join("[^/]*");
  return new RegExp(`^${source}$`).test(name);
}

/**
 * Checks a zip member list against the kind's `zipEntries` contract and returns the entries that
 * are missing. An artifact with no `zipEntries` requires nothing.
 *
 * @param {string} kind
 * @param {string[]} names zip member names, as the archive stores them
 * @returns {string[]} the patterns no member satisfies
 */
export function missingZipEntries(kind, names) {
  const required = zipEntriesForKind(kind);
  return required.filter((pattern) => !names.some((name) => matchesZipPattern(pattern, name)));
}

const ZIP_EOCD_SIGNATURE = 0x06054b50;
const ZIP_CENTRAL_SIGNATURE = 0x02014b50;
const ZIP64_LOCATOR_SIGNATURE = 0x07064b50;
const ZIP64_EOCD_SIGNATURE = 0x06064b50;
const ZIP_U16_MAX = 0xffff;
const ZIP_U32_MAX = 0xffffffff;

/**
 * Lists the member names of a zip archive from its central directory, in archive order.
 * Returns null when `buffer` is not a readable zip (no end-of-central-directory record), so a
 * caller can tell "not a zip" from "a zip with the wrong members".
 *
 * @param {Buffer} buffer
 * @returns {string[] | null}
 */
export function listZipEntries(buffer) {
  if (!Buffer.isBuffer(buffer) || buffer.length < 22) return null;
  let eocd = -1;
  const first = Math.max(0, buffer.length - 22 - 0xffff);
  for (let i = buffer.length - 22; i >= first; i -= 1) {
    if (buffer.readUInt32LE(i) === ZIP_EOCD_SIGNATURE) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) return null;
  let entries = buffer.readUInt16LE(eocd + 10);
  let offset = buffer.readUInt32LE(eocd + 16);
  if (entries === ZIP_U16_MAX || offset === ZIP_U32_MAX) {
    const locator = eocd - 20;
    if (locator < 0 || buffer.readUInt32LE(locator) !== ZIP64_LOCATOR_SIGNATURE) return null;
    const record = Number(buffer.readBigUInt64LE(locator + 8));
    if (record + 56 > buffer.length || buffer.readUInt32LE(record) !== ZIP64_EOCD_SIGNATURE) return null;
    entries = Number(buffer.readBigUInt64LE(record + 32));
    offset = Number(buffer.readBigUInt64LE(record + 48));
  }
  const names = [];
  for (let n = 0; n < entries; n += 1) {
    if (offset + 46 > buffer.length || buffer.readUInt32LE(offset) !== ZIP_CENTRAL_SIGNATURE) return null;
    const nameLength = buffer.readUInt16LE(offset + 28);
    const extraLength = buffer.readUInt16LE(offset + 30);
    const commentLength = buffer.readUInt16LE(offset + 32);
    const nameStart = offset + 46;
    if (nameStart + nameLength > buffer.length) return null;
    names.push(buffer.toString("utf8", nameStart, nameStart + nameLength));
    offset = nameStart + nameLength + extraLength + commentLength;
  }
  return names;
}

/**
 * Validates that a relative path does not escape or use traversal.
 */
export function isSafeRelativePath(relPath) {
  if (typeof relPath !== "string" || relPath.trim() === "") {
    return false;
  }
  if (relPath.includes("\0")) {
    return false;
  }
  if (relPath.startsWith("/") || relPath.startsWith("\\") || isAbsolute(relPath)) {
    return false;
  }
  const parts = relPath.split(/[/\\]/);
  for (const part of parts) {
    if (part === ".." || part === ".") {
      return false;
    }
  }
  const normalized = normalize(relPath);
  if (normalized.startsWith("..") || normalized.startsWith("/") || isAbsolute(normalized)) {
    return false;
  }
  return true;
}

/**
 * Checks for unknown properties on an object against an allowed set and required set.
 */
function assertExactKeys(obj, allowedKeys, contextName, requiredKeys = allowedKeys) {
  const allowedSet = new Set(allowedKeys);
  for (const key of Object.keys(obj)) {
    if (!allowedSet.has(key)) {
      throw new Error(`${contextName} contains unexpected property: ${key}`);
    }
  }
  for (const key of requiredKeys) {
    if (!(key in obj)) {
      throw new Error(`${contextName} missing required property: ${key}`);
    }
  }
}

/**
 * Validates a toolchains object against ALLOWED_TOOLCHAIN_KEYS and given required keys.
 *
 * @param {object} toolchains
 * @param {object} [options]
 * @param {string[]} [options.required]
 * @returns {object}
 */
export function validateToolchains(toolchains, { required = REQUIRED_TOOLCHAIN_KEYS } = {}) {
  if (!toolchains || typeof toolchains !== "object" || Array.isArray(toolchains)) {
    throw new Error("toolchains must be an object");
  }
  const allowedSet = new Set(ALLOWED_TOOLCHAIN_KEYS);
  for (const tk of Object.keys(toolchains)) {
    if (!allowedSet.has(tk)) {
      throw new Error(`unexpected toolchain property: ${tk}`);
    }
  }
  for (const rk of required) {
    if (!(rk in toolchains) || typeof toolchains[rk] !== "string" || toolchains[rk].trim() === "") {
      throw new Error(`missing required toolchain: ${rk}`);
    }
  }
  for (const [k, v] of Object.entries(toolchains)) {
    if (typeof v !== "string" || v.trim() === "") {
      throw new Error(`toolchains.${k} must be a non-empty string`);
    }
  }
  return toolchains;
}

/**
 * Parses and validates a Release Plan according to Section 4 and Section 9.
 *
 * @param {string | object} value
 * @returns {object}
 */
export function parsePlan(value) {
  let plan = value;
  if (typeof value === "string") {
    try {
      plan = JSON.parse(value);
    } catch (err) {
      throw new Error(`Plan must be valid JSON: ${err.message}`);
    }
  }

  if (!plan || typeof plan !== "object" || Array.isArray(plan)) {
    throw new Error("Plan must be a JSON object");
  }

  assertExactKeys(plan, ALLOWED_PLAN_KEYS, "Plan");

  if (plan.schemaVersion !== 1) {
    throw new Error(`schemaVersion must be 1, got ${JSON.stringify(plan.schemaVersion)}`);
  }

  if (typeof plan.runId !== "string" || plan.runId.trim() === "") {
    throw new Error("runId must be a non-empty string");
  }

  if (typeof plan.repo !== "string" || !REPO_REGEX.test(plan.repo)) {
    throw new Error(`repo must be a valid 'owner/name' string, got ${JSON.stringify(plan.repo)}`);
  }

  if (typeof plan.commitSha !== "string" || !COMMIT_SHA_REGEX.test(plan.commitSha)) {
    throw new Error(`commitSha must be a 40-character lowercase hex string, got ${JSON.stringify(plan.commitSha)}`);
  }

  // Tag validation using sync-version
  let tagInfo;
  try {
    tagInfo = parseReleaseTag(plan.tag);
  } catch (err) {
    throw new Error(`Invalid plan tag ${JSON.stringify(plan.tag)}: ${err.message}`);
  }

  const expectedAppVersion = toAppVersion(plan.tag);
  if (plan.appVersion !== expectedAppVersion) {
    throw new Error(
      `appVersion does not match tag: expected '${expectedAppVersion}', got '${plan.appVersion}'`,
    );
  }

  const expectedMsixVersion = toMsixVersion(plan.tag);
  if (plan.msixVersion !== expectedMsixVersion) {
    throw new Error(
      `msixVersion does not match tag: expected '${expectedMsixVersion}', got '${plan.msixVersion}'`,
    );
  }

  // Channels validation
  if (!plan.channels || typeof plan.channels !== "object" || Array.isArray(plan.channels)) {
    throw new Error("channels must be an object");
  }
  assertExactKeys(plan.channels, ALLOWED_CHANNELS_KEYS, "channels");
  if (typeof plan.channels.store !== "boolean") {
    throw new Error("channels.store must be a boolean");
  }
  if (typeof plan.channels.nsisMigration !== "boolean") {
    throw new Error("channels.nsisMigration must be a boolean");
  }

  // Required targets validation
  if (!Array.isArray(plan.requiredTargets)) {
    throw new Error("requiredTargets must be an array of strings");
  }
  if (plan.requiredTargets.length === 0) {
    throw new Error("requiredTargets must not be empty");
  }
  const targetSet = new Set();
  for (const t of plan.requiredTargets) {
    if (typeof t !== "string" || t.trim() === "") {
      throw new Error("Each target in requiredTargets must be a non-empty string");
    }
    if (targetSet.has(t)) {
      throw new Error(`duplicate target '${t}' in requiredTargets`);
    }
    targetSet.add(t);
  }

  const hasWindows = targetSet.has("windows-x86_64");
  if (!plan.channels.nsisMigration && hasWindows) {
    throw new Error(
      "target 'windows-x86_64' requires channels.nsisMigration to be true",
    );
  }
  if (plan.channels.nsisMigration && !hasWindows) {
    throw new Error(
      "channels.nsisMigration requires 'windows-x86_64' in requiredTargets",
    );
  }

  const baseTargets = ["darwin-aarch64", "linux-x86_64"];
  for (const bt of baseTargets) {
    if (!targetSet.has(bt)) {
      throw new Error(`requiredTargets must include '${bt}'`);
    }
  }

  // Toolchains validation
  validateToolchains(plan.toolchains, { required: REQUIRED_TOOLCHAIN_KEYS });

  // sourceDateEpoch
  if (
    typeof plan.sourceDateEpoch !== "number" ||
    !Number.isInteger(plan.sourceDateEpoch) ||
    plan.sourceDateEpoch <= 0
  ) {
    throw new Error(
      `sourceDateEpoch must be a positive integer, got ${JSON.stringify(plan.sourceDateEpoch)}`,
    );
  }

  // createdAt
  if (
    typeof plan.createdAt !== "string" ||
    Number.isNaN(Date.parse(plan.createdAt))
  ) {
    throw new Error(
      `createdAt must be a valid ISO 8601 date string, got ${JSON.stringify(plan.createdAt)}`,
    );
  }

  return plan;
}

/**
 * Returns the list of required artifact kinds for a given release plan.
 *
 * @param {object} plan
 * @returns {string[]}
 */
export function requiredKinds(plan) {
  const p = typeof plan === "string" || !plan.schemaVersion ? parsePlan(plan) : plan;
  const kinds = ["macos-updater", "dmg", "appimage", "deb", "msix", "cli-linux-amd64", "cli-darwin-universal", "rd-client-linux-amd64", "rd-client-darwin-universal", "rd-client-windows-amd64"];
  if (p.channels && p.channels.nsisMigration) {
    kinds.push("nsis");
  }
  return kinds;
}

/**
 * Parses and validates a Build Receipt according to Section 4 and Section 9.
 *
 * @param {string | object} value
 * @param {object} [plan]
 * @returns {object}
 */
export function parseReceipt(value, plan = null) {
  let receipt = value;
  if (typeof value === "string") {
    try {
      receipt = JSON.parse(value);
    } catch (err) {
      throw new Error(`Receipt must be valid JSON: ${err.message}`);
    }
  }

  if (!receipt || typeof receipt !== "object" || Array.isArray(receipt)) {
    throw new Error("Receipt must be a JSON object");
  }

  assertExactKeys(receipt, ALLOWED_RECEIPT_KEYS, "Receipt", REQUIRED_RECEIPT_KEYS);

  if (receipt.schemaVersion !== 1) {
    throw new Error(`schemaVersion must be 1, got ${JSON.stringify(receipt.schemaVersion)}`);
  }

  if (typeof receipt.runId !== "string" || receipt.runId.trim() === "") {
    throw new Error("runId must be a non-empty string");
  }

  if (!VALID_HOSTS.includes(receipt.host)) {
    throw new Error(
      `host must be one of [${VALID_HOSTS.join(", ")}], got ${JSON.stringify(receipt.host)}`,
    );
  }

  if (typeof receipt.commitSha !== "string" || !COMMIT_SHA_REGEX.test(receipt.commitSha)) {
    throw new Error(`commitSha must be a 40-character lowercase hex string, got ${JSON.stringify(receipt.commitSha)}`);
  }

  if (typeof receipt.appVersion !== "string" || receipt.appVersion.trim() === "") {
    throw new Error("appVersion must be a non-empty string");
  }

  if (
    typeof receipt.completedAt !== "string" ||
    Number.isNaN(Date.parse(receipt.completedAt))
  ) {
    throw new Error(
      `completedAt must be a valid ISO 8601 date string, got ${JSON.stringify(receipt.completedAt)}`,
    );
  }

  if (receipt.exitCode !== 0) {
    throw new Error(`exitCode must be 0, got ${JSON.stringify(receipt.exitCode)}`);
  }

  if ("toolchains" in receipt) {
    validateToolchains(receipt.toolchains, { required: [] });
  }

  // Cross-check with plan if provided
  if (plan) {
    const parsedPlan = plan.schemaVersion ? plan : parsePlan(plan);
    if (receipt.runId !== parsedPlan.runId) {
      throw new Error(`runId mismatch: expected '${parsedPlan.runId}', got '${receipt.runId}'`);
    }
    if (receipt.commitSha !== parsedPlan.commitSha) {
      throw new Error(
        `commitSha mismatch: expected '${parsedPlan.commitSha}', got '${receipt.commitSha}'`,
      );
    }
    if (receipt.appVersion !== parsedPlan.appVersion) {
      throw new Error(
        `appVersion mismatch: expected '${parsedPlan.appVersion}', got '${receipt.appVersion}'`,
      );
    }
  }

  // Artifacts validation
  if (!Array.isArray(receipt.artifacts) || receipt.artifacts.length === 0) {
    throw new Error("artifacts must be a non-empty array");
  }

  const seenKinds = new Set();
  const allowedArtifactKeysSet = new Set(ALLOWED_ARTIFACT_KEYS);

  for (let i = 0; i < receipt.artifacts.length; i += 1) {
    const art = receipt.artifacts[i];
    if (!art || typeof art !== "object" || Array.isArray(art)) {
      throw new Error(`artifact[${i}] must be an object`);
    }

    for (const k of Object.keys(art)) {
      if (!allowedArtifactKeysSet.has(k)) {
        throw new Error(`artifact[${i}] contains unexpected property: ${k}`);
      }
    }
    for (const k of ALLOWED_ARTIFACT_KEYS) {
      if (!(k in art)) {
        throw new Error(`artifact[${i}] missing required property: ${k}`);
      }
    }

    const kindDef = KIND_DEFINITIONS[art.kind];
    if (!kindDef) {
      throw new Error(`Unknown artifact kind: '${art.kind}'`);
    }

    if (kindDef.permittedHost !== receipt.host) {
      throw new Error(
        `kind '${art.kind}' not permitted for host '${receipt.host}' (expected '${kindDef.permittedHost}')`,
      );
    }

    if (seenKinds.has(art.kind)) {
      throw new Error(`duplicate artifact kind '${art.kind}' in receipt for host '${receipt.host}'`);
    }
    seenKinds.add(art.kind);

    if (typeof art.name !== "string" || art.name.trim() === "" || art.name.includes("/") || art.name.includes("\\")) {
      throw new Error(`artifact[${i}].name must be a non-empty filename without path separators`);
    }

    const matchesExtension = kindDef.allowedExtensions.some((ext) => art.name.endsWith(ext));
    if (!matchesExtension) {
      throw new Error(
        `artifact[${i}].name '${art.name}' does not match allowed extensions [${kindDef.allowedExtensions.join(", ")}] for kind '${art.kind}'`,
      );
    }

    if (!isSafeRelativePath(art.relPath)) {
      throw new Error(`artifact[${i}].relPath must be a safe relative path, got ${JSON.stringify(art.relPath)}`);
    }

    if (typeof art.bytes !== "number" || !Number.isInteger(art.bytes) || art.bytes <= 0) {
      throw new Error(`artifact[${i}].bytes must be a positive integer, got ${JSON.stringify(art.bytes)}`);
    }

    if (typeof art.sha256 !== "string" || !SHA256_REGEX.test(art.sha256)) {
      throw new Error(`artifact[${i}].sha256 must be a 64-character lowercase hex string`);
    }

    if (kindDef.isUpdater) {
      if (!art.signatureRelPath || typeof art.signatureRelPath !== "string" || !isSafeRelativePath(art.signatureRelPath)) {
        throw new Error(
          `signatureRelPath is required for updater kind '${art.kind}' and must be a safe relative path`,
        );
      }
    } else {
      if (art.signatureRelPath !== null && !isSafeRelativePath(art.signatureRelPath)) {
        throw new Error(
          `signatureRelPath for non-updater kind '${art.kind}' must be null or a safe relative path`,
        );
      }
    }

    if (!Array.isArray(art.targets)) {
      throw new Error(`artifact[${i}].targets must be an array of strings`);
    }

    // Validate targets against kind
    if (!kindDef.isUpdater) {
      if (art.targets.length !== 0) {
        throw new Error(`targets for kind '${art.kind}' must be empty, got [${art.targets.join(", ")}]`);
      }
    } else {
      if (art.targets.length === 0) {
        throw new Error(`updater kind '${art.kind}' must specify non-empty targets`);
      }
      for (const target of art.targets) {
        if (!kindDef.expectedTargets.includes(target)) {
          throw new Error(
            `invalid target '${target}' for kind '${art.kind}' (expected subset of [${kindDef.expectedTargets.join(", ")}])`,
          );
        }
      }
    }
  }

  return receipt;
}
