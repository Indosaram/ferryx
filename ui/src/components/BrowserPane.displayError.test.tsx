import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab } from "../lib/types";
import { BrowserPane } from "./BrowserPane";

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(async () => () => undefined),
}));

const browserMocks = vi.hoisted(() => ({
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async (_browserId: string, _visible: boolean) => undefined),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadRequested: vi.fn(async () => () => undefined),
  findBrowser: vi.fn(async () => ({ matchCount: 0, found: false })),
  clearBrowserFind: vi.fn(async () => undefined),
  downloadBrowserUrl: vi.fn(async () => undefined),
  openExternalUrl: vi.fn(async () => undefined),
  BROWSER_SHORTCUT_EVENT: "ferryx:browser-shortcut",
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: eventMocks.listen }));
vi.mock("./BrowserPermissionPrompt", () => ({ BrowserPermissionPrompts: () => null, BrowserMediaCaptureIndicator: () => null }));
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

afterEach(cleanup);

beforeEach(() => {
  browserMocks.setBrowserBounds.mockReset();
  browserMocks.setBrowserVisible.mockReset();
  browserMocks.setBrowserBounds.mockImplementation(async () => undefined);
  browserMocks.setBrowserVisible.mockImplementation(async () => undefined);
  window.localStorage.clear();
});

describe("BrowserPane display error banner and retry", () => {
  it("renders no error banner when setBrowserBounds resolves successfully", async () => {
    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);

    await waitFor(() => expect(browserMocks.setBrowserBounds).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith(tab.browserId, true));

    expect(screen.queryByTestId("browser-pane-error")).toBeNull();
    expect(screen.queryByTestId("browser-retry-bounds")).toBeNull();
  });

  it("renders error banner with code and operation when setBrowserBounds rejects with a structured code", async () => {
    browserMocks.setBrowserBounds.mockRejectedValueOnce({
      code: "WEBVIEW_NOT_FOUND",
      message: "The child webview instance was destroyed",
    });

    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);

    await waitFor(() => expect(browserMocks.setBrowserBounds).toHaveBeenCalledTimes(1));

    const banner = await screen.findByTestId("browser-pane-error");
    expect(banner).toBeInTheDocument();
    expect(banner).toHaveTextContent("Couldn't show the page: WEBVIEW_NOT_FOUND (bounds)");

    const retryButton = screen.getByTestId("browser-retry-bounds");
    expect(retryButton).toBeInTheDocument();
    expect(retryButton).toHaveTextContent("Retry");

    expect(browserMocks.setBrowserVisible).not.toHaveBeenCalledWith(tab.browserId, true);
  });

  it("clicking Retry calls setBrowserBounds again exactly once and clears the banner upon resolution", async () => {
    browserMocks.setBrowserBounds.mockRejectedValueOnce({
      code: "WEBVIEW_NOT_FOUND",
      message: "Initial bounds update failed",
    });

    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);

    await waitFor(() => expect(screen.getByTestId("browser-pane-error")).toBeInTheDocument());
    expect(browserMocks.setBrowserBounds).toHaveBeenCalledTimes(1);

    const retryButton = screen.getByTestId("browser-retry-bounds");

    await act(async () => {
      fireEvent.click(retryButton);
    });

    expect(browserMocks.setBrowserBounds).toHaveBeenCalledTimes(2);

    await waitFor(() => expect(screen.queryByTestId("browser-pane-error")).toBeNull());
    await waitFor(() => expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith(tab.browserId, true));
  });

  it("clicking Retry triggers setBrowserBounds only once per click even when rejection persists", async () => {
    browserMocks.setBrowserBounds.mockRejectedValue({
      code: "WEBVIEW_NOT_FOUND",
      message: "Persistent bounds failure",
    });

    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);

    await waitFor(() => expect(screen.getByTestId("browser-pane-error")).toBeInTheDocument());
    expect(browserMocks.setBrowserBounds).toHaveBeenCalledTimes(1);

    await act(async () => {
      fireEvent.click(screen.getByTestId("browser-retry-bounds"));
    });
    await waitFor(() => expect(browserMocks.setBrowserBounds).toHaveBeenCalledTimes(2));

    expect(browserMocks.setBrowserBounds).toHaveBeenCalledTimes(2);

    await act(async () => {
      fireEvent.click(screen.getByTestId("browser-retry-bounds"));
    });
    await waitFor(() => expect(browserMocks.setBrowserBounds).toHaveBeenCalledTimes(3));

    expect(browserMocks.setBrowserBounds).toHaveBeenCalledTimes(3);

    const banner = screen.getByTestId("browser-pane-error");
    expect(banner).toHaveTextContent("Couldn't show the page: WEBVIEW_NOT_FOUND (bounds)");
  });

  it("renders error banner with visibility operation if setBrowserVisible rejects", async () => {
    browserMocks.setBrowserVisible.mockRejectedValueOnce({
      code: "SURFACE_LOST",
      message: "Underlying native surface detached",
    });

    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);

    await waitFor(() => expect(browserMocks.setBrowserBounds).toHaveBeenCalledTimes(1));

    const banner = await screen.findByTestId("browser-pane-error");
    expect(banner).toBeInTheDocument();
    expect(banner).toHaveTextContent("Couldn't show the page: SURFACE_LOST (visibility)");
  });
});
