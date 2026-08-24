import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { closeBrowser, setBrowserBounds, setBrowserVisible } from "./browserTauri";

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

describe("browser native lifecycle queue", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("serializes Fast Refresh hide/show and close operations for the same child webview", async () => {
    const first = deferred();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "cmd_browser_set_visible" && vi.mocked(invoke).mock.calls.length === 1) {
        return first.promise;
      }
      return Promise.resolve();
    });

    const hidePromise = setBrowserVisible("browser-hmr", false);
    const showPromise = setBrowserVisible("browser-hmr", true);
    const closePromise = closeBrowser("browser-hmr");

    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
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
    vi.mocked(invoke).mockReset();
  });

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
    await setBrowserBounds("browser-1", bounds, 5, 1);

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
    await expect(setBrowserBounds("browser-missing", bounds, 3, 1)).rejects.toMatchObject({
      code: "WEBVIEW_NOT_FOUND",
    });

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