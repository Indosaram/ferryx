import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCoreMocks.invoke,
  isTauri: tauriCoreMocks.isTauri,
}));

// isTauri() is mocked true, so event subscriptions pass their runtime guard and
// would reach the real bridge, which has no __TAURI_INTERNALS__ under jsdom.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

class TestResizeObserver implements ResizeObserver {
  readonly observe = vi.fn();
  readonly unobserve = vi.fn();
  readonly disconnect = vi.fn();
  constructor(readonly callback: ResizeObserverCallback) {}
}

import { TerminalSplitView } from "./TerminalSplitView";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function session(id: string): TerminalSession {
  return {
    id,
    cwd: "/repo",
    worktreePath: "/repo",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
  };
}

function singleTabLayout(): LayoutState {
  const tab: TerminalTab = { id: "tab-main", label: "main", sessionId: "session-a" };
  return {
    tabs: [tab],
    activeTabId: tab.id,
    tabGroups: { g: { id: "g", tabIds: [tab.id], activeTabId: tab.id } },
    tabGroupLayout: { type: "group", groupId: "g" },
    focusedGroupId: "g",
    layoutsByTabId: {
      [tab.id]: {
        root: { type: "leaf", leafId: "leaf-a" },
        activeLeafId: "leaf-a",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-a": "session-a" },
      },
    },
  };
}

describe("pane handle reachability over a native terminal", () => {
  it("overlays the handle only inside the narrow hotspot without shrinking the terminal", () => {
    // Given: a full-height terminal with its handle hidden.
    vi.stubGlobal("ResizeObserver", TestResizeObserver);

    render(
      <TerminalSplitView
        layout={singleTabLayout()}
        sessions={{ "session-a": session("session-a") }}
      />,
    );

    const handle = screen.getByTestId("pane-toolbar");
    const terminal = screen.getByTestId("native-terminal-pane");
    const leaf = screen.getByTestId("pane-leaf");

    expect(handle).toHaveClass("h-3");
    expect(handle).toHaveClass("opacity-0", "pointer-events-none");
    expect(terminal.style.marginTop).toBe("");
    expect(terminal.style.height).toBe("");
    expect(terminal).toHaveClass("h-full");

    // When: the pointer reaches the last pixel of the 16px hotspot.
    fireEvent.mouseMove(leaf, { clientY: 16 });
    // Then: only the overlay changes; a handle press never reaches terminal mouse input.
    expect(handle).toHaveClass("opacity-100", "pointer-events-auto");
    expect(terminal.contains(handle)).toBe(false);
    tauriCoreMocks.invoke.mockClear();
    fireEvent.pointerDown(handle, { button: 0, pointerId: 1 });
    expect(tauriCoreMocks.invoke.mock.calls.some(([cmd]) => cmd === "cmd_native_terminal_mouse")).toBe(false);
    expect(terminal.style.marginTop).toBe("");
    expect(terminal.style.height).toBe("");

    fireEvent.mouseMove(leaf, { clientY: 17 });
    expect(handle).toHaveClass("opacity-0", "pointer-events-none");
  });

});
