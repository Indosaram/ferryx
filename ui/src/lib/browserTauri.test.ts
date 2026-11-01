import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  browserAutomationAct,
  browserAutomationSnapshot,
  browserTabSelectIndex,
  browserWorkspaceSelectIndex,
  closeBrowser,
  downloadBrowserUrlToFolder,
  extractBrowserCookieImportReason,
  formatBrowserCookieImportError,
  getBrowserCookieImportActionableMessage,
  goBackBrowser,
  goForwardBrowser,
  openExternalUrl,
  importInstalledBrowserCookies,
  isBrowserTabShortcutAction,
  reloadBrowser,
  revealDownloadedFile,
  setBrowserBounds,
  setBrowserVisible,
} from "./browserTauri";
import type { BrowserAutomationRequest, BrowserAutomationSnapshot } from "./types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("browser tab shortcut helpers", () => {
  it("classifies guest-forwarded tab actions", () => {
    expect(isBrowserTabShortcutAction("tab-next")).toBe(true);
    expect(isBrowserTabShortcutAction("tab-previous")).toBe(true);
    expect(isBrowserTabShortcutAction("tab-select-1")).toBe(true);
    expect(isBrowserTabShortcutAction("tab-select-9")).toBe(true);
    expect(isBrowserTabShortcutAction("tab-select-0")).toBe(false);
    expect(isBrowserTabShortcutAction("tab-select-10")).toBe(false);
    expect(isBrowserTabShortcutAction("find")).toBe(false);
  });

  it("maps tab-select actions to zero-based indices", () => {
    expect(browserTabSelectIndex("tab-select-1")).toBe(0);
    expect(browserTabSelectIndex("tab-select-9")).toBe(8);
    expect(browserTabSelectIndex("tab-select-0")).toBeNull();
    expect(browserTabSelectIndex("tab-next")).toBeNull();
    expect(browserTabSelectIndex("find")).toBeNull();
  });

  it("maps workspace-select actions to zero-based indices", () => {
    expect(browserWorkspaceSelectIndex("workspace-select-1")).toBe(0);
    expect(browserWorkspaceSelectIndex("workspace-select-9")).toBe(8);
    expect(browserWorkspaceSelectIndex("workspace-select-0")).toBeNull();
    expect(browserWorkspaceSelectIndex("tab-next")).toBeNull();
  });
});

describe("browser native lifecycle queue", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("serializes Fast Refresh hide/show and close operations for the same child webview", async () => {
    const first = deferred();
    const invoked = deferred();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "cmd_browser_set_visible" && vi.mocked(invoke).mock.calls.length === 1) {
        invoked.resolve();
        return first.promise;
      }
      return Promise.resolve();
    });

    const hidePromise = setBrowserVisible("browser-hmr", false);
    const showPromise = setBrowserVisible("browser-hmr", true);
    const closePromise = closeBrowser("browser-hmr");

    await invoked.promise;
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenNthCalledWith(1, "cmd_browser_set_visible", {
      browserId: "browser-hmr",
      visible: false,
    });

    first.resolve();
    await Promise.all([hidePromise, showPromise, closePromise]);

    expect(invoke).toHaveBeenNthCalledWith(2, "cmd_browser_set_visible", {
      browserId: "browser-hmr",
      visible: true,
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "cmd_browser_close", {
      browserId: "browser-hmr",
    });
  });
});

describe("setBrowserBounds retry on WebviewNotFound", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(invoke).mockReset();
  });
  afterEach(() => vi.useRealTimers());

  it("retries when WebviewNotFound is returned and succeeds once the webview is ready", async () => {
    let callCount = 0;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "cmd_browser_set_bounds") {
        callCount++;
        if (callCount < 3) {
          throw { code: "WEBVIEW_NOT_FOUND", message: "Webview not found: browser-1" };
        }
        return undefined;
      }
      return undefined;
    });

    const bounds = { x: 0, y: 50, width: 800, height: 550 };
    const result = setBrowserBounds("browser-1", bounds, 5, 1);
    await vi.runAllTimersAsync();
    await result;

    expect(callCount).toBe(3);
    expect(invoke).toHaveBeenCalledTimes(3);
    expect(invoke).toHaveBeenLastCalledWith("cmd_browser_set_bounds", {
      browserId: "browser-1",
      bounds,
    });
  });

  it("throws after exhausting retries if webview never appears", async () => {
    vi.mocked(invoke).mockImplementation(async () => {
      throw { code: "WEBVIEW_NOT_FOUND", message: "Webview not found: browser-missing" };
    });

    const bounds = { x: 0, y: 50, width: 800, height: 550 };
    const assertion = expect(setBrowserBounds("browser-missing", bounds, 3, 1)).rejects.toMatchObject({
      code: "WEBVIEW_NOT_FOUND",
    });
    await vi.runAllTimersAsync();
    await assertion;

    expect(invoke).toHaveBeenCalledTimes(4); // initial + 3 retries
  });

  it("fails immediately without retrying on other errors", async () => {
    vi.mocked(invoke).mockImplementation(async () => {
      throw { code: "BROWSER_NOT_FOUND", message: "Browser not found: bad-id" };
    });

    const bounds = { x: 0, y: 50, width: 800, height: 550 };
    await expect(setBrowserBounds("bad-id", bounds, 5, 1)).rejects.toMatchObject({
      code: "BROWSER_NOT_FOUND",
    });

    expect(invoke).toHaveBeenCalledTimes(1);
  });
});

describe("browser OS and engine IPC", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("preserves URL query separators at the OS command boundary", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    const url = "https://example.test/?a=1&b=2";
    await openExternalUrl(url);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("cmd_browser_open_external", { url });
  });

  it("targets the selected engine for back and forward", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    await goBackBrowser("browser-focused");
    await goForwardBrowser("browser-focused");
    expect(vi.mocked(invoke).mock.calls).toEqual([
      ["cmd_browser_go_back", { browserId: "browser-focused" }],
      ["cmd_browser_go_forward", { browserId: "browser-focused" }],
    ]);
  });

  it("carries the cache-bypass request on the reload command", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    await reloadBrowser("browser-focused", { ignoreCache: true });
    expect(vi.mocked(invoke).mock.calls).toEqual([
      ["cmd_browser_reload", { browserId: "browser-focused", ignoreCache: true }],
    ]);
  });

  it("asks for a soft reload when the caller requests no cache bypass", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    await reloadBrowser("browser-focused");
    expect(vi.mocked(invoke).mock.calls).toEqual([
      ["cmd_browser_reload", { browserId: "browser-focused", ignoreCache: false }],
    ]);
  });

  it("propagates typed unsupported input instead of reporting keypress success", async () => {
    vi.mocked(invoke).mockRejectedValue({ code: "UNSUPPORTED", message: "native input unavailable" });
    await expect(browserAutomationAct({ browserId: "browser-focused", generation: 1,
      action: { type: "keypress", key: "Backspace" },
    })).rejects.toMatchObject({ code: "UNSUPPORTED" });
  });
});

describe("browser automation ipc", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("invokes cmd_browser_automation_snapshot with browserId and returns snapshot", async () => {
    const expectedSnapshot: BrowserAutomationSnapshot = {
      browserId: "browser-1",
      generation: 3,
      url: "https://example.com",
      title: "Example Domain",
      elements: [
        {
          reference: "e1",
          role: "link",
          name: "More information...",
          tagName: "a",
        },
      ],
    };

    vi.mocked(invoke).mockResolvedValueOnce(expectedSnapshot);

    const snapshot = await browserAutomationSnapshot("browser-1");

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("cmd_browser_automation_snapshot", {
      browserId: "browser-1",
    });
    expect(snapshot).toEqual(expectedSnapshot);
  });

  it("invokes cmd_browser_automation_act with click request", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    const request: BrowserAutomationRequest = {
      browserId: "browser-1",
      generation: 3,
      action: {
        type: "click",
        reference: "e1",
      },
    };

    await browserAutomationAct(request);

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("cmd_browser_automation_act", { request });
  });

  it("invokes cmd_browser_automation_act with fill request", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    const request: BrowserAutomationRequest = {
      browserId: "browser-1",
      generation: 3,
      action: {
        type: "fill",
        reference: "e2",
        value: "hello world",
      },
    };

    await browserAutomationAct(request);

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("cmd_browser_automation_act", { request });
  });

  it("invokes cmd_browser_automation_act with keypress request", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    const request: BrowserAutomationRequest = {
      browserId: "browser-1",
      generation: 3,
      action: {
        type: "keypress",
        key: "Enter",
      },
    };

    await browserAutomationAct(request);

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("cmd_browser_automation_act", { request });
  });

  it("invokes cmd_browser_import_installed_cookies with source and profile", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ importedCount: 42, skippedCount: 5 });

    const result = await importInstalledBrowserCookies("default", "chrome", "Profile 1");

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("cmd_browser_import_installed_cookies", {
      request: {
        profileId: "default",
        source: "chrome",
        sourceProfile: "Profile 1",
      },
    });
    expect(result).toEqual({ importedCount: 42, skippedCount: 5 });
  });

  it("invokes cmd_browser_download_to_folder with the tab and url", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ id: "d1", status: "completed" });

    await downloadBrowserUrlToFolder("browser-1", "https://e.test/a.zip");

    expect(invoke).toHaveBeenCalledWith("cmd_browser_download_to_folder", {
      browserId: "browser-1",
      url: "https://e.test/a.zip",
    });
  });

  it("reveals a saved download through cmd_path_reveal", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    await revealDownloadedFile("/home/me/Downloads/a.zip");

    expect(invoke).toHaveBeenCalledWith("cmd_path_reveal", { path: "/home/me/Downloads/a.zip" });
  });
});

describe("browser cookie import error formatting and reasoning", () => {
  it("formats realistic keychain-denied IPC payload with actionable guidance", () => {
    const error = {
      code: "BROWSER_COOKIE_IMPORT_FAILED",
      message: "keychain-denied: Keychain access for Chrome Safe Storage failed",
      details: { reason: "keychain-denied" },
    };

    expect(extractBrowserCookieImportReason(error)).toBe("keychain-denied");
    expect(formatBrowserCookieImportError(error)).toBe(
      "keychain-denied: Allow Keychain access in the macOS prompt to import cookies.",
    );
  });

  it("formats realistic no-tab IPC payload with actionable guidance", () => {
    const error = {
      code: "BROWSER_COOKIE_IMPORT_FAILED",
      message: "open a browser tab using the default profile before importing cookies",
      details: { reason: "no-tab" },
    };

    expect(extractBrowserCookieImportReason(error)).toBe("no-tab");
    expect(formatBrowserCookieImportError(error)).toBe(
      "no-tab: Open a browser tab using this profile before importing cookies.",
    );
  });

  it("formats no-profile IPC payload with actionable message", () => {
    const error = {
      code: "BROWSER_COOKIE_IMPORT_FAILED",
      message: "no-profile: directory missing",
      details: { reason: "no-profile" },
    };

    expect(extractBrowserCookieImportReason(error)).toBe("no-profile");
    expect(formatBrowserCookieImportError(error)).toBe(
      "no-profile: Browser profile directory or cookie database was not found.",
    );
  });

  it("formats no-cookies and unsupported-platform payloads with actionable messages", () => {
    const noCookiesError = {
      code: "BROWSER_COOKIE_IMPORT_FAILED",
      details: { reason: "no-cookies" },
    };
    expect(formatBrowserCookieImportError(noCookiesError)).toBe(
      "no-cookies: No importable cookies found in the selected profile.",
    );

    const unsupportedPlatformError = {
      code: "BROWSER_COOKIE_IMPORT_FAILED",
      details: { reason: "unsupported-platform" },
    };
    expect(formatBrowserCookieImportError(unsupportedPlatformError)).toBe(
      "unsupported-platform: Installed browser cookie import is not supported on this platform.",
    );

    const genericFailedError = {
      code: "BROWSER_COOKIE_IMPORT_FAILED",
      details: { reason: "cookie-import-failed" },
    };
    expect(formatBrowserCookieImportError(genericFailedError)).toBe(
      "cookie-import-failed: Failed to import cookies from browser profile.",
    );
  });

  it("formats custom reason without actionable mapping as reason alone", () => {
    const customError = {
      code: "BROWSER_COOKIE_IMPORT_FAILED",
      details: { reason: "custom-source-failure" },
    };

    expect(extractBrowserCookieImportReason(customError)).toBe("custom-source-failure");
    expect(formatBrowserCookieImportError(customError)).toBe("custom-source-failure");
  });

  it("preserves raw error code semantics when details reason is missing", () => {
    const failedCodeOnly = {
      code: "BROWSER_COOKIE_IMPORT_FAILED",
      message: "Cookie import failed",
    };

    expect(extractBrowserCookieImportReason(failedCodeOnly)).toBeNull();
    expect(formatBrowserCookieImportError(failedCodeOnly)).toBe("BROWSER_COOKIE_IMPORT_FAILED");
  });

  it("preserves Error message and string errors when code and reason are absent", () => {
    expect(formatBrowserCookieImportError(new Error("Disk read error"))).toBe("Disk read error");
    expect(formatBrowserCookieImportError("Plain string failure")).toBe("Plain string failure");
    expect(formatBrowserCookieImportError({ message: "Only message provided" })).toBe("Only message provided");
  });

  it("falls back to UNKNOWN_ERROR for null, undefined, or empty payload", () => {
    expect(formatBrowserCookieImportError(null)).toBe("UNKNOWN_ERROR");
    expect(formatBrowserCookieImportError(undefined)).toBe("UNKNOWN_ERROR");
    expect(formatBrowserCookieImportError({})).toBe("UNKNOWN_ERROR");
    expect(formatBrowserCookieImportError("   ")).toBe("UNKNOWN_ERROR");
  });

  it("getBrowserCookieImportActionableMessage returns null for unrecognized reasons", () => {
    expect(getBrowserCookieImportActionableMessage("unknown-reason")).toBeNull();
  });
});
