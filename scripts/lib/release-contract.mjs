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
  // Linux receipts only: the container image the AppImage/deb were built in and the glibc
  // floor the CLI was linked against (release-platforms.mjs createLinuxBuildScript).
  "linuxBuilder",
]);

export const REQUIRED_TOOLCHAIN_KEYS = Object.freeze(["node", "bun", "zig"]);

export const VALID_HOSTS = Object.freeze(["macbook", "omaki", "maho-win"]);

export const KIND_DEFINITIONS = Object.freeze({
  "macos-updater": {
    permittedHost: "macbook",
    isUpdater: true,
    expectedTargets: ["darwin-aarch64", "darwin-x86_64"],
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
    allowedExtensions: ["-linux-amd64", "ferryx-cli"],
  },
  "cli-darwin-universal": {
    permittedHost: "macbook",
    isUpdater: false,
    expectedTargets: [],
    stableAlias: "ferryx-cli-darwin-universal",
    allowedExtensions: ["-darwin-universal", "ferryx-cli"],
  },
  "cli-windows-amd64": {
    permittedHost: "maho-win",
    isUpdater: false,
    expectedTargets: [],
    stableAlias: "ferryx-cli-windows-amd64.exe",
    allowedExtensions: ["-windows-amd64.exe"],
  },
});

/**
 * Every updater target a release publishes in its manifests. src-tauri/src/ipc/updater.rs
 * (`PUBLISHED_UPDATER_TARGETS`) treats any other build target as externally managed.
 */
export const UPDATER_TARGETS = Object.freeze(
  Object.values(KIND_DEFINITIONS)
    .filter((definition) => definition.isUpdater)
    .flatMap((definition) => definition.expectedTargets)
    .sort(),
);

/**
 * Updater manifests written by build-latest-json.mjs. The stable channel reads
 * `releases/latest/download/latest.json`; `releases/latest` never resolves to a prerelease, so the
 * beta channel reads `releases/download/<BETA_CHANNEL_TAG>/latest-beta.json` from a fixed tag
 * (src-tauri/src/ipc/updater.rs `BETA_UPDATE_ENDPOINT`).
 */
export const STABLE_MANIFEST_NAME = "latest.json";
export const BETA_MANIFEST_NAME = "latest-beta.json";
export const BETA_CHANNEL_TAG = "beta";

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
  // Release floor gate results ({ waiver, checks }); see FLOOR_CHECKS.
  "floors",
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

/**
 * Advertised floors every released artifact must meet before a release is assembled.
 * - glibc: highest GLIBC_ symbol version any ELF file inside the artifact requires.
 * - macos-minos: highest LC_BUILD_VERSION minos per architecture over all Mach-O files
 *   ({ x86_64: "10.15", arm64: "11.0" }); the limit is the app's bundle.macOS.minimumSystemVersion
 *   (src-tauri/tauri.conf.json, supplied by the caller), raised to 11.0 for arm64 slices because
 *   macOS 11 is the first release on Apple Silicon and the linker never emits less.
 * - notarization: `spctl -a -vvv -t install` reports Notarized Developer ID and
 *   `xcrun stapler validate` passes, for the app and (dmg kind) the disk image.
 * - crt: `dumpbin /dependents` lists no Visual C++ redistributable DLL; a clean Windows does
 *   not have it and the MSIX declares no VCLibs dependency.
 */
export const FLOOR_CHECKS = Object.freeze({
  "macos-updater": Object.freeze(["macos-minos", "notarization"]),
  dmg: Object.freeze(["macos-minos", "notarization"]),
  "cli-darwin-universal": Object.freeze(["macos-minos"]),
  appimage: Object.freeze(["glibc"]),
  deb: Object.freeze(["glibc"]),
  "cli-linux-amd64": Object.freeze(["glibc"]),
  msix: Object.freeze(["crt"]),
  nsis: Object.freeze(["crt"]),
  "cli-windows-amd64": Object.freeze(["crt"]),
});

export const GLIBC_FLOORS = Object.freeze({ "cli-linux-amd64": "2.17", appimage: "2.35", deb: "2.35" });

/** Measurement field that carries each check's input in a host's floor measurements. */
const FLOOR_MEASUREMENT_FIELD = Object.freeze({
  glibc: "glibc",
  "macos-minos": "minos",
  notarization: "notarization",
  crt: "dependents",
});

const VC_REDIST_DLL_REGEX = /^(vcruntime|msvcp|vccorlib|concrt|vcomp|mfc)[0-9a-z_]*\.dll$/i;
const APPLE_SILICON_MINIMUM = "11.0";
const DOTTED_VERSION_REGEX = /^\d+(\.\d+)*$/;

/** Compares dotted numeric versions ("2.35" vs "2.17"); missing parts count as 0. */
export function compareDottedVersions(a, b) {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i += 1) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return Math.sign(d);
  }
  return 0;
}

/**
 * Evaluates one floor check from its measured value. Never trusts a recorded pass flag:
 * assembleRelease re-runs this on every receipt's recorded measurement.
 *
 * @returns {{ kind: string, check: string, limit: string | null, measured: unknown, pass: boolean, reason?: string }}
 */
export function evaluateFloorCheck({ kind, check, measured, macosMinimum = null }) {
  const result = (limit, pass, reason) => ({
    kind,
    check,
    limit,
    measured: measured ?? null,
    pass,
    ...(pass ? {} : { reason }),
  });
  if (check === "glibc") {
    const limit = GLIBC_FLOORS[kind];
    if (typeof measured !== "string" || !DOTTED_VERSION_REGEX.test(measured)) {
      return result(limit, false, "no GLIBC_ symbol version was measured");
    }
    return compareDottedVersions(measured, limit) <= 0
      ? result(limit, true)
      : result(limit, false, `requires GLIBC_${measured}, above the GLIBC_${limit} floor`);
  }
  if (check === "macos-minos") {
    if (typeof macosMinimum !== "string" || !DOTTED_VERSION_REGEX.test(macosMinimum)) {
      throw new Error(`macOS minimum system version must be a dotted version, got ${JSON.stringify(macosMinimum)}`);
    }
    const slices = measured && typeof measured === "object" && !Array.isArray(measured) ? Object.entries(measured) : [];
    if (slices.length === 0 || slices.some(([, v]) => typeof v !== "string" || !DOTTED_VERSION_REGEX.test(v))) {
      return result(macosMinimum, false, "no LC_BUILD_VERSION minos was measured");
    }
    const problems = [];
    for (const [arch, minos] of slices) {
      const raised = arch.startsWith("arm64") && compareDottedVersions(macosMinimum, APPLE_SILICON_MINIMUM) < 0;
      const limit = raised ? APPLE_SILICON_MINIMUM : macosMinimum;
      if (compareDottedVersions(minos, limit) > 0) {
        problems.push(
          `${arch} slice declares minos ${minos}, above ${raised ? `${limit} (the Apple Silicon floor)` : `the configured minimum ${limit}`}`,
        );
      }
    }
    return problems.length === 0 ? result(macosMinimum, true) : result(macosMinimum, false, problems.join("; "));
  }
  if (check === "notarization") {
    const required = kind === "dmg" ? ["app", "dmg"] : ["app"];
    const probes = Array.isArray(measured) ? measured : [];
    const problems = [];
    for (const target of required) {
      const probe = probes.find((p) => p && p.target === target);
      if (!probe) {
        problems.push(`${target}: not checked`);
        continue;
      }
      const spctl = typeof probe.spctl === "string" ? probe.spctl : "";
      if (probe.spctlStatus !== 0 || !spctl.includes("Notarized Developer ID")) {
        problems.push(`${target}: spctl does not report Notarized Developer ID (${spctl.replace(/\s+/g, " ").trim() || `exit ${probe.spctlStatus}`})`);
      }
      if (probe.staplerStatus !== 0) {
        problems.push(`${target}: stapler validate failed (exit ${probe.staplerStatus})`);
      }
    }
    return problems.length === 0 ? result(null, true) : result(null, false, problems.join("; "));
  }
  if (check === "crt") {
    if (!Array.isArray(measured) || measured.length === 0 || measured.some((d) => typeof d !== "string")) {
      return result(null, false, "no dumpbin /dependents output was captured");
    }
    const redist = measured.filter((d) => VC_REDIST_DLL_REGEX.test(d));
    return redist.length === 0
      ? result(null, true)
      : result(null, false, `depends on ${redist.join(", ")} (Visual C++ runtime, absent on a clean Windows)`);
  }
  throw new Error(`Unknown floor check '${check}' for kind '${kind}'`);
}

/**
 * Evaluates every floor check of the given artifact kinds from a host's measurements
 * ({ [kind]: { glibc?, minos?, notarization?, dependents? } }).
 */
export function evaluateFloors({ kinds, measurements, macosMinimum = null }) {
  const checks = [];
  for (const kind of kinds) {
    for (const check of FLOOR_CHECKS[kind] ?? []) {
      const measured = measurements?.[kind]?.[FLOOR_MEASUREMENT_FIELD[check]];
      checks.push(evaluateFloorCheck({ kind, check, measured, macosMinimum }));
    }
  }
  return checks;
}

/** Formats failing floor checks as one error line. */
export function describeFloorFailures(checks) {
  return checks
    .filter((c) => !c.pass)
    .map((c) => `${c.kind} ${c.check}: ${c.reason}`)
    .join("; ");
}

function validateFloors(floors) {
  if (!floors || typeof floors !== "object" || Array.isArray(floors)) {
    throw new Error("floors must be an object");
  }
  assertExactKeys(floors, ["waiver", "checks"], "floors");
  if (floors.waiver !== null && (typeof floors.waiver !== "string" || floors.waiver.trim() === "")) {
    throw new Error("floors.waiver must be null or a non-empty reason");
  }
  if (!Array.isArray(floors.checks)) {
    throw new Error("floors.checks must be an array");
  }
  for (const [i, c] of floors.checks.entries()) {
    if (!c || typeof c !== "object" || Array.isArray(c)) {
      throw new Error(`floors.checks[${i}] must be an object`);
    }
    if (!FLOOR_CHECKS[c.kind]?.includes(c.check)) {
      throw new Error(`floors.checks[${i}] has unknown check '${c.check}' for kind '${c.kind}'`);
    }
    if (typeof c.pass !== "boolean") {
      throw new Error(`floors.checks[${i}].pass must be a boolean`);
    }
  }
}

const SHA256_REGEX = /^[0-9a-f]{64}$/;
const COMMIT_SHA_REGEX = /^[0-9a-f]{40}$/;
const REPO_REGEX = /^[a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+$/;

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

  const baseTargets = ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"];
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
  const kinds = [
    "macos-updater",
    "dmg",
    "appimage",
    "deb",
    "msix",
    "cli-linux-amd64",
    "cli-darwin-universal",
    "cli-windows-amd64",
  ];
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

  if ("floors" in receipt) {
    validateFloors(receipt.floors);
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
