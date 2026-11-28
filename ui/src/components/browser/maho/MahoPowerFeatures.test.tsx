// Maho power features (Todo 13): Ctrl+Tab switcher, auto-archive, Tidy, Now Playing.
//
// Every case drives the shipped code over a fresh in-memory store. Time is never
// slept: the archive policy takes an injected clock and the tests move it. The
// switcher's occlusion contract is observed through the real
// `useNativeTerminalVisibility()` hook, so a regression that leaves the overlay
// mounted (or that never renders `role="dialog"`) fails here rather than in the app.

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => {
  const captured: {
    event: string | null;
    handler: ((event: { payload: unknown }) => void) | null;
  } = { event: null, handler: null };
  return {
    captured,
    invoke: vi.fn(),
    listen: vi.fn(async (_event: string, handler: (event: { payload: unknown }) => void): Promise<() => void> => {
      captured.event = _event;
      captured.handler = handler;
      return () => undefined;
    }),
  };
});

// `importOriginal` keeps the real module's other exports (`isTauri`, `convertFileSrc`,
// ...) in place: `nativeTerminalVisibility` reads `isTauri` when it reports overlay
// ownership, so a bare `{ invoke }` mock would fail that call on every mutation.
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke: tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/event")>()),
  listen: tauri.listen,
}));
// `mahoBrowserTabActions` pulls in the toast channel for its Undo affordances; the
// power features only read its pure helpers, so the channel is a no-op here.
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn(), info: vi.fn() }) }));

import {
  DEFAULT_AUTO_ARCHIVE_HOURS,
  isAutoArchivable,
  loadAutoArchiveHours,
  runAutoArchiveSweep,
  saveAutoArchiveHours,
} from "./mahoAutoArchive";
import { MahoNowPlayingCard } from "./MahoNowPlayingCard";
import { MahoTabSwitcher } from "./MahoTabSwitcher";
import { planTidyGroups, tidyDomainOf, tidyTodayTabs } from "./mahoTabTidy";
import { browserMediaStore, type BrowserMediaState } from "../../../lib/browserMedia";
import { useNativeTerminalVisibility } from "../../../lib/nativeTerminalVisibility";
import {
  DEFAULT_SPACE_ID,
  createBrowserStore,
  selectMruTabs,
  selectTodayTabs,
  type BrowserStore,
  type BrowserStoreState,
  type UpsertBrowserTabInput,
} from "../../../state/browserStore";
import { BROWSER_AUTO_ARCHIVE_HOURS_STORAGE_KEY } from "../../../lib/storageKeys";

const HOUR = 3_600_000;
const T0 = 1_700_000_000_000;

function makeStore(seed?: (state: BrowserStoreState) => BrowserStoreState): BrowserStore {
  const store = createBrowserStore({ storage: null, now: () => T0 });
  if (seed) store.setState(seed);
  return store;
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

function NativeSurfaceProbe() {
  return <span data-testid="native-surface-probe">{String(useNativeTerminalVisibility())}</span>;
}

function mediaState(overrides: Partial<BrowserMediaState> = {}): BrowserMediaState {
  return {
    browserId: "tab-a",
    title: "Song",
    artist: "Artist",
    album: "Album",
    artworkUrl: "https://cdn.example.com/a.jpg",
    playing: true,
    muted: false,
    hasMedia: true,
    ...overrides,
  };
}

const emitMedia = (payload: BrowserMediaState) => {
  act(() => {
    tauri.captured.handler?.({ payload });
  });
};

beforeEach(() => {
  tauri.invoke.mockReset();
  tauri.invoke.mockResolvedValue({ action: "toggle", applied: true });
  tauri.listen.mockClear();
  tauri.captured.event = null;
  tauri.captured.handler = null;
  browserMediaStore.reset();
  window.localStorage.clear();
});

// ---- Ctrl+Tab MRU switcher ------------------------------------------------

describe("MahoTabSwitcher", () => {
  function renderSwitcher(store: BrowserStore, overrides: { open?: boolean; selectedIndex?: number } = {}) {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    const view = render(
      <MahoTabSwitcher
        open={overrides.open ?? true}
        tabs={selectMruTabs(store.getState())}
        selectedIndex={overrides.selectedIndex ?? 0}
        onSelect={onSelect}
        onClose={onClose}
        store={store}
      />,
    );
    return { ...view, onSelect, onClose };
  }

  it("lists the tabs in MRU order, most recent first", () => {
    const store = makeStore();
    addTab(store, "tab-a", { title: "Alpha" });
    addTab(store, "tab-b", { title: "Beta" });
    addTab(store, "tab-c", { title: "Gamma" });
    // Using A, then B, then C leaves the MRU head at C.
    store.touch("tab-a");
    store.touch("tab-b");
    store.touch("tab-c");

    renderSwitcher(store);

    expect(selectMruTabs(store.getState()).map((tab) => tab.browserId)).toEqual(["tab-c", "tab-b", "tab-a"]);
    const cards = screen.getAllByRole("option");
    expect(cards.map((card) => card.getAttribute("data-browser-id"))).toEqual(["tab-c", "tab-b", "tab-a"]);
    expect(cards[0]).toHaveAttribute("aria-selected", "true");
    // Each card carries the host and the Space badge.
    expect(cards[0].textContent).toContain("tab-c.example.com");
    expect(screen.getAllByTestId("maho-tab-switcher-space-badge")[0].textContent).toContain("Personal");
  });

  it("cycles with Tab and commits the highlighted tab when Control is released", () => {
    const store = makeStore();
    addTab(store, "tab-a");
    addTab(store, "tab-b");
    addTab(store, "tab-c");
    store.touch("tab-a");
    store.touch("tab-b");
    store.touch("tab-c");

    const { onSelect, onClose } = renderSwitcher(store);

    fireEvent.keyDown(window, { key: "Tab" });
    expect(screen.getAllByRole("option")[1]).toHaveAttribute("aria-selected", "true");
    fireEvent.keyDown(window, { key: "Tab" });
    expect(screen.getAllByRole("option")[2]).toHaveAttribute("aria-selected", "true");
    // Wrapping forward lands back on the head.
    fireEvent.keyDown(window, { key: "Tab" });
    expect(screen.getAllByRole("option")[0]).toHaveAttribute("aria-selected", "true");

    fireEvent.keyUp(window, { key: "Control" });

    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith("tab-c");
    expect(onClose).toHaveBeenCalledTimes(1);
    // The committed tab is now the MRU head.
    expect(store.getState().mru[0]).toBe("tab-c");
  });

  it("steps backwards on Shift+Tab and commits on Enter", () => {
    const store = makeStore();
    addTab(store, "tab-a");
    addTab(store, "tab-b");
    addTab(store, "tab-c");
    store.touch("tab-a");
    store.touch("tab-b");
    store.touch("tab-c");

    const { onSelect, onClose } = renderSwitcher(store);

    fireEvent.keyDown(window, { key: "Tab", shiftKey: true });
    // Backwards from the head wraps to the last card.
    expect(screen.getAllByRole("option")[2]).toHaveAttribute("aria-selected", "true");

    fireEvent.keyDown(window, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith("tab-a");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("cancels on Esc without selecting", () => {
    const store = makeStore();
    addTab(store, "tab-a");
    addTab(store, "tab-b");

    const { onSelect, onClose } = renderSwitcher(store);
    fireEvent.keyDown(window, { key: "Escape" });

    expect(onSelect).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("hides native surfaces while open and restores them on close", async () => {
    const store = makeStore();
    addTab(store, "tab-a");
    addTab(store, "tab-b");

    const switcher = (open: boolean) => (
      <>
        <NativeSurfaceProbe />
        <MahoTabSwitcher
          open={open}
          tabs={selectMruTabs(store.getState())}
          selectedIndex={0}
          onSelect={vi.fn()}
          onClose={vi.fn()}
          store={store}
        />
      </>
    );

    const { rerender } = render(switcher(false));
    await act(async () => {});
    expect(screen.getByTestId("native-surface-probe")).toHaveTextContent("true");

    rerender(switcher(true));
    await act(async () => {});
    expect(document.querySelector('[role="dialog"][aria-label="Tab switcher"]')).not.toBeNull();
    expect(screen.getByTestId("native-surface-probe")).toHaveTextContent("false");

    rerender(switcher(false));
    await act(async () => {});
    expect(document.querySelector('[role="dialog"][aria-label="Tab switcher"]')).toBeNull();
    expect(screen.getByTestId("native-surface-probe")).toHaveTextContent("true");
  });

  it("releases native occlusion when it unmounts while open", async () => {
    const store = makeStore();
    addTab(store, "tab-a");

    render(<NativeSurfaceProbe />);
    const { unmount } = renderSwitcher(store);
    await act(async () => {});
    expect(screen.getByTestId("native-surface-probe")).toHaveTextContent("false");

    unmount();
    await act(async () => {});
    expect(document.querySelector('[role="dialog"][aria-label="Tab switcher"]')).toBeNull();
    expect(screen.getByTestId("native-surface-probe")).toHaveTextContent("true");
  });
});

// ---- Auto-archive policy --------------------------------------------------

describe("auto-archive hours setting", () => {
  it("defaults to 12 hours and rejects unusable values", () => {
    expect(DEFAULT_AUTO_ARCHIVE_HOURS).toBe(12);
    expect(loadAutoArchiveHours()).toBe(12);

    window.localStorage.setItem(BROWSER_AUTO_ARCHIVE_HOURS_STORAGE_KEY, "3.5");
    expect(loadAutoArchiveHours()).toBe(3.5);

    for (const invalid of ["", "   ", "abc", "0", "-4", "NaN"]) {
      window.localStorage.setItem(BROWSER_AUTO_ARCHIVE_HOURS_STORAGE_KEY, invalid);
      expect(loadAutoArchiveHours(), `value ${JSON.stringify(invalid)}`).toBe(DEFAULT_AUTO_ARCHIVE_HOURS);
    }
  });

  it("persists a valid window and falls back for an invalid one", () => {
    expect(saveAutoArchiveHours(6)).toBe(6);
    expect(loadAutoArchiveHours()).toBe(6);
    expect(saveAutoArchiveHours(Number.NaN)).toBe(DEFAULT_AUTO_ARCHIVE_HOURS);
    expect(loadAutoArchiveHours()).toBe(DEFAULT_AUTO_ARCHIVE_HOURS);
  });
});

describe("runAutoArchiveSweep", () => {
  it("archives Today tabs idle for more than 12 hours, using the injected clock", () => {
    const store = makeStore();
    addTab(store, "tab-stale", { lastActiveAt: T0 - 13 * HOUR });
    addTab(store, "tab-fresh", { lastActiveAt: T0 - 1 * HOUR });

    const result = runAutoArchiveSweep(store, { now: () => T0 });

    expect(result.archivedCount).toBe(1);
    expect(result.archivedIds).toEqual(["tab-stale"]);
    expect(store.getState().tabs["tab-stale"]).toBeUndefined();
    expect(store.getState().tabs["tab-fresh"]).toBeDefined();
    // The archived tab is restorable, not destroyed.
    expect(store.getState().archive[0].tab.browserId).toBe("tab-stale");
  });

  it("keeps a tab exactly at the boundary and honours maxIdleHours", () => {
    const store = makeStore();
    addTab(store, "tab-edge", { lastActiveAt: T0 - 12 * HOUR });

    expect(runAutoArchiveSweep(store, { now: () => T0 }).archivedCount).toBe(0);
    expect(runAutoArchiveSweep(store, { now: () => T0, maxIdleHours: 1 }).archivedIds).toEqual(["tab-edge"]);
  });

  it("never archives pinned, favorite, agent-controlled or active tabs", () => {
    const store = makeStore();
    const stale = T0 - 48 * HOUR;
    addTab(store, "tab-favorite", { section: "favorite", lastActiveAt: stale });
    addTab(store, "tab-pinned", { section: "pinned", lastActiveAt: stale });
    addTab(store, "tab-agent", { agentControlled: true, lastActiveAt: stale });
    addTab(store, "tab-active", { lastActiveAt: stale });
    addTab(store, "tab-doomed", { lastActiveAt: stale });
    // Active without touching it: `touch` would also refresh `lastActiveAt`, which
    // would make the tab ineligible for a different reason than the guard under test.
    store.setState((prev) => ({
      ...prev,
      activeBrowserIdBySpace: { ...prev.activeBrowserIdBySpace, [DEFAULT_SPACE_ID]: "tab-active" },
    }));

    const result = runAutoArchiveSweep(store, { now: () => T0 });

    expect(result.archivedIds).toEqual(["tab-doomed"]);
    expect(store.getState().tabs["tab-favorite"]).toBeDefined();
    expect(store.getState().tabs["tab-pinned"]).toBeDefined();
    expect(store.getState().tabs["tab-agent"]).toBeDefined();
    expect(store.getState().tabs["tab-active"]).toBeDefined();
  });

  it("never archives the Mini Pane tab or a private tab", () => {
    const store = makeStore();
    const stale = T0 - 48 * HOUR;
    addTab(store, "tab-mini", { lastActiveAt: stale });
    addTab(store, "tab-private", { lastActiveAt: stale, private: true });
    store.setMini("tab-mini");

    const state = store.getState();
    expect(isAutoArchivable(state.tabs["tab-mini"], state)).toBe(false);
    expect(runAutoArchiveSweep(store, { now: () => T0 }).archivedCount).toBe(0);
  });

  it("does nothing when the window is not a positive number", () => {
    const store = makeStore();
    addTab(store, "tab-stale", { lastActiveAt: T0 - 100 * HOUR });
    expect(runAutoArchiveSweep(store, { now: () => T0, maxIdleHours: 0 }).archivedCount).toBe(0);
  });
});

// ---- Tidy -----------------------------------------------------------------

describe("tidyTodayTabs", () => {
  it("maps a URL to its registrable host, stripping www and ignoring junk", () => {
    expect(tidyDomainOf("https://www.github.com/ferryx/ferryx")).toBe("github.com");
    expect(tidyDomainOf("https://docs.rs/serde/latest")).toBe("docs.rs");
    expect(tidyDomainOf("about:blank")).toBeNull();
    expect(tidyDomainOf("")).toBeNull();
    expect(tidyDomainOf("not a url")).toBeNull();
  });

  it("groups only hosts with two or more Today tabs", () => {
    const store = makeStore();
    addTab(store, "gh-1", { url: "https://github.com/a" });
    addTab(store, "gh-2", { url: "https://www.github.com/b" });
    addTab(store, "rs-1", { url: "https://docs.rs/x" });
    addTab(store, "rs-2", { url: "https://docs.rs/y" });
    addTab(store, "solo", { url: "https://example.com/only" });
    addTab(store, "blank", { url: "about:blank" });

    const groups = planTidyGroups(selectTodayTabs(store.getState()));
    expect(groups.map((group) => group.domain)).toEqual(["docs.rs", "github.com"]);
    expect(groups.every((group) => group.tabs.length >= 2)).toBe(true);
  });

  it("folds two domains into two folders, leaves the singleton in Today, and undoes cleanly", () => {
    const store = makeStore();
    addTab(store, "gh-1", { url: "https://github.com/ferryx/ferryx", title: "Ferryx" });
    addTab(store, "gh-2", { url: "https://www.github.com/ferryx/issues", title: "Issues" });
    addTab(store, "gh-3", { url: "https://github.com/ferryx/pulls", title: "Pulls" });
    addTab(store, "rs-1", { url: "https://docs.rs/serde", title: "serde" });
    addTab(store, "rs-2", { url: "https://docs.rs/tokio", title: "tokio" });
    addTab(store, "rs-3", { url: "https://docs.rs/reqwest", title: "reqwest" });
    addTab(store, "solo", { url: "https://example.com/only", title: "Only" });

    const before = store.getState();
    const beforeToday = selectTodayTabs(before).map((tab) => tab.browserId);

    const result = tidyTodayTabs(store, DEFAULT_SPACE_ID);

    expect(result.foldersCreated.map((folder) => folder.name).sort()).toEqual(["docs.rs", "github.com"]);
    expect(result.movedCount).toBe(6);

    const after = store.getState();
    expect(Object.values(after.folders).map((folder) => folder.name).sort()).toEqual(["docs.rs", "github.com"]);
    // The singleton never moved.
    expect(selectTodayTabs(after).map((tab) => tab.browserId)).toEqual(["solo"]);
    expect(after.tabs["solo"].folderId).toBeUndefined();

    const githubFolder = Object.values(after.folders).find((folder) => folder.name === "github.com");
    expect(githubFolder).toBeDefined();
    expect(after.tabs["gh-1"]).toMatchObject({ section: "pinned", folderId: githubFolder?.id });
    expect(after.tabs["gh-3"]).toMatchObject({ section: "pinned", folderId: githubFolder?.id });

    result.undo();

    const undone = store.getState();
    expect(Object.keys(undone.folders)).toHaveLength(0);
    expect(selectTodayTabs(undone).map((tab) => tab.browserId)).toEqual(beforeToday);
    expect(undone.tabs["gh-1"]).toMatchObject({ section: "today", order: before.tabs["gh-1"].order });
    expect(undone.tabs["gh-1"].folderId).toBeUndefined();
    expect(undone.tabs["solo"].section).toBe("today");
  });

  it("reuses an existing folder of the same name instead of duplicating it", () => {
    const store = makeStore();
    addTab(store, "gh-1", { url: "https://github.com/a" });
    addTab(store, "gh-2", { url: "https://github.com/b" });
    const existing = store.createFolder({ name: "github.com", spaceId: DEFAULT_SPACE_ID });
    expect(existing.ok).toBe(true);
    if (!existing.ok) return;

    const result = tidyTodayTabs(store, DEFAULT_SPACE_ID);

    expect(result.foldersCreated).toHaveLength(0);
    expect(Object.keys(store.getState().folders)).toHaveLength(1);
    expect(store.getState().tabs["gh-1"].folderId).toBe(existing.value.id);
    // Undo keeps the folder the user already had.
    result.undo();
    expect(Object.keys(store.getState().folders)).toEqual([existing.value.id]);
    expect(store.getState().tabs["gh-1"].section).toBe("today");
  });
});

// ---- Now Playing card -----------------------------------------------------

describe("MahoNowPlayingCard", () => {
  it("renders nothing when no open tab has media", async () => {
    const store = makeStore();
    addTab(store, "tab-a");
    const onSelectTab = vi.fn();

    const { container } = render(<MahoNowPlayingCard store={store} onSelectTab={onSelectTab} />);
    await act(async () => {});

    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByTestId("maho-now-playing")).toBeNull();
  });

  it("shows the title and artist of the tab that is playing", async () => {
    const store = makeStore();
    addTab(store, "tab-a", { title: "A tab" });
    render(<MahoNowPlayingCard store={store} onSelectTab={vi.fn()} />);
    await act(async () => {});

    emitMedia(mediaState({ browserId: "tab-a", title: "Song", artist: "Artist" }));

    expect(screen.getByTestId("maho-now-playing-title")).toHaveTextContent("Song");
    expect(screen.getByTestId("maho-now-playing-artist")).toHaveTextContent("Artist");
    expect(screen.queryByTestId("maho-now-playing-artwork")).toBeNull();
  });

  it("toggles pause and mute through controlBrowserMedia", async () => {
    const store = makeStore();
    addTab(store, "tab-a");
    render(<MahoNowPlayingCard store={store} onSelectTab={vi.fn()} />);
    await act(async () => {});
    emitMedia(mediaState({ browserId: "tab-a", playing: true, muted: false }));

    fireEvent.click(screen.getByTestId("maho-now-playing-toggle"));
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_media_control", {
      browserId: "tab-a",
      action: "pause",
    }));

    fireEvent.click(screen.getByTestId("maho-now-playing-mute"));
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_media_control", {
      browserId: "tab-a",
      action: "mute",
    }));

    // A paused, muted report flips both buttons to their opposite action.
    emitMedia(mediaState({ browserId: "tab-a", playing: false, muted: true }));
    tauri.invoke.mockClear();

    fireEvent.click(screen.getByTestId("maho-now-playing-toggle"));
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_media_control", {
      browserId: "tab-a",
      action: "play",
    }));

    fireEvent.click(screen.getByTestId("maho-now-playing-mute"));
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_media_control", {
      browserId: "tab-a",
      action: "unmute",
    }));
  });

  it("jumps to the playing tab when the card is clicked", async () => {
    const store = makeStore();
    addTab(store, "tab-a");
    const onSelectTab = vi.fn();
    render(<MahoNowPlayingCard store={store} onSelectTab={onSelectTab} />);
    await act(async () => {});
    emitMedia(mediaState({ browserId: "tab-a" }));

    fireEvent.click(screen.getByRole("button", { name: "Jump to source tab" }));

    expect(onSelectTab).toHaveBeenCalledWith("tab-a");
  });

  it("dismisses without stopping media or closing the tab", async () => {
    const store = makeStore();
    addTab(store, "tab-a");
    render(<MahoNowPlayingCard store={store} onSelectTab={vi.fn()} />);
    await act(async () => {});
    emitMedia(mediaState());
    fireEvent.click(screen.getByLabelText("Dismiss Now Playing"));
    expect(screen.queryByTestId("maho-now-playing")).toBeNull();
    expect(store.getState().tabs["tab-a"]).toBeDefined();
    expect(browserMediaStore.getState().has("tab-a")).toBe(true);
    expect(tauri.invoke).not.toHaveBeenCalled();
  });

  it("hides and forgets the media entry once its tab is closed", async () => {
    const store = makeStore();
    addTab(store, "tab-a");
    addTab(store, "tab-b");
    const { container } = render(<MahoNowPlayingCard store={store} onSelectTab={vi.fn()} />);
    await act(async () => {});
    emitMedia(mediaState({ browserId: "tab-a", title: "Song" }));
    expect(screen.getByTestId("maho-now-playing")).toBeInTheDocument();

    act(() => {
      store.closeTab("tab-a");
    });

    await waitFor(() => expect(screen.queryByTestId("maho-now-playing")).toBeNull());
    expect(container).toBeEmptyDOMElement();
    // The lingering track is dropped from the shared media store, not just hidden.
    await waitFor(() => expect(browserMediaStore.getState().has("tab-a")).toBe(false));
  });

  it("surfaces a structured control failure instead of swallowing it", async () => {
    const store = makeStore();
    addTab(store, "tab-a");
    render(<MahoNowPlayingCard store={store} onSelectTab={vi.fn()} />);
    await act(async () => {});
    emitMedia(mediaState({ browserId: "tab-a" }));
    tauri.invoke.mockRejectedValue({ code: "INVALID_REQUEST", message: "unknown media action" });

    fireEvent.click(screen.getByTestId("maho-now-playing-toggle"));

    await waitFor(() => expect(screen.getByTestId("maho-now-playing-error")).toHaveTextContent("INVALID_REQUEST"));
  });
});
