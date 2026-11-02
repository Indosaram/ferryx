import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const config = readFileSync(resolve(__dirname, "../tailwind.config.js"), "utf-8");
const block = config.match(/fontSize:\s*\{([\s\S]*?)\n\s{6}\},/)?.[1] ?? "";
const sizes = [...block.matchAll(/"(phone-[\w-]+)":\s*\["([^"]+)",\s*\{\s*lineHeight:\s*"([^"]+)"\s*\}\]/g)].map(
  ([, name, size, lineHeight]) => ({ name, size, lineHeight }),
);
const px = (rem: string) => Number.parseFloat(rem) * 16;

describe("phone type scale", () => {
  it("defines the phone scale", () => {
    expect(sizes.map((entry) => entry.name)).toEqual(
      expect.arrayContaining(["phone-meta", "phone-body", "phone-input"]),
    );
  });

  it("is rem-based so the density setting and 200% text scale every phone label", () => {
    for (const { name, size, lineHeight } of sizes) {
      expect(size, name).toMatch(/^\d+(\.\d+)?rem$/);
      expect(lineHeight, name).toMatch(/^\d+(\.\d+)?rem$/);
    }
  });

  it("never goes below 12px for text or 16px for inputs", () => {
    for (const { name, size } of sizes) expect(px(size), name).toBeGreaterThanOrEqual(12);
    const input = sizes.find((entry) => entry.name === "phone-input");
    expect(input && px(input.size)).toBeGreaterThanOrEqual(16);
  });
});
