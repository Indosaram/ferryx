import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserLibraryPanel } from "./BrowserLibraryPanel";
import type { DownloadRecord } from "../lib/browserTauri";

const calls = vi.hoisted(() => [] as Array<{ command: string; args: Record<string, unknown> }>);
const data = vi.hoisted(() => ({
  history: [] as unknown[],
  tree: { version: 1, folders: [] as unknown[], bookmarks: [] as unknown[] },
  downloads: [] as unknown[],
}));
const eventListeners = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args: Record<string, unknown> = {}) => {
    calls.push({ command, args });
    if (command === "cmd_browser_history_list") return data.history;
    if (command === "cmd_browser_bookmarks_get") return { tree: data.tree, recovery: null };
    if (command.startsWith("cmd_browser_bookmark")) return { tree: data.tree, recovery: null };
    if (command === "cmd_browser_download_list") return data.downloads;
    if (command === "cmd_browser_download_pause" || command === "cmd_browser_download_cancel") return true;
    return undefined;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, listener: (event: { payload: unknown }) => void) => {
    eventListeners.set(event, listener);
    return () => {
      if (eventListeners.get(event) === listener) eventListeners.delete(event);
    };
  }),
}));

beforeEach(() => {
  calls.length = 0;
  data.downloads = [];
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

describe("BrowserLibraryPanel downloads", () => {
  const download = (overrides: Partial<DownloadRecord>): DownloadRecord => ({
    id: "wv-1",
    url: "https://files.example/setup.exe",
    filePath: "/Users/test/Downloads/setup.exe",
    status: "inProgress",
    totalBytes: 100,
    receivedBytes: 25,
    createdAtMs: 1,
    updatedAtMs: 1,
    ...overrides,
  });

  it("lists downloads with only the controls each one supports", async () => {
    data.downloads = [
      download({ id: "wv-1", canPause: true, canCancel: true }),
      download({ id: "wv-2", url: "https://files.example/linux.tar", filePath: "", canPause: false, canCancel: true, createdAtMs: 2 }),
      download({ id: "wv-3", url: "https://files.example/fixed.bin", filePath: "", canPause: false, canCancel: false, createdAtMs: 3 }),
    ];
    render(<BrowserLibraryPanel view="downloads" {...props} />);
    expect(screen.getByRole("tab", { name: "downloads" })).toHaveAttribute("aria-selected", "true");

    const pausable = await screen.findByTestId("download-row-wv-1");
    expect(within(pausable).getByRole("button", { name: "Pause download" })).toBeInTheDocument();
    expect(within(pausable).getByRole("button", { name: "Cancel" })).toBeInTheDocument();

    const cancelOnly = screen.getByTestId("download-row-wv-2");
    expect(within(cancelOnly).queryByRole("button", { name: /download$/ })).not.toBeInTheDocument();
    expect(within(cancelOnly).getByRole("button", { name: "Cancel" })).toBeInTheDocument();

    const uncontrolled = screen.getByTestId("download-row-wv-3");
    expect(within(uncontrolled).queryByRole("button", { name: /download$/ })).not.toBeInTheDocument();
    expect(within(uncontrolled).queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
  });

  it("follows download updates and routes Pause, Resume and Cancel to the engine", async () => {
    data.downloads = [download({ canPause: true, canCancel: true })];
    render(<BrowserLibraryPanel view="downloads" {...props} />);
    const row = await screen.findByTestId("download-row-wv-1");

    fireEvent.click(within(row).getByRole("button", { name: "Pause download" }));
    await waitFor(() => expect(calls).toContainEqual({ command: "cmd_browser_download_pause", args: { downloadId: "wv-1", pause: true } }));

    const update = eventListeners.get("browser_download_updated");
    expect(update).toBeDefined();
    await act(async () => update?.({ payload: download({ status: "paused", canPause: true, canCancel: true, updatedAtMs: 2 }) }));
    expect(within(row).getByText("Paused")).toBeInTheDocument();
    fireEvent.click(within(row).getByRole("button", { name: "Resume download" }));
    await waitFor(() => expect(calls).toContainEqual({ command: "cmd_browser_download_pause", args: { downloadId: "wv-1", pause: false } }));

    fireEvent.click(within(row).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(calls).toContainEqual({ command: "cmd_browser_download_cancel", args: { downloadId: "wv-1" } }));
  });

  it("switches to the downloads tab", () => {
    const onViewChange = vi.fn();
    render(<BrowserLibraryPanel view="history" {...props} onViewChange={onViewChange} />);
    fireEvent.click(screen.getByRole("tab", { name: "downloads" }));
    expect(onViewChange).toHaveBeenCalledWith("downloads");
  });
});
