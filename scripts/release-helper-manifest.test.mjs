import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { refreshBundledHelperManifest } from "./lib/release-platforms.mjs";

// Signing rewrites each helper code-signature blob, changing its length and
// sha256, so the bundled manifest must be refreshed from the signed bytes or
// every SSH spawn fails with helper_asset_length_mismatch. That refresh writes
// inside Contents/Resources, which the code signature seals: doing it after
// signing leaves the app reporting "a sealed resource is missing or invalid",
// and no notarization ticket can be stapled. finalizeMacosBundle must therefore
// re-seal the app after refreshing the manifest.
describe("refreshBundledHelperManifest", () => {
  test("rewrites stale sha256 and byteLength from the signed helper bytes", () => {
    const root = mkdtempSync(join(tmpdir(), "ferryx-manifest-"));
    const helpersDir = join(root, "Contents", "Resources", "helpers");
    const targetDir = join(helpersDir, "aarch64-apple-darwin");
    mkdirSync(targetDir, { recursive: true });

    const bytes = Buffer.from("signed-helper-bytes");
    writeFileSync(join(targetDir, "ferryx-remote-helper"), bytes);
    const manifestPath = join(helpersDir, "manifest.json");
    writeFileSync(
      manifestPath,
      JSON.stringify({
        artifacts: [
          {
            target: "aarch64-apple-darwin",
            filename: "ferryx-remote-helper",
            sha256: "0".repeat(64),
            byteLength: 1,
          },
        ],
      }),
    );

    expect(refreshBundledHelperManifest(root)).toBe(1);

    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    expect(manifest.artifacts[0].sha256).toBe(createHash("sha256").update(bytes).digest("hex"));
    expect(manifest.artifacts[0].byteLength).toBe(bytes.length);
  });

  test("reports no change when the manifest already matches the helper bytes", () => {
    const root = mkdtempSync(join(tmpdir(), "ferryx-manifest-ok-"));
    const helpersDir = join(root, "Contents", "Resources", "helpers");
    const targetDir = join(helpersDir, "aarch64-apple-darwin");
    mkdirSync(targetDir, { recursive: true });

    const bytes = Buffer.from("already-current");
    writeFileSync(join(targetDir, "ferryx-remote-helper"), bytes);
    writeFileSync(
      join(helpersDir, "manifest.json"),
      JSON.stringify({
        artifacts: [
          {
            target: "aarch64-apple-darwin",
            filename: "ferryx-remote-helper",
            sha256: createHash("sha256").update(bytes).digest("hex"),
            byteLength: bytes.length,
          },
        ],
      }),
    );

    expect(refreshBundledHelperManifest(root)).toBe(0);
  });
});
