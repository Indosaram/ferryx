import { describe, expect, it } from "vitest";

import { resolveTerminalAppearance, type AppearanceTheme } from "./appearanceSettings";

describe("resolveTerminalAppearance", () => {
  it.each<[AppearanceTheme, boolean, "light" | "dark"]>([
    ["light", false, "light"],
    ["light", true, "light"],
    ["dark", false, "dark"],
    ["dark", true, "dark"],
    ["charcoal", false, "dark"],
    ["charcoal", true, "dark"],
    ["system", false, "dark"],
    ["system", true, "light"],
  ])("theme %s with prefersLight=%s resolves to %s", (theme, prefersLight, expected) => {
    expect(resolveTerminalAppearance(theme, prefersLight)).toBe(expected);
  });
});
