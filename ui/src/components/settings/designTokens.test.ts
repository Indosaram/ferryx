import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import * as primitives from "./primitives";

const SECTION_FILES = [
  "AppearanceSection.tsx",
  "BrowserSection.tsx",
  "GeneralSection.tsx",
  "NotificationsSection.tsx",
  "RemoteAccessSection.tsx",
  "ShortcutsSection.tsx",
  "TerminalSection.tsx",
  "AgentsSection.tsx",
  "primitives.tsx",
];

describe("settings design tokens and primitives", () => {
  it("does not contain text-[9px] or text-[10px] in any settings section", () => {
    for (const filename of SECTION_FILES) {
      const filePath = resolve(__dirname, filename);
      const content = readFileSync(filePath, "utf-8");
      expect(
        content,
        `Expected ${filename} not to contain text-[9px] or text-[10px]`,
      ).not.toMatch(/text-\[(?:9|10)px\]/);
    }
  });

  it("does not contain bare text-xs class in any settings section", () => {
    for (const filename of SECTION_FILES) {
      const filePath = resolve(__dirname, filename);
      const content = readFileSync(filePath, "utf-8");
      expect(
        content,
        `Expected ${filename} not to contain bare text-xs class`,
      ).not.toMatch(/\btext-xs\b/);
    }
  });

  it("exports SettingsGroup from primitives", () => {
    expect((primitives as Record<string, unknown>).SettingsGroup).toBeDefined();
    expect(typeof (primitives as Record<string, unknown>).SettingsGroup).toBe("function");
  });
});
