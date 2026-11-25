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
import type { BrowserEngine } from "../../../lib/types";
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
    expect(screen.getByRole("toolbar", { name: "Browser canvas" })).toBeInTheDocument();
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

  it("hides every canvas pane when the mode is not on screen", async () => {
    const store = makeStore();
    addTab(store, "github");
    addTab(store, "docs");
    store.touch("docs");
    store.touch("github");

    const { container } = render(canvas(store, { active: false }));
    fireEvent.click(screen.getByRole("button", { name: "Split view" }));

    expect(paneProps.calls.length).toBeGreaterThanOrEqual(2);
    expect(paneProps.calls.every((call) => call.visible === false)).toBe(true);
    for (const pane of panes(container)) expect(pane.dataset.visible).toBe("false");

    await waitFor(() => expect(browserMocks.setBrowserVisible).toHaveBeenCalled());
    expect(browserMocks.setBrowserVisible.mock.calls.some(([, visible]) => visible === true)).toBe(false);
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
    expect(screen.getByText("No tabs in this Space")).toBeInTheDocument();
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
