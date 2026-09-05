import { act, cleanup, render } from "@testing-library/react";
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

function lastVisibleCall(): boolean | undefined {
  const calls = browserMocks.setBrowserVisible.mock.calls;
  return calls[calls.length - 1]?.[1];
}

afterEach(cleanup);

beforeEach(() => {
  browserMocks.setBrowserBounds.mockClear();
  browserMocks.setBrowserVisible.mockClear();
  window.localStorage.clear();
  document.body.querySelectorAll("[role=dialog], [role=search]").forEach((el) => el.remove());
});

describe("BrowserPane global surface masking", () => {
  it("hides the child webview while a role=dialog surface is mounted and restores it on dismissal", async () => {
    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);

    expect(lastVisibleCall()).toBe(true);

    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    await act(async () => {
      document.body.appendChild(dialog);
    });

    expect(lastVisibleCall()).toBe(false);

    await act(async () => {
      dialog.remove();
    });

    expect(lastVisibleCall()).toBe(true);
  });

  it("keeps a role=search surface masking the child webview", async () => {
    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);
    expect(lastVisibleCall()).toBe(true);

    const search = document.createElement("div");
    search.setAttribute("role", "search");
    await act(async () => {
      document.body.appendChild(search);
    });

    expect(lastVisibleCall()).toBe(false);
  });

  it("stays hidden when owner visibility is false even after a dialog is dismissed", async () => {
    render(<BrowserPane tab={tab} visible={false} onNavigate={() => undefined} onReload={() => undefined} />);
    expect(lastVisibleCall()).toBe(false);

    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    await act(async () => {
      document.body.appendChild(dialog);
    });
    expect(lastVisibleCall()).toBe(false);

    await act(async () => {
      dialog.remove();
    });
    expect(lastVisibleCall()).toBe(false);
  });
});
