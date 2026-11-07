import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab } from "../lib/types";
import { BrowserPane } from "./BrowserPane";

const browserMocks = vi.hoisted(() => ({
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadRequested: vi.fn(async () => () => undefined),
  openExternalUrl: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));
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
  url: "https://example.com/page",
  loading: false,
  canGoBack: false,
  canGoForward: false,
};

afterEach(cleanup);

beforeEach(() => {
  browserMocks.openExternalUrl.mockClear();
  window.localStorage.clear();
});

describe("BrowserPane loading and load-error surfaces", () => {
  it("shows the loading bar only while a system-webview tab is loading", () => {
    const { rerender } = render(<BrowserPane tab={{ ...tab, loading: true }} onNavigate={vi.fn()} onReload={vi.fn()} />);
    expect(screen.getByTestId("browser-loading-bar")).toHaveAttribute("role", "progressbar");

    rerender(<BrowserPane tab={{ ...tab, loading: false }} onNavigate={vi.fn()} onReload={vi.fn()} />);
    expect(screen.queryByTestId("browser-loading-bar")).toBeNull();
  });

  it("leaves loading feedback to Chromium tabs", () => {
    render(<BrowserPane tab={{ ...tab, loading: true, engine: "chromium" }} onNavigate={vi.fn()} onReload={vi.fn()} />);
    expect(screen.queryByTestId("browser-loading-bar")).toBeNull();
  });

  it("opens the failed URL in the system browser from the load-error card", () => {
    render(<BrowserPane tab={{ ...tab, loadError: "Connection failed" }} onNavigate={vi.fn()} onReload={vi.fn()} />);

    fireEvent.click(screen.getByTestId("browser-load-error-open-external"));
    expect(browserMocks.openExternalUrl).toHaveBeenCalledWith(tab.url);
  });
});
