import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("web manifest and service worker assets", () => {
  it("parses manifest.webmanifest and has Ferryx branding", () => {
    const manifestPath = resolve(process.cwd(), "public/manifest.webmanifest");
    expect(existsSync(manifestPath)).toBe(true);
    const content = readFileSync(manifestPath, "utf8");
    const manifest = JSON.parse(content);
    expect(manifest.name).toBe("Ferryx Remote");
    expect(manifest.short_name).toBe("Ferryx");
  });

  it("every icon path in manifest exists on disk", () => {
    const manifestPath = resolve(process.cwd(), "public/manifest.webmanifest");
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    expect(Array.isArray(manifest.icons)).toBe(true);
    expect(manifest.icons.length).toBeGreaterThan(0);

    for (const icon of manifest.icons) {
      const iconPath = resolve(
        process.cwd(),
        icon.src.startsWith("/src/") ? `.${icon.src}` : `public${icon.src}`
      );
      expect(existsSync(iconPath), `Manifest icon ${icon.src} does not exist at ${iconPath}`).toBe(true);
    }
  });

  it("every asset path in sw.js precache list exists on disk", () => {
    const swPath = resolve(process.cwd(), "public/sw.js");
    expect(existsSync(swPath)).toBe(true);
    const content = readFileSync(swPath, "utf8");

    const match = content.match(/ASSETS_TO_CACHE\s*=\s*\[([\s\S]*?)\]/);
    expect(match).toBeTruthy();
    const assets = (match ? match[1] : "")
      .split(",")
      .map((s) => s.trim().replace(/^['"]|['"]$/g, ""))
      .filter((s) => s.length > 0);

    expect(assets.length).toBeGreaterThan(0);
    for (const asset of assets) {
      let filePath: string;
      if (asset === "/" || asset === "/index.html") {
        filePath = resolve(process.cwd(), "index.html");
      } else if (asset.startsWith("/src/")) {
        filePath = resolve(process.cwd(), `.${asset}`);
      } else {
        filePath = resolve(process.cwd(), `public${asset}`);
      }
      expect(existsSync(filePath), `Precached asset ${asset} does not exist at ${filePath}`).toBe(true);
    }
  });
});
