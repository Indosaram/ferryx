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
import { MahoLibraryMode } from "./MahoLibraryMode";

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
  it("renders the four Maho categories and switches panels", async () => {
    host.history = [
      { url: "https://a.example/", title: "A page", visitCount: 1, lastVisitedAt: 5_000, frecency: 100 },
    ];
    render(<MahoLibraryMode store={makeStore()} defaultCategory="archive" />);

    const rail = screen.getByRole("tablist", { name: "Browser library" });
    expect(within(rail).getAllByRole("tab").map((tab) => tab.getAttribute("aria-label"))).toEqual([
      "Archive",
      "Downloads",
      "Bookmarks",
      "History",
    ]);
    expect(within(rail).getByRole("tab", { name: "Archive" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("maho-library-archive")).toBeInTheDocument();

    // Downloads reads the registry; it answers with an empty list here.
    fireEvent.click(within(rail).getByRole("tab", { name: "Downloads" }));
    expect(screen.getByRole("tab", { name: "Downloads" })).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByTestId("maho-library-downloads")).toBeInTheDocument();
    expect(await screen.findByText("No downloads yet.")).toBeInTheDocument();

    fireEvent.click(within(rail).getByRole("tab", { name: "Bookmarks" }));
    expect(await screen.findByTestId("maho-library-bookmarks")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "History" })).toHaveAttribute("aria-selected", "false");

    fireEvent.click(within(rail).getByRole("tab", { name: "History" }));
    expect(screen.getByTestId("maho-library-history")).toBeInTheDocument();
    const row = await screen.findByRole("button", { name: "History: A page" });
    expect(row).toBeInTheDocument();
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

    fireEvent.click(await screen.findByRole("button", { name: "Restore GitHub to Today" }));

    const state = store.getState();
    expect(state.tabs.github).toBeDefined();
    expect(state.tabs.github.section).toBe("today");
    expect(state.archive).toHaveLength(0);
    expect(onOpenTab).toHaveBeenCalledWith("github");
    expect(screen.getByText("Nothing archived yet.")).toBeInTheDocument();
  });

  it("filters archived tabs by title and URL", async () => {
    const store = makeStore();
    archiveClosedTab(store, "github", "GitHub");
    archiveClosedTab(store, "docs", "Docs");
    render(<MahoLibraryMode store={store} />);

    expect(screen.getByRole("button", { name: "Restore GitHub to Today" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Search archive"), { target: { value: "docs" } });
    expect(screen.queryByRole("button", { name: "Restore GitHub to Today" })).toBeNull();
    expect(screen.getByRole("button", { name: "Restore Docs to Today" })).toBeInTheDocument();
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
