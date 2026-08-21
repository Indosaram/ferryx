import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, SplitMode, TerminalSession, TerminalTab } from "../lib/types";
import { TerminalSplitView } from "./TerminalSplitView";

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-backend-session-id={session.backendSessionId ?? ""} />
  ),
}));

afterEach(cleanup);

function tab(id: string, sessionId: string): TerminalTab {
  return { id, label: id, sessionId };
}

function session(id: string, backendSessionId: string): TerminalSession {
  return {
    id,
    cwd: "/repo",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId,
    lifecycle: "working",
  };
}

function splitFixture(split: Exclude<SplitMode, "none">) {
  const primary = tab("tab-1", "session-1");
  const secondary = tab("tab-2", "session-2");
  const layout: LayoutState = {
    tabs: [primary, secondary],
    primaryTabId: primary.id,
    secondaryTabId: secondary.id,
    split,
  };
  const sessions = {
    "session-1": session("session-1", "backend-1"),
    "session-2": session("session-2", "backend-2"),
  };
  return { layout, sessions };
}

function mockLayoutBounds(width = 800, height = 800) {
  const layout = screen.getByTestId("terminal-layout");
  vi.spyOn(layout, "getBoundingClientRect").mockReturnValue({
    width,
    height,
    left: 0,
    top: 0,
    right: width,
    bottom: height,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
  return layout;
}

describe("TerminalSplitView", () => {
  it("renders a mirror pane immediately for a single-tab split state", () => {
    const primary = tab("tab-1", "session-1");
    const layout: LayoutState = {
      tabs: [primary],
      primaryTabId: primary.id,
      secondaryTabId: primary.id,
      split: "horizontal",
    };
    const sessions = { "session-1": session("session-1", "backend-1") };

    render(<TerminalSplitView layout={layout} sessions={sessions} />);

    expect(screen.getByTestId("primary-pane")).toBeInTheDocument();
    expect(screen.getByTestId("secondary-pane")).toBeInTheDocument();
    expect(screen.getAllByTestId("terminal-pane")).toHaveLength(2);
    expect(screen.getAllByTestId("terminal-pane")[0]).toHaveAttribute("data-backend-session-id", "backend-1");
    expect(screen.getAllByTestId("terminal-pane")[1]).toHaveAttribute("data-backend-session-id", "backend-1");
  });

  it("drags a horizontal split divider and clamps both panes to the minimum size", () => {
    const { layout, sessions } = splitFixture("horizontal");
    render(<TerminalSplitView layout={layout} sessions={sessions} />);
    mockLayoutBounds();

    const divider = screen.getByRole("separator", { name: "Resize terminal panes" });
    const primaryPane = screen.getByTestId("primary-pane");
    const secondaryPane = screen.getByTestId("secondary-pane");
    expect(divider).toHaveAttribute("aria-orientation", "vertical");

    fireEvent.pointerDown(divider, { clientX: 400, clientY: 400 });
    fireEvent.pointerMove(window, { clientX: 80, clientY: 400 });

    expect(primaryPane).toHaveStyle({ flexBasis: "20%" });
    expect(secondaryPane).toHaveStyle({ flexBasis: "80%" });
    fireEvent.pointerUp(window);
  });

  it("drags a vertical split divider and clamps both panes to the minimum size", () => {
    const { layout, sessions } = splitFixture("vertical");
    render(<TerminalSplitView layout={layout} sessions={sessions} />);
    mockLayoutBounds();

    const divider = screen.getByRole("separator", { name: "Resize terminal panes" });
    const primaryPane = screen.getByTestId("primary-pane");
    const secondaryPane = screen.getByTestId("secondary-pane");
    expect(divider).toHaveAttribute("aria-orientation", "horizontal");

    fireEvent.pointerDown(divider, { clientX: 400, clientY: 400 });
    fireEvent.pointerMove(window, { clientX: 400, clientY: 760 });

    expect(primaryPane).toHaveStyle({ flexBasis: "80%" });
    expect(secondaryPane).toHaveStyle({ flexBasis: "20%" });
    fireEvent.pointerUp(window);
  });
});
