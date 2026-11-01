import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserSection } from "./BrowserSection";

const browserMocks = vi.hoisted(() => ({
  focusBrowser: vi.fn(),
  listBrowsers: vi.fn(),
  setBrowserZoom: vi.fn(async () => 1),
  getBrowserEngineStatus: vi.fn(async () => ({ state: "unsupported" as const })),
}));

// The agent access group has its own tests and talks to the backend on mount.
vi.mock("./BrowserAgentAccessGroup", () => ({ BrowserAgentAccessGroup: () => null }));

vi.mock("../../lib/browserTauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/browserTauri")>();
  return { ...actual, ...browserMocks };
});

const activeBrowser = {
  browserId: "browser-1",
  webviewLabel: "browser-browser-1",
  workspaceId: "workspace-1",
  worktreePath: null,
  profileId: "default",
  generation: 1,
  url: "https://example.com",
  title: "Example",
  loading: false,
  canGoBack: false,
  canGoForward: false,
  zoomFactor: 1,
  loadError: null,
  visible: true,
};

afterEach(cleanup);

beforeEach(() => {
  window.localStorage.clear();
  browserMocks.focusBrowser.mockReset();
  browserMocks.listBrowsers.mockReset();
  browserMocks.listBrowsers.mockResolvedValue([activeBrowser]);
});

describe("BrowserSection silent failure surfacing", () => {
  it("shows a rejected Focus action as an alert inside that browser's row", async () => {
    browserMocks.focusBrowser.mockRejectedValueOnce({
      code: "WEBVIEW_NOT_FOUND",
      message: "The child webview instance was destroyed",
    });

    render(<BrowserSection />);

    const focusButton = await screen.findByRole("button", { name: "Focus browser tab Example" });
    fireEvent.click(focusButton);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Focus failed: WEBVIEW_NOT_FOUND");

    const row = screen.getByText("Example").closest("div.flex");
    expect(row).not.toBeNull();
    expect(row?.contains(alert)).toBe(true);
  });

  it("re-lists open browsers before applying the reset default zoom", async () => {
    render(<BrowserSection />);
    await screen.findByRole("button", { name: "Focus browser tab Example" });

    // A tab opened after the settings page mounted must also get the reset zoom.
    browserMocks.listBrowsers.mockResolvedValue([
      activeBrowser,
      { ...activeBrowser, browserId: "browser-2", title: "Later" },
    ]);
    browserMocks.setBrowserZoom.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Reset to defaults" }));

    await waitFor(() => expect(browserMocks.setBrowserZoom).toHaveBeenCalledWith("browser-2", 1));
    expect(browserMocks.setBrowserZoom).toHaveBeenCalledWith("browser-1", 1);
  });
});
