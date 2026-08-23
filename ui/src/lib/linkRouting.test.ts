import { beforeEach, describe, expect, it, vi } from "vitest";

import { resetBrowserSettings, saveBrowserSettings } from "./browserSettings";
import {
  registerBuiltInBrowserLinkOpener,
  requestTerminalLinkOpen,
  routeHttpLink,
  TERMINAL_LINK_ACTION_EVENT,
} from "./linkRouting";
import { openExternalUrl } from "./browserTauri";

vi.mock("./browserTauri", () => ({
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
