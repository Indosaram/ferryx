import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab } from "../lib/types";
import { BrowserPane } from "./BrowserPane";

const eventState = vi.hoisted(() => ({
  stateChanged: null as null | ((event: { payload: unknown }) => void),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(async (eventName: string, handler: (event: { payload: unknown }) => void) => {
    if (eventName === "browser_state_changed") eventState.stateChanged = handler;
    return () => {
      if (eventState.stateChanged === handler) eventState.stateChanged = null;
    };
  }),
}));

const browserMocks = vi.hoisted(() => ({
  BROWSER_SHORTCUT_EVENT: "ferryx:browser-shortcut",
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadUpdated: vi.fn(async () => () => undefined),
  extractBrowserErrorCode: vi.fn((err: unknown) => String((err as { code?: unknown })?.code ?? "UNKNOWN_ERROR")),
  findBrowser: vi.fn(),
  clearBrowserFind: vi.fn(),
  downloadBrowserUrlWithSession: vi.fn(),
  openExternalUrl: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: eventMocks.listen }));
vi.mock("./BrowserPermissionPrompt", () => ({ BrowserPermissionPrompts: () => null, BrowserMediaCaptureIndicator: () => null }));
vi.mock("../lib/browserTauri", () => browserMocks);
vi.mock("./BrowserToolbar", () => ({
  BrowserToolbar: ({ tab }: { tab: BrowserTab }) => (
    <div data-testid="browser-toolbar">
      <span data-testid="toolbar-url">{tab.url}</span>
      <span data-testid="toolbar-can-go-back">{String(tab.canGoBack)}</span>
    </div>
  ),
}));

const tab: BrowserTab = {
  kind: "browser",
  id: "tab-browser",
  label: "Browser",
  browserId: "browser-1",
  url: "https://example.com/start",
  loading: false,
  canGoBack: false,
  canGoForward: false,
};

afterEach(cleanup);

beforeEach(() => {
  window.localStorage.clear();
  eventState.stateChanged = null;
});

describe("BrowserPane live tab state", () => {
  it("keeps the live url when the parent re-renders with an equal-valued new tab object", async () => {
    const { rerender } = render(
      <BrowserPane tab={{ ...tab }} onNavigate={() => undefined} onReload={() => undefined} />,
    );
    await waitFor(() => expect(eventState.stateChanged).toBeTypeOf("function"));

    act(() => {
      eventState.stateChanged?.({
        payload: {
          browserId: "browser-1",
          generation: 1,
          url: "https://example.com/live",
          title: "Live",
          loading: false,
          canGoBack: true,
          canGoForward: false,
          zoomFactor: 1,
          loadError: null,
        },
      });
    });
    expect(screen.getByTestId("toolbar-url")).toHaveTextContent("https://example.com/live");

    // The parent rebuilds the tab from store state that never saw the live event.
    rerender(<BrowserPane tab={{ ...tab }} onNavigate={() => undefined} onReload={() => undefined} />);

    expect(screen.getByTestId("toolbar-url")).toHaveTextContent("https://example.com/live");
    expect(screen.getByTestId("toolbar-can-go-back")).toHaveTextContent("true");
  });

  it("applies a tab prop field that actually changed", async () => {
    const { rerender } = render(
      <BrowserPane tab={{ ...tab }} onNavigate={() => undefined} onReload={() => undefined} />,
    );
    await waitFor(() => expect(eventState.stateChanged).toBeTypeOf("function"));

    rerender(
      <BrowserPane tab={{ ...tab, url: "https://example.com/next" }} onNavigate={() => undefined} onReload={() => undefined} />,
    );

    expect(screen.getByTestId("toolbar-url")).toHaveTextContent("https://example.com/next");
  });
});
