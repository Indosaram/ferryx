import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { closeBrowser, setBrowserVisible } from "./browserTauri";

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