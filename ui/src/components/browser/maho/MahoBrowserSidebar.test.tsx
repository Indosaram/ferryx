// Maho Browser sidebar contracts, driven through the real store.
//
// Each case renders the shipped components over a fresh in-memory store. Nothing is
// mocked except `sonner` (the toast channel), so a case observes exactly the state
// transitions the sidebar makes. Sections that use dnd-kit are rendered inside a
// `DndContext`, because the sortable hooks require one; the drop decisions
// themselves are exercised through `applyMahoDrop`, which is the one function a
// pointer drag and a keyboard drag both funnel into.

import { DndContext } from "@dnd-kit/core";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import type { ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_SPACE_ID,
  createBrowserStore,
  type BrowserStore,
  type BrowserStoreState,
  type UpsertBrowserTabInput,
} from "../../../state/browserStore";
import { MahoActionRow } from "./MahoActionRow";
import { MahoBrowserSidebar } from "./MahoBrowserSidebar";
import { favoriteColumns, MahoFavoritesGrid } from "./MahoFavoritesGrid";
import { MahoPinnedSection } from "./MahoPinnedSection";
import { MahoSidebarFooter } from "./MahoSidebarFooter";
import { MahoSidebarTopBar } from "./MahoSidebarTopBar";
import { MahoTodayList } from "./MahoTodayList";
import { MahoUrlPill } from "./MahoUrlPill";
import {
  FAVORITE_LIMIT_MESSAGE,
  applyMahoDrop,
  displayHost,
  mahoDragId,
  parseMahoDragId,
} from "./mahoBrowserTabActions";
import { DEFAULT_SPACE_PRESET_ID, SPACE_PRESETS, deriveSpacePalette } from "./mahoSpacePalette";

const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  message: vi.fn(),
  info: vi.fn(),
}));

vi.mock("sonner", () => ({ toast: toastMocks, Toaster: () => null }));
const lifecycleMock = vi.hoisted(() => ({ close: vi.fn(), create: vi.fn() }));
vi.mock("../../../lib/browserTauri", () => ({ closeBrowser: lifecycleMock.close, createBrowser: lifecycleMock.create }));
const menuMock = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("../../../lib/nativeMenu", () => ({ openNativePopupMenu: menuMock.open }));

function tabMenu(label: string, command: string) {
  fireEvent.contextMenu(screen.getByRole("button", { name: label }));
  const call = menuMock.open.mock.calls[menuMock.open.mock.calls.length - 1]!;
  act(() => call[3](command));
}

/** Sections use sortable/droppable hooks, which need a DndContext ancestor. */
function withDnd(ui: ReactElement) {
  return render(<DndContext>{ui}</DndContext>);
}

function makeStore(seed?: (state: BrowserStoreState) => BrowserStoreState): BrowserStore {
  const store = createBrowserStore({ storage: null, now: () => 1_700_000_000_000 });
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

function sidebar(store: BrowserStore, props: Partial<Parameters<typeof MahoBrowserSidebar>[0]> = {}) {
  return render(<MahoBrowserSidebar store={store} scheme="dark" {...props} />);
}

function favoriteOrder(store: BrowserStore): string[] {
  return Object.values(store.getState().tabs)
    .filter((tab) => tab.section === "favorite")
    .sort((left, right) => left.order - right.order)
    .map((tab) => tab.browserId);
}

beforeEach(() => {
  lifecycleMock.close.mockReset().mockResolvedValue(undefined);
  lifecycleMock.create.mockReset().mockResolvedValue(undefined);
  menuMock.open.mockReset().mockResolvedValue(() => undefined);
  for (const mock of Object.values(toastMocks)) mock.mockClear();
});

describe("Space creation and switching", () => {
  it("creates a Space from the footer and activates it", () => {
    const store = makeStore();
    sidebar(store);

    fireEvent.click(screen.getByRole("button", { name: "Create Space" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Space name" }), { target: { value: "Work" } });
    fireEvent.click(within(screen.getByRole("group", { name: "Space color" })).getByRole("button", { name: "Rose" }));
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    const work = Object.values(store.getState().spaces).find((space) => space.name === "Work");
    expect(work).toBeDefined();
    expect(work?.color).toBe(SPACE_PRESETS.find((preset) => preset.id === "rose")?.hex);
    expect(store.getState().activeSpaceId).toBe(work?.id);
  });

  it("creates a Space with an identity and browser profile", () => {
    const store = makeStore();
    const { container } = sidebar(store);
    fireEvent.click(screen.getByRole("button", { name: "Create Space" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Space name" }), { target: { value: "Studio" } });
    fireEvent.click(screen.getByRole("button", { name: "Choose space icon" }));
    fireEvent.click(screen.getByRole("button", { name: "Palette" }));
    fireEvent.change(screen.getByRole("combobox", { name: "Choose profile" }), { target: { value: "default" } });
    fireEvent.click(screen.getByRole("button", { name: "Create", exact: true }));
    const space = store.getState().spaces[store.getState().activeSpaceId];
    expect(space).toMatchObject({ name: "Studio", icon: "Palette", profileId: "default" });
    expect(container.querySelector(".maho-space-header .maho-space-identity")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Switch to Space Studio" })).toHaveAttribute("data-has-icon", "true");
  });

  it("refuses an empty Space name", () => {
    const store = makeStore();
    sidebar(store);

    fireEvent.click(screen.getByRole("button", { name: "Create Space" }));
    const submit = screen.getByRole("button", { name: "Create" });
    expect(submit).toBeDisabled();
    fireEvent.click(submit);
    expect(Object.keys(store.getState().spaces)).toHaveLength(1);
  });

  it("switches Spaces from the Space Dots and repaints the palette attributes", () => {
    const store = makeStore();
    const created = store.createSpace({ name: "Work", color: SPACE_PRESETS[6].hex });
    expect(created.ok).toBe(true);
    if (!created.ok) return;

    const { container } = sidebar(store);
    const rail = () => container.querySelector("aside.maho-sidebar") as HTMLElement;

    expect(rail().getAttribute("data-maho-space")).toBe(DEFAULT_SPACE_ID);
    expect(rail().getAttribute("aria-label")).toContain("Personal");

    fireEvent.click(screen.getByRole("button", { name: "Switch to Space Work" }));

    expect(store.getState().activeSpaceId).toBe(created.value.id);
    expect(rail().getAttribute("data-maho-space")).toBe(created.value.id);
    expect(rail().getAttribute("aria-label")).toContain("Work");

    const palette = deriveSpacePalette(created.value.color, "dark");
    expect(rail().style.getPropertyValue("--maho-space-surface")).toBe(palette.surface);
    expect(rail().style.getPropertyValue("--maho-space-foreground")).toBe(palette.foreground);
    expect(rail().style.getPropertyValue("--maho-space-gradient")).toContain(palette.gradientFrom);
  });

  it("renames and recolors the active Space from the header popover", () => {
    const store = makeStore();
    sidebar(store);

    fireEvent.click(screen.getByRole("button", { name: "Edit Space" }));
    const nameInput = screen.getByRole("textbox", { name: "Space name" });
    fireEvent.change(nameInput, { target: { value: "Studio" } });
    fireEvent.submit(nameInput.closest("form") as HTMLFormElement);
    expect(store.getState().spaces[DEFAULT_SPACE_ID].name).toBe("Studio");

    fireEvent.click(screen.getByRole("button", { name: "Edit Space" }));
    fireEvent.click(within(screen.getByRole("group", { name: "Space color" })).getByRole("button", { name: "Sky" }));
    expect(store.getState().spaces[DEFAULT_SPACE_ID].color).toBe(
      SPACE_PRESETS.find((preset) => preset.id === DEFAULT_SPACE_PRESET_ID)?.hex,
    );
  });
});

describe("Favorites grid", () => {
  it("uses the native essentials column count for every supported occupancy", () => {
    expect(Array.from({ length: 13 }, (_, count) => favoriteColumns(count))).toEqual(
      [1, 1, 2, 3, 2, 4, 3, 4, 4, 3, 4, 4, 4],
    );
  });

  it("dismisses the empty hint without removing the Favorites drop target", () => {
    const store = makeStore();
    withDnd(<MahoFavoritesGrid store={store} />);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss favorites hint" }));
    expect(screen.queryByRole("button", { name: "Dismiss favorites hint" })).toBeNull();
    expect(screen.getByRole("region", { name: "Favorites" })).toHaveAttribute("data-hint-dismissed", "true");
  });

  it("lays out populated favorites without reserving empty slots and activates a tile", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub", section: "favorite", order: 0 });
    addTab(store, "docs", { title: "Docs", section: "favorite", order: 1 });
    addTab(store, "mail", { title: "Mail", section: "favorite", order: 2 });

    const onActivateTab = vi.fn();
    const { container } = sidebar(store, { onActivateTab });
    const grid = container.querySelector(".maho-favorites") as HTMLElement;
    expect(grid.getAttribute("data-columns")).toBe("3");
    expect(grid.getAttribute("data-capacity")).toBe("12");
    expect(grid.querySelectorAll(".maho-favorite-tile")).toHaveLength(3);
    expect(grid.querySelectorAll(".maho-favorite-tile__action")).toHaveLength(3);

    fireEvent.click(screen.getByRole("button", { name: "Open GitHub" }));
    expect(onActivateTab).toHaveBeenCalledWith("github");
  });

  it("moves a Today tab into Favorites from the row action", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub" });
    sidebar(store);

    tabMenu("Today: GitHub", "favorite");
    expect(store.getState().tabs.github.section).toBe("favorite");
    expect(screen.getByRole("button", { name: "Open GitHub" })).toBeInTheDocument();
  });

  it("rejects a thirteenth favorite with the favorite-limit toast", () => {
    const store = makeStore();
    for (let index = 0; index < 12; index += 1) {
      addTab(store, `fav-${index}`, { title: `Fav ${index}`, section: "favorite", order: index });
    }
    addTab(store, "thirteenth", { title: "Thirteenth" });

    withDnd(<MahoFavoritesGrid store={store} />);
    expect(store.getState().tabs.thirteenth.section).toBe("today");

    let dropped = true;
    act(() => {
      dropped = applyMahoDrop(store, { kind: "tab", tabId: "thirteenth" }, { kind: "section", section: "favorite" });
    });
    expect(dropped).toBe(false);
    expect(store.getState().tabs.thirteenth.section).toBe("today");
    expect(toastMocks.error).toHaveBeenCalledWith(FAVORITE_LIMIT_MESSAGE);

    // The Today row action takes the same path and refuses the same way.
    toastMocks.error.mockClear();
    withDnd(<MahoTodayList store={store} />);
    tabMenu("Today: Thirteenth", "favorite");
    expect(store.getState().tabs.thirteenth.section).toBe("today");
    expect(toastMocks.error).toHaveBeenCalledWith(FAVORITE_LIMIT_MESSAGE);
  });

  it("unpins a favorite back to Today", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub", section: "favorite" });
    withDnd(<MahoFavoritesGrid store={store} />);

    fireEvent.click(screen.getByRole("button", { name: "Unpin GitHub to Today" }));
    expect(store.getState().tabs.github.section).toBe("today");
  });

  it("reorders favorites with arrayMove semantics", () => {
    const store = makeStore();
    addTab(store, "first", { section: "favorite", order: 0 });
    addTab(store, "second", { section: "favorite", order: 1 });
    addTab(store, "third", { section: "favorite", order: 2 });

    expect(applyMahoDrop(store, { kind: "tab", tabId: "third" }, { kind: "tab", tabId: "first" })).toBe(true);
    expect(favoriteOrder(store)).toEqual(["third", "first", "second"]);
  });
});

describe("Pinned and folders", () => {
  it("has one trailing unload action and restores the unloaded tab on activation", async () => {
    const store = makeStore();
    addTab(store, "saved", { title: "Saved", section: "pinned" });
    const { container } = withDnd(<MahoPinnedSection store={store} />);
    expect(container.querySelectorAll(".maho-tab-row__actions button")).toHaveLength(1);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Unload Saved" })); });
    expect(lifecycleMock.close).toHaveBeenCalledWith("saved");
    expect(store.getState().tabs.saved.suspended).toBe(true);
    expect(screen.getByRole("button", { name: "Close Saved" })).toBeInTheDocument();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Pinned: Saved" })); });
    expect(lifecycleMock.create).toHaveBeenCalledWith(expect.objectContaining({ browserId: "saved", url: "https://saved.example.com/" }));
    expect(store.getState().tabs.saved.suspended).toBe(false);
    expect(screen.getByRole("button", { name: "Unload Saved" })).toBeInTheDocument();
  });

  it("pins a Today tab and unpins it back", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub" });
    sidebar(store);

    tabMenu("Today: GitHub", "pin");
    expect(store.getState().tabs.github.section).toBe("pinned");

    const pinned = within(screen.getByRole("region", { name: "Pinned" }));
    expect(pinned.getByRole("button", { name: "Pinned: GitHub" })).toBeInTheDocument();

    tabMenu("Pinned: GitHub", "pin");
    expect(store.getState().tabs.github.section).toBe("today");
  });

  it("creates, renames, collapses and deletes a folder", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub", section: "pinned" });
    withDnd(<MahoPinnedSection store={store} />);

    fireEvent.click(screen.getByRole("button", { name: "New folder" }));
    const folderInput = screen.getByRole("textbox", { name: "New folder name" });
    fireEvent.change(folderInput, { target: { value: "Research" } });
    fireEvent.submit(folderInput.closest("form") as HTMLFormElement);

    const folders = Object.values(store.getState().folders);
    expect(folders).toHaveLength(1);
    expect(folders[0].name).toBe("Research");

    fireEvent.click(screen.getByRole("button", { name: "Rename folder Research" }));
    const renameInput = screen.getByRole("textbox", { name: "Rename folder Research" });
    fireEvent.change(renameInput, { target: { value: "Reading" } });
    fireEvent.submit(renameInput.closest("form") as HTMLFormElement);
    expect(Object.values(store.getState().folders)[0].name).toBe("Reading");

    const collapse = screen.getByRole("button", { name: "Collapse folder Reading" });
    expect(collapse).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(collapse);
    expect(screen.getByRole("button", { name: "Expand folder Reading" })).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(screen.getByRole("button", { name: "Delete folder Reading" }));
    expect(Object.values(store.getState().folders)).toHaveLength(0);
  });

  it("moves a pinned tab into a folder and back out to Today", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub", section: "pinned" });
    const folder = store.createFolder({ name: "Research" });
    expect(folder.ok).toBe(true);
    if (!folder.ok) return;

    const target = { kind: "section", section: "pinned", folderId: folder.value.id } as const;
    expect(applyMahoDrop(store, { kind: "tab", tabId: "github" }, target)).toBe(true);
    expect(store.getState().tabs.github.folderId).toBe(folder.value.id);

    withDnd(<MahoPinnedSection store={store} />);
    expect(screen.getByRole("button", { name: "Pinned folder Research: GitHub" })).toBeInTheDocument();

    tabMenu("Pinned folder Research: GitHub", "pin");
    expect(store.getState().tabs.github.section).toBe("today");
    expect(store.getState().tabs.github.folderId).toBeUndefined();
  });

  it("round-trips dnd ids for tabs, sections and folders", () => {
    expect(mahoDragId({ kind: "tab", tabId: "abc" })).toBe("maho-tab:abc");
    expect(parseMahoDragId("maho-tab:abc")).toEqual({ kind: "tab", tabId: "abc" });
    expect(parseMahoDragId(mahoDragId({ kind: "section", section: "today" }))).toEqual({
      kind: "section",
      section: "today",
    });
    expect(parseMahoDragId(mahoDragId({ kind: "section", section: "pinned", folderId: "f1" }))).toEqual({
      kind: "section",
      section: "pinned",
      folderId: "f1",
    });
    expect(parseMahoDragId("nonsense")).toBeNull();
  });
});

describe("Action row", () => {
  it("renders the Maho action row with New Tab, Tidy and Clear", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub" });
    render(<MahoActionRow store={store} onNewTab={vi.fn()} onTidy={vi.fn()} />);

    const row = screen.getByRole("toolbar", { name: "Tab actions" });
    expect(row.className).toContain("maho-action-row");
    expect(screen.getByRole("button", { name: "New Tab" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Tidy Today" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Clear Today (1)" })).toBeEnabled();
  });

  it("fires the New Tab and Tidy callbacks", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub" });
    const onNewTab = vi.fn();
    const onTidy = vi.fn();
    render(<MahoActionRow store={store} onNewTab={onNewTab} onTidy={onTidy} />);

    fireEvent.click(screen.getByRole("button", { name: "New Tab" }));
    fireEvent.click(screen.getByRole("button", { name: "Tidy Today" }));
    expect(onNewTab).toHaveBeenCalledTimes(1);
    expect(onTidy).toHaveBeenCalledTimes(1);
  });

  it("disables Tidy and Clear while Today is empty", () => {
    const store = makeStore();
    render(<MahoActionRow store={store} onTidy={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Tidy Today" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Clear Today (0)" })).toBeDisabled();
  });

  it("clears Today into the archive and offers an undo that restores it", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub", order: 0 });
    addTab(store, "docs", { title: "Docs", order: 1 });
    addTab(store, "mail", { title: "Mail" });
    store.moveTab("mail", { section: "pinned" });
    render(<MahoActionRow store={store} />);

    fireEvent.click(screen.getByRole("button", { name: "Clear Today (2)" }));

    const cleared = store.getState();
    expect(Object.values(cleared.tabs).filter((tab) => tab.section === "today")).toHaveLength(0);
    expect(cleared.tabs.mail.section).toBe("pinned");
    expect(cleared.archive).toHaveLength(2);
    expect(cleared.archive.every((entry) => entry.reason === "cleared")).toBe(true);
    expect(toastMocks.success).toHaveBeenCalledWith("Cleared 2 tabs from Today", expect.anything());

    const options = toastMocks.success.mock.calls[0][1] as { action?: { onClick: () => void } };
    options.action?.onClick();
    expect(Object.values(store.getState().tabs).filter((tab) => tab.section === "today")).toHaveLength(2);
  });
});

describe("URL pill, top bar and accessibility", () => {
  it("shows the clean host, opens the Command Bar, and copies the URL", () => {
    const store = makeStore();
    addTab(store, "github", { url: "https://www.github.com/ferryx/ui", title: "GitHub" });
    const onOpenCommandBar = vi.fn();
    const onCopyUrl = vi.fn();
    render(
      <MahoUrlPill
        tab={store.getState().tabs.github}
        onOpenCommandBar={onOpenCommandBar}
        onCopyUrl={onCopyUrl}
      />,
    );

    expect(displayHost("https://www.github.com/ferryx/ui")).toBe("github.com");
    expect(displayHost("about:blank")).toBe("New Tab");

    fireEvent.click(screen.getByRole("button", { name: "Address: github.com" }));
    expect(onOpenCommandBar).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Copy URL" }));
    expect(onCopyUrl).toHaveBeenCalledWith("https://www.github.com/ferryx/ui");
  });

  it("keeps address and navigation controls in the contents header, not the sidebar", () => {
    const store = makeStore();
    sidebar(store);
    const navigation = screen.getByRole("toolbar", { name: "Browser navigation" });
    expect(within(navigation).queryByRole("button", { name: "Back" })).toBeNull();
    expect(within(navigation).queryByRole("button", { name: "Forward" })).toBeNull();
    expect(within(navigation).queryByRole("button", { name: "Reload" })).toBeNull();
    expect(within(navigation).getByRole("button", { name: "Collapse sidebar" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Address bar" })).toBeNull();
  });

  it("wires top bar back/forward/reload and the collapse toggle", () => {
    const onBack = vi.fn();
    const onForward = vi.fn();
    const onReload = vi.fn();
    const onToggleSidebar = vi.fn();
    const { rerender } = render(
      <MahoSidebarTopBar
        canGoBack
        canGoForward={false}
        onBack={onBack}
        onForward={onForward}
        onReload={onReload}
        onToggleSidebar={onToggleSidebar}
      />,
    );

    expect(screen.getByRole("button", { name: "Forward" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    fireEvent.click(screen.getByRole("button", { name: "Forward" }));
    fireEvent.click(screen.getByRole("button", { name: "Reload" }));
    fireEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }));

    expect(onBack).toHaveBeenCalledTimes(1);
    expect(onForward).not.toHaveBeenCalled();
    expect(onReload).toHaveBeenCalledTimes(1);
    expect(onToggleSidebar).toHaveBeenCalledTimes(1);

    rerender(<MahoSidebarTopBar collapsed onToggleSidebar={onToggleSidebar} />);
    expect(screen.getByRole("button", { name: "Expand sidebar" })).toHaveAttribute("aria-expanded", "false");
  });

  it("exposes labelled roles for every section", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub" });
    sidebar(store);

    expect(screen.getByRole("toolbar", { name: "Browser navigation" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Address bar" })).toBeNull();
    expect(screen.getByRole("region", { name: "Favorites" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Pinned" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Today" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Spaces" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Library" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "Today: GitHub" })).toBeInTheDocument();
  });

  it("keeps a drag grip reachable for keyboard users on every sortable row", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub", section: "favorite" });
    addTab(store, "docs", { title: "Docs" });
    sidebar(store);

    const grips = screen.getAllByRole("button", { name: /^Reorder / });
    expect(grips).toHaveLength(2);
    for (const grip of grips) {
      // dnd-kit's keyboard sensor needs a focusable activator; it must stay enabled.
      expect(grip).not.toBeDisabled();
      expect(grip.tabIndex).toBeGreaterThanOrEqual(0);
    }
  });

  it("renders the optional slots and swaps the body for library content", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub" });
    const onToggleLibrary = vi.fn();
    const { container, rerender } = sidebar(store, {
      agentSection: <p>Agent tasks</p>,
      nowPlayingSlot: <p>Now playing</p>,
      controlActivity: <span>Controlled</span>,
    });

    expect(screen.getByText("Agent tasks")).toBeInTheDocument();
    expect(screen.getByText("Now playing")).toBeInTheDocument();
    expect(screen.getByText("Controlled")).toBeInTheDocument();
    expect(container.querySelector(".maho-sidebar__library")).toBeNull();

    rerender(
      <MahoBrowserSidebar
        store={store}
        scheme="light"
        libraryOpen
        onToggleLibrary={onToggleLibrary}
        libraryContent={<p>Archive</p>}
      />,
    );
    expect(screen.getByText("Archive")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Today" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Library" }));
    expect(onToggleLibrary).toHaveBeenCalledTimes(1);
  });
});

describe("Today list", () => {
  it("closes a Today tab into the archive", () => {
    const store = makeStore();
    addTab(store, "github", { title: "GitHub" });
    withDnd(<MahoTodayList store={store} />);

    fireEvent.click(screen.getByRole("button", { name: "Close GitHub" }));
    expect(store.getState().tabs.github).toBeUndefined();
    expect(store.getState().archive[0].tab.browserId).toBe("github");
    expect(store.getState().archive[0].reason).toBe("closed");
  });

  it("marks the active tab and shows the worktree tag", () => {
    const store = makeStore();
    addTab(store, "github", {
      title: "GitHub",
      worktreeTag: { workspaceId: "ws-1", worktreePath: "/code/ferryx-wt/feature-login", label: "feature-login" },
    });
    store.touch("github");
    withDnd(<MahoTodayList store={store} />);

    expect(screen.getByRole("button", { name: "Today: GitHub" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByText("feature-login")).toBeInTheDocument();
  });
});

describe("Space Dots", () => {
  it("renders one dot per Space, marks the active one, and switches on click", () => {
    const store = makeStore();
    const created = store.createSpace({ name: "Work" });
    expect(created.ok).toBe(true);
    if (!created.ok) return;

    render(<MahoSidebarFooter store={store} />);

    const dots = screen.getByRole("group", { name: "Spaces" });
    const buttons = within(dots).getAllByRole("button");
    expect(buttons).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Create Space" }).parentElement).toBe(dots.parentElement);
    expect(buttons[0]).toHaveAttribute("aria-pressed", "true");
    expect(buttons[0].querySelector(".maho-space-dot__indicator")).not.toBeNull();
    expect(buttons[1]).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(screen.getByRole("button", { name: "Switch to Space Work" }));
    expect(store.getState().activeSpaceId).toBe(created.value.id);
    expect(screen.getByRole("button", { name: "Switch to Space Work" })).toHaveAttribute("aria-pressed", "true");
  });

  it("toggles the Library control", () => {
    const store = makeStore();
    const onToggleLibrary = vi.fn();
    render(<MahoSidebarFooter store={store} libraryOpen onToggleLibrary={onToggleLibrary} />);

    const library = screen.getByRole("button", { name: "Library" });
    expect(library).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(library);
    expect(onToggleLibrary).toHaveBeenCalledTimes(1);
  });
});
