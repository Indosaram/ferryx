import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { BrowserToolbar } from "./BrowserToolbar";
import type { BrowserTab } from "../lib/types";

const browserNative = vi.hoisted(() => ({
  openExternalUrl: vi.fn(),
  setBrowserZoom: vi.fn(),
  focusBrowser: vi.fn(),
  getBrowserState: vi.fn(),
}));

vi.mock(import("../lib/browserTauri"), async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    openExternalUrl: browserNative.openExternalUrl,
    setBrowserZoom: browserNative.setBrowserZoom,
    focusBrowser: browserNative.focusBrowser,
    getBrowserState: browserNative.getBrowserState,
  };
});

afterEach(cleanup);

beforeEach(() => {
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
});
