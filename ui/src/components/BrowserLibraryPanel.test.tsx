import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserLibraryPanel } from "./BrowserLibraryPanel";

const calls = vi.hoisted(() => [] as Array<{ command: string; args: Record<string, unknown> }>);
const data = vi.hoisted(() => ({
  history: [] as unknown[],
  tree: { version: 1, folders: [] as unknown[], bookmarks: [] as unknown[] },
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args: Record<string, unknown> = {}) => {
    calls.push({ command, args });
    if (command === "cmd_browser_history_list") return data.history;
    if (command === "cmd_browser_bookmarks_get") return { tree: data.tree, recovery: null };
    if (command.startsWith("cmd_browser_bookmark")) return { tree: data.tree, recovery: null };
    return undefined;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));

beforeEach(() => {
  calls.length = 0;
  data.history = [{ url: "https://a.example/", title: "A page", visitCount: 2, lastVisitedAt: 5_000, frecency: 200 }];
  data.tree = {
    version: 1,
    folders: [{ id: "f1", name: "Work", parentId: "bar", position: 0, createdAt: 1 }],
    bookmarks: [
      { id: "b1", title: "Docs", url: "https://docs.example/", parentId: "bar", position: 1, createdAt: 1 },
      { id: "b2", title: "Wiki", url: "https://wiki.example/", parentId: "f1", position: 0, createdAt: 1 },
    ],
  };
});
afterEach(cleanup);

const props = { onViewChange: vi.fn(), onOpen: vi.fn(), onClose: vi.fn() };

describe("BrowserLibraryPanel history", () => {
  it("removes one entry and clears the chosen time range", async () => {
    render(<BrowserLibraryPanel view="history" {...props} now={() => 10 * 3_600_000} />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove A page from history" }));
    await waitFor(() => expect(calls).toContainEqual({ command: "cmd_browser_history_delete", args: { url: "https://a.example/" } }));

    fireEvent.change(screen.getByLabelText("Clear range"), { target: { value: "day" } });
    fireEvent.click(screen.getByRole("button", { name: "Clear browsing data" }));
    await waitFor(() => expect(calls).toContainEqual({
      command: "cmd_browser_history_clear",
      args: { from: 10 * 3_600_000 - 24 * 3_600_000, to: 10 * 3_600_000 + 1 },
    }));
  });

  it("searches through the host store", async () => {
    render(<BrowserLibraryPanel view="history" {...props} />);
    fireEvent.change(screen.getByLabelText("Search history"), { target: { value: "news" } });
    await waitFor(() => expect(calls.some((c) => c.command === "cmd_browser_history_list" && c.args.query === "news")).toBe(true));
  });
});

describe("BrowserLibraryPanel bookmarks", () => {
  it("lists a folder, searches across folders, moves and deletes", async () => {
    render(<BrowserLibraryPanel view="bookmarks" {...props} />);
    // Folder names also appear as <option>s in the folder pickers, so scope to the list's rows.
    const list = screen.getByRole("list", { name: "Bookmarks" });
    expect(await within(list).findByText("Docs")).toBeInTheDocument();
    expect(within(list).getByRole("button", { name: "Work" })).toBeInTheDocument();
    expect(within(list).queryByText("Wiki")).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Search bookmarks"), { target: { value: "wiki" } });
    expect(within(list).getByText("Wiki")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Search bookmarks"), { target: { value: "" } });

    fireEvent.change(screen.getByLabelText("Move Docs"), { target: { value: "f1" } });
    await waitFor(() => expect(calls).toContainEqual({ command: "cmd_browser_bookmark_move", args: { id: "b1", parentId: "f1", index: null } }));
    fireEvent.click(screen.getByRole("button", { name: "Delete Docs" }));
    await waitFor(() => expect(calls).toContainEqual({ command: "cmd_browser_bookmark_delete", args: { id: "b1" } }));
  });
});
