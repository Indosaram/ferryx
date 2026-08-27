import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab } from "../lib/types";
import { BROWSER_SHORTCUT_EVENT } from "../lib/browserTauri";
import { BrowserPane } from "./BrowserPane";

const callbackState = vi.hoisted(() => ({
  shortcut: null as null | ((payload: { browserId: string; action: "focus-address" | "reload" | "back" | "forward" | "find" }) => void),
  download: null as null | ((payload: { browserId: string; targetUrl: string }) => void),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(async () => () => undefined),
}));

const dialogMocks = vi.hoisted(() => ({
  save: vi.fn(async () => "/tmp/example.zip"),
}));

const browserMocks = vi.hoisted(() => ({
  getBrowserState: vi.fn(),
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  onBrowserShortcutRequested: vi.fn(async (listener: (payload: { browserId: string; action: "focus-address" | "reload" | "back" | "forward" | "find" }) => void) => {
    callbackState.shortcut = listener;
    return () => { callbackState.shortcut = null; };
  }),
  onBrowserDownloadRequested: vi.fn(async (listener: (payload: { browserId: string; targetUrl: string }) => void) => {
    callbackState.download = listener;
    return () => { callbackState.download = null; };
  }),
  findBrowser: vi.fn(async () => ({ matchCount: 2, found: true })),
  clearBrowserFind: vi.fn(async () => undefined),
  downloadBrowserUrl: vi.fn(async () => undefined),
  openExternalUrl: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: eventMocks.listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: dialogMocks.save }));
vi.mock("../lib/browserTauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/browserTauri")>();
  return { ...actual, ...browserMocks };
});

const baseTab: BrowserTab = {
  kind: "browser",
  id: "tab-browser",
  label: "Browser",
  browserId: "browser-1",
  url: "https://example.com/page",
  loading: false,
  canGoBack: false,
  canGoForward: false,
  zoomFactor: 1,
  profileId: "default",
};

afterEach(cleanup);

beforeEach(() => {
  callbackState.shortcut = null;
  callbackState.download = null;
  dialogMocks.save.mockClear();
  browserMocks.findBrowser.mockClear();
  browserMocks.clearBrowserFind.mockClear();
  browserMocks.downloadBrowserUrl.mockClear();
  browserMocks.openExternalUrl.mockClear();
  browserMocks.setBrowserBounds.mockClear();
  browserMocks.setBrowserVisible.mockClear();
  window.localStorage.clear();
});

describe("BrowserPane parity affordances", () => {
  it("opens find-in-page from the browser shortcut and reports matches", async () => {
    render(<BrowserPane tab={baseTab} onNavigate={vi.fn()} onReload={vi.fn()} />);

    fireEvent(window, new CustomEvent(BROWSER_SHORTCUT_EVENT, { detail: { action: "find" } }));
    const input = await screen.findByLabelText("Find in page");
    fireEvent.change(input, { target: { value: "example" } });

    await waitFor(() => {
      expect(browserMocks.findBrowser).toHaveBeenCalledWith("browser-1", "example", false);
      expect(screen.getByText("2 matches")).toBeInTheDocument();
    });
  });

  it("shows the attempted URL on load failure and retries without replacing it", () => {
    const onReload = vi.fn();
    render(
      <BrowserPane
        tab={{ ...baseTab, url: "https://example.invalid/failed", loadError: "Connection failed" }}
        onNavigate={vi.fn()}
        onReload={onReload}
      />,
    );

    expect(screen.getByTestId("browser-load-error")).toHaveTextContent("https://example.invalid/failed");
    expect(screen.getByTestId("browser-load-error")).toHaveTextContent("Connection failed");
    fireEvent.click(screen.getByRole("button", { name: /Retry/i }));
    expect(onReload).toHaveBeenCalledOnce();
  });

  it("navigates an HTTP URL dropped as text/uri-list", () => {
    const onNavigate = vi.fn();
    render(<BrowserPane tab={baseTab} onNavigate={onNavigate} onReload={vi.fn()} />);
    const dataTransfer = {
      types: ["text/uri-list"],
      getData: (type: string) => type === "text/uri-list" ? "https://example.com/dropped\n" : "",
    } as unknown as DataTransfer;

    fireEvent.drop(screen.getByTestId("browser-viewport"), { dataTransfer });
    expect(onNavigate).toHaveBeenCalledWith("https://example.com/dropped");
  });

  it("offers system-browser and Save as actions for intercepted downloads", async () => {
    render(<BrowserPane tab={baseTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(callbackState.download).not.toBeNull());

    act(() => callbackState.download?.({ browserId: "browser-1", targetUrl: "https://example.com/files/example.zip" }));
    expect(await screen.findByTestId("browser-download-prompt")).toHaveTextContent("example.zip");

    fireEvent.click(screen.getByRole("button", { name: /Open in system browser/i }));
    expect(browserMocks.openExternalUrl).toHaveBeenCalledWith("https://example.com/files/example.zip");

    fireEvent.click(screen.getByRole("button", { name: "Save as…" }));
    await waitFor(() => {
      expect(dialogMocks.save).toHaveBeenCalled();
      expect(browserMocks.downloadBrowserUrl).toHaveBeenCalledWith(
        "https://example.com/files/example.zip",
        "/tmp/example.zip",
      );
    });
  });
});
