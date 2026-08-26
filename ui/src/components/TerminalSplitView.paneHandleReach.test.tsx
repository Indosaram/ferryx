import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { NATIVE_TERMINAL_HANDLE_INSET_PX } from "./NativeTerminalPane";
import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCoreMocks.invoke,
  isTauri: tauriCoreMocks.isTauri,
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
  it("keeps the handle strip outside the terminal surface box", () => {
    // The native terminal focuses its PTY from `onPointerDown` and calls
    // preventDefault(). If its box still covered the handle strip, that press
    // would cancel the gesture before dnd-kit's distance threshold, so the
    // handle would hover but never drag. The reservation must therefore keep
    // the strip out of the terminal's own box.
    vi.stubGlobal("ResizeObserver", TestResizeObserver);

    render(
      <TerminalSplitView
        layout={singleTabLayout()}
        sessions={{ "session-a": session("session-a") }}
      />,
    );

    const handle = screen.getByTestId("pane-toolbar");
    const terminal = screen.getByTestId("native-terminal-pane");

    // The handle row and the reserved strip are the same height, so the
    // terminal starting exactly below the strip leaves the handle uncovered.
    expect(handle).toHaveClass("h-3");
    expect(terminal.style.marginTop).toBe(`${NATIVE_TERMINAL_HANDLE_INSET_PX}px`);
    expect(terminal.style.height).toBe(
      `calc(100% - ${NATIVE_TERMINAL_HANDLE_INSET_PX}px)`,
    );

    // The handle is a sibling of the terminal, so a press on it must not be
    // observable by the terminal's pointer handler at all.
    expect(terminal.contains(handle)).toBe(false);
  });

});
