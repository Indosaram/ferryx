import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import dagRunSampleJson from "../state/__fixtures__/dagRunSample.json";
import type { TerminalActivity } from "../lib/activity";
import { parseDagRunSnapshot } from "../lib/dagTypes";
import type { TerminalSession } from "../lib/types";
import { dagStore } from "../state/dagStore";
import {
  NATIVE_TERMINAL_BOTTOM_INSET_PX,
  NATIVE_TERMINAL_HANDLE_INSET_PX,
} from "./NativeTerminalPane";
import { TerminalPane } from "./TerminalPane";

const dagSnapshot = parseDagRunSnapshot(dagRunSampleJson)!;

const agentActivity: TerminalActivity = {
  state: "working",
  title: "omo",
  isAgent: true,
  agentType: "omo",
};

vi.mock("./NativeTerminalPane", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./NativeTerminalPane")>();
  return {
    ...actual,
    NativeTerminalPane: vi.fn(({ sessionId, session }) => (
      <div
        data-testid="native-terminal-pane"
        data-session-id={sessionId}
        data-backend-id={session?.backendSessionId}
      />
    )),
  };
});

vi.mock("./TerminalSearchOverlay", () => ({
  TerminalSearchOverlay: vi.fn(({ sessionId, onClose }) => (
    <div data-testid="terminal-search-overlay" data-session-id={sessionId} onClick={onClose} />
  )),
}));

function createSession(id = "session-native"): TerminalSession {
  return {
    id,
    cwd: "/repo/main",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
  };
}

describe("TerminalPane native routing contract", () => {
  beforeEach(() => {
    dagStore.reset();
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("mounts NativeTerminalPane for active and inactive states and conditionally renders search overlay", () => {
    const session = createSession();
    const onCloseSearch = vi.fn();
    const { rerender } = render(
      <TerminalPane
        session={session}
        active={true}
        searchOpen={false}
        onCloseSearch={onCloseSearch}
      />,
    );

    expect(screen.getByTestId("native-terminal-pane")).toBeInTheDocument();
    expect(screen.queryByTestId("terminal-search-overlay")).toBeNull();

    rerender(
      <TerminalPane
        session={session}
        active={false}
        searchOpen={true}
        onCloseSearch={onCloseSearch}
      />,
    );

    expect(screen.getByTestId("native-terminal-pane")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-search-overlay")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-search-overlay")).toHaveAttribute("data-session-id", "backend-session-native");
  });

  it("binds the floating DAG badge to the terminal worktree rather than its nested cwd", () => {
    dagStore.applySnapshot("/repo/worktree", {
      ...dagSnapshot,
      runId: "worktree-dag",
      status: "running",
    });
    const session = {
      ...createSession(),
      cwd: "/repo/worktree/packages/ui",
      worktreePath: "/repo/worktree",
    };

    render(<TerminalPane session={session} active={true} activity={agentActivity} />);

    expect(screen.getByTestId("dag-pane-badge")).toBeInTheDocument();
  });

  it("keeps a sibling shell pane clean while its project has a running dag", () => {
    dagStore.applySnapshot("/repo/main", {
      ...dagSnapshot,
      runId: "main-dag-sibling",
      status: "running",
    });

    render(
      <TerminalPane
        session={createSession()}
        active={true}
        activity={{ ...agentActivity, isAgent: false, title: "zsh" }}
      />,
    );

    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("renders running DAG indicator as a DOM sibling positioned in the reserved bottom strip outside native terminal viewport", () => {
    dagStore.applySnapshot("/repo/main", {
      ...dagSnapshot,
      runId: "main-dag-running",
      status: "running",
    });
    const session = createSession();

    render(<TerminalPane session={session} active={true} activity={agentActivity} />);

    const surface = screen.getByTestId("terminal-pane-surface");
    const badge = screen.getByTestId("dag-pane-badge");
    const nativePane = screen.getByTestId("native-terminal-pane");

    // The badge must be a DOM sibling of the native terminal host under the surface container,
    // NOT rendered inside the native terminal's reported viewport where macOS child view paints over it
    expect(surface).toContainElement(badge);
    expect(surface).toContainElement(nativePane);
    expect(badge.parentElement).toBe(surface);
    expect(nativePane.parentElement).toBe(surface);
    expect(nativePane).not.toContainElement(badge);

    // Badge placement classes position it in the bottom strip reserved below the native terminal
    expect(badge).toHaveClass("absolute");
    expect(badge).toHaveClass("bottom-0");
    expect(badge).toHaveClass("right-5");

    // Verify native host reservation constants for handle inset and bottom strip
    expect(NATIVE_TERMINAL_HANDLE_INSET_PX).toBe(12);
    expect(NATIVE_TERMINAL_BOTTOM_INSET_PX).toBe(20);
    const totalReservedInsetPx = NATIVE_TERMINAL_HANDLE_INSET_PX + NATIVE_TERMINAL_BOTTOM_INSET_PX;
    expect(totalReservedInsetPx).toBe(32);
  });

  it("exposes role='dialog' on modal open so native terminal visibility mechanism yields the surface", () => {
    dagStore.applySnapshot("/repo/main", {
      ...dagSnapshot,
      runId: "main-dag-popover",
      status: "running",
    });
    const session = createSession();

    render(<TerminalPane session={session} active={true} activity={agentActivity} />);

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.querySelector('[role="dialog"]')).toBeNull();

    const badgeButton = screen.getByTestId("dag-pane-badge-button");
    fireEvent.click(badgeButton);

    const modal = screen.getByRole("dialog", { name: /dag runs/i });
    expect(modal).toBeInTheDocument();
    expect(screen.getByTestId("dag-pane-modal")).toBe(modal);
    expect(document.querySelector('[role="dialog"]')).toBe(modal);

    // Closing the modal dismisses the dialog role
    const backdrop = screen.getByTestId("dag-pane-modal-backdrop");
    fireEvent.click(backdrop);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });
});
