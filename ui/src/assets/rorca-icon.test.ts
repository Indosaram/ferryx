import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("rorca app icon", () => {
  it("ships a labeled crab-Orca SVG master asset", () => {
    const svg = readFileSync(resolve(process.cwd(), "src/assets/rorca-icon.svg"), "utf8");

    expect(svg).toContain('aria-label="rorca crab icon"');
    expect(svg).toContain('id="shell"');
    expect(svg).toContain('id="wave"');
  });
});
