import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("ferryx browser document identity", () => {
  it("uses the ferryx title and crab-Orca favicon", () => {
    const html = readFileSync(resolve(process.cwd(), "index.html"), "utf8");

    expect(html).toContain("<title>ferryx</title>");
    expect(html).toContain('href="/src/assets/ferryx-icon.svg"');
  });
});
