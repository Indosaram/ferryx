import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { FileTab, SystemPermissionsStatus } from "../lib/types";
import { TabBar } from "./TabBar";

const nativeWindow = vi.hoisted(() => ({
  startDragging: vi.fn(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => nativeWindow,
}));

const nativeMenu = vi.hoisted(() => ({
  lastCall: null as null | {
    command: string;
    items: Array<Record<string, unknown>>;
    position: { x: number; y: number };
    onAction: (id: string) => void;
  },
}));

vi.mock("../lib/nativeMenu", () => ({
  openNativePopupMenu: vi.fn(
    async (
      command: string,
      items: Array<Record<string, unknown>>,
      position: { x: number; y: number },
      onAction: (id: string) => void,
    ) => {
      nativeMenu.lastCall = { command, items, position, onAction };
      return () => undefined;
    },
  ),
}));

const mockTauri = vi.hoisted(() => ({
  getSystemPermissionsStatus: vi.fn(),
}));

vi.mock("../lib/tauri", async () => {
  const actual = await vi.importActual<typeof import("../lib/tauri")>("../lib/tauri");
  return {
    ...actual,
    getSystemPermissionsStatus: () => mockTauri.getSystemPermissionsStatus(),
  };
});

const mockClipboard = vi.hoisted(() => ({
  copyTextToClipboard: vi.fn().mockResolvedValue(true),
}));

vi.mock("../lib/clipboard", () => ({
  copyTextToClipboard: (text: string) => mockClipboard.copyTextToClipboard(text),
}));

function menuItems() {
  if (!nativeMenu.lastCall) throw new Error("native menu was not opened");
  return nativeMenu.lastCall.items;
}

function findEntryRecursively(
  items: Array<Record<string, unknown>>,
  idOrLabel: string,
): { id: string; label: string; enabled?: boolean } | undefined {
  for (const item of items) {
    if (
      (item as { id?: string }).id === idOrLabel ||
      ((item as { label?: string }).label ?? "").includes(idOrLabel)
    ) {
      return item as { id: string; label: string; enabled?: boolean };
    }
    if (item.kind === "submenu" && Array.isArray(item.items)) {
      const found = findEntryRecursively(item.items as Array<Record<string, unknown>>, idOrLabel);
      if (found) return found;
    }
  }
  return undefined;
}

function menuItem(idOrLabel: string) {
  const items = menuItems();
  const found = findEntryRecursively(items, idOrLabel);
  if (!found) {
    throw new Error(`menu item not found: ${idOrLabel}; got ${JSON.stringify(items)}`);
  }
  return found;
}

function clickMenuItem(idOrLabel: string) {
  if (!nativeMenu.lastCall) throw new Error("native menu was not opened");
  const item = menuItem(idOrLabel);
  nativeMenu.lastCall.onAction(item.id);
}

function createFileTab(id = "tab-file-1", label = "index.ts", path = "/workspace/project/src/index.ts"): FileTab {
  return {
    kind: "file",
    id,
    label,
    path,
    backendSessionId: "backend-session-1",
    line: 12,
    col: 4,
    workspaceId: "workspace-1",
    previewId: "preview-file-1",
  };
}

describe("TabBar file tabs", () => {
  beforeEach(() => {
    nativeMenu.lastCall = null;
    mockClipboard.copyTextToClipboard.mockClear();
    mockTauri.getSystemPermissionsStatus.mockResolvedValue({
      platform: "darwin",
    } as SystemPermissionsStatus);
  });

  afterEach(() => {
    cleanup();
  });

  it("renders tab element with title equal to the absolute path when filePathsForTab is provided", () => {
    const fileTab = createFileTab("file-1", "index.ts", "src/index.ts");
    render(
      <TabBar
        tabs={[fileTab]}
        activeTabId="file-1"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
        filePathsForTab={() => ({
          absolute: "/workspace/project/src/index.ts",
          relative: "src/index.ts",
        })}
      />,
    );

    const tabElement = screen.getByRole("tab");
    expect(tabElement).toHaveAttribute("title", "/workspace/project/src/index.ts");
  });

  it("falls back to tab.path for title when filePathsForTab is not provided", () => {
    const fileTab = createFileTab("file-1", "index.ts", "/custom/path/index.ts");
    render(
      <TabBar
        tabs={[fileTab]}
        activeTabId="file-1"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    const tabElement = screen.getByRole("tab");
    expect(tabElement).toHaveAttribute("title", "/custom/path/index.ts");
  });

  it("includes ids for copy path, relative, reveal, open externally, and reload on right-click", () => {
    const fileTab = createFileTab("file-1", "index.ts", "/workspace/project/src/index.ts");
    const onReloadFileTab = vi.fn();
    const onOpenFileExternally = vi.fn();
    const onRevealFileTab = vi.fn();

    render(
      <TabBar
        tabs={[fileTab]}
        activeTabId="file-1"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
        filePathsForTab={() => ({
          absolute: "/workspace/project/src/index.ts",
          relative: "src/index.ts",
        })}
        onReloadFileTab={onReloadFileTab}
        onOpenFileExternally={onOpenFileExternally}
        onRevealFileTab={onRevealFileTab}
      />,
    );

    fireEvent.contextMenu(screen.getByRole("tab"));

    expect(menuItem("copy-path")).toBeDefined();
    expect(menuItem("copy-relative-path")).toBeDefined();
    expect(menuItem("reveal")).toBeDefined();
    expect(menuItem("open-externally")).toBeDefined();
    expect(menuItem("reload")).toBeDefined();
  });

  it("writes absolute path via mocked clipboard when triggering Copy Path", () => {
    const fileTab = createFileTab("file-1", "index.ts", "/workspace/project/src/index.ts");

    render(
      <TabBar
        tabs={[fileTab]}
        activeTabId="file-1"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
        filePathsForTab={() => ({
          absolute: "/workspace/project/src/index.ts",
          relative: "src/index.ts",
        })}
      />,
    );

    fireEvent.contextMenu(screen.getByRole("tab"));
    clickMenuItem("copy-path");

    expect(mockClipboard.copyTextToClipboard).toHaveBeenCalledWith(
      "/workspace/project/src/index.ts",
    );
  });

  it("disables relative item when relative path is null", () => {
    const fileTab = createFileTab("file-1", "hosts", "/etc/hosts");

    render(
      <TabBar
        tabs={[fileTab]}
        activeTabId="file-1"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
        filePathsForTab={() => ({
          absolute: "/etc/hosts",
          relative: null,
        })}
      />,
    );

    fireEvent.contextMenu(screen.getByRole("tab"));
    const relativeItem = menuItem("copy-relative-path");

    expect(relativeItem.enabled).toBe(false);
  });

  it("calls onRevealFileTab, onOpenFileExternally, and onReloadFileTab with tab id when clicked", () => {
    const fileTab = createFileTab("file-abc", "index.ts", "/workspace/project/src/index.ts");
    const onReloadFileTab = vi.fn();
    const onOpenFileExternally = vi.fn();
    const onRevealFileTab = vi.fn();

    render(
      <TabBar
        tabs={[fileTab]}
        activeTabId="file-abc"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
        filePathsForTab={() => ({
          absolute: "/workspace/project/src/index.ts",
          relative: "src/index.ts",
        })}
        onReloadFileTab={onReloadFileTab}
        onOpenFileExternally={onOpenFileExternally}
        onRevealFileTab={onRevealFileTab}
      />,
    );

    fireEvent.contextMenu(screen.getByRole("tab"));
    clickMenuItem("reveal");
    expect(onRevealFileTab).toHaveBeenCalledWith("file-abc");

    fireEvent.contextMenu(screen.getByRole("tab"));
    clickMenuItem("open-externally");
    expect(onOpenFileExternally).toHaveBeenCalledWith("file-abc");

    fireEvent.contextMenu(screen.getByRole("tab"));
    clickMenuItem("reload");
    expect(onReloadFileTab).toHaveBeenCalledWith("file-abc");
  });

  it("omits items when their callback/prop is not provided", () => {
    const fileTab = createFileTab("file-1", "index.ts", "/workspace/project/src/index.ts");

    render(
      <TabBar
        tabs={[fileTab]}
        activeTabId="file-1"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    fireEvent.contextMenu(screen.getByRole("tab"));
    const items = menuItems();

    expect(findEntryRecursively(items, "copy-path")).toBeUndefined();
    expect(findEntryRecursively(items, "copy-relative-path")).toBeUndefined();
    expect(findEntryRecursively(items, "reveal")).toBeUndefined();
    expect(findEntryRecursively(items, "open-externally")).toBeUndefined();
    expect(findEntryRecursively(items, "reload")).toBeUndefined();
  });

  it("disambiguates duplicate file tab basenames at render time and drops it after a close", () => {
    const first = createFileTab("file-a", "index.ts", "/a/src/index.ts");
    const second = createFileTab("file-b", "index.ts", "/b/ui/index.ts");
    const props = { onActivate: vi.fn(), onClose: vi.fn(), onAdd: vi.fn() };

    const { rerender } = render(<TabBar tabs={[first, second]} activeTabId="file-a" {...props} />);
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      expect.stringContaining("src/index.ts"),
      expect.stringContaining("ui/index.ts"),
    ]);

    rerender(<TabBar tabs={[second]} activeTabId="file-b" {...props} />);
    const [remaining] = screen.getAllByRole("tab");
    expect(remaining).toHaveTextContent("index.ts");
    expect(remaining).not.toHaveTextContent("ui/index.ts");
  });

  it("normalizes a stale persisted disambiguated label but keeps a user rename", () => {
    const stale = createFileTab("file-a", "src/index.ts", "/a/src/index.ts");
    const renamed = createFileTab("file-b", "Entry point", "/b/ui/index.ts");
    const props = { onActivate: vi.fn(), onClose: vi.fn(), onAdd: vi.fn() };

    render(<TabBar tabs={[stale, renamed]} activeTabId="file-a" {...props} />);
    const [staleTab, renamedTab] = screen.getAllByRole("tab");
    expect(staleTab).toHaveTextContent("index.ts");
    expect(staleTab).not.toHaveTextContent("src/index.ts");
    expect(renamedTab).toHaveTextContent("Entry point");
  });
});
