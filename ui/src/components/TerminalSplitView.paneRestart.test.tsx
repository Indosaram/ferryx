import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";
import { TerminalSplitView } from "./TerminalSplitView";

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-session-id={session.id}>
      <textarea data-testid="native-terminal-focus-sink" />
    </div>
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
    worktreePath: "/repo",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
  };
}

function splitLayout(): LayoutState {
  return {
    tabs: [tab("tab-1", "session-1")],
    activeTabId: "tab-1",
    layoutsByTabId: {
      "tab-1": {
        root: {
          type: "split",
          direction: "horizontal",
          first: { type: "leaf", leafId: "leaf-1" },
          second: { type: "leaf", leafId: "leaf-2" },
          ratio: 0.5,
        },
        activeLeafId: "leaf-1",
        expandedLeafId: null,
        sessionIdsByLeafId: {
          "leaf-1": "session-1",
          "leaf-2": "session-2",
        },
      },
    },
  };
}

const sessions = {
  "session-1": session("session-1"),
  "session-2": session("session-2"),
};

describe("TerminalSplitView pane restart", () => {
  it("restarts only the pane whose toolbar button was pressed", () => {
    const onRestartPane = vi.fn().mockResolvedValue(undefined);
    render(<TerminalSplitView layout={splitLayout()} sessions={sessions} onRestartPane={onRestartPane} />);

    const buttons = screen.getAllByTestId("pane-restart");
    expect(buttons).toHaveLength(2);

    fireEvent.click(buttons[1]);

    expect(onRestartPane).toHaveBeenCalledTimes(1);
    expect(onRestartPane).toHaveBeenCalledWith("session-2");
  });

  it("ignores repeat presses while the pane restart is still in flight", async () => {
    let finish: () => void = () => {};
    const onRestartPane = vi.fn().mockImplementation(
      () => new Promise<void>((resolve) => { finish = resolve; }),
    );
    render(<TerminalSplitView layout={splitLayout()} sessions={sessions} onRestartPane={onRestartPane} />);

    const button = screen.getAllByTestId("pane-restart")[0];
    fireEvent.click(button);
    fireEvent.click(button);

    expect(onRestartPane).toHaveBeenCalledTimes(1);
    await act(async () => { finish(); });
    await waitFor(() => expect(screen.getAllByTestId("pane-restart")[0].hasAttribute("disabled")).toBe(false));
  });

  it("keeps the pane mounted and offers Retry when the restart fails", async () => {
    const onRestartPane = vi.fn()
      .mockRejectedValueOnce(new Error("daemon unreachable"))
      .mockResolvedValueOnce(undefined);
    render(<TerminalSplitView layout={splitLayout()} sessions={sessions} onRestartPane={onRestartPane} />);

    fireEvent.click(screen.getAllByTestId("pane-restart")[0]);

    const banner = await screen.findByTestId("pane-restart-error");
    expect(banner.textContent).toContain("daemon unreachable");
    expect(screen.getAllByTestId("pane-leaf")).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(onRestartPane).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(screen.queryByTestId("pane-restart-error")).toBeNull());
  });
});
