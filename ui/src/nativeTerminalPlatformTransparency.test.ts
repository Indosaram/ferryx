import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

const css = readFileSync(join(process.cwd(), "src", "index.css"), "utf8");
const main = readFileSync(join(process.cwd(), "src", "main.tsx"), "utf8");

describe("native terminal platform transparency", () => {
  it("keeps root application surfaces opaque outside macOS", () => {
    expect(css).not.toMatch(/(?:^|\n)html:has\(\[data-testid="native-terminal-pane"\]\)/);
    expect(css).toContain('html.platform-macos:has([data-testid="native-terminal-pane"])');
    expect(main).toContain('document.documentElement.classList.toggle("platform-macos", isMacShortcutPlatform())');
  });
});
