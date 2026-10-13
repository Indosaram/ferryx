import { describe, expect, it } from "vitest";

import { classifyNavigatorPlatform } from "./platform";

describe("classifyNavigatorPlatform", () => {
  it("classifies the webview platform signals shared by every UI caller", () => {
    expect(classifyNavigatorPlatform({ platform: "MacIntel" })).toBe("macos");
    expect(classifyNavigatorPlatform({ platform: "iPhone" })).toBe("macos");
    expect(classifyNavigatorPlatform({ platform: "Win32", userAgent: "Windows NT 10.0" })).toBe("windows");
    expect(classifyNavigatorPlatform({ platform: "Linux x86_64", userAgent: "X11; Linux x86_64" })).toBe("linux");
    expect(classifyNavigatorPlatform({ userAgent: "Mozilla/5.0 (X11; CrOS x86_64)" })).toBe("linux");
    expect(classifyNavigatorPlatform({ userAgentData: { platform: "macOS" } })).toBe("macos");
    expect(classifyNavigatorPlatform({})).toBe("unknown");
    expect(classifyNavigatorPlatform(undefined)).toBe("unknown");
  });
});
