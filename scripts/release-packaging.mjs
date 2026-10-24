#!/usr/bin/env node
/**
 * Renders the Homebrew cask (macOS DMG) and formula (linux x64 ferryx-cli) for one release
 * from its checksum listing. The committed files in packaging/homebrew are the templates; only
 * the version, sha256 and download URL lines change per release. Nothing is published here:
 * the rendered files become release assets, and a tap is updated later by copying them.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const TEMPLATE_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..", "packaging", "homebrew");
const CHECKSUM_LINE = /^([0-9a-f]{64}) {2}(.+)$/;
const TAG_PATTERN = /^v[0-9A-Za-z.-]+$/;
const REPO_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;

export const DEFAULT_REPO = "Indosaram/ferryx";

/** Rendered file name -> template file and the release asset whose hash it pins. */
export const HOMEBREW_PACKAGES = Object.freeze([
  Object.freeze({ name: "ferryx.rb", template: "ferryx.rb", asset: "Ferryx_universal.dmg" }),
  Object.freeze({ name: "ferryx-cli.rb", template: "ferryx-cli.rb", asset: "ferryx-cli-linux-amd64" }),
]);
export const HOMEBREW_PACKAGE_NAMES = Object.freeze(HOMEBREW_PACKAGES.map((pkg) => pkg.name));

/** Parses `shasum -a 256` output (SHA256SUMS / SHA256SUMS.txt) into name -> sha256. */
export function parseChecksums(text) {
  const sums = new Map();
  for (const line of text.split("\n")) {
    if (line === "") continue;
    const match = CHECKSUM_LINE.exec(line);
    if (!match) throw new Error(`Malformed checksum line: ${JSON.stringify(line)}`);
    if (sums.has(match[2])) throw new Error(`Duplicate checksum entry: ${match[2]}`);
    sums.set(match[2], match[1]);
  }
  return sums;
}

function replaceOnce(text, pattern, replacement, label) {
  const matches = text.match(new RegExp(pattern.source, `${pattern.flags.replace("g", "")}g`));
  if (!matches || matches.length !== 1) {
    throw new Error(`Homebrew template must contain exactly one ${label} line, found ${matches ? matches.length : 0}`);
  }
  return text.replace(pattern, replacement);
}

function renderTemplate(template, { version, sha256, url }) {
  let out = replaceOnce(template, /^(\s*)version "[^"]*"$/m, `$1version "${version}"`, "version");
  out = replaceOnce(out, /^(\s*)sha256 "[^"]*"$/m, `$1sha256 "${sha256}"`, "sha256");
  return replaceOnce(out, /^(\s*)url "[^"]*"$/m, `$1url "${url}"`, "url");
}

/**
 * Returns `{ "ferryx.rb": text, "ferryx-cli.rb": text }` for `tag`, pinned to the hashes in
 * `checksums` (the text of SHA256SUMS). Throws when a pinned asset is not listed.
 */
export function renderHomebrewPackages({ checksums, tag, repo = DEFAULT_REPO, templateDir = TEMPLATE_DIR }) {
  if (typeof tag !== "string" || !TAG_PATTERN.test(tag)) throw new Error(`Invalid release tag: ${JSON.stringify(tag)}`);
  if (typeof repo !== "string" || !REPO_PATTERN.test(repo)) throw new Error(`Invalid repo: ${JSON.stringify(repo)}`);
  const sums = parseChecksums(checksums);
  const missing = HOMEBREW_PACKAGES.filter((pkg) => !sums.has(pkg.asset)).map((pkg) => pkg.asset);
  if (missing.length > 0) throw new Error(`Release checksums do not list required asset(s): ${missing.join(", ")}`);
  const version = tag.slice(1);
  const rendered = {};
  for (const pkg of HOMEBREW_PACKAGES) {
    rendered[pkg.name] = renderTemplate(readFileSync(join(templateDir, pkg.template), "utf8"), {
      version,
      sha256: sums.get(pkg.asset),
      url: `https://github.com/${repo}/releases/download/${tag}/${pkg.asset}`,
    });
  }
  return rendered;
}

/** Renders from `sumsPath` and writes the files into `outDir`; returns the written names. */
export function writeHomebrewPackages({ sumsPath, outDir, tag, repo = DEFAULT_REPO, templateDir = TEMPLATE_DIR }) {
  const rendered = renderHomebrewPackages({ checksums: readFileSync(sumsPath, "utf8"), tag, repo, templateDir });
  for (const [name, text] of Object.entries(rendered)) writeFileSync(join(outDir, name), text);
  return Object.keys(rendered);
}

function main(argv) {
  const options = new Map();
  for (let i = 0; i < argv.length; i += 2) {
    if (!argv[i].startsWith("--") || argv[i + 1] === undefined) throw new Error(`Unexpected argument: ${argv[i]}`);
    options.set(argv[i].slice(2), argv[i + 1]);
  }
  const sumsPath = options.get("sums");
  const outDir = options.get("out");
  const tag = options.get("tag");
  if (!sumsPath || !outDir || !tag) {
    console.error("Usage: release-packaging --sums <SHA256SUMS> --tag <vYYYY.MM.DD[.R]> --out <dir> [--repo owner/name]");
    process.exitCode = 2;
    return;
  }
  const names = writeHomebrewPackages({ sumsPath, outDir, tag, repo: options.get("repo") || DEFAULT_REPO });
  console.log(`Rendered ${names.join(", ")} for ${tag}`);
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  try {
    main(process.argv.slice(2));
  } catch (err) {
    console.error(`release-packaging: ${err.message}`);
    process.exit(1);
  }
}
