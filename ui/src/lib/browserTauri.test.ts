import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  browserAutomationAct,
  browserAutomationSnapshot,
  browserTabSelectIndex,
  browserWorkspaceSelectIndex,
  closeBrowser,
  goBackBrowser,
  goForwardBrowser,
  openExternalUrl,
  isBrowserTabShortcutAction,
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
});
