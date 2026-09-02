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

const parsedDagSnapshot = parseDagRunSnapshot(dagRunSampleJson);
if (!parsedDagSnapshot) throw new TypeError("DAG test fixture is invalid");
const dagSnapshot = parsedDagSnapshot;

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

function createExitedSession(overrides: Partial<TerminalSession> = {}): TerminalSession {
  return {
    ...createSession("session-exited"),
    backendSessionId: null,
    lifecycle: "exited",
    agentType: "omo",
    providerSession: { key: "session_id", id: "provider-omo-1" },
    ...overrides,
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

  it("calls reconnect once with the local session id and disables repeat activation while pending", () => {
    // Given: an exited Omo session with an authoritative provider reference.
    const onReconnect = vi.fn(() => new Promise<void>(() => undefined));
    render(<TerminalPane session={createExitedSession()} active={true} onReconnect={onReconnect} />);

    expect(screen.getByText("OMO", { exact: true })).toBeInTheDocument();
    expect(screen.getByRole("presentation")).toHaveAttribute("src", expect.stringContaining("svg"));

    // When: reconnect is activated by the keyboard-generated click contract, then clicked again.
    const button = screen.getByRole("button", { name: /Reconnect OMO session/i });
    fireEvent.click(button, { detail: 0 });
    fireEvent.click(button);

    // Then: one callback receives the frontend-local id and progress prevents repeats.
    expect(onReconnect).toHaveBeenCalledOnce();
    expect(onReconnect).toHaveBeenCalledWith("session-exited");
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button).toHaveAccessibleName(/Reconnecting OMO session/i);
    expect(screen.getByText("Reconnecting session...", { exact: true })).toBeInTheDocument();
  });

  it("renders deterministic conflict copy and never calls reconnect for a duplicate active claim", () => {
    // Given: another live pane owns the same normalized provider session.
    const exited = createExitedSession();
    const live = createExitedSession({
      id: "session-live-owner",
      backendSessionId: "backend-live-owner",
      lifecycle: "working",
    });
    const onReconnect = vi.fn();

    // When: the exited pane derives its reconnect affordance from both sessions.
    render(
      <TerminalPane
        session={exited}
        sessions={{ [exited.id]: exited, [live.id]: live }}
        active={true}
        onReconnect={onReconnect}
      />,
    );

    // Then: conflict copy explicitly identifies the other pane and no reconnect button exists.
    expect(screen.getByText("This session is already active in another pane (session-live-owner).")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Reconnect OMO session/i })).toBeNull();
    expect(onReconnect).not.toHaveBeenCalled();
  });

  it("renders deterministic unavailable copy for missing and unsupported agent references with no action", () => {
    // Given: exited panes that cannot produce a provider-native resume request.
    const { rerender } = render(
      <TerminalPane
        session={createExitedSession({ providerSession: null, agentSessionId: null })}
        active={true}
        onReconnect={vi.fn()}
      />,
    );

    // Then: a missing reference is distinguished from unsupported resume behavior and shows no action.
    expect(screen.getByText("Session reference unavailable.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /reconnect/i })).toBeNull();
    rerender(
      <TerminalPane
        session={createExitedSession({ agentType: "unknown-agent" })}
        active={true}
        onReconnect={vi.fn()}
      />,
    );
    expect(screen.getByText(/sessions cannot be reconnected/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /reconnect/i })).toBeNull();
  });

  it("shows deterministic typed failure copy and exposes retry", () => {
    // Given: a reconnect attempt rejected an invalid provider reference.
    const onReconnect = vi.fn();
    render(
      <TerminalPane
        session={createExitedSession({
          reconnectLifecycle: "failed",
          reconnectError: { code: "AGENT_RESUME_INVALID", message: "backend wording is not UI copy" },
        })}
        active={true}
        onReconnect={onReconnect}
      />,
    );

    // When: retry is activated.
    expect(screen.getByText("This session has an invalid reconnect reference.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Retry OMO session/i }));

    // Then: retry targets the same frontend-local session.
    expect(onReconnect).toHaveBeenCalledOnce();
    expect(onReconnect).toHaveBeenCalledWith("session-exited");
  });

  it.each(["validating", "spawning", "binding"] as const)(
    "shows disabled reconnect progress while %s",
    (reconnectLifecycle) => {
      // Given: the reconnect transaction is in a non-idle phase.
      const onReconnect = vi.fn();
      render(
        <TerminalPane
          session={createExitedSession({ reconnectLifecycle })}
          active={true}
          onReconnect={onReconnect}
        />,
      );

      // Then: progress is announced and repeat activation is blocked.
      const button = screen.getByRole("button", { name: /Reconnecting OMO session/i });
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute("aria-busy", "true");
      expect(screen.getByText("Reconnecting session...", { exact: true })).toBeInTheDocument();
      fireEvent.click(button);
      expect(onReconnect).not.toHaveBeenCalled();
    },
  );

  it("never renders a reconnect surface over a live native terminal", () => {
    // Given: a live backend session that can paint a native surface.
    render(
      <TerminalPane
        session={{
          ...createSession(),
          agentType: "omo",
          providerSession: { key: "session_id", id: "provider-live" },
        }}
        active={true}
        onReconnect={vi.fn()}
      />,
    );

    // Then: the native pane remains and no disconnected action overlays it.
    expect(screen.getByTestId("native-terminal-pane")).toBeInTheDocument();
    expect(screen.queryByText("Session disconnected")).toBeNull();
    expect(screen.queryByRole("button", { name: /reconnect/i })).toBeNull();
  });

  it("distinguishes an exited shell replacement from agent resume", () => {
    // Given: an ordinary shell whose backend exited.
    const onOpenNewShell = vi.fn();
    render(
      <TerminalPane
        session={createExitedSession({ agentType: null, providerSession: null })}
        active={true}
        onOpenNewShell={onOpenNewShell}
      />,
    );

    // When: the user explicitly requests a fresh shell using keyboard activation.
    fireEvent.click(screen.getByRole("button", { name: "Open new shell" }), { detail: 0 });

    // Then: the callback receives the local pane session id and resume is never implied.
    expect(onOpenNewShell).toHaveBeenCalledOnce();
    expect(onOpenNewShell).toHaveBeenCalledWith("session-exited");
    expect(screen.queryByRole("button", { name: /reconnect/i })).toBeNull();
    expect(screen.getByText("Shell exited")).toBeInTheDocument();
  });

  it("keeps an exited shell action unavailable when the application did not wire replacement", () => {
    render(
      <TerminalPane
        session={createExitedSession({ agentType: null, providerSession: null })}
        active={true}
      />,
    );

    expect(screen.getByRole("button", { name: "Open new shell" })).toBeDisabled();
  });

  it("shows Session disconnected when session.agentType is null but session.providerSession is present", () => {
    render(
      <TerminalPane
        session={createExitedSession({
          agentType: null,
          providerSession: { key: "session_id", id: "omo-provider-1" },
        })}
        active={true}
      />,
    );

    expect(screen.getByText("Session disconnected")).toBeInTheDocument();
    expect(screen.queryByText("Shell exited")).toBeNull();
  });

  it("shows Session disconnected when session.agentType is null but activity identifies an agent", () => {
    render(
      <TerminalPane
        session={createExitedSession({
          agentType: null,
          providerSession: null,
        })}
        activity={{
          state: "done",
          title: "Claude turn",
          isAgent: true,
          agentType: "claude",
        }}
        active={true}
      />,
    );

    expect(screen.getByText("Session disconnected")).toBeInTheDocument();
    expect(screen.getByText("Claude Code")).toBeInTheDocument();
    expect(screen.queryByText("Shell exited")).toBeNull();
  });
});
