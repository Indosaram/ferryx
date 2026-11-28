// Browser mode canvas contracts, driven through the real store and the real panes.
//
// Only the two things a jsdom cannot own are replaced: the Tauri IPC surface (so the
// native surface calls are observable) and the Ferryx toolbar (so "the canvas never
// draws a duplicate toolbar" is an assertion instead of a guess). `BrowserPane`
// itself stays real - it is what claims the surfaces from
// `browserViewportCoordinator` - and the module mock only records the props it was
// handed before delegating.

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BROWSER_SPLIT_RATIO_STORAGE_KEY } from "../../../lib/storageKeys";
import { browserViewportCoordinator } from "../../../lib/browserViewportCoordinator";
import type { BrowserEngine, BrowserTab } from "../../../lib/types";
import {
  createBrowserStore,
  type BrowserStore,
  type UpsertBrowserTabInput,
} from "../../../state/browserStore";
import {
  DEFAULT_SPLIT_RATIO,
  MAX_SPLIT_RATIO,
  MIN_SPLIT_RATIO,
  BrowserModeCanvas,
  clampSplitRatio,
  readStoredSplitRatio,
  toBrowserPaneTab,
} from "./BrowserModeCanvas";
import { BrowserPane } from "../../BrowserPane";

type RecordedPaneProps = {
  browserId: string;
  engine?: BrowserEngine;
  visible?: boolean;
  chromeMode?: string;
  surfacePriority?: number;
};

const paneProps = vi.hoisted(() => ({ calls: [] as RecordedPaneProps[] }));

const browserMocks = vi.hoisted(() => ({
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async (_browserId: string, _visible: boolean) => undefined),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadUpdated: vi.fn(async () => () => undefined),
  findBrowser: vi.fn(async () => ({ matchCount: 0, found: false })),
  clearBrowserFind: vi.fn(async () => undefined),
  openExternalUrl: vi.fn(async () => undefined),
  focusAppWebview: vi.fn(async () => undefined),
  focusBrowser: vi.fn(async () => undefined),
  listBrowsers: vi.fn(async () => []),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    // The downloads shelf reads its registry on mount; anything else fails like a host without
    // the browser backend, which the panes report instead of throwing.
    if (command === "cmd_browser_download_list") return [];
    throw { code: "BROWSER_NOT_AVAILABLE" };
  }),
  isTauri: () => false,
}));
vi.mock("../../BrowserPermissionPrompt", () => ({
  BrowserPermissionPrompts: () => null,
  BrowserMediaCaptureIndicator: () => null,
}));
vi.mock("../../BrowserVaultBar", () => ({ BrowserVaultBar: () => null }));
// A Ferryx toolbar that did render would be findable: the canvas must never show one.
vi.mock("../../BrowserToolbar", () => ({ BrowserToolbar: () => <div data-testid="ferryx-toolbar" /> }));
vi.mock("../../../lib/browserTauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../lib/browserTauri")>();
  return { ...actual, ...browserMocks };
});
vi.mock("../../BrowserPane", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../BrowserPane")>();
  return {
    BrowserPane: (props: Parameters<typeof actual.BrowserPane>[0]) => {
      paneProps.calls.push({
        browserId: props.tab.browserId,
        engine: props.tab.engine,
        visible: props.visible,
        chromeMode: props.chromeMode,
        surfacePriority: props.surfacePriority,
      });
      return actual.BrowserPane(props);
    },
  };
});

function makeStore(): BrowserStore {
  return createBrowserStore({ storage: null, now: () => 1_700_000_000_000 });
}

/** A standalone pane, standing in for the worktree split that owns the same browser. */
function otherSurfaceTab(browserId: string): BrowserTab {
  return {
    kind: "browser",
    id: browserId,
    label: browserId,
    browserId,
    url: `https://${browserId}.example.com/`,
    loading: false,
    canGoBack: false,
    canGoForward: false,
  };
}

function addTab(store: BrowserStore, browserId: string, overrides: Partial<UpsertBrowserTabInput> = {}): void {
  const result = store.upsertTab({
    browserId,
    url: `https://${browserId}.example.com/`,
    title: browserId,
    ...overrides,
  });
  expect(result.ok, `seeded ${browserId}`).toBe(true);
}

function canvas(store: BrowserStore, props: Partial<Parameters<typeof BrowserModeCanvas>[0]> = {}) {
  return <BrowserModeCanvas store={store} active {...props} />;
}

function panes(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>('[data-testid="browser-canvas-pane"]'));
}

function paneBrowserIds(container: HTMLElement): string[] {
  return panes(container).map((pane) => pane.dataset.browserId ?? "");
}

/** Visibility flags the native surface IPC saw for one browser, in issue order. */
function visibilityFor(browserId: string): boolean[] {
  return browserMocks.setBrowserVisible.mock.calls
    .filter(([browser]) => browser === browserId)
    .map(([, visible]) => visible);
}

beforeEach(() => {
  paneProps.calls = [];
  browserViewportCoordinator.reset();
  browserMocks.setBrowserBounds.mockClear();
  browserMocks.setBrowserVisible.mockClear();
  window.localStorage.clear();
});

afterEach(cleanup);

describe("BrowserModeCanvas chrome", () => {
  it("draws the active tab through BrowserPane in Maho chrome with no Ferryx toolbar", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub" });
    store.touch("github");

    const { container } = render(canvas(store));

    expect(paneBrowserIds(container)).toEqual(["github"]);
    expect(paneProps.calls.length).toBeGreaterThan(0);
    expect(paneProps.calls[0]).toMatchObject({ browserId: "github", chromeMode: "maho-minimal", visible: true });
    expect(screen.queryByTestId("ferryx-toolbar")).toBeNull();
    const header = screen.getByRole("toolbar", { name: "Page" });
    expect(header).toHaveAttribute("data-active", "true");
    expect(header).toHaveTextContent("github.example.com");
  });

  it("paints the canvas with the Space palette so the rail and the page frame share one surface", () => {
    const store = makeStore();
    addTab(store, "github");
    store.touch("github");

    render(canvas(store, { scheme: "light" }));

    const section = screen.getByTestId("browser-mode-canvas");
    expect(section).toHaveAttribute("data-maho-space", store.getState().activeSpaceId);
    expect(section).toHaveAttribute("data-maho-scheme", "light");
    expect(section.style.getPropertyValue("--maho-space-surface")).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("routes the contents header to the pane's tab", () => {
    const store = makeStore();
    addTab(store, "github");
    store.touch("github");
    const onReload = vi.fn();
    const onOpenAddress = vi.fn();
    const onCopyUrl = vi.fn();

    render(canvas(store, { onReload, onOpenAddress, onCopyUrl }));

    fireEvent.click(screen.getByRole("button", { name: "Reload" }));
    fireEvent.click(screen.getByRole("button", { name: /^Address: github\.example\.com/ }));
    fireEvent.click(screen.getByRole("button", { name: "Copy URL" }));
    expect(onReload).toHaveBeenCalledWith("github");
    expect(onOpenAddress).toHaveBeenCalledWith("github");
    expect(onCopyUrl).toHaveBeenCalledWith("https://github.example.com/");
  });

  it("keeps a Chromium tab on its own engine chrome and passes the engine through", () => {
    const store = makeStore();
    addTab(store, "cef", { title: "CEF", engine: "chromium" });
    store.touch("cef");

    const { container } = render(canvas(store));

    const pane = panes(container)[0];
    expect(pane.dataset.engine).toBe("chromium");
    expect(paneProps.calls[0].engine).toBe("chromium");
    // The engine draws its own toolbar, so Ferryx must not stack a second one on it.
    expect(screen.queryByTestId("ferryx-toolbar")).toBeNull();
  });

  it("mounts no panes while the mode is off screen", () => {
    const store = makeStore();
    addTab(store, "github");
    store.touch("github");

    const { container } = render(canvas(store, { active: false }));

    // A hidden canvas must not mount a BrowserPane at all: a mount claims the browser's native
    // surface from the coordinator and would hide a live worktree split showing the same tab.
    expect(paneProps.calls).toEqual([]);
    expect(panes(container)).toEqual([]);
    expect(browserMocks.setBrowserVisible).not.toHaveBeenCalled();
    expect(browserViewportCoordinator.getOwner("github")).toBeNull();
  });
});

describe("BrowserModeCanvas surface ownership while hidden", () => {
  it("never takes the surface from a visible pane showing the same browser", async () => {
    const store = makeStore();
    addTab(store, "shared");
    store.touch("shared");

    // The worktree split (or the mini pane) is on screen with this browser.
    const split = render(<BrowserPane tab={otherSurfaceTab("shared")} onNavigate={vi.fn()} onReload={vi.fn()} />);
    await waitFor(() => expect(browserViewportCoordinator.getOwner("shared")).not.toBeNull());
    const splitOwner = browserViewportCoordinator.getOwner("shared");
    await waitFor(() => expect(visibilityFor("shared")).toContain(true));

    browserMocks.setBrowserVisible.mockClear();

    // Browser mode is not on screen: it must not claim (and therefore hide) that surface.
    render(canvas(store, { active: false }));
    await act(async () => undefined);

    expect(browserViewportCoordinator.getOwner("shared")).toEqual(splitOwner);
    expect(visibilityFor("shared")).toEqual([]);

    split.unmount();
  });

  it("hides once when the mode is turned off with no successor claiming the surface", async () => {
    const store = makeStore();
    addTab(store, "solo");
    store.touch("solo");

    const { rerender } = render(canvas(store));
    await waitFor(() => expect(visibilityFor("solo")).toContain(true));

    browserMocks.setBrowserVisible.mockClear();
    rerender(canvas(store, { active: false }));

    // Leaving the mode unmounts the pane, and its release is the one hide that should happen.
    await waitFor(() => expect(visibilityFor("solo")).toEqual([false]));
    expect(browserViewportCoordinator.getOwner("solo")).toBeNull();
  });
});

describe("BrowserModeCanvas tab ownership", () => {
  it("claims the new browserId on a tab switch and releases the previous one without hiding the new surface", async () => {
    const store = makeStore();
    addTab(store, "browser-a");
    addTab(store, "browser-b");
    store.touch("browser-b");
    store.touch("browser-a");

    const { container } = render(canvas(store));

    await waitFor(() => expect(browserViewportCoordinator.getOwner("browser-a")).not.toBeNull());
    expect(browserViewportCoordinator.getOwner("browser-b")).toBeNull();
    expect(visibilityFor("browser-a")).toContain(true);

    act(() => {
      store.touch("browser-b");
    });

    await waitFor(() => expect(browserViewportCoordinator.getOwner("browser-b")).not.toBeNull());
    expect(paneBrowserIds(container)).toEqual(["browser-b"]);

    // The old surface is released (hidden on unmount) and the new one is only ever revealed:
    // a hide between its claim and its reveal would be the visible flash this canvas removes.
    await waitFor(() => expect(browserViewportCoordinator.getOwner("browser-a")).toBeNull());
    expect(visibilityFor("browser-b")).toContain(true);
    expect(visibilityFor("browser-b")).not.toContain(false);
    expect(visibilityFor("browser-a").slice(-1)).toEqual([false]);
  });

  it("selects the next MRU tab of the Space when the active tab is closed", async () => {
    const store = makeStore();
    addTab(store, "browser-a");
    addTab(store, "browser-b");
    addTab(store, "browser-c");
    store.touch("browser-c");
    store.touch("browser-b");
    store.touch("browser-a");

    const { container } = render(canvas(store));
    expect(paneBrowserIds(container)).toEqual(["browser-a"]);

    act(() => {
      store.closeTab("browser-a");
    });

    await waitFor(() => expect(paneBrowserIds(container)).toEqual(["browser-b"]));
    expect(store.getState().activeBrowserIdBySpace["space-default"]).toBe("browser-b");
    expect(store.getState().tabs["browser-a"]).toBeUndefined();
  });

  it("renders the Maho empty state for a Space without tabs", () => {
    const store = makeStore();
    const onNewTab = vi.fn();

    render(canvas(store, { onNewTab }));

    expect(screen.getByTestId("browser-mode-canvas")).toHaveAttribute("data-empty", "true");
    expect(screen.getByRole("toolbar", { name: "Page" })).toHaveTextContent("Search or Enter URL...");
    expect(screen.queryByText("No tabs in this Space")).toBeNull();
    expect(paneProps.calls).toEqual([]);

    fireEvent.click(screen.getByRole("button", { name: "New Tab" }));
    expect(onNewTab).toHaveBeenCalledTimes(1);
  });
});

describe("BrowserModeCanvas in-browser split", () => {
  it("renders both panes with distinct native surfaces", async () => {
    const store = makeStore();
    addTab(store, "browser-a");
    addTab(store, "browser-b");
    store.touch("browser-b");
    store.touch("browser-a");

    const { container } = render(canvas(store));
    fireEvent.click(screen.getByRole("button", { name: "Split view" }));

    expect(paneBrowserIds(container)).toEqual(["browser-a", "browser-b"]);
    expect(screen.getByRole("separator", { name: "Resize split" })).toBeInTheDocument();

    await waitFor(() => {
      expect(browserViewportCoordinator.getOwner("browser-a")).not.toBeNull();
      expect(browserViewportCoordinator.getOwner("browser-b")).not.toBeNull();
    });
    // Two panes, two surfaces: neither may drive the other's webview.
    const primary = browserViewportCoordinator.getOwner("browser-a");
    const secondary = browserViewportCoordinator.getOwner("browser-b");
    expect(primary?.surfaceId).not.toBe(secondary?.surfaceId);
  });

  it("swaps the panes without hiding either native surface", async () => {
    const store = makeStore();
    addTab(store, "browser-a");
    addTab(store, "browser-b");
    store.touch("browser-b");
    store.touch("browser-a");

    const { container } = render(canvas(store));
    fireEvent.click(screen.getByRole("button", { name: "Split view" }));
    await waitFor(() => {
      expect(browserViewportCoordinator.getOwner("browser-a")).not.toBeNull();
      expect(browserViewportCoordinator.getOwner("browser-b")).not.toBeNull();
    });

    browserMocks.setBrowserVisible.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Swap split panes" }));
    await act(async () => undefined);

    // The swap reorders two mounted panes; a remount would release each surface and hide it.
    expect(paneBrowserIds(container)).toEqual(["browser-b", "browser-a"]);
    expect(browserMocks.setBrowserVisible.mock.calls.filter(([, visible]) => visible === false)).toEqual([]);
    expect(browserViewportCoordinator.getOwner("browser-a")).not.toBeNull();
    expect(browserViewportCoordinator.getOwner("browser-b")).not.toBeNull();
  });

  it("persists the split ratio across a remount", () => {
    const store = makeStore();
    addTab(store, "browser-a");
    addTab(store, "browser-b");
    store.touch("browser-b");
    store.touch("browser-a");

    const first = render(canvas(store));
    fireEvent.click(screen.getByRole("button", { name: "Split view" }));
    fireEvent.keyDown(screen.getByRole("separator", { name: "Resize split" }), { key: "End" });

    expect(screen.getByRole("separator", { name: "Resize split" })).toHaveAttribute("aria-valuenow", "75");
    expect(window.localStorage.getItem(BROWSER_SPLIT_RATIO_STORAGE_KEY)).toBe("0.75");

    first.unmount();

    render(canvas(store));
    fireEvent.click(screen.getByRole("button", { name: "Split view" }));
    expect(screen.getByRole("separator", { name: "Resize split" })).toHaveAttribute("aria-valuenow", "75");
  });

  it("clamps the ratio to the 0.25-0.75 band and ignores an unreadable stored value", () => {
    expect(clampSplitRatio(0.9)).toBe(MAX_SPLIT_RATIO);
    expect(clampSplitRatio(0.1)).toBe(MIN_SPLIT_RATIO);
    expect(clampSplitRatio(Number.NaN)).toBe(DEFAULT_SPLIT_RATIO);
    expect(readStoredSplitRatio(null)).toBe(DEFAULT_SPLIT_RATIO);

    window.localStorage.setItem(BROWSER_SPLIT_RATIO_STORAGE_KEY, "not-a-number");
    expect(readStoredSplitRatio()).toBe(DEFAULT_SPLIT_RATIO);

    window.localStorage.setItem(BROWSER_SPLIT_RATIO_STORAGE_KEY, "0.94");
    expect(readStoredSplitRatio()).toBe(MAX_SPLIT_RATIO);
  });

  it("returns to a single fullscreen pane when the secondary is closed", async () => {
    const store = makeStore();
    addTab(store, "browser-a");
    addTab(store, "browser-b");
    store.touch("browser-b");
    store.touch("browser-a");

    const { container } = render(canvas(store));
    fireEvent.click(screen.getByRole("button", { name: "Split view" }));
    await waitFor(() => expect(browserViewportCoordinator.getOwner("browser-b")).not.toBeNull());

    fireEvent.click(screen.getByRole("button", { name: "Close split pane" }));

    expect(paneBrowserIds(container)).toEqual(["browser-a"]);
    expect(screen.queryByRole("separator", { name: "Resize split" })).toBeNull();
    await waitFor(() => expect(browserViewportCoordinator.getOwner("browser-b")).toBeNull());
    expect(browserViewportCoordinator.getOwner("browser-a")).not.toBeNull();
    expect(store.getState().tabs["browser-b"]).toBeDefined();
  });
});

describe("toBrowserPaneTab", () => {
  it("maps a store tab onto the pane's tab shape", () => {
    const store = makeStore();
    addTab(store, "cef", { title: "Cef", url: "https://cef.example.com/", engine: "chromium" });
    const pane = toBrowserPaneTab(store.getState().tabs.cef);

    expect(pane).toMatchObject({ kind: "browser", id: "cef", browserId: "cef", url: "https://cef.example.com/" });
    expect(pane.engine).toBe("chromium");
    expect(pane.label).toBe("Cef");
  });
});
