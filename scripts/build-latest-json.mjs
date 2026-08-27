#!/usr/bin/env node
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const UPDATER_ARTIFACT = /(\.app\.tar\.gz|\.nsis\.zip|-setup\.exe|\.AppImage(?:\.tar\.gz)?)$/;

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

function targetsFor(filename) {
  if (filename.endsWith(".app.tar.gz")) {
    if (/aarch64|arm64/i.test(filename)) return ["darwin-aarch64"];
    if (/x64|x86_64/i.test(filename)) return ["darwin-x86_64"];
    return ["darwin-aarch64", "darwin-x86_64"];
  }
  if (filename.endsWith(".nsis.zip") || filename.endsWith("-setup.exe")) {
    return ["windows-x86_64"];
  }
  if (filename.endsWith(".AppImage.tar.gz") || filename.endsWith(".AppImage")) {
    return ["linux-x86_64"];
  }
  return [];
}

function buildPlatforms(dir, repo, tag) {
  const platforms = {};
  for (const filename of readdirSync(dir).sort()) {
    if (!UPDATER_ARTIFACT.test(filename)) continue;

    const signaturePath = join(dir, `${filename}.sig`);
    if (!existsSync(signaturePath)) {
      process.stderr.write(`skipping ${filename}: no ${filename}.sig sibling was produced\n`);
      continue;
    }

    const entry = {
      signature: readFileSync(signaturePath, "utf8").trim(),
      url: `https://github.com/${repo}/releases/download/${tag}/${filename}`,
    };
    for (const target of targetsFor(filename)) {
      platforms[target] = entry;
    }
  }
  return platforms;
}

function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return 2;
  }

  const required = ["version", "dir", "out", "repo", "tag"];
  const missing = required.filter((name) => !args.get(name));
  if (missing.length > 0) {
    process.stderr.write(
      `usage: build-latest-json.mjs ${required.map((name) => `--${name} <value>`).join(" ")} [--notes <text>]\n` +
        `missing: ${missing.join(", ")}\n`,
    );
    return 2;
  }

  const version = args.get("version");
  const platforms = buildPlatforms(args.get("dir"), args.get("repo"), args.get("tag"));

  if (Object.keys(platforms).length === 0) {
    process.stderr.write(
      `no signed updater artifact found in ${args.get("dir")}: refusing to publish an empty manifest\n`,
    );
    return 1;
  }

  const manifest = {
    version,
    notes: args.get("notes") ?? `Ferryx ${version}`,
    pub_date: new Date().toISOString(),
    platforms,
  };

  writeFileSync(args.get("out"), `${JSON.stringify(manifest, null, 2)}\n`);
  process.stdout.write(`platforms=${Object.keys(platforms).join(",")}\n`);
  return 0;
}

process.exit(main());
