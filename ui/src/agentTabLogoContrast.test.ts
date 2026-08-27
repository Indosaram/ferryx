import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

// Do not switch to `import "./index.css?raw"`: Vite's CSS pipeline claims the module before the
// raw loader and yields "", which makes every assertion below pass vacuously.
const stylesheet = readFileSync(resolve(import.meta.dirname, "index.css"), "utf8");

describe("agent tab logo contrast", () => {
  it("inverts monochrome logo files on dark chrome but not the light theme", () => {
    expect(stylesheet).toMatch(/\.agent-tab-logo--monochrome\s*\{\s*filter:\s*invert\(1\)/);
    expect(stylesheet).toMatch(/:root\[data-theme="light"\]\s+\.agent-tab-logo--monochrome\s*\{\s*filter:\s*none/);
  });
});
