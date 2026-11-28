// Maho Library mode contracts.
//
// Every case renders the shipped component over a fresh in-memory `browserStore`;
// the only mocks are the IPC bridge (`@tauri-apps/api/core`) and the Tauri event
// channel, so a case observes exactly what the library does to the host commands
// and to the global store. `useNativeTerminalVisibility` runs for real against
// jsdom's MutationObserver: the overlay case proves the dialog role is what makes
// the native surface yield, so the assertion cannot pass on a stale prop.

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createBrowserStore, type BrowserStore } from "../../../state/browserStore";
import { useNativeTerminalVisibility } from "../../../lib/nativeTerminalVisibility";
import {
  MahoLibraryMode,
  archiveCountText,
  archiveDateGroup,
  downloadDateGroup,
  downloadFileKind,
  groupArchiveRows,
} from "./MahoLibraryMode";

type Call = { command: string; args: Record<string, unknown> };

const host = vi.hoisted(() => ({
  calls: [] as Call[],
  recovery: null as "recoveredFromBackup" | "resetCorrupt" | null,
  /** One-shot host failure: the next bookmarks read rejects with a structured IPC error. */
  failNextBookmarksRead: false,
  tree: { version: 1, folders: [] as unknown[], bookmarks: [] as unknown[] },
  history: [] as unknown[],
  downloads: [] as unknown[],
  eventListeners: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: vi.fn(async (command: string, args: Record<string, unknown> = {}) => {
    host.calls.push({ command, args });
    if (command === "cmd_browser_bookmarks_get") {
      if (host.failNextBookmarksRead) {
        host.failNextBookmarksRead = false;
        throw { code: "IO_ERROR", message: "unreadable" };
      }
      return { tree: host.tree, recovery: host.recovery };
    }
    if (command === "cmd_browser_bookmark_add") {
      const mark = {
        id: "b-new",
        title: String(args.title ?? ""),
        url: String(args.url ?? ""),
        parentId: String(args.parentId ?? "bar"),
        position: host.tree.bookmarks.length,
        createdAt: 1,
      };
      host.tree = { ...host.tree, bookmarks: [...host.tree.bookmarks, mark] };
      return mark;
    }
    if (command === "cmd_browser_bookmark_update") {
      host.tree = {
        ...host.tree,
        bookmarks: host.tree.bookmarks.map((candidate) => {
          const mark = candidate as { id: string; title: string; url: string };
          if (mark.id !== args.id) return candidate;
          return { ...mark, title: String(args.title ?? mark.title), url: String(args.url ?? mark.url) };
        }),
      };
      return { tree: host.tree, recovery: host.recovery };
    }
    if (command === "cmd_browser_bookmark_delete") {
      host.tree = {
        ...host.tree,
        bookmarks: host.tree.bookmarks.filter((candidate) => (candidate as { id: string }).id !== args.id),
      };
      return { tree: host.tree, recovery: host.recovery };
    }
    if (command === "cmd_browser_bookmark_folder_create") {
      return { id: "f-new", name: String(args.name ?? ""), parentId: String(args.parentId ?? "bar"), position: 0, createdAt: 1 };
    }
    if (command === "cmd_browser_history_list") return host.history;
    if (command === "cmd_browser_download_list") return host.downloads;
    return undefined;
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, listener: (event: { payload: unknown }) => void) => {
    host.eventListeners.set(event, listener);
    return () => {
      if (host.eventListeners.get(event) === listener) host.eventListeners.delete(event);
    };
  }),
}));

// The visibility hook reports DOM overlay ownership to the native host; that bridge is the
// one thing this suite keeps out of the loop so the case measures the DOM contract alone.
vi.mock("../../../lib/tauri", () => ({
  setNativeTerminalDomOverlayInput: vi.fn(async () => undefined),
}));

function makeStore(): BrowserStore {
  return createBrowserStore({ storage: null, now: () => 1_700_000_000_000 });
}

/** A Today tab closed through the store: that is exactly what fills the archive. */
function archiveClosedTab(store: BrowserStore, browserId: string, title: string): void {
  expect(store.upsertTab({ browserId, url: `https://${browserId}.example.com/`, title }).ok).toBe(true);
  const closed = store.closeTab(browserId);
  expect(closed.ok && closed.value.archived).toBe(true);
}

beforeEach(() => {
  host.calls.length = 0;
  host.recovery = null;
  host.failNextBookmarksRead = false;
  host.history = [];
  host.downloads = [];
  host.tree = { version: 1, folders: [], bookmarks: [] };
  host.eventListeners.clear();
});

describe("category rail", () => {
  it("renders Maho's categories in order and keeps the managers reachable", async () => {
    host.history = [
      { url: "https://a.example/", title: "A page", visitCount: 1, lastVisitedAt: 5_000, frecency: 100 },
    ];
    render(<MahoLibraryMode store={makeStore()} defaultCategory="archive" />);

    const rail = screen.getByRole("tablist", { name: "Browser library" });
    // Downloads and Archived Tabs lead the rail exactly as in Maho; Spaces follows, and the
    // bookmark/history managers stay reachable after the divider. Media is opt-in.
    expect(within(rail).getAllByRole("tab").map((tab) => tab.getAttribute("aria-label"))).toEqual([
      "Downloads",
      "Archived Tabs",
      "Spaces",
      "Bookmarks",
      "History",
    ]);
    expect(within(rail).getAllByRole("tab").slice(3).every((tab) => tab.getAttribute("data-manager") === "true")).toBe(true);
    expect(within(rail).getByRole("tab", { name: "Archived Tabs" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("maho-library-archive")).toBeInTheDocument();

    // Downloads reads the registry; it answers with an empty list here.
    fireEvent.click(within(rail).getByRole("tab", { name: "Downloads" }));
    expect(screen.getByRole("tab", { name: "Downloads" })).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByTestId("maho-library-downloads")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Search downloads" })).toBeInTheDocument();

    fireEvent.click(within(rail).getByRole("tab", { name: "Bookmarks" }));
    expect(await screen.findByTestId("maho-library-bookmarks")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "History" })).toHaveAttribute("aria-selected", "false");

    fireEvent.click(within(rail).getByRole("tab", { name: "History" }));
    expect(screen.getByTestId("maho-library-history")).toBeInTheDocument();
    const row = await screen.findByRole("button", { name: "History: A page" });
    expect(row).toBeInTheDocument();
  });

  it("adds the Media category only when the owner enables it", () => {
    const { rerender } = render(<MahoLibraryMode store={makeStore()} />);
    expect(screen.queryByRole("tab", { name: "Media" })).toBeNull();
    rerender(<MahoLibraryMode store={makeStore()} showMedia />);
    const labels = screen.getAllByRole("tab").map((tab) => tab.getAttribute("aria-label"));
    expect(labels).toContain("Media");
    // Maho's own order: the four categories, then the Ferryx managers.
    expect(labels.slice(0, 4)).toEqual(["Downloads", "Archived Tabs", "Spaces", "Media"]);
  });

  it("reports the chosen category to its owner and opens a history entry", async () => {
    host.history = [
      { url: "https://docs.example/guide", title: "Guide", visitCount: 3, lastVisitedAt: 9_000, frecency: 300 },
    ];
    const onCategoryChange = vi.fn();
    const onOpenUrl = vi.fn();
    render(<MahoLibraryMode store={makeStore()} onCategoryChange={onCategoryChange} onOpenUrl={onOpenUrl} />);

    fireEvent.click(screen.getByRole("tab", { name: "History" }));
    expect(onCategoryChange).toHaveBeenCalledWith("history");

    fireEvent.click(await screen.findByRole("button", { name: "History: Guide" }));
    expect(onOpenUrl).toHaveBeenCalledWith("https://docs.example/guide");
  });

  it("filters history through the host query", async () => {
    render(<MahoLibraryMode store={makeStore()} defaultCategory="history" />);
    fireEvent.change(screen.getByLabelText("Search history"), { target: { value: "news" } });
    await waitFor(() =>
      expect(host.calls.some((call) => call.command === "cmd_browser_history_list" && call.args.query === "news")).toBe(true),
    );
  });
});

describe("archive", () => {
  it("restores an archived tab back to Today in one click", async () => {
    const store = makeStore();
    archiveClosedTab(store, "github", "GitHub");
    expect(store.getState().archive).toHaveLength(1);
    expect(store.getState().tabs.github).toBeUndefined();

    const onOpenTab = vi.fn();
    render(<MahoLibraryMode store={store} onOpenTab={onOpenTab} />);

    fireEvent.click(await screen.findByRole("button", { name: "Restore archived tab GitHub" }));

    const state = store.getState();
    expect(state.tabs.github).toBeDefined();
    expect(state.tabs.github.section).toBe("today");
    expect(state.archive).toHaveLength(0);
    expect(onOpenTab).toHaveBeenCalledWith("github");
    expect(screen.queryByRole("button", { name: "Restore archived tab GitHub" })).toBeNull();
  });

  it("filters archived tabs by title and URL", async () => {
    const store = makeStore();
    archiveClosedTab(store, "github", "GitHub");
    archiveClosedTab(store, "docs", "Docs");
    render(<MahoLibraryMode store={store} />);

    expect(screen.getByRole("button", { name: "Restore archived tab GitHub" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Search Archive"), { target: { value: "docs" } });
    expect(screen.queryByRole("button", { name: "Restore archived tab GitHub" })).toBeNull();
    expect(screen.getByRole("button", { name: "Restore archived tab Docs" })).toBeInTheDocument();
  });
});

describe("download controls", () => {
  it("searches filenames and keeps completed file actions wired", async () => {
    host.downloads = [
      { id: "report", url: "https://example.com/report.pdf", filePath: "/downloads/report.pdf", status: "completed", receivedBytes: 1024, totalBytes: 1024, createdAtMs: 1, updatedAtMs: 1 },
      { id: "photo", url: "https://example.com/photo.png", filePath: "/downloads/photo.png", status: "completed", receivedBytes: 2048, totalBytes: 2048, createdAtMs: 2, updatedAtMs: 2 },
    ];
    render(<MahoLibraryMode store={makeStore()} defaultCategory="downloads" />);
    await screen.findByTestId("download-row-report");
    fireEvent.change(screen.getByRole("textbox", { name: "Search downloads" }), { target: { value: "report" } });
    expect(screen.queryByTestId("download-row-photo")).toBeNull();
    fireEvent.click(within(screen.getByTestId("download-row-report")).getByRole("button", { name: "Open download" }));
    await waitFor(() => expect(host.calls).toContainEqual({ command: "cmd_open_file_path", args: { path: "/downloads/report.pdf", editor: "system" } }));
  });

  it("puts an In Progress group first and counts every group", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(1_700_000_000_000);
    try {
      const now = Date.now();
      host.downloads = [
        { id: "old", url: "https://example.com/old.zip", filePath: "/downloads/old.zip", status: "completed", receivedBytes: 10, totalBytes: 10, createdAtMs: now - 20 * 86_400_000, updatedAtMs: now },
        { id: "done", url: "https://example.com/done.pdf", filePath: "/downloads/done.pdf", status: "completed", receivedBytes: 20, totalBytes: 20, createdAtMs: now - 1_000, updatedAtMs: now },
        { id: "live", url: "https://example.com/live.iso", filePath: "/downloads/live.iso", status: "inProgress", receivedBytes: 30, totalBytes: 100, createdAtMs: now - 5_000, updatedAtMs: now, canPause: true },
      ];
      render(<MahoLibraryMode store={makeStore()} defaultCategory="downloads" />);
      await screen.findByTestId("download-row-live");

      // The unfinished download is in the first group even though it is older than the
      // completed one: Maho's section order, not the timestamp, decides.
      expect(screen.getByTestId("download-group-count-In Progress")).toHaveTextContent("1");
      expect(screen.getByTestId("download-group-count-Today")).toHaveTextContent("1");
      expect(screen.getByTestId("download-group-count-Older")).toHaveTextContent("1");
      const groups = Array.from(document.querySelectorAll('[data-testid^="download-group-count-"]'))
        .map((node) => node.getAttribute("data-testid"));
      expect(groups[0]).toBe("download-group-count-In Progress");
      expect(downloadDateGroup("paused", now)).toBe("In Progress");
      expect(downloadDateGroup("completed", now - 30 * 86_400_000)).toBe("Older");
    } finally {
      vi.useRealTimers();
    }
  });

  it("gives each row its file-type icon and a timestamp", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(1_700_000_000_000);
    try {
      const now = Date.now();
      host.downloads = [
        { id: "image", url: "https://example.com/a.png", filePath: "/downloads/a.png", status: "completed", receivedBytes: 1, totalBytes: 1, createdAtMs: now - 2_000, updatedAtMs: now },
        { id: "code", url: "https://example.com/b.rs", filePath: "/downloads/b.rs", status: "completed", receivedBytes: 1, totalBytes: 1, createdAtMs: now - 30 * 86_400_000, updatedAtMs: now },
      ];
      render(<MahoLibraryMode store={makeStore()} defaultCategory="downloads" />);
      await screen.findByTestId("download-row-image");

      expect(screen.getByTestId("download-row-image")).toHaveAttribute("data-file-kind", "image");
      expect(screen.getByTestId("download-row-code")).toHaveAttribute("data-file-kind", "code");
      // Today's rows show a clock time, older ones a date; both are non-empty.
      expect(screen.getByTestId("download-time-image")).not.toBeEmptyDOMElement();
      expect(screen.getByTestId("download-time-code")).not.toBeEmptyDOMElement();
      expect(downloadFileKind("archive.tar.gz")).toBe("archive");
      expect(downloadFileKind("README")).toBe("file");
      expect(downloadFileKind("trailing.")).toBe("file");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("archive controls", () => {
  it("deletes an archived tab without restoring it", () => {
    const store = makeStore();
    archiveClosedTab(store, "docs", "Docs");
    render(<MahoLibraryMode store={store} />);
    fireEvent.click(screen.getByRole("button", { name: "Delete archived tab Docs" }));
    expect(store.getState().archive).toHaveLength(0);
    expect(store.getState().tabs.docs).toBeUndefined();
  });

  it("groups the archive into date sections that each carry their own count", () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_700_000_000_000);
    try {
      const store = makeStore();
      archiveClosedTab(store, "today-a", "Today A");
      archiveClosedTab(store, "today-b", "Today B");
      // Backdate one entry a day so it lands in Yesterday, exactly as Maho buckets it.
      store.setState((state) => ({
        ...state,
        archive: state.archive.map((entry) => (entry.tab.browserId === "today-b"
          ? { ...entry, archivedAt: entry.archivedAt - 86_400_000 }
          : entry)),
      }));

      render(<MahoLibraryMode store={store} />);

      expect(archiveDateGroup(Date.now())).toBe("Today");
      expect(screen.getByTestId("archive-group-count-Today")).toHaveTextContent("1");
      expect(screen.getByTestId("archive-group-count-Yesterday")).toHaveTextContent("1");
      // Each section is a list of its own rows, labelled by the bucket name.
      const yesterday = screen.getByRole("list", { name: "Yesterday" });
      expect(within(yesterday).getByRole("button", { name: "Restore archived tab Today B" })).toBeInTheDocument();
      expect(within(screen.getByRole("list", { name: "Today" })).queryByRole("button", { name: "Restore archived tab Today B" })).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("offers Restore and Delete on the archived row's context menu", () => {
    const store = makeStore();
    archiveClosedTab(store, "docs", "Docs");
    const onOpenTab = vi.fn();
    render(<MahoLibraryMode store={store} onOpenTab={onOpenTab} />);

    fireEvent.contextMenu(screen.getByRole("button", { name: "Restore archived tab Docs" }).closest(".maho-library-row") as Element);

    const menu = screen.getByRole("dialog", { name: "Archived tab actions" });
    expect(within(menu).getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["Restore", "Delete"]);

    fireEvent.click(within(menu).getByRole("menuitem", { name: "Restore" }));
    expect(store.getState().tabs.docs).toBeDefined();
    expect(store.getState().archive).toHaveLength(0);
    expect(onOpenTab).toHaveBeenCalledWith("docs");
    expect(screen.queryByRole("dialog", { name: "Archived tab actions" })).toBeNull();
  });

  it("deletes from the context menu and closes it on Escape", () => {
    const store = makeStore();
    archiveClosedTab(store, "docs", "Docs");
    render(<MahoLibraryMode store={store} />);

    fireEvent.contextMenu(screen.getByRole("button", { name: "Restore archived tab Docs" }).closest(".maho-library-row") as Element);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Archived tab actions" })).toBeNull();
    expect(store.getState().archive).toHaveLength(1);

    fireEvent.contextMenu(screen.getByRole("button", { name: "Restore archived tab Docs" }).closest(".maho-library-row") as Element);
    fireEvent.click(within(screen.getByRole("dialog", { name: "Archived tab actions" })).getByRole("menuitem", { name: "Delete" }));
    expect(store.getState().archive).toHaveLength(0);
    expect(store.getState().tabs.docs).toBeUndefined();
  });

  it("shows an SVG illustration in the empty state", () => {
    render(<MahoLibraryMode store={makeStore()} />);
    const empty = screen.getByText("Nothing archived yet").closest(".maho-library-empty") as HTMLElement;
    expect(empty.querySelector("svg")).not.toBeNull();
    expect(empty.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
  });

  it("filters archived tabs by date with a fixed clock", () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_700_000_000_000);
    try {
      const store = makeStore();
      archiveClosedTab(store, "docs", "Docs");
      render(<MahoLibraryMode store={store} />);
      fireEvent.change(screen.getByRole("combobox", { name: "Filter archived tabs" }), { target: { value: "Older" } });
      expect(screen.queryByRole("button", { name: "Restore archived tab Docs" })).toBeNull();
      fireEvent.change(screen.getByRole("combobox", { name: "Filter archived tabs" }), { target: { value: "Today" } });
      expect(screen.getByRole("button", { name: "Restore archived tab Docs" })).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("dismisses the covering library on Escape", () => {
    const onClose = vi.fn();
    render(<MahoLibraryMode store={makeStore()} overlay onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });
});

describe("archive bucketing helpers", () => {
  it("names Maho's date buckets from a day offset", () => {
    const now = 1_700_000_000_000;
    expect(archiveDateGroup(now, now)).toBe("Today");
    expect(archiveDateGroup(now - 86_400_000, now)).toBe("Yesterday");
    expect(archiveDateGroup(now - 3 * 86_400_000, now)).toBe("3 days ago");
    expect(archiveDateGroup(now - 10 * 86_400_000, now)).toBe("Last 2 weeks");
    expect(archiveDateGroup(now - 40 * 86_400_000, now)).toBe("Earlier");
    expect(archiveDateGroup(0, now)).toBe("Earlier");
  });

  it("builds one section per contiguous bucket and counts each", () => {
    const now = Date.now();
    const rows = [
      { archivedAt: now, id: "a" },
      { archivedAt: now - 60_000, id: "b" },
      { archivedAt: now - 2 * 86_400_000, id: "c" },
    ];
    const sections = groupArchiveRows(rows);
    expect(sections.map((section) => section.title)).toEqual(["Today", "2 days ago"]);
    expect(sections[0].entries.map((row) => row.id)).toEqual(["a", "b"]);
    expect(sections[1].entries.map((row) => row.id)).toEqual(["c"]);
  });

  it("pluralises the visible/total count and never hides the total", () => {
    expect(archiveCountText(0, 0)).toBe("No tabs");
    expect(archiveCountText(1, 1)).toBe("1 tab");
    expect(archiveCountText(3, 3)).toBe("3 tabs");
    expect(archiveCountText(2, 9)).toBe("2 tabs of 9");
  });
});

describe("bookmarks", () => {
  it("creates a bookmark through the host commands and lists it", async () => {
    render(<MahoLibraryMode store={makeStore()} defaultCategory="bookmarks" />);
    await screen.findByRole("list", { name: "Bookmarks" });

    fireEvent.change(screen.getByLabelText("New bookmark title"), { target: { value: "Ferryx" } });
    fireEvent.change(screen.getByLabelText("New bookmark URL"), { target: { value: "https://ferryx.example/" } });
    fireEvent.click(screen.getByRole("button", { name: "Add bookmark" }));

    await waitFor(() =>
      expect(host.calls).toContainEqual({
        command: "cmd_browser_bookmark_add",
        args: { title: "Ferryx", url: "https://ferryx.example/", parentId: "bar" },
      }),
    );
    const list = screen.getByRole("list", { name: "Bookmarks" });
    expect(await within(list).findByText("Ferryx")).toBeInTheDocument();
    // The form clears itself so the next bookmark starts empty.
    expect(screen.getByLabelText("New bookmark title")).toHaveValue("");
  });

  it("edits a bookmark in place through updateBookmark", async () => {
    host.tree = {
      version: 1,
      folders: [],
      bookmarks: [{ id: "b1", title: "Docs", url: "https://docs.example/", parentId: "bar", position: 0, createdAt: 1 }],
    };
    render(<MahoLibraryMode store={makeStore()} defaultCategory="bookmarks" />);
    expect(await screen.findByText("Docs")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Edit Docs" }));
    fireEvent.change(screen.getByLabelText("Bookmark name"), { target: { value: "Handbooks" } });
    fireEvent.change(screen.getByLabelText("Bookmark URL"), { target: { value: "https://handbook.example/" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(host.calls).toContainEqual({
        command: "cmd_browser_bookmark_update",
        args: { id: "b1", title: "Handbooks", url: "https://handbook.example/" },
      }),
    );
    expect(await screen.findByText("Handbooks")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Edit Handbooks" })).toBeInTheDocument();
  });

  it("creates a folder through the inline editor rather than window.prompt", async () => {
    render(<MahoLibraryMode store={makeStore()} defaultCategory="bookmarks" />);
    fireEvent.click(await screen.findByRole("button", { name: "New folder" }));
    fireEvent.change(screen.getByLabelText("New folder name"), { target: { value: "Research" } });
    fireEvent.click(screen.getByRole("button", { name: "Create folder" }));

    await waitFor(() =>
      expect(host.calls).toContainEqual({
        command: "cmd_browser_bookmark_folder_create",
        args: { name: "Research", parentId: "bar" },
      }),
    );
    expect(screen.queryByLabelText("New folder name")).toBeNull();
  });

  it("shows the recovery notice instead of an error when the store was reset", async () => {
    host.recovery = "resetCorrupt";
    render(<MahoLibraryMode store={makeStore()} defaultCategory="bookmarks" />);

    const notice = await screen.findByTestId("maho-library-bookmark-recovery");
    expect(notice).toHaveAttribute("role", "status");
    expect(notice.textContent).toContain("damaged");
    // Recovery is a state, not a failure: no destructive alert is raised for it.
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("keeps a real load failure on the alert channel", async () => {
    host.failNextBookmarksRead = true;
    render(<MahoLibraryMode store={makeStore()} defaultCategory="bookmarks" />);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("IO_ERROR");
    expect(screen.queryByTestId("maho-library-bookmark-recovery")).toBeNull();
  });
});

describe("native occlusion", () => {
  /** A real `useNativeTerminalVisibility` owner beside the library, so the hook observes the
   *  library's own DOM instead of a simulation of it. */
  function Harness({ overlay = false }: { overlay?: boolean }) {
    const visible = useNativeTerminalVisibility();
    return (
      <div>
        <span data-testid="native-interactive">{String(visible)}</span>
        <MahoLibraryMode store={makeStore()} overlay={overlay} />
      </div>
    );
  }

  it("claims the dialog role and hides native surfaces only in overlay mode", async () => {
    const { rerender } = render(<Harness />);
    // In flow (the sidebar drawer) the library is a panel, never a dialog.
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByTestId("native-interactive")).toHaveTextContent("true");

    // Overlaying the page makes it a dialog, which is the surface the host yields to.
    await act(async () => {
      rerender(<Harness overlay />);
    });

    const dialog = screen.getByRole("dialog", { name: "Browser library" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAttribute("data-maho-library-overlay", "true");
    expect(screen.getByTestId("native-interactive")).toHaveTextContent("false");

    // Closing the overlay releases the native surface again.
    await act(async () => {
      rerender(<Harness />);
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByTestId("native-interactive")).toHaveTextContent("true");
  });
});
