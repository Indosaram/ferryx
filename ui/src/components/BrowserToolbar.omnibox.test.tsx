import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { saveBrowserSettings } from "../lib/browserSettings";
import { BROWSER_SHORTCUT_EVENT } from "../lib/browserTauri";
import type { HostHistoryEntry } from "../lib/browserLibrary";
import type { BrowserTab } from "../lib/types";
import { BrowserToolbar } from "./BrowserToolbar";

const browserMocks = vi.hoisted(() => ({
  focusBrowser: vi.fn(async () => undefined),
  focusAppWebview: vi.fn(async () => undefined),
  getBrowserState: vi.fn(),
  goBackBrowser: vi.fn(async () => undefined),
  goForwardBrowser: vi.fn(async () => undefined),
  openExternalUrl: vi.fn(async () => undefined),
  setBrowserZoom: vi.fn(async () => 1),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
}));

vi.mock("../lib/browserTauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/browserTauri")>();
  return { ...actual, ...browserMocks };
});

const host = vi.hoisted(() => ({
  history: [] as HostHistoryEntry[],
  bookmarks: [] as Array<{ id: string; title: string; url: string; parentId: string; position: number; createdAt: number }>,
  suggest: vi.fn(async (_args: { engine: string; query: string }): Promise<string[]> => []),
  added: [] as Array<{ title: string; url: string }>,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args: Record<string, unknown> = {}) => {
    switch (command) {
      case "cmd_browser_history_list": {
        const query = String(args.query ?? "").toLowerCase();
        return host.history.filter((e) => `${e.url} ${e.title ?? ""}`.toLowerCase().includes(query));
      }
      case "cmd_browser_bookmarks_get":
        return { tree: { version: 1, folders: [], bookmarks: host.bookmarks }, recovery: null };
      case "cmd_browser_bookmark_add":
        host.added.push({ title: String(args.title), url: String(args.url) });
        return { id: "new", title: args.title, url: args.url, parentId: "bar", position: 0, createdAt: 1 };
      case "cmd_browser_search_suggest":
        return host.suggest(args as { engine: string; query: string });
      default:
        return undefined;
    }
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));

const tab: BrowserTab = {
  kind: "browser",
  id: "tab-browser",
  label: "Browser",
  browserId: "browser-1",
  url: "https://start.example.com",
  canGoBack: false,
  canGoForward: false,
  loading: false,
};

function visit(url: string, title: string | null, lastVisitedAt: number, frecency: number): HostHistoryEntry {
  return { url, title, visitCount: 1, lastVisitedAt, frecency };
}

beforeEach(() => {
  localStorage.clear();
  host.history = [
    visit("https://github.com/ferryx/ferryx", "Ferryx GitHub", 200, 100),
    visit("https://docs.example.com/browser", "Browser docs", 100, 50),
  ];
  host.bookmarks = [];
  host.added = [];
  host.suggest.mockReset();
  host.suggest.mockResolvedValue([]);
});

afterEach(cleanup);

async function focusOmnibox() {
  const input = screen.getByLabelText("URL address bar");
  fireEvent.focus(input);
  await screen.findByRole("listbox", { name: "Address bar suggestions" });
  return input;
}

describe("BrowserToolbar omnibox", () => {
  it("opens on focus, filters by typed text, and navigates the keyboard-selected result", async () => {
    const onNavigate = vi.fn();
    render(<BrowserToolbar tab={tab} onNavigate={onNavigate} onReload={vi.fn()} />);
    const input = await focusOmnibox();

    fireEvent.change(input, { target: { value: "git" } });
    await waitFor(() => expect(screen.queryByText("Browser docs")).not.toBeInTheDocument());
    expect(screen.getByText("Ferryx GitHub")).toBeInTheDocument();

    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(screen.getByRole("option", { name: /Ferryx GitHub/i })).toHaveAttribute("aria-selected", "true");
    fireEvent.submit(input.closest("form") as HTMLFormElement);

    expect(onNavigate).toHaveBeenCalledWith("https://github.com/ferryx/ferryx");
  });

  it("shows one row per page even when history holds trailing-slash and fragment variants", async () => {
    host.history = [
      visit("https://example.com/", "Example Domain", 300, 300),
      visit("https://example.com", "Example Domain", 200, 100),
      visit("https://example.com/#top", "Example Domain", 100, 50),
    ];
    render(<BrowserToolbar tab={tab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    const input = await focusOmnibox();
    fireEvent.change(input, { target: { value: "example" } });
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(1));
  });

  it("ranks a bookmark and an open tab with history, and switches to the open tab", async () => {
    host.bookmarks = [{ id: "bm", title: "Rust book", url: "https://doc.rust-lang.org/book/", parentId: "bar", position: 0, createdAt: 1 }];
    const switched = vi.fn();
    window.addEventListener("ferryx:browser-switch-to-tab", switched);
    render(
      <BrowserToolbar
        tab={tab}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
        openTabs={[{ browserId: "browser-2", url: "https://rust-lang.org/", title: "Rust" }]}
      />,
    );
    const input = await focusOmnibox();
    fireEvent.change(input, { target: { value: "rust" } });
    const list = await screen.findByRole("listbox", { name: "Address bar suggestions" });
    await waitFor(() => expect(within(list).getAllByRole("option")).toHaveLength(2));
    const kinds = within(list).getAllByRole("option").map((option) => option.getAttribute("data-suggestion-kind"));
    expect(kinds.sort()).toEqual(["bookmark", "tab"]);

    fireEvent.click(within(list).getByRole("option", { name: /Switch to tab/ }));
    expect(switched).toHaveBeenCalledOnce();
    window.removeEventListener("ferryx:browser-switch-to-tab", switched);
  });

  it("adds search-engine suggestions only when opted in, and keeps local results when the endpoint is offline", async () => {
    host.suggest.mockResolvedValue(["github copilot"]);
    const { unmount } = render(<BrowserToolbar tab={tab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    let input = await focusOmnibox();
    fireEvent.change(input, { target: { value: "git" } });
    await screen.findByText("Ferryx GitHub");
    expect(host.suggest).not.toHaveBeenCalled();
    unmount();

    saveBrowserSettings({ searchSuggestions: true });
    const onNavigate = vi.fn();
    const second = render(<BrowserToolbar tab={tab} onNavigate={onNavigate} onReload={vi.fn()} />);
    input = await focusOmnibox();
    fireEvent.change(input, { target: { value: "git" } });
    const option = await screen.findByRole("option", { name: /github copilot/ });
    fireEvent.click(option);
    expect(onNavigate).toHaveBeenCalledWith("https://duckduckgo.com/?q=github%20copilot");
    second.unmount();

    host.suggest.mockRejectedValue({ code: "HOST_UNAVAILABLE", message: "offline" });
    render(<BrowserToolbar tab={tab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    input = await focusOmnibox();
    fireEvent.change(input, { target: { value: "git" } });
    await screen.findByText("Ferryx GitHub");
    await waitFor(() => expect(host.suggest).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("option", { name: /github copilot/ })).not.toBeInTheDocument();
  });

  it("closes on Escape and offers no history when history is disabled", async () => {
    const { unmount } = render(<BrowserToolbar tab={tab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    const input = await focusOmnibox();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    unmount();

    saveBrowserSettings({ rememberBrowsingHistory: false });
    render(<BrowserToolbar tab={tab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    fireEvent.focus(screen.getByLabelText("URL address bar"));
    await act(async () => undefined);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("closes the suggestion list once a navigation commits, on blur, and when the page takes focus", async () => {
    const props = { onNavigate: vi.fn(), onReload: vi.fn() };
    const { rerender } = render(<BrowserToolbar tab={tab} {...props} />);
    await focusOmnibox();
    // Focus stays in the omnibox; the committed url must not leave the old suggestions open.
    rerender(<BrowserToolbar tab={{ ...tab, url: "https://start.example.com/next" }} {...props} />);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();

    const input = await focusOmnibox();
    fireEvent.blur(input);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();

    // Clicking into the native page view is seen only as the app window losing focus.
    await focusOmnibox();
    act(() => {
      window.dispatchEvent(new Event("blur"));
    });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();

    // A page shortcut (find) closes it as well.
    await focusOmnibox();
    act(() => {
      window.dispatchEvent(new CustomEvent(BROWSER_SHORTCUT_EVENT, { detail: { browserId: tab.browserId, action: "find" } }));
    });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("keeps half-typed omnibox text when the page url changes, and syncs again after blur", () => {
    const props = { onNavigate: vi.fn(), onReload: vi.fn() };
    const { rerender } = render(<BrowserToolbar tab={tab} {...props} />);
    const input = screen.getByLabelText("URL address bar") as HTMLInputElement;

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "docs.exa" } });
    rerender(<BrowserToolbar tab={{ ...tab, url: "https://start.example.com/redirected" }} {...props} />);
    expect(input.value).toBe("docs.exa");

    fireEvent.blur(input);
    rerender(<BrowserToolbar tab={{ ...tab, url: "https://start.example.com/later" }} {...props} />);
    expect(input.value).toBe("https://start.example.com/later");
  });

  it("exposes the combobox pattern with an active descendant only while the list is open", async () => {
    render(<BrowserToolbar tab={tab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    const input = screen.getByRole("combobox", { name: "URL address bar" });
    expect(input).toHaveAttribute("aria-autocomplete", "list");
    expect(input).toHaveAttribute("aria-expanded", "false");
    expect(input).not.toHaveAttribute("aria-controls");

    await focusOmnibox();
    const listbox = screen.getByRole("listbox", { name: "Address bar suggestions" });
    expect(input).toHaveAttribute("aria-expanded", "true");
    expect(input.getAttribute("aria-controls")).toBe(listbox.id);
    expect(input).not.toHaveAttribute("aria-activedescendant");

    fireEvent.keyDown(input, { key: "ArrowDown" });
    const selected = screen.getByRole("option", { selected: true });
    expect(selected.id).not.toBe("");
    expect(input.getAttribute("aria-activedescendant")).toBe(selected.id);

    fireEvent.keyDown(input, { key: "Escape" });
    expect(input).not.toHaveAttribute("aria-controls");
    expect(input).not.toHaveAttribute("aria-activedescendant");
  });

  it("reverts the typed text to the current page address on Escape and closes suggestions", async () => {
    const props = { onNavigate: vi.fn(), onReload: vi.fn() };
    const { rerender } = render(<BrowserToolbar tab={tab} {...props} />);
    const input = (await focusOmnibox()) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "git" } });
    expect(input.value).toBe("git");

    fireEvent.keyDown(input, { key: "Escape" });
    expect(input.value).toBe(tab.url);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(props.onNavigate).not.toHaveBeenCalled();

    // The page moved on while the user typed: Escape shows the page's address now.
    fireEvent.change(input, { target: { value: "half typed" } });
    rerender(<BrowserToolbar tab={{ ...tab, url: "https://next.example.com/" }} {...props} />);
    expect(input.value).toBe("half typed");
    fireEvent.keyDown(input, { key: "Escape" });
    expect(input.value).toBe("https://next.example.com/");
  });

  it("moves OS keyboard focus to the app webview before focusing the address bar", async () => {
    const order: string[] = [];
    let release: () => void = () => undefined;
    browserMocks.focusAppWebview.mockImplementation(() => {
      order.push("native");
      return new Promise<undefined>((resolve) => {
        release = () => resolve(undefined);
      });
    });
    render(<BrowserToolbar tab={tab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    const input = screen.getByLabelText("URL address bar");
    const focus = HTMLElement.prototype.focus;
    vi.spyOn(HTMLElement.prototype, "focus").mockImplementation(function (this: HTMLElement, options?: FocusOptions) {
      if (this === input) order.push("input");
      focus.call(this, options);
    });

    act(() => {
      window.dispatchEvent(new CustomEvent(BROWSER_SHORTCUT_EVENT, { detail: { browserId: tab.browserId, action: "focus-address" } }));
    });
    expect(browserMocks.focusAppWebview).toHaveBeenCalledOnce();
    expect(order[0]).toBe("native");
    expect(order.indexOf("input")).toBeGreaterThan(0);

    const before = order.length;
    await act(async () => release());
    expect(order.slice(before)).toEqual(["input"]);
    expect(document.activeElement).toBe(input);
  });

  it("stars the page with the button and the bookmark shortcut", async () => {
    render(<BrowserToolbar tab={{ ...tab, title: "Start" }} onNavigate={vi.fn()} onReload={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Bookmark this page" }));
    await waitFor(() => expect(host.added).toEqual([{ title: "Start", url: "https://start.example.com" }]));

    act(() => {
      window.dispatchEvent(new CustomEvent("ferryx:browser-shortcut", { detail: { browserId: "browser-1", action: "bookmark-page" } }));
    });
    await waitFor(() => expect(host.added).toHaveLength(2));
  });
});
