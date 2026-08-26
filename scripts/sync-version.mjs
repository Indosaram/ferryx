#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DATE_VERSION = /^\d{4}\.\d{2}\.\d{2}(\.\d+)?$/;
const SEMVER = /^\d+\.\d+\.\d+$/;

function parseArgs(argv) {
  const args = new Map();
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith("--")) continue;
    const next = argv[index + 1];
    if (next === undefined || next.startsWith("--")) {
      throw new Error(`option ${token} requires a value`);
    }
    args.set(token.slice(2), next);
    index += 1;
  }
  return args;
}

function resolveVersion(tag) {
  const input = tag.startsWith("v") ? tag.slice(1) : tag;
  if (DATE_VERSION.test(input)) {
    const [year, month, day, revision = "0"] = input.split(".");
    return `${Number(year)}.${Number(month) * 100 + Number(day)}.${Number(revision)}`;
  }
  if (!SEMVER.test(input)) {
    throw new Error(
      `tag "${tag}" is not a release version: expected vYYYY.MM.DD, vYYYY.MM.DD.N, or vX.Y.Z`,
    );
  }
  return input;
}

function rewriteTauriConf(path, version) {
  const config = JSON.parse(readFileSync(path, "utf8"));
  config.version = version;
  writeFileSync(path, `${JSON.stringify(config, null, 2)}\n`);
}

// Only the [package] version is rewritten; every dependency `version = "..."` line must survive
// byte-identical, so the scan stops at the next section header.
function rewriteCargoToml(path, version) {
  const lines = readFileSync(path, "utf8").split("\n");
  let inPackage = false;
  let replaced = false;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const section = line.match(/^\s*\[([^\]]+)\]\s*$/);
    if (section) {
      if (replaced) break;
      inPackage = section[1] === "package";
      continue;
    }
    if (!inPackage) continue;
    if (/^\s*version\s*=/.test(line)) {
      lines[index] = `version = "${version}"`;
      replaced = true;
      inPackage = false;
    }
  }
  if (!replaced) {
    throw new Error(`no [package] version line found in ${path}`);
  }
  writeFileSync(path, lines.join("\n"));
}

function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return 2;
  }

  const tag = args.get("tag");
  if (!tag) {
    process.stderr.write("usage: sync-version.mjs --tag <tag> [--conf <path>] [--cargo <path>]\n");
    return 2;
  }

  let version;
  try {
    version = resolveVersion(tag);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return 1;
  }

  const confPath = resolve(args.get("conf") ?? resolve(REPO_ROOT, "src-tauri/tauri.conf.json"));
  const cargoPath = resolve(args.get("cargo") ?? resolve(REPO_ROOT, "src-tauri/Cargo.toml"));

  try {
    rewriteTauriConf(confPath, version);
    rewriteCargoToml(cargoPath, version);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return 1;
  }

  process.stdout.write(`version=${version}\n`);
  return 0;
}

process.exit(main());
