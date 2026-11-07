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

const browserSystem = vi.hoisted(() => ({
  stepBrowserZoom: vi.fn(),
  applyBrowserSiteZoom: vi.fn(),
}));

vi.mock(import("../lib/browserSystem"), async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    stepBrowserZoom: browserSystem.stepBrowserZoom,
    applyBrowserSiteZoom: browserSystem.applyBrowserSiteZoom,
  };
});

// The host's stepping (src-tauri/src/browser/system.rs step_zoom over ZOOM_STEPS), so the mock
// answers what cmd_browser_zoom_step answers.
const HOST_STEPS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 3.0, 5.0];
function hostStep(current: number, direction: "in" | "out" | "reset"): number {
  if (direction === "reset") return 1.0;
  if (direction === "in") return HOST_STEPS.find((step) => step > current + 1e-6) ?? 5.0;
  return [...HOST_STEPS].reverse().find((step) => step < current - 1e-6) ?? 0.25;
}

/** A host that keeps one zoom per tab and steps it like cmd_browser_zoom_step. */
function steppingHost(initial = 1.0) {
  const host = { zoom: initial };
  browserSystem.stepBrowserZoom.mockImplementation(async (_browserId: string, direction: "in" | "out" | "reset") => {
    host.zoom = hostStep(host.zoom, direction);
    return host.zoom;
  });
  return host;
}

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
  browserSystem.stepBrowserZoom.mockReset();
  steppingHost();
  browserSystem.applyBrowserSiteZoom.mockReset();
  browserSystem.applyBrowserSiteZoom.mockResolvedValue(null);
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

  it("steps the zoom through the host's per-site zoom from the toolbar buttons", async () => {
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );
    const readout = screen.getByTestId("browser-zoom-readout");

    fireEvent.click(screen.getByLabelText("Zoom in"));
    await waitFor(() => expect(readout).toHaveTextContent("110%"));
    fireEvent.click(screen.getByLabelText("Zoom in"));
    // The host's next step after 110% is 125%, not 120%.
    await waitFor(() => expect(readout).toHaveTextContent("125%"));
    fireEvent.click(screen.getByLabelText("Zoom out"));
    await waitFor(() => expect(readout).toHaveTextContent("110%"));

    expect(browserSystem.stepBrowserZoom.mock.calls).toEqual([["b-1", "in"], ["b-1", "in"], ["b-1", "out"]]);
    // The un-stepped, un-remembered zoom command is never used for a zoom step.
    expect(browserNative.setBrowserZoom).not.toHaveBeenCalled();
  });

  it("shows the host's step from a restored zoom that is not one of the steps", async () => {
    steppingHost(2.4);
    render(
      <BrowserToolbar
        tab={{ ...mockTab, zoomFactor: 2.4 }}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );
    const readout = screen.getByTestId("browser-zoom-readout");
    expect(readout).toHaveTextContent("240%");

    fireEvent.click(screen.getByLabelText("Zoom out"));
    await waitFor(() => expect(readout).toHaveTextContent("200%"));
    expect(browserSystem.stepBrowserZoom).toHaveBeenCalledExactlyOnceWith("b-1", "out");
  });

  it("steps twice when the second zoom press comes before the first answer", async () => {
    const answers: Array<(zoom: number) => void> = [];
    browserSystem.stepBrowserZoom.mockImplementation(() => new Promise<number>((resolve) => answers.push(resolve)));
    render(<BrowserToolbar tab={mockTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    const readout = screen.getByTestId("browser-zoom-readout");

    fireEvent.click(screen.getByLabelText("Zoom in"));
    expect(readout).toHaveTextContent("110%");
    fireEvent.click(screen.getByLabelText("Zoom in"));
    // The optimistic readout steps from the first guess, as the host will.
    expect(readout).toHaveTextContent("125%");
    expect(answers).toHaveLength(2);

    // The second answer lands first; the late first answer must not take the readout back.
    await act(async () => { answers[1](1.25); });
    await act(async () => { answers[0](1.1); });
    expect(readout).toHaveTextContent("125%");
  });

  it("shows the site's remembered zoom after a navigation", async () => {
    browserSystem.applyBrowserSiteZoom.mockResolvedValueOnce(1.5);
    const { rerender } = render(<BrowserToolbar tab={mockTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    const readout = screen.getByTestId("browser-zoom-readout");
    expect(browserSystem.applyBrowserSiteZoom).not.toHaveBeenCalled();

    rerender(<BrowserToolbar tab={{ ...mockTab, url: "https://zoomed.example.com/" }} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(readout).toHaveTextContent("150%"));
    expect(browserSystem.applyBrowserSiteZoom).toHaveBeenCalledExactlyOnceWith("b-1");

    // A site without a remembered zoom is shown at 100%, not at the previous site's zoom.
    rerender(<BrowserToolbar tab={{ ...mockTab, url: "https://plain.example.com/" }} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(readout).toHaveTextContent("100%"));
    expect(browserSystem.applyBrowserSiteZoom).toHaveBeenCalledTimes(2);
  });

  it("keeps a private tab's zoom across navigations", async () => {
    steppingHost();
    const privateTab: BrowserTab = { ...mockTab, profileId: "private" };
    const { rerender } = render(<BrowserToolbar tab={privateTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    const readout = screen.getByTestId("browser-zoom-readout");
    fireEvent.click(screen.getByLabelText("Zoom in"));
    await waitFor(() => expect(readout).toHaveTextContent("110%"));

    // The host remembers no zoom for private tabs and answers null; the page keeps 110%.
    rerender(<BrowserToolbar tab={{ ...privateTab, url: "https://other.example.com/" }} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(browserSystem.applyBrowserSiteZoom).toHaveBeenCalledTimes(1));
    await act(async () => undefined);
    expect(readout).toHaveTextContent("110%");
  });

  it("surfaces a rejected site zoom with its structured code", async () => {
    browserSystem.applyBrowserSiteZoom.mockRejectedValueOnce({ code: "BROWSER_NOT_FOUND", message: "gone" });
    const { rerender } = render(<BrowserToolbar tab={mockTab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    rerender(<BrowserToolbar tab={{ ...mockTab, url: "https://next.example.com/" }} onNavigate={vi.fn()} onReload={vi.fn()} />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Site zoom failed: BROWSER_NOT_FOUND");
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
    browserSystem.stepBrowserZoom.mockRejectedValueOnce({
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

  it("shows the zoom the host actually applied", async () => {
    browserSystem.stepBrowserZoom.mockResolvedValueOnce(1.5);
    render(<BrowserToolbar tab={mockTab} onNavigate={vi.fn()} onReload={vi.fn()} />);

    fireEvent.click(screen.getByLabelText("Zoom in"));

    expect(await screen.findByText("150%")).toBeInTheDocument();
    expect(browserSystem.stepBrowserZoom).toHaveBeenCalledWith("b-1", "in");
  });

  it("zooms in, out and back to 100% from the zoom shortcuts and claims each event", async () => {
    await act(async () => { render(<BrowserToolbar tab={mockTab} onNavigate={vi.fn()} onReload={vi.fn()} />); });
    const readout = screen.getByTestId("browser-zoom-readout");
    expect(readout).toHaveTextContent("100%");

    const shortcut = (browserId: string, action: string) => {
      const event = new CustomEvent("ferryx:browser-shortcut", { detail: { browserId, action }, cancelable: true });
      act(() => { window.dispatchEvent(event); });
      return event;
    };

    // Mod+= twice lands on the host's 125% step, not 120%.
    expect(shortcut("b-1", "zoom-in").defaultPrevented).toBe(true);
    await waitFor(() => expect(readout).toHaveTextContent("110%"));
    expect(shortcut("b-1", "zoom-in").defaultPrevented).toBe(true);
    await waitFor(() => expect(readout).toHaveTextContent("125%"));

    expect(shortcut("b-1", "zoom-out").defaultPrevented).toBe(true);
    await waitFor(() => expect(readout).toHaveTextContent("110%"));

    expect(shortcut("b-1", "zoom-reset").defaultPrevented).toBe(true);
    await waitFor(() => expect(readout).toHaveTextContent("100%"));

    expect(shortcut("b-1", "zoom-out").defaultPrevented).toBe(true);
    await waitFor(() => expect(readout).toHaveTextContent("90%"));
    expect(browserSystem.stepBrowserZoom.mock.calls).toEqual([
      ["b-1", "in"],
      ["b-1", "in"],
      ["b-1", "out"],
      ["b-1", "reset"],
      ["b-1", "out"],
    ]);

    // Another tab's zoom shortcut is neither applied nor claimed here.
    expect(shortcut("b-2", "zoom-in").defaultPrevented).toBe(false);
    expect(browserSystem.stepBrowserZoom).toHaveBeenCalledTimes(5);
    expect(readout).toHaveTextContent("90%");
    expect(browserNative.setBrowserZoom).not.toHaveBeenCalled();
  });

  it("zooms from a native zoom shortcut for its own tab", async () => {
    await act(async () => { render(<BrowserToolbar tab={mockTab} onNavigate={vi.fn()} onReload={vi.fn()} />); });
    const listener = browserNative.onBrowserShortcutRequested.mock.calls[0][0] as (payload: { browserId: string; action: string }) => void;

    act(() => listener({ browserId: "b-1", action: "zoom-in" }));

    await waitFor(() => expect(screen.getByTestId("browser-zoom-readout")).toHaveTextContent("110%"));
    expect(browserSystem.stepBrowserZoom).toHaveBeenCalledExactlyOnceWith("b-1", "in");
  });
});
