import { beforeEach, describe, expect, it, vi } from "vitest";

import { resetBrowserSettings, saveBrowserSettings } from "./browserSettings";
import {
  openTerminalToken,
  registerBuiltInBrowserLinkOpener,
  requestTerminalLinkOpen,
  resolveTokenAtCol,
  routeHttpLink,
  TERMINAL_LINK_ACTION_EVENT,
} from "./linkRouting";
import { openExternalUrl } from "./browserTauri";
import { invoke, isTauri } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

vi.mock("./browserTauri", () => ({
  BROWSER_SHORTCUT_EVENT: "ferryx:browser-shortcut",
  openExternalUrl: vi.fn(() => Promise.resolve()),
}));

beforeEach(() => {
  localStorage.clear();
  resetBrowserSettings();
  vi.mocked(openExternalUrl).mockReset().mockResolvedValue(undefined);
});

describe("HTTP link routing", () => {
  it("routes ordinary links to the registered built-in browser by default", async () => {
    const openBuiltIn = vi.fn(() => Promise.resolve());
    const unregister = registerBuiltInBrowserLinkOpener(openBuiltIn);
    try {
      await expect(routeHttpLink("https://example.com/docs", { source: "app" })).resolves.toBe("builtin");
      expect(openBuiltIn).toHaveBeenCalledWith("https://example.com/docs");
      expect(openExternalUrl).not.toHaveBeenCalled();
    } finally {
      unregister();
    }
  });

  it("routes Shift-click to the system browser when the modifier setting is enabled", async () => {
    const openBuiltIn = vi.fn(() => Promise.resolve());
    const unregister = registerBuiltInBrowserLinkOpener(openBuiltIn);
    try {
      saveBrowserSettings({ shiftOpensSystemBrowser: true, openLinksInBuiltInBrowser: true });
      await expect(routeHttpLink("https://example.com", { shiftKey: true, source: "app" })).resolves.toBe("external");
      expect(openExternalUrl).toHaveBeenCalledWith("https://example.com");
      expect(openBuiltIn).not.toHaveBeenCalled();
    } finally {
      unregister();
    }
  });

  it("keeps Shift on the normal built-in route when the modifier override is disabled", async () => {
    const openBuiltIn = vi.fn(() => Promise.resolve());
    const unregister = registerBuiltInBrowserLinkOpener(openBuiltIn);
    try {
      saveBrowserSettings({ shiftOpensSystemBrowser: false, openLinksInBuiltInBrowser: true });
      await expect(routeHttpLink("https://example.com", { shiftKey: true, source: "app" })).resolves.toBe("builtin");
      expect(openBuiltIn).toHaveBeenCalledOnce();
      expect(openExternalUrl).not.toHaveBeenCalled();
    } finally {
      unregister();
    }
  });

  it("opens system browser when built-in routing is disabled", async () => {
    saveBrowserSettings({ openLinksInBuiltInBrowser: false });
    await expect(routeHttpLink("https://example.com", { source: "markdown" })).resolves.toBe("external");
    expect(openExternalUrl).toHaveBeenCalledWith("https://example.com");
  });

  it("shows terminal actions when enabled and routes immediately when disabled", async () => {
    const actionEvents: string[] = [];
    const listener = (event: Event) => actionEvents.push((event as CustomEvent<{ url: string }>).detail.url);
    window.addEventListener(TERMINAL_LINK_ACTION_EVENT, listener);
    const openBuiltIn = vi.fn(() => Promise.resolve());
    const unregister = registerBuiltInBrowserLinkOpener(openBuiltIn);
    try {
      saveBrowserSettings({ showTerminalLinkActions: true });
      await expect(requestTerminalLinkOpen("https://example.com/terminal")).resolves.toBe("chooser");
      expect(actionEvents).toEqual(["https://example.com/terminal"]);
      expect(openBuiltIn).not.toHaveBeenCalled();

      saveBrowserSettings({ showTerminalLinkActions: false });
      await expect(requestTerminalLinkOpen("https://example.com/direct")).resolves.toBe("builtin");
      expect(openBuiltIn).toHaveBeenCalledWith("https://example.com/direct");
    } finally {
      unregister();
      window.removeEventListener(TERMINAL_LINK_ACTION_EVENT, listener);
    }
  });

  it("rejects non-http schemes before any opener is called", async () => {
    await expect(routeHttpLink("file:///etc/passwd")).rejects.toThrow(/http\(s\)/i);
    expect(openExternalUrl).not.toHaveBeenCalled();
  });
});

describe("resolveTokenAtCol", () => {
  it("extracts HTTP/HTTPS URLs when column lands within the URL", () => {
    const line = "See documentation at https://ferryx.dev/guide?tab=overview#sec for info";
    const token = resolveTokenAtCol(line, 25);
    expect(token).toEqual({
      type: "url",
      target: "https://ferryx.dev/guide?tab=overview#sec",
    });
  });

  it("returns null when column is outside the URL", () => {
    const line = "See documentation at https://ferryx.dev/guide for info";
    expect(resolveTokenAtCol(line, 5)).toBeNull();
    expect(resolveTokenAtCol(line, 50)).toBeNull();
  });

  it("extracts relative file path with line and column", () => {
    const line = "  --> src/components/App.tsx:42:10";
    const token = resolveTokenAtCol(line, 15);
    expect(token).toEqual({
      type: "file",
      path: "src/components/App.tsx",
      line: 42,
      col: 10,
      raw: "src/components/App.tsx:42:10",
    });
  });

  it("extracts absolute file path with line number only", () => {
    const line = "ERROR at /Users/user/project/src/main.rs:125: error found";
    const token = resolveTokenAtCol(line, 20);
    expect(token).toEqual({
      type: "file",
      path: "/Users/user/project/src/main.rs",
      line: 125,
      col: undefined,
      raw: "/Users/user/project/src/main.rs:125",
    });
  });

  it("extracts single file with extension like Cargo.toml:5", () => {
    const line = "failed to parse Cargo.toml:5";
    const token = resolveTokenAtCol(line, 20);
    expect(token).toEqual({
      type: "file",
      path: "Cargo.toml",
      line: 5,
      col: undefined,
      raw: "Cargo.toml:5",
    });
  });

  it("extracts plain file path without line or column", () => {
    const line = "Check file ./ui/package.json for dependencies";
    const token = resolveTokenAtCol(line, 18);
    expect(token).toEqual({
      type: "file",
      path: "./ui/package.json",
      raw: "./ui/package.json",
    });
  });

  it("strips enclosing quotes and brackets from path", () => {
    const line = "at Object.<anonymous> (/Users/user/index.js:15:3)";
    const token = resolveTokenAtCol(line, 30);
    expect(token).toEqual({
      type: "file",
      path: "/Users/user/index.js",
      line: 15,
      col: 3,
      raw: "/Users/user/index.js:15:3",
    });
  });

  it("handles balanced parentheses in URLs and strips trailing punctuation", () => {
    const wiki = "check https://en.wikipedia.org/wiki/Rust_(programming_language) here";
    expect(resolveTokenAtCol(wiki, 20)).toEqual({
      type: "url",
      target: "https://en.wikipedia.org/wiki/Rust_(programming_language)",
    });

    const trailingDot = "visit https://example.com/docs.";
    expect(resolveTokenAtCol(trailingDot, 15)).toEqual({
      type: "url",
      target: "https://example.com/docs",
    });
  });

  it("does not false-positive on IP addresses, versions, or decimal numbers", () => {
    expect(resolveTokenAtCol("Listening on 127.0.0.1:3000 ready", 16)).toBeNull();
    expect(resolveTokenAtCol("version v1.2.3 released", 10)).toBeNull();
    expect(resolveTokenAtCol("cost is 3.50 USD", 9)).toBeNull();
    expect(resolveTokenAtCol("module load foo.bar.baz here", 15)).toBeNull();
  });

  it("extracts quoted paths containing spaces", () => {
    const line = "check 'ui/my dir/config.json:10' for settings";
    expect(resolveTokenAtCol(line, 15)).toEqual({
      type: "file",
      path: "ui/my dir/config.json",
      line: 10,
      col: undefined,
      raw: "ui/my dir/config.json:10",
    });
  });

  it("extracts paths with apostrophes and escaped spaces", () => {
    const apostropheLine = "cat /Users/o'brien/file.txt";
    expect(resolveTokenAtCol(apostropheLine, 15)).toEqual({
      type: "file",
      path: "/Users/o'brien/file.txt",
      raw: "/Users/o'brien/file.txt",
    });

    const escapedSpaceLine = "open /Users/user/my\\ file.txt";
    expect(resolveTokenAtCol(escapedSpaceLine, 15)).toEqual({
      type: "file",
      path: "/Users/user/my file.txt",
      raw: "/Users/user/my file.txt",
    });
  });

  it("handles indented lines and CJK wide characters without column drift", () => {
    const indented = "        alpha/one.ts:1:1 and beta/two.ts:2:2";
    expect(resolveTokenAtCol(indented, 8)).toEqual({
      type: "file",
      path: "alpha/one.ts",
      line: 1,
      col: 1,
      raw: "alpha/one.ts:1:1",
    });

    // "日本語 src/App.tsx:1:2": "日本語 " occupies cols 0..6 (3 wide chars = 6 cols + space = 7)
    const cjkLine = "日本語 src/App.tsx:1:2";
    expect(resolveTokenAtCol(cjkLine, 7)).toEqual({
      type: "file",
      path: "src/App.tsx",
      line: 1,
      col: 2,
      raw: "src/App.tsx:1:2",
    });
  });

  it("returns null for empty or plain text without file or URL tokens", () => {
    expect(resolveTokenAtCol("", 0)).toBeNull();
    expect(resolveTokenAtCol("hello world just words", 5)).toBeNull();
  });
});

describe("openTerminalToken", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset().mockResolvedValue(true);
    vi.mocked(isTauri).mockReturnValue(true);
  });

  it("routes URL token through requestTerminalLinkOpen", async () => {
    const openBuiltIn = vi.fn(() => Promise.resolve());
    const unregister = registerBuiltInBrowserLinkOpener(openBuiltIn);
    try {
      saveBrowserSettings({ showTerminalLinkActions: false });
      const res = await openTerminalToken({
        type: "url",
        target: "https://example.com/test",
      });
      expect(res).toBe(true);
      expect(openBuiltIn).toHaveBeenCalledWith("https://example.com/test");
    } finally {
      unregister();
    }
  });

  it("routes file token through cmd_open_file_path with cwd and line/col", async () => {
    const res = await openTerminalToken(
      {
        type: "file",
        path: "src/main.rs",
        line: 42,
        col: 5,
        raw: "src/main.rs:42:5",
      },
      { cwd: "/Users/user/project" },
    );
    expect(res).toBe(true);
    expect(invoke).toHaveBeenCalledWith("cmd_open_file_path", {
      path: "src/main.rs",
      cwd: "/Users/user/project",
      line: 42,
      col: 5,
    });
  });
});
