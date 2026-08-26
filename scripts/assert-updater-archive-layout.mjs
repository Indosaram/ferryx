#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";

const BLOCK_SIZE = 512;

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
    result.push({ path, type });
    offset += BLOCK_SIZE + paddedSize(size);
  }

  return result;
}

function invalidEntry(path) {
  return path.split("/").some((component) => component.startsWith("._"));
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

  process.stdout.write(`updater archive layout OK: ${archiveEntries.length} entries\n`);
  return 0;
}

process.exit(main());
