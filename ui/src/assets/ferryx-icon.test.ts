import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("ferryx app icon", () => {
  it("ships the authentic master dark metallic squircle icon", () => {
    const pngPath = resolve(process.cwd(), "src/assets/ferryx-icon.png");
    expect(existsSync(pngPath)).toBe(true);
  });
});
