#!/usr/bin/env node
import { randomUUID } from "node:crypto";
import fs from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SEMVER_STRICT = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

function isLeapYear(year) {
  return (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
}

function getDaysInMonth(year, month) {
  if (month === 2) return isLeapYear(year) ? 29 : 28;
  return [4, 6, 9, 11].includes(month) ? 30 : 31;
}

function isCalVerShaped(tag) {
  return typeof tag === "string" && /^v?\d{4}\.\d{1,2}\.\d{1,2}(?:\.|$)/.test(tag);
}

function parseGenericSemVer(tag) {
  if (typeof tag !== "string") return null;
  const match = tag.match(SEMVER_STRICT);
  if (!match) return null;
  const [major, minor, patch] = [Number(match[1]), Number(match[2]), Number(match[3])];
  if (major > 65535 || minor > 65535 || patch > 65535) {
    throw new Error(`SemVer components exceed MSIX limit 65535: "${tag}"`);
  }
  return { major, minor, patch };
}

export function parseReleaseTag(tag) {
  if (typeof tag !== "string" || !tag.trim()) {
    throw new Error(`Invalid release tag: expected non-empty string, got ${typeof tag}`);
  }
  if (/^v?\d{4}\.\d{2}\.\d{2}\.0\d+$/.test(tag)) {
    throw new Error(`Invalid release tag "${tag}": leading zeros are not allowed in revision`);
  }
  if (/^v?\d{4}\.(?:\d|\d{3,})\.\d+(?:\.\d+)?$/.test(tag) || /^v?\d{4}\.\d+\.(?:\d|\d{3,})(?:\.\d+)?$/.test(tag)) {
    throw new Error(`Invalid release tag "${tag}": month and day must be 2 digits (MM.DD)`);
  }

  const match = tag.match(/^v?(\d{4})\.(\d{2})\.(\d{2})(?:\.(0|[1-9]\d*))?$/);
  if (!match) {
    throw new Error(`Invalid release tag format: "${tag}". Expected vYYYY.MM.DD or vYYYY.MM.DD.R`);
  }

  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
  const revision = match[4] !== undefined ? Number(match[4]) : 0;

  if (year < 2026) {
    throw new Error(`Invalid release tag "${tag}": year must be >= 2026, got ${year}`);
  }
  if (year > 65535) {
    throw new Error(`Invalid release tag "${tag}": year ${year} exceeds MSIX limit 65535`);
  }
  if (month < 1 || month > 12) {
    throw new Error(`Invalid release tag "${tag}": month must be 1..12, got ${month}`);
  }

  const maxDays = getDaysInMonth(year, month);
  if (day < 1 || day > maxDays) {
    if (month === 2 && day === 29 && !isLeapYear(year)) {
      throw new Error(`Invalid calendar date in release tag "${tag}": ${year} is not a leap year (February has 28 days)`);
    }
    throw new Error(`Invalid calendar date in release tag "${tag}": month ${month} has ${maxDays} days, got day ${day}`);
  }
  if (revision < 0 || revision > 65535) {
    throw new Error(`Invalid release tag "${tag}": revision must be 0..65535, got ${revision}`);
  }

  return { year, month, day, revision };
}

function resolveTag(tag) {
  if (typeof tag !== "string" || !tag.trim()) {
    throw new Error(`Invalid release tag: expected non-empty string, got ${typeof tag}`);
  }
  if (isCalVerShaped(tag)) {
    const { year, month, day, revision } = parseReleaseTag(tag);
    return { major: year, minor: month * 100 + day, patch: revision };
  }
  const semver = parseGenericSemVer(tag);
  if (semver) return semver;
  return parseReleaseTag(tag);
}

export function toAppVersion(tag) {
  const { major, minor, patch } = resolveTag(tag);
  return `${major}.${minor}.${patch}`;
}

export function toMsixVersion(tag) {
  const { major, minor, patch } = resolveTag(tag);
  return `${major}.${minor}.${patch}.0`;
}

function prepareTauriConf(rawContent, version) {
  let config;
  try {
    config = JSON.parse(rawContent);
  } catch (err) {
    throw new Error(`Failed to parse tauri.conf.json: ${err.message}`);
  }
  config.version = version;
  return `${JSON.stringify(config, null, 2)}\n`;
}

function prepareCargoToml(rawContent, version) {
  const lines = rawContent.split("\n");
  let inPackage = false;
  let replaced = false;
  for (let i = 0; i < lines.length; i += 1) {
    const section = lines[i].match(/^\s*\[([^\]]+)\]\s*$/);
    if (section) {
      if (replaced) break;
      inPackage = section[1] === "package";
      continue;
    }
    if (inPackage && /^\s*version\s*=/.test(lines[i])) {
      lines[i] = `version = "${version}"`;
      replaced = true;
      inPackage = false;
    }
  }
  if (!replaced) throw new Error("no [package] version line found in Cargo.toml");
  return lines.join("\n");
}

export async function syncVersion({ tag, confPath, cargoPath, dryRun = false } = {}) {
  if (!tag) throw new Error("tag is required");

  const version = toAppVersion(tag);
  const msixVersion = toMsixVersion(tag);

  const resolvedConf = resolve(confPath ?? resolve(REPO_ROOT, "src-tauri/tauri.conf.json"));
  const resolvedCargo = resolve(cargoPath ?? resolve(REPO_ROOT, "src-tauri/Cargo.toml"));

  // Read and validate BOTH files before any mutation
  const originalConf = fs.readFileSync(resolvedConf, "utf8");
  const newConf = prepareTauriConf(originalConf, version);

  const originalCargo = fs.readFileSync(resolvedCargo, "utf8");
  const newCargo = prepareCargoToml(originalCargo, version);

  if (dryRun) return { version, msixVersion };

  // Atomic write using unique sibling temporary files and best-effort rollback
  const tmpConf = `${resolvedConf}.${randomUUID()}.tmp`;
  const tmpCargo = `${resolvedCargo}.${randomUUID()}.tmp`;
  let confReplaced = false;

  try {
    fs.writeFileSync(tmpConf, newConf, "utf8");
    fs.writeFileSync(tmpCargo, newCargo, "utf8");

    fs.renameSync(tmpConf, resolvedConf);
    confReplaced = true;

    fs.renameSync(tmpCargo, resolvedCargo);
  } catch (error) {
    const cleanupErrors = [];

    for (const tmpFile of [tmpConf, tmpCargo]) {
      try {
        fs.rmSync(tmpFile, { force: true });
      } catch (rmErr) {
        cleanupErrors.push(rmErr);
      }
    }

    if (confReplaced) {
      try {
        fs.writeFileSync(resolvedConf, originalConf, "utf8");
      } catch (rollbackErr) {
        cleanupErrors.push(rollbackErr);
      }
    }

    if (cleanupErrors.length > 0) {
      throw new AggregateError(
        [error, ...cleanupErrors],
        `Version sync failed with error "${error.message}" and rollback/cleanup encountered errors`,
      );
    }

    throw error;
  }

  return { version, msixVersion };
}

function parseArgs(argv) {
  const args = new Map();
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith("--")) continue;
    const key = token.slice(2);
    if (key === "dry-run") {
      args.set(key, "true");
      continue;
    }
    const next = argv[index + 1];
    if (next === undefined || next.startsWith("--")) {
      throw new Error(`option ${token} requires a value`);
    }
    args.set(key, next);
    index += 1;
  }
  return args;
}

async function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return 2;
  }

  const tag = args.get("tag");
  if (!tag) {
    process.stderr.write("usage: sync-version.mjs --tag <tag> [--conf <path>] [--cargo <path>] [--dry-run]\n");
    return 2;
  }

  try {
    const { version } = await syncVersion({
      tag,
      confPath: args.get("conf"),
      cargoPath: args.get("cargo"),
      dryRun: args.has("dry-run"),
    });
    process.stdout.write(`version=${version}\n`);
    return 0;
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return 1;
  }
}

const isDirectExecution = () => {
  if (!process.argv[1]) return false;
  try {
    const scriptPath = fileURLToPath(import.meta.url);
    const entryPath = resolve(process.argv[1]);
    if (scriptPath === entryPath) return true;
    return fs.realpathSync(scriptPath) === fs.realpathSync(entryPath);
  } catch {
    return false;
  }
};

if (isDirectExecution()) {
  main().then((code) => {
    process.exit(code);
  }).catch((err) => {
    process.stderr.write(`${err.message}\n`);
    process.exit(1);
  });
}
