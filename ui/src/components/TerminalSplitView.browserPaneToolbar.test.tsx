import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab, LayoutState, TerminalSession, TerminalTab } from "../lib/types";

const browserMocks = vi.hoisted(() => ({
  BROWSER_SHORTCUT_EVENT: "ferryx:browser-shortcut",
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadUpdated: vi.fn(async () => () => undefined),
  extractBrowserErrorCode: vi.fn((err: unknown) => String((err as { code?: unknown })?.code ?? "UNKNOWN_ERROR")),
}));

vi.mock("../lib/browserTauri", () => browserMocks);

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-session-id={session.id} />
  ),
}));

import { TerminalSplitView } from "./TerminalSplitView";

const BROWSER_ID = "browser-pane-toolbar";

beforeEach(() => {
  browserMocks.setBrowserVisible.mockClear();
});

afterEach(() => {
  cleanup();
});

function browserLayout(): LayoutState {
  const tab: BrowserTab = {
    kind: "browser",
    id: "tab-browser",
    label: "Browser",
    browserId: BROWSER_ID,
    url: "https://example.com",
    loading: false,
    canGoBack: false,
    canGoForward: false,
  };
  return {
    tabs: [tab],
    activeTabId: tab.id,
    tabGroups: { g: { id: "g", tabIds: [tab.id], activeTabId: tab.id } },
    tabGroupLayout: { type: "group", groupId: "g" },
    focusedGroupId: "g",
    layoutsByTabId: {
      [tab.id]: {
        root: { type: "leaf", leafId: "leaf-browser" },
        activeLeafId: "leaf-browser",
        expandedLeafId: null,
        sessionIdsByLeafId: {},
      },
    },
  };
}

function terminalLayout(): LayoutState {
  const tab: TerminalTab = { id: "tab-terminal", label: "main", sessionId: "session-a" };
  return {
    tabs: [tab],
    activeTabId: tab.id,
    tabGroups: { g: { id: "g", tabIds: [tab.id], activeTabId: tab.id } },
    tabGroupLayout: { type: "group", groupId: "g" },
    focusedGroupId: "g",
    layoutsByTabId: {
      [tab.id]: {
        root: { type: "leaf", leafId: "leaf-terminal" },
        activeLeafId: "leaf-terminal",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-terminal": "session-a" },
      },
    },
  };
}

const terminalSession: TerminalSession = {
  id: "session-a",
  cwd: "/repo",
  worktreePath: "/repo",
  workspaceId: "ws-main",
  worktree: { wsId: "ws-main", slug: "main" },
  backendSessionId: "backend-session-a",
  lifecycle: "working",
};

describe("browser pane toolbar placement", () => {
  it("keeps browser content below the pane toolbar strip", () => {
    render(<TerminalSplitView layout={browserLayout()} sessions={{}} />);

    const toolbar = screen.getByTestId("pane-toolbar");
    const content = screen.getByTestId("pane-content");

    expect(content).toHaveAttribute("data-pane-content-kind", "browser");
    expect(content).toHaveAttribute("data-pane-toolbar-inset", "20");
    expect(content.style.paddingTop).toBe("20px");
    expect(content.contains(toolbar)).toBe(false);
  });

  it("does not offset terminal content", () => {
    render(<TerminalSplitView layout={terminalLayout()} sessions={{ "session-a": terminalSession }} />);

    const content = screen.getByTestId("pane-content");

    expect(content).toHaveAttribute("data-pane-content-kind", "terminal");
    expect(content).toHaveAttribute("data-pane-toolbar-inset", "0");
    expect(content.style.paddingTop).toBe("");
  });
});

describe("browser pane drag handle keyboard activation", () => {
  async function renderBrowserPane() {
    render(<TerminalSplitView layout={browserLayout()} sessions={{}} />);
    await waitFor(() => expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith(BROWSER_ID, true));
    browserMocks.setBrowserVisible.mockClear();
    return screen.getByTestId("pane-toolbar");
  }

  it("does not start a pane drag on Enter after a pointer press focused the handle", async () => {
    const handle = await renderBrowserPane();

    fireEvent.pointerDown(handle, { button: 0, pointerId: 1 });
    handle.focus();
    expect(document.activeElement).toBe(handle);
    fireEvent.keyDown(handle, { key: "Enter", code: "Enter" });
    fireEvent.keyDown(handle, { key: " ", code: "Space" });

    expect(screen.queryByTestId("workspace-drag-overlay")).toBeNull();
    expect(browserMocks.setBrowserVisible).not.toHaveBeenCalledWith(BROWSER_ID, false);
  });

  it("starts a pane drag on Enter when the handle was focused without a pointer", async () => {
    const handle = await renderBrowserPane();

    handle.focus();
    fireEvent.keyDown(handle, { key: "Enter", code: "Enter" });

    await waitFor(() => expect(screen.getByTestId("workspace-drag-overlay")).toHaveAttribute("data-preview-kind", "pane"));
    expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith(BROWSER_ID, false);
  });

  it("re-enables keyboard drag once the pointer-focused handle loses focus", async () => {
    const handle = await renderBrowserPane();

    fireEvent.pointerDown(handle, { button: 0, pointerId: 1 });
    handle.focus();
    handle.blur();
    handle.focus();
    fireEvent.keyDown(handle, { key: "Enter", code: "Enter" });

    await waitFor(() => expect(screen.getByTestId("workspace-drag-overlay")).toBeInTheDocument());
  });
});
