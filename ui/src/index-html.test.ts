import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("rorca browser document identity", () => {
  it("uses the rorca title and crab-Orca favicon", () => {
    const html = readFileSync(resolve(process.cwd(), "index.html"), "utf8");

    expect(html).toContain("<title>rorca</title>");
    expect(html).toContain('href="/src/assets/rorca-icon.svg"');
  });
});
