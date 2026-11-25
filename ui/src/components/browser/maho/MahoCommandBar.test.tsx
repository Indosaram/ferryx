import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
const events = vi.hoisted(() => ({ listen: vi.fn() }));

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke: ipc.invoke,
}));
vi.mock("@tauri-apps/api/event", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/event")>()),
  listen: events.listen,
}));

import {
  MahoCommandBar,
  buildMahoCommandBarResults,
  type MahoCommandBarInput,
} from "./MahoCommandBar";
import { DEFAULT_BROWSER_SETTINGS } from "../../../lib/browserSettings";
import type { BrowserHistoryEntry } from "../../../lib/browserHistory";
import type { Bookmark } from "../../../lib/browserLibrary";
import { useNativeTerminalVisibility } from "../../../lib/nativeTerminalVisibility";
import { browserStore, type BrowserSpace, type BrowserStoreTab } from "../../../state/browserStore";

function NativeSurfaceProbe() {
  return <span data-testid="native-surface-probe">{String(useNativeTerminalVisibility())}</span>;
}

const SPACE: BrowserSpace = { id: "space-default", name: "Personal", color: "#6C8CFF", order: 0 };
const spaces: Readonly<Record<string, BrowserSpace>> = { [SPACE.id]: SPACE };

function tab(overrides: Partial<BrowserStoreTab> & { browserId: string; url: string }): BrowserStoreTab {
  return {
    spaceId: SPACE.id,
    title: "",
    section: "today",
    order: 0,
    lastActiveAt: 0,
    ...overrides,
  };
}

function visit(id: string, url: string, title: string | null, visitedAt: number): BrowserHistoryEntry {
  return { id, browserId: "history-tab", url, title, visitedAt };
}

function bookmark(id: string, url: string, title: string): Bookmark {
  return { id, url, title, parentId: "bar", position: 0, createdAt: 0 };
}

function input(patch: Partial<MahoCommandBarInput> = {}): MahoCommandBarInput {
  return {
    query: "",
    tabs: [],
    spaces,
    bookmarks: [],
    history: [],
    settings: DEFAULT_BROWSER_SETTINGS,
    ...patch,
  };
}

describe("buildMahoCommandBarResults", () => {
  it("ranks an open GitHub tab above history and the web-search fallback for \"git\"", () => {
    const results = buildMahoCommandBarResults(input({
      query: "git",
      tabs: [tab({ browserId: "tab-github", url: "https://github.com/ferryx/ferryx", title: "GitHub", lastActiveAt: 5 })],
      bookmarks: [bookmark("bm-docs", "https://docs.rs/git2", "git2 docs")],
      history: [visit("h1", "https://git-scm.com/", "Git", 9)],
    }));

    expect(results[0]).toMatchObject({
      source: "tab",
      browserId: "tab-github",
      url: "https://github.com/ferryx/ferryx",
      spaceName: "Personal",
      spaceColor: "#6C8CFF",
    });
    expect(results[0].score).toBeGreaterThan(results[1].score);
    expect(results.map((row) => row.source)).toEqual(["tab", "bookmark", "history", "action"]);

    // The search row is the fallback: present, and last.
    const action = results[results.length - 1];
    expect(action).toMatchObject({ source: "action", isSearch: true });
    expect(action.url).toBe("https://duckduckgo.com/?q=git");
  });

  it("keeps a favorite tab ahead of a plain open tab with the same match", () => {
    const results = buildMahoCommandBarResults(input({
      query: "docs",
      tabs: [
        tab({ browserId: "tab-plain", url: "https://docs.rs", title: "docs.rs", lastActiveAt: 9 }),
        tab({ browserId: "tab-favorite", url: "https://docs.python.org", title: "docs.python", section: "favorite" }),
      ],
    }));

    // Two tab rows, then the search fallback for anything else.
    expect(results.map((row) => row.source)).toEqual(["tab", "tab", "action"]);
    expect(results.filter((row) => row.source === "tab").map((row) => row.browserId))
      .toEqual(["tab-favorite", "tab-plain"]);
    expect(results[0].favorite).toBe(true);
    expect(results[1].favorite).toBe(false);
    expect(results[results.length - 1]).toMatchObject({ source: "action", isSearch: true });
  });

  it("treats a domain as an address to navigate to", () => {
    const results = buildMahoCommandBarResults(input({ query: "example.com" }));

    expect(results).toHaveLength(1);
    expect(results[0]).toMatchObject({ source: "action", isSearch: false, url: "https://example.com" });
  });

  it("treats a phrase as a web search on the configured engine", () => {
    const results = buildMahoCommandBarResults(input({ query: "how to rust" }));

    expect(results).toHaveLength(1);
    expect(results[0]).toMatchObject({ source: "action", isSearch: true });
    expect(results[0].url).toBe("https://duckduckgo.com/?q=how%20to%20rust");
  });

  it("behaves like a recency-ordered tab switcher before anything is typed", () => {
    const results = buildMahoCommandBarResults(input({
      tabs: [
        tab({ browserId: "old", url: "https://old.test", title: "Old", lastActiveAt: 1 }),
        tab({ browserId: "new", url: "https://new.test", title: "New", lastActiveAt: 2 }),
      ],
      history: [visit("h1", "https://git-scm.com/", "Git", 9)],
    }));

    expect(results.map((row) => row.browserId)).toEqual(["new", "old"]);
  });
});

describe("MahoCommandBar", () => {
  beforeEach(() => {
    window.localStorage.clear();
    browserStore.reset();
    ipc.invoke.mockReset();
    ipc.invoke.mockResolvedValue(null);
    events.listen.mockReset();
    events.listen.mockImplementation(async () => () => {});
  });

  function renderBar(props: { open: boolean; initialQuery?: string; mode?: "navigate" | "new-tab" } = { open: true }) {
    const onClose = vi.fn();
    const onNavigate = vi.fn();
    const onSelectOpenTab = vi.fn();
    const view = render(
      <MahoCommandBar
        open={props.open}
        initialQuery={props.initialQuery}
        mode={props.mode}
        onClose={onClose}
        onNavigate={onNavigate}
        onSelectOpenTab={onSelectOpenTab}
      />,
    );
    return { ...view, onClose, onNavigate, onSelectOpenTab };
  }

  it("maps Enter, Cmd/Ctrl+Enter and Shift+Enter to current, new-tab and mini", () => {
    const { onNavigate } = renderBar();
    const field = screen.getByTestId("maho-command-bar-input");
    fireEvent.change(field, { target: { value: "example.com" } });

    fireEvent.keyDown(field, { key: "Enter" });
    expect(onNavigate).toHaveBeenLastCalledWith("https://example.com", "current");

    fireEvent.keyDown(field, { key: "Enter", metaKey: true });
    expect(onNavigate).toHaveBeenLastCalledWith("https://example.com", "new-tab");

    fireEvent.keyDown(field, { key: "Enter", ctrlKey: true });
    expect(onNavigate).toHaveBeenLastCalledWith("https://example.com", "new-tab");

    fireEvent.keyDown(field, { key: "Enter", shiftKey: true });
    expect(onNavigate).toHaveBeenLastCalledWith("https://example.com", "mini");
  });

  it("lands a typed address in a new tab when the bar was opened as Cmd+T", () => {
    const { onNavigate } = renderBar({ open: true, mode: "new-tab" });
    const field = screen.getByTestId("maho-command-bar-input");
    fireEvent.change(field, { target: { value: "example.com" } });
    fireEvent.keyDown(field, { key: "Enter" });

    expect(onNavigate).toHaveBeenLastCalledWith("https://example.com", "new-tab");
  });

  it("switches to an open tab instead of navigating to its address", () => {
    browserStore.upsertTab({ browserId: "tab-github", url: "https://github.com/ferryx/ferryx", title: "GitHub" });
    const { onNavigate, onSelectOpenTab, onClose } = renderBar();
    const field = screen.getByTestId("maho-command-bar-input");
    fireEvent.change(field, { target: { value: "github" } });
    fireEvent.keyDown(field, { key: "Enter" });

    expect(onSelectOpenTab).toHaveBeenCalledWith("tab-github");
    expect(onNavigate).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("moves the highlight with ArrowDown/ArrowUp", () => {
    browserStore.upsertTab({ browserId: "tab-github", url: "https://github.com/ferryx/ferryx", title: "GitHub" });
    renderBar();
    const field = screen.getByTestId("maho-command-bar-input");
    fireEvent.change(field, { target: { value: "git" } });

    const options = screen.getAllByRole("option");
    expect(options.length).toBeGreaterThan(1);
    expect(options[0]).toHaveAttribute("aria-selected", "true");

    fireEvent.keyDown(field, { key: "ArrowDown" });
    expect(screen.getAllByRole("option")[1]).toHaveAttribute("aria-selected", "true");

    fireEvent.keyDown(field, { key: "ArrowUp" });
    expect(screen.getAllByRole("option")[0]).toHaveAttribute("aria-selected", "true");

    // Wrapping up from the first row lands on the last one.
    fireEvent.keyDown(field, { key: "ArrowUp" });
    const rows = screen.getAllByRole("option");
    expect(rows[rows.length - 1]).toHaveAttribute("aria-selected", "true");
  });

  it("closes on Esc and hands focus back to whoever opened it", () => {
    const onClose = vi.fn();
    const bar = (open: boolean) => (
      <>
        <button type="button">Opener</button>
        <MahoCommandBar
          open={open}
          onClose={onClose}
          onNavigate={vi.fn()}
          onSelectOpenTab={vi.fn()}
        />
      </>
    );
    const { rerender } = render(bar(false));
    const opener = screen.getByRole("button", { name: "Opener" });
    opener.focus();
    expect(opener).toHaveFocus();

    rerender(bar(true));
    const field = screen.getByTestId("maho-command-bar-input");
    expect(field).toHaveFocus();

    fireEvent.keyDown(field, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);

    rerender(bar(false));
    expect(screen.queryByTestId("maho-command-bar-input")).toBeNull();
    expect(opener).toHaveFocus();
  });

  it("hides native surfaces while open and shows them again when closed", async () => {
    const bar = (open: boolean) => (
      <>
        <NativeSurfaceProbe />
        <MahoCommandBar
          open={open}
          onClose={vi.fn()}
          onNavigate={vi.fn()}
          onSelectOpenTab={vi.fn()}
        />
      </>
    );
    const { rerender } = render(bar(false));
    await act(async () => {});
    expect(screen.getByTestId("native-surface-probe")).toHaveTextContent("true");

    rerender(bar(true));
    await act(async () => {});
    expect(document.querySelector('[role="dialog"][aria-label="Maho command bar"]')).not.toBeNull();
    expect(screen.getByTestId("native-surface-probe")).toHaveTextContent("false");

    rerender(bar(false));
    await act(async () => {});
    expect(screen.getByTestId("native-surface-probe")).toHaveTextContent("true");
  });

  it("releases native occlusion when it unmounts while open", async () => {
    // A separate root owns the probe, so it outlives the palette and observes the release.
    render(<NativeSurfaceProbe />);
    const { unmount } = renderBar();
    await act(async () => {});
    expect(document.querySelector('[role="dialog"][aria-label="Maho command bar"]')).not.toBeNull();
    expect(screen.getByTestId("native-surface-probe")).toHaveTextContent("false");

    unmount();
    await act(async () => {});
    expect(document.querySelector('[role="dialog"][aria-label="Maho command bar"]')).toBeNull();
    expect(screen.getByTestId("native-surface-probe")).toHaveTextContent("true");
  });
});
