import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("ferryx app icon", () => {
  it("ships a labeled crab-Orca SVG master asset", () => {
    const svg = readFileSync(resolve(process.cwd(), "src/assets/ferryx-icon.svg"), "utf8");

    expect(svg).toContain('aria-label="ferryx crab icon"');
    expect(svg).toContain('id="shell"');
    expect(svg).toContain('id="wave"');
  });
});
