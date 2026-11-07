import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BROWSER_TOOLBAR_COMPACT_WIDTH, BrowserToolbar } from "./BrowserToolbar";
import type { BrowserTab } from "../lib/types";

const native = vi.hoisted(() => ({
  getBrowserEngineStatus: vi.fn(),
  getBrowserSnapshotCapability: vi.fn(),
  onBrowserShortcutRequested: vi.fn(),
}));

const extensions = vi.hoisted(() => ({
  openSettingsSection: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock("../lib/browserExtensions", () => ({ openSettingsSection: extensions.openSettingsSection }));
vi.mock(import("../lib/browserTauri"), async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    getBrowserEngineStatus: native.getBrowserEngineStatus,
    getBrowserSnapshotCapability: native.getBrowserSnapshotCapability,
    onBrowserShortcutRequested: native.onBrowserShortcutRequested,
  };
});

const tab: BrowserTab = {
  kind: "browser",
  id: "tab-1",
  label: "Localhost",
  browserId: "b-1",
  url: "http://localhost:3000",
  profileId: "default",
  canGoBack: false,
  canGoForward: false,
};

// The toolbar reads its own width from getBoundingClientRect and re-reads it from a ResizeObserver
// callback, so both are driven here without any timers.
let toolbarWidth = 1000;
let resizeCallbacks: Array<() => void> = [];

class CapturingResizeObserver {
  constructor(callback: () => void) {
    resizeCallbacks.push(callback);
  }
  observe() {}
  unobserve() {}
  disconnect() {}
}

function renderToolbar(width: number, props: Partial<BrowserTab> = {}) {
  toolbarWidth = width;
  return render(<BrowserToolbar tab={{ ...tab, ...props }} onNavigate={vi.fn()} onReload={vi.fn()} />);
}

const SECONDARY_LABELS = ["Zoom out", "Zoom in", "Focus webview", "Sync browser state", "DevTools", "Select element"];
const ALWAYS_VISIBLE_LABELS = ["Back", "Forward", "Reload", "URL address bar", "History", "Extensions", "Open in external browser"];

beforeEach(() => {
  resizeCallbacks = [];
  vi.stubGlobal("ResizeObserver", CapturingResizeObserver);
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(
    () => ({ width: toolbarWidth, height: 32, x: 0, y: 0, top: 0, left: 0, right: toolbarWidth, bottom: 32, toJSON: () => ({}) }),
  );
  window.localStorage.clear();
  native.getBrowserEngineStatus.mockReset();
  native.getBrowserEngineStatus.mockResolvedValue({ state: "notInstalled" });
  native.getBrowserSnapshotCapability.mockReset();
  native.getBrowserSnapshotCapability.mockResolvedValue({ supported: true, formats: ["png"], pick: true });
  native.onBrowserShortcutRequested.mockReset();
  native.onBrowserShortcutRequested.mockResolvedValue(() => undefined);
  extensions.openSettingsSection.mockReset();
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("BrowserToolbar responsive layout", () => {
  it("keeps every action inline at or above the compact width", async () => {
    await act(async () => { renderToolbar(BROWSER_TOOLBAR_COMPACT_WIDTH); });

    expect(screen.getByTestId("browser-toolbar-root")).toHaveAttribute("data-compact", "false");
    expect(screen.queryByTestId("browser-toolbar-more")).toBeNull();
    for (const label of [...ALWAYS_VISIBLE_LABELS, ...SECONDARY_LABELS]) {
      expect(screen.getByLabelText(label)).toBeInTheDocument();
    }
  });

  it("collapses zoom, focus, sync, devtools and picker into More below the compact width", async () => {
    await act(async () => { renderToolbar(BROWSER_TOOLBAR_COMPACT_WIDTH - 1); });

    expect(screen.getByTestId("browser-toolbar-root")).toHaveAttribute("data-compact", "true");
    for (const label of ALWAYS_VISIBLE_LABELS) {
      expect(screen.getByLabelText(label)).toBeInTheDocument();
    }
    for (const label of SECONDARY_LABELS) {
      expect(screen.queryByLabelText(label)).toBeNull();
    }
    const more = screen.getByRole("button", { name: "More" });
    expect(more).toHaveAttribute("aria-haspopup", "true");
    expect(more).toHaveAttribute("aria-expanded", "false");
  });

  it("toggles an inline row with the collapsed actions from More", async () => {
    await act(async () => { renderToolbar(500); });
    const more = screen.getByTestId("browser-toolbar-more");

    fireEvent.click(more);
    expect(more).toHaveAttribute("aria-expanded", "true");
    const row = screen.getByTestId("browser-toolbar-more-row");
    for (const label of SECONDARY_LABELS) {
      expect(within(row).getByRole("button", { name: label })).toHaveTextContent(label);
    }

    fireEvent.click(more);
    expect(more).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId("browser-toolbar-more-row")).toBeNull();
  });

  it("re-measures from the ResizeObserver callback", async () => {
    await act(async () => { renderToolbar(1000); });
    expect(screen.getByTestId("browser-toolbar-root")).toHaveAttribute("data-compact", "false");

    toolbarWidth = 400;
    act(() => resizeCallbacks.forEach((callback) => callback()));
    expect(screen.getByTestId("browser-toolbar-root")).toHaveAttribute("data-compact", "true");
    expect(screen.getByTestId("browser-toolbar-more")).toBeInTheDocument();

    toolbarWidth = 900;
    act(() => resizeCallbacks.forEach((callback) => callback()));
    expect(screen.getByTestId("browser-toolbar-root")).toHaveAttribute("data-compact", "false");
    expect(screen.queryByTestId("browser-toolbar-more")).toBeNull();
  });

  it("explains every disabled control through its title", async () => {
    native.getBrowserSnapshotCapability.mockResolvedValue({ supported: false, formats: [], pick: false });
    await act(async () => { renderToolbar(1000, { url: "about:blank" }); });

    await waitFor(() => expect(screen.getByLabelText("Select element")).toBeDisabled());
    for (const label of ["Back", "Forward", "Select element", "Open in external browser"]) {
      const button = screen.getByLabelText(label);
      expect(button).toBeDisabled();
      expect(button.getAttribute("title")).toBeTruthy();
    }
  });
});

describe("BrowserToolbar extensions row", () => {
  it("offers the engine install when the engine is missing", async () => {
    await act(async () => { renderToolbar(1000); });
    expect(native.getBrowserEngineStatus).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("browser-toolbar-extensions"));
    const row = screen.getByTestId("browser-extensions-row");
    const install = await within(row).findByRole("button", { name: "Install engine" });
    expect(native.getBrowserEngineStatus).toHaveBeenCalledTimes(1);

    fireEvent.click(install);
    expect(extensions.openSettingsSection).toHaveBeenCalledWith("browser", "browser-engine");

    fireEvent.click(screen.getByTestId("browser-toolbar-extensions"));
    expect(screen.queryByTestId("browser-extensions-row")).toBeNull();
  });

  it("opens the current URL in a new Chromium tab when the engine is installed", async () => {
    native.getBrowserEngineStatus.mockResolvedValue({ state: "installed", version: "1" });
    const opened = vi.fn();
    window.addEventListener("ferryx:open-browser-url", opened);
    try {
      await act(async () => { renderToolbar(1000); });
      fireEvent.click(screen.getByTestId("browser-toolbar-extensions"));
      fireEvent.click(await screen.findByRole("button", { name: "Open new Chromium tab" }));

      expect(opened).toHaveBeenCalledTimes(1);
      expect((opened.mock.calls[0][0] as CustomEvent).detail).toEqual({ url: tab.url, profileId: "default" });
      expect(extensions.openSettingsSection).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("ferryx:open-browser-url", opened);
    }
  });

  it("offers no action where the Chromium engine is unsupported", async () => {
    native.getBrowserEngineStatus.mockResolvedValue({ state: "unsupported" });
    await act(async () => { renderToolbar(1000); });
    fireEvent.click(screen.getByTestId("browser-toolbar-extensions"));

    const row = screen.getByTestId("browser-extensions-row");
    await waitFor(() => expect(native.getBrowserEngineStatus).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(within(row).queryByRole("status")).toBeNull());
    expect(within(row).queryByRole("button")).toBeNull();
  });
});
