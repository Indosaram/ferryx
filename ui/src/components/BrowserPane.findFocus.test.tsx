import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab } from "../lib/types";
import { BROWSER_SHORTCUT_EVENT } from "../lib/browserTauri";
import { BrowserPane } from "./BrowserPane";

const native = vi.hoisted(() => ({
  order: [] as string[],
  release: null as null | (() => void),
}));

const browserMocks = vi.hoisted(() => ({
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadRequested: vi.fn(async () => () => undefined),
  clearBrowserFind: vi.fn(async () => undefined),
  focusAppWebview: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock("./BrowserPermissionPrompt", () => ({ BrowserPermissionPrompts: () => null, BrowserMediaCaptureIndicator: () => null }));
vi.mock("./BrowserVaultBar", () => ({ BrowserVaultBar: () => null }));
vi.mock("./BrowserToolbar", () => ({ BrowserToolbar: () => <div data-testid="browser-toolbar" /> }));
vi.mock("../lib/browserTauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/browserTauri")>();
  return { ...actual, ...browserMocks };
});

const tab: BrowserTab = {
  kind: "browser",
  id: "tab-browser",
  label: "Browser",
  browserId: "browser-1",
  url: "http://localhost:3000",
  loading: false,
  canGoBack: false,
  canGoForward: false,
};

beforeEach(() => {
  native.order = [];
  native.release = null;
  window.localStorage.clear();
  browserMocks.focusAppWebview.mockImplementation(() => {
    native.order.push("native");
    return new Promise<void>((resolve) => {
      native.release = resolve;
    });
  });
  const focus = HTMLElement.prototype.focus;
  vi.spyOn(HTMLElement.prototype, "focus").mockImplementation(function (this: HTMLElement, options?: FocusOptions) {
    if (this.getAttribute("aria-label") === "Find in page") native.order.push("input");
    focus.call(this, options);
  });
});
afterEach(cleanup);

describe("BrowserPane find keyboard focus", () => {
  it("moves OS keyboard focus to the app webview before focusing the find input", async () => {
    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);
    act(() => {
      fireEvent(window, new CustomEvent(BROWSER_SHORTCUT_EVENT, { detail: { browserId: tab.browserId, action: "find" } }));
    });
    const input = screen.getByLabelText("Find in page");

    expect(browserMocks.focusAppWebview).toHaveBeenCalledOnce();
    expect(native.order[0]).toBe("native");
    expect(native.order.indexOf("input")).toBeGreaterThan(0);

    // Once the native focus moved, the input is focused again so it holds the keys.
    const before = native.order.length;
    await act(async () => native.release?.());
    expect(native.order.slice(before)).toEqual(["input"]);
    expect(document.activeElement).toBe(input);
  });

  it("still focuses the find input when the app webview cannot take focus", async () => {
    browserMocks.focusAppWebview.mockImplementation(() => {
      native.order.push("native");
      return Promise.reject({ code: "WEBVIEW_NOT_FOUND", message: "main" });
    });
    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);
    await act(async () => {
      fireEvent(window, new CustomEvent(BROWSER_SHORTCUT_EVENT, { detail: { browserId: tab.browserId, action: "find" } }));
    });
    expect(native.order[0]).toBe("native");
    expect(document.activeElement).toBe(screen.getByLabelText("Find in page"));
    expect(screen.queryByTestId("browser-pane-alert")).toBeNull();
  });
});
