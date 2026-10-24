#!/usr/bin/env bun
// Release tag policy: every new release tag is `vYYYY.MMDD.N`.
//
//   YYYY  four-digit year, 2026 or later
//   MMDD  month and day of a real calendar date, zero-padded (0101..1231)
//   N     revision of that day, 1..65535, without leading zeros
//
// The app/updater version is `YYYY.(MM * 100 + DD).N`, so `v2026.1024.1` ships as `2026.1024.1`.
// Older published tags (`0.1.0`, `v0.1.0`, `vYYYY.MM.DD.N`) stay in place as legacy tags: they are
// listed in CHANGELOG.md and are never deleted or moved, but no new release may use their form.
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const RELEASE_TAG_SCHEME = "vYYYY.MMDD.N";
const TAG_PATTERN = /^v(\d{4})\.(\d{2})(\d{2})\.([1-9]\d*)$/;

function daysInMonth(year, month) {
  if (month === 2) return (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0 ? 29 : 28;
  return [4, 6, 9, 11].includes(month) ? 30 : 31;
}

/**
 * Checks a tag against the release tag policy.
 *
 * @param {unknown} tag
 * @returns {{ ok: true, year: number, month: number, day: number, revision: number } | { ok: false, reason: string }}
 */
export function checkReleaseTag(tag) {
  if (typeof tag !== "string" || tag.length === 0) {
    return { ok: false, reason: `expected a non-empty tag string, got ${JSON.stringify(tag)}` };
  }
  const match = tag.match(TAG_PATTERN);
  if (!match) {
    return { ok: false, reason: `"${tag}" does not match ${RELEASE_TAG_SCHEME} (for example v2026.1024.1)` };
  }
  const [year, month, day, revision] = match.slice(1).map(Number);
  if (year < 2026) return { ok: false, reason: `"${tag}": year must be 2026 or later` };
  if (month < 1 || month > 12) return { ok: false, reason: `"${tag}": month must be 01..12` };
  if (day < 1 || day > daysInMonth(year, month)) {
    return { ok: false, reason: `"${tag}": ${year}-${match[2]}-${match[3]} is not a calendar date` };
  }
  if (revision > 65535) return { ok: false, reason: `"${tag}": revision must be 1..65535` };
  return { ok: true, year, month, day, revision };
}

/**
 * Throws unless `tag` follows the release tag policy.
 *
 * @param {unknown} tag
 * @returns {{ year: number, month: number, day: number, revision: number }}
 */
export function assertReleaseTag(tag) {
  const result = checkReleaseTag(tag);
  if (!result.ok) throw new Error(`Invalid release tag: ${result.reason}`);
  const { year, month, day, revision } = result;
  return { year, month, day, revision };
}

// CLI: `bun scripts/release-tag-policy.mjs <tag>` exits 0 for a conforming tag, 1 otherwise.
if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const result = checkReleaseTag(process.argv[2]);
  if (result.ok) {
    process.stdout.write(`${process.argv[2]} follows ${RELEASE_TAG_SCHEME}\n`);
  } else {
    process.stderr.write(`Invalid release tag: ${result.reason}\n`);
    process.exit(1);
  }
}
