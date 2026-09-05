import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab } from "../lib/types";
import { BrowserPane } from "./BrowserPane";

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(async () => () => undefined),
}));

const browserMocks = vi.hoisted(() => ({
  BROWSER_SHORTCUT_EVENT: "ferryx:browser-shortcut",
  getBrowserState: vi.fn(async () => ({
    browserId: "browser-1",
    webviewLabel: "browser-browser-1",
    workspaceId: "workspace-1",
    worktreePath: null,
    profileId: "default" as const,
    generation: 1,
    url: "http://localhost:3000",
    title: null,
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomFactor: 1,
    loadError: null,
    visible: true,
  })),
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadRequested: vi.fn(async () => () => undefined),
  findBrowser: vi.fn(),
  clearBrowserFind: vi.fn(),
  downloadBrowserUrl: vi.fn(),
  openExternalUrl: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: eventMocks.listen }));
vi.mock("../lib/browserTauri", () => browserMocks);
vi.mock("./BrowserToolbar", () => ({ BrowserToolbar: () => <div data-testid="browser-toolbar" /> }));

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

afterEach(cleanup);

describe("BrowserPane native webview lifecycle", () => {
  beforeEach(() => {
    window.localStorage.clear();
    eventMocks.listen.mockClear();
    browserMocks.getBrowserState.mockClear();
    browserMocks.setBrowserBounds.mockClear();
    browserMocks.setBrowserVisible.mockClear();
  });

  it("shows the child webview while mounted and hides it during cleanup", async () => {
    const { unmount } = render(
      <BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />,
    );

    await waitFor(() => {
      expect(browserMocks.setBrowserBounds).toHaveBeenCalledWith("browser-1", {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
      });
      expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith("browser-1", true);
    });

    unmount();

    await waitFor(() => {
      expect(browserMocks.setBrowserVisible).toHaveBeenLastCalledWith("browser-1", false);
    });
  });

  it("keeps an explicitly hidden child webview hidden", async () => {
    render(
      <BrowserPane tab={tab} visible={false} onNavigate={() => undefined} onReload={() => undefined} />,
    );

    await waitFor(() => {
      expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith("browser-1", false);
    });
    expect(browserMocks.setBrowserBounds).not.toHaveBeenCalled();
  });

  it("defensively clamps webview top edge to at or below the toolbar bottom", async () => {
    // Override getBoundingClientRect for both toolbar and container
    const originalGetBoundingClientRect = HTMLElement.prototype.getBoundingClientRect;
    HTMLElement.prototype.getBoundingClientRect = function () {
      if (this.getAttribute("data-testid") === "browser-viewport") {
        return {
          x: 10,
          y: 20, // Erroneous y overlapping toolbar (which ends at 50)
          width: 800,
          height: 600,
          top: 20,
          bottom: 620,
          left: 10,
          right: 810,
          toJSON: () => {},
        };
      }
      // Toolbar element wrapper
      if (this.querySelector('[data-testid="browser-toolbar"]') || this.getAttribute("data-testid") === "browser-toolbar") {
        return {
          x: 10,
          y: 10,
          width: 800,
          height: 40,
          top: 10,
          bottom: 50, // Toolbar bottom is at y = 50
          left: 10,
          right: 810,
          toJSON: () => {},
        };
      }
      return originalGetBoundingClientRect.call(this);
    };

    try {
      render(
        <BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />,
      );

      await waitFor(() => {
        // Clamped y should be 50 (toolbar bottom), clamped height should be 600 - (50 - 20) = 570
        expect(browserMocks.setBrowserBounds).toHaveBeenCalledWith("browser-1", {
          x: 10,
          y: 50,
          width: 800,
          height: 570,
        });
      });
    } finally {
      HTMLElement.prototype.getBoundingClientRect = originalGetBoundingClientRect;
    }
  });
});
