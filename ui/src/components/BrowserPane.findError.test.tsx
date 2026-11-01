import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BROWSER_SHORTCUT_EVENT } from "../lib/browserTauri";
import type { BrowserTab } from "../lib/types";
import { BrowserPane } from "./BrowserPane";

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(async () => () => undefined),
}));

const browserMocks = vi.hoisted(() => ({
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadRequested: vi.fn(async () => () => undefined),
  findBrowser: vi.fn(),
  clearBrowserFind: vi.fn(async () => undefined),
  downloadBrowserUrl: vi.fn(async () => undefined),
  openExternalUrl: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: eventMocks.listen }));
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

afterEach(cleanup);

beforeEach(() => {
  browserMocks.findBrowser.mockReset();
  window.localStorage.clear();
});

describe("BrowserPane find failure surfacing", () => {
  it("shows the structured IPC code when find rejects, not a generic fallback", async () => {
    browserMocks.findBrowser.mockRejectedValueOnce({
      code: "BROWSER_FIND_FAILED",
      message: "find script evaluation failed",
    });

    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);

    act(() => {
      fireEvent(window, new CustomEvent(BROWSER_SHORTCUT_EVENT, { detail: { browserId: tab.browserId, action: "find" } }));
    });
    const input = screen.getByLabelText("Find in page");

    await act(async () => {
      fireEvent.change(input, { target: { value: "example" } });
    });

    expect(await screen.findByText("BROWSER_FIND_FAILED")).toBeInTheDocument();
    expect(screen.queryByText("Find failed")).toBeNull();
  });
});
