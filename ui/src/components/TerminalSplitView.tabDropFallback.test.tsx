import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";
import { TerminalSplitView } from "./TerminalSplitView";

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-session-id={session.id} />
  ),
}));

afterEach(cleanup);

function tab(id: string, sessionId: string): TerminalTab {
  return { id, label: id, sessionId };
}

function session(id: string): TerminalSession {
  return {
    id,
    cwd: "/repo",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
  };
}

function plainTextOnlyTransfer(tabId: string) {
  return {
    setData: vi.fn(),
    getData: vi.fn((type: string) => (type === "text/plain" ? tabId : "")),
    get types() {
      return ["text/plain"];
    },
    effectAllowed: "move",
    dropEffect: "",
  };
}

describe("TerminalSplitView legacy HTML5 tab drop", () => {
  it("ignores text/plain HTML5 tab drops because runtime tab splitting is pointer/group based", () => {
    const layout: LayoutState = {
      tabs: [tab("tab-target", "session-target"), tab("tab-source", "session-source")],
      activeTabId: "tab-target",
      layoutsByTabId: {
        "tab-target": {
          root: { type: "leaf", leafId: "leaf-target" },
          activeLeafId: "leaf-target",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-target": "session-target" },
        },
        "tab-source": {
          root: { type: "leaf", leafId: "leaf-source" },
          activeLeafId: "leaf-source",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-source": "session-source" },
        },
      },
    };
    const onSplitPane = vi.fn();
    render(
      <TerminalSplitView
        layout={layout}
        sessions={{
          "session-target": session("session-target"),
          "session-source": session("session-source"),
        }}
        onSplitPane={onSplitPane}
        onReorderTab={vi.fn()}
      />,
    );

    const pane = screen.getByTestId("pane-leaf");
    const transfer = plainTextOnlyTransfer("tab-source");
    fireEvent.dragOver(pane, { clientX: 195, clientY: 50, dataTransfer: transfer });
    fireEvent.drop(pane, { clientX: 195, clientY: 50, dataTransfer: transfer });

    expect(screen.queryByTestId("pane-drop-preview")).not.toBeInTheDocument();
    expect(onSplitPane).not.toHaveBeenCalled();
  });
});
