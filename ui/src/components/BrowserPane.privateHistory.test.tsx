import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { loadBrowserHistory } from "../lib/browserHistory";
import { DEFAULT_BROWSER_PROFILE, PRIVATE_BROWSER_PROFILE } from "../lib/browserSettings";
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
  getBrowserState: vi.fn(async () => ({
    browserId: "browser-1",
    webviewLabel: "browser-browser-1",
    workspaceId: "workspace-1",
    worktreePath: null,
    profileId: "default",
    generation: 1,
    url: "https://example.com/start",
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
  url: "https://example.com/start",
  loading: false,
  canGoBack: false,
  canGoForward: false,
};

function emitSettledLoad(url: string, title: string | null = "Example Domain") {
  act(() => {
    eventState.stateChanged?.({
      payload: {
        browserId: "browser-1",
        generation: 1,
        url,
        title,
        loading: false,
        canGoBack: true,
        canGoForward: false,
        zoomFactor: 1,
        loadError: null,
      },
    });
  });
}

afterEach(cleanup);

beforeEach(() => {
  window.localStorage.clear();
  eventState.stateChanged = null;
  eventMocks.listen.mockClear();
  browserMocks.getBrowserState.mockClear();
  browserMocks.setBrowserBounds.mockClear();
  browserMocks.setBrowserVisible.mockClear();
});

describe("BrowserPane history privacy", () => {
  it("does not record history for a private-profile pane", async () => {
    render(
      <BrowserPane
        tab={{ ...tab, profileId: PRIVATE_BROWSER_PROFILE.id }}
        onNavigate={() => undefined}
        onReload={() => undefined}
      />,
    );

    await waitFor(() => expect(eventState.stateChanged).toBeTypeOf("function"));
    emitSettledLoad("https://example.com/private-page");

    expect(loadBrowserHistory()).toEqual([]);
  });

  it("still records history for a default-profile pane", async () => {
    render(
      <BrowserPane
        tab={{ ...tab, profileId: DEFAULT_BROWSER_PROFILE.id }}
        onNavigate={() => undefined}
        onReload={() => undefined}
      />,
    );

    await waitFor(() => expect(eventState.stateChanged).toBeTypeOf("function"));
    emitSettledLoad("https://example.com/public-page");

    expect(loadBrowserHistory().map((entry) => entry.url)).toEqual(["https://example.com/public-page"]);
  });
});
