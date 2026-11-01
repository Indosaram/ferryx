import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab } from "../lib/types";
import { BROWSER_SHORTCUT_EVENT } from "../lib/browserTauri";
import { saveBrowserSettings } from "../lib/browserSettings";
import { BrowserPane } from "./BrowserPane";

const callbackState = vi.hoisted(() => ({
  shortcut: null as null | ((payload: { browserId: string; action: "focus-address" | "reload" | "back" | "forward" | "find" }) => void),
  download: null as null | ((payload: { browserId: string; targetUrl: string; requiresConfirmation?: boolean }) => void),
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
  onBrowserDownloadRequested: vi.fn(async (listener: (payload: { browserId: string; targetUrl: string; requiresConfirmation?: boolean }) => void) => {
    callbackState.download = listener;
    return () => { callbackState.download = null; };
  }),
  findBrowser: vi.fn(async () => ({ matchCount: 2, found: true })),
  clearBrowserFind: vi.fn(async () => undefined),
  downloadBrowserUrl: vi.fn(async () => undefined),
  downloadBrowserUrlToFolder: vi.fn(async () => ({
    id: "dl-folder",
    url: "https://example.com/files/example.zip",
    filePath: "/home/me/Downloads/example.zip",
    status: "completed" as const,
    receivedBytes: 1024,
    createdAtMs: 1000,
    updatedAtMs: 1000,
  })),
  downloadBrowserUrlWithSession: vi.fn(async () => ({
    id: "dl-1",
    url: "https://example.com/files/example.zip",
    filePath: "/tmp/example.zip",
    status: "completed" as const,
    receivedBytes: 1024,
    createdAtMs: 1000,
    updatedAtMs: 1000,
  })),
  openExternalUrl: vi.fn(async () => undefined),
  revealDownloadedFile: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: eventMocks.listen }));
vi.mock("./BrowserPermissionPrompt", () => ({ BrowserPermissionPrompts: () => null, BrowserMediaCaptureIndicator: () => null }));
vi.mock("./BrowserVaultBar", () => ({ BrowserVaultBar: () => null }));
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
  browserMocks.downloadBrowserUrlToFolder.mockClear();
  browserMocks.downloadBrowserUrlWithSession.mockClear();
  browserMocks.revealDownloadedFile.mockClear();
  browserMocks.openExternalUrl.mockClear();
  browserMocks.setBrowserBounds.mockClear();
  browserMocks.setBrowserVisible.mockClear();
  window.localStorage.clear();
});

describe("BrowserPane parity affordances", () => {
  it("opens find-in-page from the browser shortcut and reports matches", async () => {
    render(<BrowserPane tab={baseTab} onNavigate={vi.fn()} onReload={vi.fn()} />);

    fireEvent(window, new CustomEvent(BROWSER_SHORTCUT_EVENT, { detail: { browserId: baseTab.browserId, action: "find" } }));
    const input = screen.getByLabelText("Find in page");
    await act(async () => {
      fireEvent.change(input, { target: { value: "example" } });
      expect(browserMocks.findBrowser).toHaveBeenCalledWith("browser-1", "example", false);
      await browserMocks.findBrowser.mock.results[0].value;
    });
    expect(screen.getByText("2 matches")).toBeInTheDocument();
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

  it("forwards the cache-bypass request from the hard-reload shortcut to onReload", () => {
    const onReload = vi.fn();
    render(<BrowserPane tab={baseTab} onNavigate={vi.fn()} onReload={onReload} />);

    fireEvent(window, new CustomEvent(BROWSER_SHORTCUT_EVENT, {
      detail: { browserId: baseTab.browserId, action: "reload-hard" },
    }));
    expect(onReload).toHaveBeenCalledWith({ ignoreCache: true });

    fireEvent(window, new CustomEvent(BROWSER_SHORTCUT_EVENT, {
      detail: { browserId: baseTab.browserId, action: "reload" },
    }));
    expect(onReload).toHaveBeenLastCalledWith();
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

  it("saves a clicked download to the download folder at once and offers Show in folder", async () => {
    render(<BrowserPane tab={baseTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(callbackState.download).not.toBeNull());

    act(() => callbackState.download?.({ browserId: "browser-1", targetUrl: "https://example.com/files/example.zip" }));

    await waitFor(() => expect(screen.getByTestId("browser-download-status")).toHaveAttribute("data-phase", "saved"));
    expect(browserMocks.downloadBrowserUrlToFolder).toHaveBeenCalledWith("browser-1", "https://example.com/files/example.zip");
    expect(dialogMocks.save).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /Open in system browser/i })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Show in folder/i }));
    expect(browserMocks.revealDownloadedFile).toHaveBeenCalledWith("/home/me/Downloads/example.zip");
  });

  it("drops an unanswered download offer when the tab navigates away", async () => {
    saveBrowserSettings({ askWhereToSave: true });
    const { rerender } = render(<BrowserPane tab={baseTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(callbackState.download).not.toBeNull());
    act(() => callbackState.download?.({ browserId: "browser-1", targetUrl: "https://example.com/files/a.zip" }));
    await screen.findByTestId("browser-download-prompt");

    rerender(<BrowserPane tab={{ ...baseTab, url: "https://example.com/elsewhere" }} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(screen.queryByTestId("browser-download-prompt")).toBeNull());
  });

  it("with ask-where-to-save, leads with Save and keeps the system browser in the overflow menu", async () => {
    saveBrowserSettings({ askWhereToSave: true });
    render(<BrowserPane tab={baseTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(callbackState.download).not.toBeNull());

    act(() => callbackState.download?.({ browserId: "browser-1", targetUrl: "https://example.com/files/example.zip" }));
    expect(await screen.findByTestId("browser-download-prompt")).toHaveTextContent("example.zip");
    expect(browserMocks.downloadBrowserUrlToFolder).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /Open in system browser/i })).toBeNull();

    const more = screen.getByRole("button", { name: "More download actions" });
    more.focus();
    fireEvent.keyDown(more, { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: /Open in system browser/i }));
    expect(browserMocks.openExternalUrl).toHaveBeenCalledWith("https://example.com/files/example.zip");

    fireEvent.click(screen.getByRole("button", { name: "Save as…" }));
    await waitFor(() => {
      expect(dialogMocks.save).toHaveBeenCalled();
      expect(browserMocks.downloadBrowserUrlWithSession).toHaveBeenCalledWith(
        "browser-1",
        "https://example.com/files/example.zip",
        "/tmp/example.zip",
      );
    });
  });

  it("fetches nothing for a needs-confirmation download until the user clicks Save", async () => {
    render(<BrowserPane tab={baseTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(callbackState.download).not.toBeNull());

    act(() => callbackState.download?.({
      browserId: "browser-1",
      targetUrl: "https://example.com/files/unattested.zip",
      requiresConfirmation: true,
    }));
    const prompt = await screen.findByTestId("browser-download-prompt");
    expect(prompt).toHaveAttribute("data-requires-confirmation", "true");
    expect(screen.getByTestId("browser-download-needs-confirmation")).toHaveTextContent("Needs confirmation");
    expect(dialogMocks.save).not.toHaveBeenCalled();
    expect(browserMocks.downloadBrowserUrlWithSession).not.toHaveBeenCalled();
    expect(browserMocks.downloadBrowserUrlToFolder).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(browserMocks.downloadBrowserUrlToFolder).toHaveBeenCalledWith(
        "browser-1",
        "https://example.com/files/unattested.zip",
      );
    });
    expect(dialogMocks.save).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByTestId("browser-download-needs-confirmation")).toBeNull());
  });

  it("treats a download without requiresConfirmation as user-initiated", async () => {
    saveBrowserSettings({ askWhereToSave: true });
    render(<BrowserPane tab={baseTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(callbackState.download).not.toBeNull());
    act(() => callbackState.download?.({ browserId: "browser-1", targetUrl: "https://example.com/files/a.zip" }));
    const prompt = await screen.findByTestId("browser-download-prompt");
    expect(prompt).not.toHaveAttribute("data-requires-confirmation");
    expect(screen.queryByTestId("browser-download-needs-confirmation")).toBeNull();
    expect(screen.getByRole("button", { name: "Save as…" })).toBeTruthy();
  });

  it("surfaces a rejected save dialog in the download prompt instead of an unhandled rejection", async () => {
    dialogMocks.save.mockRejectedValueOnce({ code: "DIALOG_FAILED", message: "dialog unavailable" });
    saveBrowserSettings({ askWhereToSave: true });
    render(<BrowserPane tab={baseTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(callbackState.download).not.toBeNull());
    act(() => callbackState.download?.({ browserId: "browser-1", targetUrl: "https://example.com/files/a.zip" }));

    fireEvent.click(await screen.findByRole("button", { name: "Save as…" }));

    await waitFor(() => expect(screen.getByTestId("browser-download-prompt")).toHaveTextContent("DIALOG_FAILED"));
    expect(browserMocks.downloadBrowserUrlWithSession).not.toHaveBeenCalled();
  });

  it("surfaces a rejected open-in-system-browser as a pane alert", async () => {
    browserMocks.openExternalUrl.mockRejectedValueOnce({ code: "OPEN_EXTERNAL_FAILED", message: "no handler" });
    saveBrowserSettings({ askWhereToSave: true });
    render(<BrowserPane tab={baseTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(callbackState.download).not.toBeNull());
    act(() => callbackState.download?.({ browserId: "browser-1", targetUrl: "https://example.com/files/a.zip" }));

    const more = await screen.findByRole("button", { name: "More download actions" });
    more.focus();
    fireEvent.keyDown(more, { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: /Open in system browser/i }));

    const alert = await screen.findByTestId("browser-pane-alert");
    expect(alert).toHaveTextContent("Open in system browser failed: OPEN_EXTERNAL_FAILED");
  });
});
