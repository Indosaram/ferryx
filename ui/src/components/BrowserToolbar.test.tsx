import { act, render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { BrowserToolbar } from "./BrowserToolbar";
import type { BrowserTab } from "../lib/types";

const browserNative = vi.hoisted(() => ({
  openExternalUrl: vi.fn(),
  setBrowserZoom: vi.fn(),
  focusBrowser: vi.fn(),
  getBrowserState: vi.fn(),
  goBackBrowser: vi.fn(),
  goForwardBrowser: vi.fn(),
  navigateBrowser: vi.fn(),
  onBrowserShortcutRequested: vi.fn(),
}));

vi.mock(import("../lib/browserTauri"), async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    openExternalUrl: browserNative.openExternalUrl,
    setBrowserZoom: browserNative.setBrowserZoom,
    focusBrowser: browserNative.focusBrowser,
    getBrowserState: browserNative.getBrowserState,
    goBackBrowser: browserNative.goBackBrowser,
    goForwardBrowser: browserNative.goForwardBrowser,
    navigateBrowser: browserNative.navigateBrowser,
    onBrowserShortcutRequested: browserNative.onBrowserShortcutRequested,
  };
});

const platformMocks = vi.hoisted(() => ({
  isHardReloadSupportedPlatform: vi.fn(() => true),
}));

vi.mock(import("../lib/shortcuts"), async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    isHardReloadSupportedPlatform: platformMocks.isHardReloadSupportedPlatform,
  };
});

const hostHistory = vi.hoisted(() => ({ entries: [] as unknown[] }));

vi.mock("@tauri-apps/api/core", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tauri-apps/api/core")>();
  return {
    ...actual,
    invoke: vi.fn(async (command: string) => {
      if (command === "cmd_browser_history_list") return hostHistory.entries;
      throw { code: "UNSUPPORTED", message: `${command} is not mocked` };
    }),
  };
});

afterEach(cleanup);

beforeEach(() => {
  window.localStorage.clear();
  hostHistory.entries = [];
  browserNative.openExternalUrl.mockReset();
  browserNative.setBrowserZoom.mockReset();
  browserNative.setBrowserZoom.mockResolvedValue(1.1);
  browserNative.focusBrowser.mockReset();
  browserNative.focusBrowser.mockResolvedValue(undefined);
  browserNative.getBrowserState.mockReset();
  browserNative.getBrowserState.mockResolvedValue({
    browserId: "b-1",
    webviewLabel: "webview-1",
    url: "http://localhost:3000",
    loading: false,
    canGoBack: true,
    canGoForward: false,
    zoomFactor: 1.0,
    profileId: "default",
    generation: 1,
    visible: true,
  });
  browserNative.goBackBrowser.mockReset();
  browserNative.goBackBrowser.mockResolvedValue(undefined);
  browserNative.goForwardBrowser.mockReset();
  browserNative.goForwardBrowser.mockResolvedValue(undefined);
  browserNative.navigateBrowser.mockReset();
  browserNative.navigateBrowser.mockResolvedValue(undefined);
  browserNative.onBrowserShortcutRequested.mockReset();
  browserNative.onBrowserShortcutRequested.mockResolvedValue(() => undefined);
  platformMocks.isHardReloadSupportedPlatform.mockReset();
  platformMocks.isHardReloadSupportedPlatform.mockReturnValue(true);
});

describe("BrowserToolbar", () => {
  const mockTab: BrowserTab = {
    kind: "browser",
    id: "tab-1",
    label: "Localhost",
    browserId: "b-1",
    url: "http://localhost:3000",
    loading: false,
    canGoBack: true,
    canGoForward: false,
  };

  it.each([undefined, "unrelated", "b-1"])("accepts navigation only for its DOM target %s", async (browserId) => {
    const reload = vi.fn();
    await act(async () => { render(<BrowserToolbar tab={mockTab} onNavigate={vi.fn()} onReload={reload} />); });
    // Effects have installed the DOM listener before the action.
    await act(async () => {
      for (const action of ["reload", "back", "forward"]) {
        fireEvent(window, new CustomEvent("ferryx:browser-shortcut", { detail: { browserId, action } }));
      }
    });
    const expected = browserId === mockTab.browserId;
    expect(reload).toHaveBeenCalledTimes(expected ? 1 : 0);
    expect(browserNative.goBackBrowser.mock.calls).toEqual(expected ? [["b-1"]] : []);
    expect(browserNative.goForwardBrowser.mock.calls).toEqual(expected ? [["b-1"]] : []);
  });

  it("renders address input with tab url", () => {
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );

    const input = screen.getByLabelText("URL address bar");
    expect(input).toHaveValue("http://localhost:3000");
  });

  it("submits navigated url on enter", () => {
    const onNavigate = vi.fn();
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={onNavigate}
        onReload={vi.fn()}
      />
    );

    const input = screen.getByLabelText("URL address bar");
    fireEvent.change(input, { target: { value: "github.com" } });
    fireEvent.submit(input);

    expect(onNavigate).toHaveBeenCalledWith("https://github.com");
  });

  it("triggers reload on reload click", () => {
    const onReload = vi.fn();
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={onReload}
      />
    );

    const reloadBtn = screen.getByLabelText("Reload");
    fireEvent.click(reloadBtn);
    expect(onReload).toHaveBeenCalled();
  });

  it("asks for a cache-bypassing reload on Shift+Click and a plain reload otherwise", () => {
    const onReload = vi.fn();
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={onReload}
      />
    );

    const reloadBtn = screen.getByLabelText("Reload");
    expect(reloadBtn.getAttribute("title")).toContain("Shift+Click");

    fireEvent.click(reloadBtn);
    expect(onReload).toHaveBeenLastCalledWith();

    fireEvent.click(reloadBtn, { shiftKey: true });
    expect(onReload).toHaveBeenLastCalledWith({ ignoreCache: true });
  });

  it("keeps Shift+Click a plain reload where the platform cannot bypass the cache", () => {
    platformMocks.isHardReloadSupportedPlatform.mockReturnValue(false);
    const onReload = vi.fn();
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={onReload}
      />
    );

    const reloadBtn = screen.getByLabelText("Reload");
    expect(reloadBtn.getAttribute("title")).not.toContain("Shift+Click");

    fireEvent.click(reloadBtn, { shiftKey: true });
    expect(onReload).toHaveBeenLastCalledWith();
  });

  it("routes the hard-reload shortcut to a cache-bypassing reload", async () => {
    const reload = vi.fn();
    await act(async () => { render(<BrowserToolbar tab={mockTab} onNavigate={vi.fn()} onReload={reload} />); });
    await act(async () => {
      fireEvent(window, new CustomEvent("ferryx:browser-shortcut", { detail: { browserId: "b-1", action: "reload-hard" } }));
    });
    expect(reload).toHaveBeenCalledWith({ ignoreCache: true });
  });

  it("wires Back and Forward to the native browser session", async () => {
    render(
      <BrowserToolbar
        tab={{ ...mockTab, canGoForward: true }}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );

    fireEvent.click(screen.getByLabelText("Back"));
    fireEvent.click(screen.getByLabelText("Forward"));

    await waitFor(() => {
      expect(browserNative.goBackBrowser).toHaveBeenCalledWith("b-1");
      expect(browserNative.goForwardBrowser).toHaveBeenCalledWith("b-1");
    });
  });

  it("shows recent browsing history and navigates the native browser when an entry is selected", async () => {
    hostHistory.entries = [{
      url: "https://example.com/previous",
      title: "Previous page",
      visitCount: 1,
      lastVisitedAt: 1787490000000,
      frecency: 100,
    }];

    const onNavigate = vi.fn();
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={onNavigate}
        onReload={vi.fn()}
      />
    );

    fireEvent.click(screen.getByLabelText("History"));
    expect(await screen.findByText("Previous page")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Previous page"));
    await waitFor(() => {
      expect(onNavigate).toHaveBeenCalledWith("https://example.com/previous");
    });
  });

  it("handles zoom controls and calls setBrowserZoom", async () => {
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );

    const zoomInBtn = screen.getByLabelText("Zoom in");
    fireEvent.click(zoomInBtn);
    await waitFor(() => {
      expect(browserNative.setBrowserZoom).toHaveBeenCalledWith("b-1", 1.1);
    });

    const zoomOutBtn = screen.getByLabelText("Zoom out");
    fireEvent.click(zoomOutBtn);
    await waitFor(() => {
      expect(browserNative.setBrowserZoom).toHaveBeenCalledWith("b-1", 1.0);
    });
  });

  it("clamps zoom to the Settings-exposed range derived from BROWSER_ZOOM_LEVELS", async () => {
    render(
      <BrowserToolbar
        tab={{ ...mockTab, zoomFactor: 0.8 }}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );

    // 0.7 is below the Settings minimum (75%), so the toolbar must clamp to 0.75.
    fireEvent.click(screen.getByLabelText("Zoom out"));
    await waitFor(() => {
      expect(browserNative.setBrowserZoom).toHaveBeenCalledWith("b-1", 0.75);
    });

    // A restored 250% zoom sits above the Settings maximum (200%), so zoom in clamps to 2.0.
    cleanup();
    render(
      <BrowserToolbar
        tab={{ ...mockTab, zoomFactor: 2.5 }}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );
    fireEvent.click(screen.getByLabelText("Zoom in"));
    await waitFor(() => {
      expect(browserNative.setBrowserZoom).toHaveBeenLastCalledWith("b-1", 2.0);
    });
  });

  it("calls focusBrowser on focus button click", async () => {
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );

    const focusBtn = screen.getByLabelText("Focus webview");
    fireEvent.click(focusBtn);
    await waitFor(() => {
      expect(browserNative.focusBrowser).toHaveBeenCalledWith("b-1");
    });
  });

  it("fetches browser state via getBrowserState during state sync / reload", async () => {
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );

    const syncBtn = screen.getByLabelText("Sync browser state");
    fireEvent.click(syncBtn);
    await waitFor(() => {
      expect(browserNative.getBrowserState).toHaveBeenCalledWith("b-1");
    });
  });

  it("surfaces a rejected zoom with its structured code and restores the previous zoom", async () => {
    browserNative.setBrowserZoom.mockRejectedValueOnce({
      code: "BROWSER_ZOOM_FAILED",
      message: "failed to set browser webview zoom",
    });
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );

    fireEvent.click(screen.getByLabelText("Zoom in"));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Zoom failed: BROWSER_ZOOM_FAILED");
    // The optimistic 110% must not survive the rejected IPC.
    expect(screen.getByText("100%")).toBeInTheDocument();

    // The next successful action clears the alert.
    fireEvent.click(screen.getByLabelText("Back"));
    await waitFor(() => {
      expect(screen.queryByRole("alert")).toBeNull();
    });
  });

  it("surfaces a rejected back navigation with its structured code", async () => {
    browserNative.goBackBrowser.mockRejectedValueOnce({
      code: "BROWSER_NOT_FOUND",
      message: "browser session is gone",
    });
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );

    fireEvent.click(screen.getByLabelText("Back"));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Back failed: BROWSER_NOT_FOUND");
  });

  it("surfaces a failed shortcut listener registration instead of discarding it", async () => {
    browserNative.onBrowserShortcutRequested.mockRejectedValueOnce({
      code: "EVENT_LISTEN_FAILED",
      message: "event system unavailable",
    });
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Shortcut listener failed: EVENT_LISTEN_FAILED");
  });

  it("subscribes to native shortcuts once across re-renders with new inline callbacks", async () => {
    const { rerender } = render(<BrowserToolbar tab={mockTab} onNavigate={() => undefined} onReload={() => undefined} />);
    await waitFor(() => expect(browserNative.onBrowserShortcutRequested).toHaveBeenCalledTimes(1));

    const latestReload = vi.fn();
    rerender(<BrowserToolbar tab={{ ...mockTab, canGoForward: true }} onNavigate={() => undefined} onReload={() => undefined} />);
    rerender(<BrowserToolbar tab={{ ...mockTab, canGoForward: true }} onNavigate={() => undefined} onReload={latestReload} />);
    await act(async () => undefined);
    expect(browserNative.onBrowserShortcutRequested).toHaveBeenCalledTimes(1);

    // The single subscription still reaches the newest callback.
    const listener = browserNative.onBrowserShortcutRequested.mock.calls[0][0] as (payload: { browserId: string; action: string }) => void;
    act(() => listener({ browserId: "b-1", action: "reload" }));
    expect(latestReload).toHaveBeenCalledTimes(1);
  });

  it("shows the zoom the backend actually applied", async () => {
    browserNative.setBrowserZoom.mockResolvedValueOnce(1.25);
    render(<BrowserToolbar tab={mockTab} onNavigate={vi.fn()} onReload={vi.fn()} />);

    fireEvent.click(screen.getByLabelText("Zoom in"));

    expect(await screen.findByText("125%")).toBeInTheDocument();
    expect(browserNative.setBrowserZoom).toHaveBeenCalledWith("b-1", 1.1);
  });

  it("zooms in, out and back to 100% from the zoom shortcuts and claims each event", async () => {
    browserNative.setBrowserZoom.mockImplementation(async (_browserId: string, zoom: number) => zoom);
    await act(async () => { render(<BrowserToolbar tab={mockTab} onNavigate={vi.fn()} onReload={vi.fn()} />); });
    const readout = screen.getByTestId("browser-zoom-readout");
    expect(readout).toHaveTextContent("100%");

    const shortcut = (browserId: string, action: string) => {
      const event = new CustomEvent("ferryx:browser-shortcut", { detail: { browserId, action }, cancelable: true });
      act(() => { window.dispatchEvent(event); });
      return event;
    };

    expect(shortcut("b-1", "zoom-in").defaultPrevented).toBe(true);
    await waitFor(() => expect(readout).toHaveTextContent("110%"));
    expect(browserNative.setBrowserZoom).toHaveBeenLastCalledWith("b-1", 1.1);

    expect(shortcut("b-1", "zoom-out").defaultPrevented).toBe(true);
    await waitFor(() => expect(readout).toHaveTextContent("100%"));
    expect(browserNative.setBrowserZoom).toHaveBeenLastCalledWith("b-1", 1.0);

    expect(shortcut("b-1", "zoom-out").defaultPrevented).toBe(true);
    await waitFor(() => expect(readout).toHaveTextContent("90%"));

    expect(shortcut("b-1", "zoom-reset").defaultPrevented).toBe(true);
    await waitFor(() => expect(readout).toHaveTextContent("100%"));
    expect(browserNative.setBrowserZoom).toHaveBeenLastCalledWith("b-1", 1.0);
    expect(browserNative.setBrowserZoom).toHaveBeenCalledTimes(4);

    // Another tab's zoom shortcut is neither applied nor claimed here.
    expect(shortcut("b-2", "zoom-in").defaultPrevented).toBe(false);
    expect(browserNative.setBrowserZoom).toHaveBeenCalledTimes(4);
    expect(readout).toHaveTextContent("100%");
  });

  it("zooms from a native zoom shortcut for its own tab", async () => {
    browserNative.setBrowserZoom.mockImplementation(async (_browserId: string, zoom: number) => zoom);
    await act(async () => { render(<BrowserToolbar tab={mockTab} onNavigate={vi.fn()} onReload={vi.fn()} />); });
    const listener = browserNative.onBrowserShortcutRequested.mock.calls[0][0] as (payload: { browserId: string; action: string }) => void;

    act(() => listener({ browserId: "b-1", action: "zoom-in" }));

    await waitFor(() => expect(screen.getByTestId("browser-zoom-readout")).toHaveTextContent("110%"));
    expect(browserNative.setBrowserZoom).toHaveBeenCalledExactlyOnceWith("b-1", 1.1);
  });
});
