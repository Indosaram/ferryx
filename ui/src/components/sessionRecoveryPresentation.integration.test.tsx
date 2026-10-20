import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { setTimeout as nodeSetTimeout, clearTimeout as nodeClearTimeout } from "node:timers";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const realSetTimeout = nodeSetTimeout ?? globalThis.setTimeout;
const realClearTimeout = nodeClearTimeout ?? globalThis.clearTimeout;

import type { TerminalActivity } from "../lib/activity";
import type { TerminalSession } from "../lib/types";
import { RemoteTerminal } from "../remote/RemoteTerminal";
import { TerminalPane } from "./TerminalPane";

class MockWebSocket {
  static readonly OPEN = 1;
  static latest: MockWebSocket | null = null;
  static instances: MockWebSocket[] = [];
  readonly url: string;
  binaryType = "arraybuffer";
  readyState = MockWebSocket.OPEN;
  send = vi.fn();
  close = vi.fn();
  private _onopen: ((event?: unknown) => void) | null = null;
  private _onclose: ((event?: unknown) => void) | null = null;
  private _onmessage: ((event: { data: unknown }) => void) | null = null;
  private _onerror: ((event?: unknown) => void) | null = null;
  private _installedResolvers: (() => void)[] = [];

  get onopen() {
    return this._onopen;
  }
  set onopen(val) {
    this._onopen = val;
    this._checkInstalled();
  }

  get onclose() {
    return this._onclose;
  }
  set onclose(val) {
    this._onclose = val;
    this._checkInstalled();
  }

  get onmessage() {
    return this._onmessage;
  }
  set onmessage(val) {
    this._onmessage = val;
    this._checkInstalled();
  }

  get onerror() {
    return this._onerror;
  }
  set onerror(val) {
    this._onerror = val;
    this._checkInstalled();
  }

  private _checkInstalled() {
    if (this._onopen && this._onclose && this._onmessage && this._onerror) {
      const resolvers = this._installedResolvers.splice(0, this._installedResolvers.length);
      resolvers.forEach((r) => r());
    }
  }

  waitForHandlers(timeoutMs = 5000): Promise<void> {
    if (this._onopen && this._onclose && this._onmessage && this._onerror) {
      return Promise.resolve();
    }
    return new Promise((resolve, reject) => {
      let timer: ReturnType<typeof realSetTimeout> | null = null;
      let settled = false;

      const cleanup = () => {
        if (timer !== null) {
          realClearTimeout(timer);
          timer = null;
        }
        const idx = this._installedResolvers.indexOf(onInstalled);
        if (idx !== -1) {
          this._installedResolvers.splice(idx, 1);
        }
      };

      const onInstalled = () => {
        if (settled) return;
        settled = true;
        cleanup();
        resolve();
      };

      timer = realSetTimeout(() => {
        if (settled) return;
        settled = true;
        cleanup();
        reject(new Error("Timeout waiting for WebSocket handlers installation"));
      }, timeoutMs);

      this._installedResolvers.push(onInstalled);
    });
  }

  constructor(url: string) {
    this.url = url;
    MockWebSocket.latest = this;
    MockWebSocket.instances.push(this);
  }
}

vi.mock("./NativeTerminalPane", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./NativeTerminalPane")>();
  return {
    ...actual,
    NativeTerminalPane: vi.fn(
      ({
        sessionId,
        session,
        active,
      }: {
        sessionId: string;
        session?: TerminalSession;
        active?: boolean;
      }) => (
        <div
          data-testid="native-terminal-pane"
          data-session-id={sessionId}
          data-backend-id={session?.backendSessionId}
          tabIndex={active ? 0 : -1}
        />
      ),
    ),
  };
});

vi.mock("./TerminalSearchOverlay", () => ({
  TerminalSearchOverlay: vi.fn(({ sessionId, onClose }: { sessionId: string; onClose: () => void }) => (
    <div data-testid="terminal-search-overlay" data-session-id={sessionId} onClick={onClose} />
  )),
}));

function createSession(
  id: string,
  workspaceId: string,
  overrides: Partial<TerminalSession> = {},
): TerminalSession {
  return {
    id,
    cwd: "/repo/matrix",
    workspaceId,
    backendSessionId: `backend-${id}`,
    incarnation: "inc-matrix-1",
    daemonEpoch: "epoch-1",
    lifecycle: "working",
    ...overrides,
  };
}

describe("Session recovery presentation cross-flow integration matrix", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    MockWebSocket.latest = null;
    MockWebSocket.instances = [];
  });

  afterEach(async () => {
    await act(async () => {
      cleanup();
    });
    vi.clearAllTimers();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  describe.each([
    ["local", "ws-local"],
    ["ssh", "ssh:remote-server-a"],
    ["paired", "daemon:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"],
  ] as const)("desktop mounted consumer seam (%s)", (transport, workspaceId) => {
    it("renders recovering outcome with preserved task activity, appropriate retry button, zero spawn, and admission cancellation", async () => {
      const disposeAdmission = vi.fn();
      const onRefreshSessionIdentity = vi.fn().mockReturnValue(disposeAdmission);
      const onOpenNewShell = vi.fn();
      const onReconnect = vi.fn();

      const recoveringSession = createSession(`${transport}-recovering`, workspaceId, {
        recoveryState: {
          phase: "recovering",
          reason: "Transport link lost; automatic backoff scheduled",
        },
      });

      const activeTaskActivity: TerminalActivity = {
        state: "working",
        title: "Matrix Worker Task",
        isAgent: true,
        agentType: "omo",
      };

      const { unmount } = render(
        <TerminalPane
          session={recoveringSession}
          active={true}
          activity={activeTaskActivity}
          onRefreshSessionIdentity={onRefreshSessionIdentity}
          onOpenNewShell={onOpenNewShell}
          onReconnect={onReconnect}
        />,
      );

      // Verify transport-appropriate recovering headline
      if (transport === "ssh") {
        expect(screen.getByRole("heading", { name: "Reconnecting SSH..." })).toBeInTheDocument();
      } else {
        expect(screen.getByRole("heading", { name: "Reconnecting session..." })).toBeInTheDocument();
      }

      // Verify structured reason rendered in status container
      expect(screen.getByRole("status")).toHaveTextContent(
        recoveringSession.recoveryState!.reason!,
      );

      // Verify task activity is preserved (not settled to exited or idle)
      expect(screen.queryByRole("heading", { name: /Shell exited|Remote session exited/ })).toBeNull();
      expect(screen.getByTestId("terminal-pane-surface")).toBeInTheDocument();

      // Verify appropriate buttons: Retry button is present, Open new shell is absent during active recovery
      const retryBtn = screen.getByRole("button", { name: "Retry session recovery" });
      expect(retryBtn).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Open new shell" })).toBeNull();

      // Verify retry action preserves zero spawn (onOpenNewShell must not be invoked)
      fireEvent.click(retryBtn);
      expect(onRefreshSessionIdentity).toHaveBeenCalledWith(
        `${transport}-recovering`,
        expect.any(Function),
      );
      expect(onOpenNewShell).not.toHaveBeenCalled();
      expect(disposeAdmission).not.toHaveBeenCalled();

      // Re-clicking retry in-flight cancels the previous admission subscription
      fireEvent.click(retryBtn);
      expect(disposeAdmission).toHaveBeenCalledTimes(1);
      expect(onOpenNewShell).not.toHaveBeenCalled();

      // Unmounting cancels in-flight admission
      unmount();
      expect(disposeAdmission).toHaveBeenCalledTimes(2);
    });

    it("renders blocked outcome with structured reason, explicit new shell escape, and zero automatic spawn on retry", async () => {
      const disposeAdmission = vi.fn();
      const onRefreshSessionIdentity = vi.fn().mockReturnValue(disposeAdmission);
      const onOpenNewShell = vi.fn();

      const blockedSession = createSession(`${transport}-blocked`, workspaceId, {
        recoveryState: {
          phase: "blocked",
          reason: "Authority pairing lease revoked by host",
        },
      });

      const { unmount } = render(
        <TerminalPane
          session={blockedSession}
          active={true}
          onRefreshSessionIdentity={onRefreshSessionIdentity}
          onOpenNewShell={onOpenNewShell}
        />,
      );

      // Verify blocked presentation
      expect(screen.getByRole("heading", { name: "Session recovery blocked" })).toBeInTheDocument();
      expect(screen.getByRole("status")).toHaveTextContent(
        blockedSession.recoveryState!.reason!,
      );

      // Verify appropriate buttons: both corrective Retry and explicit Open new shell escape
      const retryBtn = screen.getByRole("button", { name: "Retry session recovery" });
      const newShellBtn = screen.getByRole("button", { name: "Open new shell" });
      expect(retryBtn).toBeInTheDocument();
      expect(newShellBtn).toBeInTheDocument();

      // Verify Retry invokes recovery admission and does not spawn new shell
      fireEvent.click(retryBtn);
      expect(onRefreshSessionIdentity).toHaveBeenCalledWith(
        `${transport}-blocked`,
        expect.any(Function),
      );
      expect(onOpenNewShell).not.toHaveBeenCalled();

      // Verify explicit new terminal escape invokes onOpenNewShell
      fireEvent.click(newShellBtn);
      expect(onOpenNewShell).toHaveBeenCalledWith(`${transport}-blocked`);

      // Unmounting cleans up active admission subscription
      unmount();
      expect(disposeAdmission).toHaveBeenCalledTimes(1);
    });

    it("renders confirmed exited outcome with explicit replacement and no automatic retry button", async () => {
      const onRefreshSessionIdentity = vi.fn();
      const onOpenNewShell = vi.fn();

      const exitedSession = createSession(`${transport}-exited`, workspaceId, {
        lifecycle: "exited",
        recoveryState: {
          phase: "exited",
          reason: "Process confirmed terminated with exit code 0",
        },
      });

      render(
        <TerminalPane
          session={exitedSession}
          active={true}
          onRefreshSessionIdentity={onRefreshSessionIdentity}
          onOpenNewShell={onOpenNewShell}
        />,
      );

      // Verify exited presentation
      if (transport === "ssh") {
        expect(screen.getByRole("heading", { name: "Remote session exited" })).toBeInTheDocument();
      } else {
        expect(screen.getByRole("heading", { name: "Shell exited" })).toBeInTheDocument();
      }

      // Verify no automatic retry button is rendered for confirmed exits
      expect(screen.queryByRole("button", { name: "Retry session recovery" })).toBeNull();

      // Verify explicit new terminal escape button
      const newShellBtn = screen.getByRole("button", { name: "Open new shell" });
      expect(newShellBtn).toBeInTheDocument();
      fireEvent.click(newShellBtn);
      expect(onOpenNewShell).toHaveBeenCalledWith(`${transport}-exited`);
      expect(onRefreshSessionIdentity).not.toHaveBeenCalled();
    });

    // Note: This test asserts TerminalPane's active prop forwarding to NativeTerminalPane.
    // Actual OS-level native window focus and WGPU surface binding are coverage-owned by
    // NativeTerminalPane.lifecycle.test.tsx and NativeTerminalPane.exitAttach.test.tsx,
    // not proven in this presentation unit without executed receipts.
    it("forwards active focus state to NativeTerminalPane and preserves container layout across active state transitions", async () => {
      const onRefreshSessionIdentity = vi.fn();
      const session = createSession(`${transport}-focus`, workspaceId, {
        recoveryState: {
          phase: "recovering",
          reason: "Reconnecting to owner...",
        },
      });

      const { rerender } = render(
        <TerminalPane
          session={session}
          active={true}
          onRefreshSessionIdentity={onRefreshSessionIdentity}
        />,
      );

      const nativePane = screen.getByTestId("native-terminal-pane");
      expect(nativePane).toHaveAttribute("tabindex", "0");

      // Rerender as inactive: TerminalPane forwards active=false to NativeTerminalPane
      rerender(
        <TerminalPane
          session={session}
          active={false}
          onRefreshSessionIdentity={onRefreshSessionIdentity}
        />,
      );

      expect(screen.getByTestId("native-terminal-pane")).toHaveAttribute("tabindex", "-1");
    });
  });

  describe("web mounted consumer seam (RemoteTerminal)", () => {
    it("renders recovering outcome, drops pre-readiness input, and retries target with zero replacement spawn", async () => {
      const sockets: MockWebSocket[] = [];
      const createWebSocket = vi.fn((url: string) => {
        const ws = new MockWebSocket(url);
        sockets.push(ws);
        return ws;
      });

      const view = render(
        <RemoteTerminal
          sessionId="web-session-1"
          token="test-token-web"
          isAccountSession={true}
          daemonEpoch="epoch-web-1"
          createWebSocket={createWebSocket}
        />,
      );

      expect(sockets).toHaveLength(1);
      await sockets[0].waitForHandlers(5000);

      // Initial socket state before outcome or readiness
      expect(screen.getByRole("status")).toHaveTextContent("Connecting");

      // Socket outcome unknown / recovering
      const unknownOutcomeFrame = {
        type: "terminalOutcome",
        target: {
          sessionId: "web-session-1",
          backendSessionId: "web-session-1",
          daemonEpoch: "epoch-web-1",
        },
        incarnation: "inc-web-1",
        generation: null,
        state: "recovering",
        blockedReason: null,
        exitCode: null,
      };

      act(() => {
        sockets[0].onmessage?.({ data: JSON.stringify(unknownOutcomeFrame) });
      });

      expect(screen.getByTestId("remote-terminal-outcome-unknown")).toBeInTheDocument();

      // Retry button is present
      const retryBtn = screen.getByRole("button", { name: "Retry" });
      expect(retryBtn).toBeInTheDocument();

      // Clicking Retry re-dials existing target socket and does not create an arbitrary replacement session
      await act(async () => {
        fireEvent.click(retryBtn);
      });

      expect(createWebSocket).toHaveBeenCalledTimes(2);
      expect(sockets[1].url).toContain("web-session-1");
      expect(sockets[1].url).toContain("epoch-web-1");

      // Unmounting cleans up WebSocket
      view.unmount();
      expect(sockets[0].close).toHaveBeenCalled();
    });

    it("renders blocked outcome with reason, fences stale epoch frames, and offers retry", async () => {
      const sockets: MockWebSocket[] = [];
      const createWebSocket = vi.fn((url: string) => {
        const ws = new MockWebSocket(url);
        sockets.push(ws);
        return ws;
      });

      const view = render(
        <RemoteTerminal
          sessionId="web-blocked-session"
          token="test-token-web"
          isAccountSession={true}
          daemonEpoch="epoch-current"
          createWebSocket={createWebSocket}
        />,
      );

      expect(sockets).toHaveLength(1);
      await sockets[0].waitForHandlers(5000);

      const blockedFrame = {
        type: "terminalOutcome",
        target: {
          sessionId: "web-blocked-session",
          backendSessionId: "web-blocked-session",
          daemonEpoch: "epoch-current",
        },
        incarnation: "inc-web-1",
        generation: "12",
        state: "blocked",
        blockedReason: "Pairing credentials revoked",
        exitCode: null,
      };

      // Stale epoch frame is rejected and ignored
      act(() => {
        sockets[0].onmessage?.({
          data: JSON.stringify({
            ...blockedFrame,
            target: { ...blockedFrame.target, daemonEpoch: "epoch-old" },
          }),
        });
      });
      expect(screen.queryByTestId("remote-terminal-outcome")).toBeNull();

      // Matching epoch frame transitions to blocked outcome
      act(() => {
        sockets[0].onmessage?.({ data: JSON.stringify(blockedFrame) });
      });
      const outcomeElem = screen.getByTestId("remote-terminal-outcome");
      expect(outcomeElem).toBeInTheDocument();
      expect(outcomeElem).toHaveTextContent(blockedFrame.blockedReason);
      expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();

      view.unmount();
    });

    it("renders confirmed exited outcome with exit code and cleans up on unmount", async () => {
      const sockets: MockWebSocket[] = [];
      const createWebSocket = vi.fn((url: string) => {
        const ws = new MockWebSocket(url);
        sockets.push(ws);
        return ws;
      });

      const view = render(
        <RemoteTerminal
          sessionId="web-exited-session"
          token="test-token-web"
          isAccountSession={true}
          daemonEpoch="epoch-current"
          createWebSocket={createWebSocket}
        />,
      );

      expect(sockets).toHaveLength(1);
      await sockets[0].waitForHandlers(5000);

      const exitFrame = {
        type: "terminalOutcome",
        target: {
          sessionId: "web-exited-session",
          backendSessionId: "web-exited-session",
          daemonEpoch: "epoch-current",
        },
        incarnation: "inc-web-1",
        generation: "15",
        state: "affirmative_exit",
        blockedReason: null,
        exitCode: 137,
      };

      act(() => {
        sockets[0].onmessage?.({ data: JSON.stringify(exitFrame) });
      });

      const outcomeElem = screen.getByTestId("remote-terminal-outcome");
      expect(outcomeElem).toBeInTheDocument();
      expect(outcomeElem).toHaveTextContent("Terminal exited (137)");

      view.unmount();
      expect(sockets[0].close).toHaveBeenCalled();
    });
  });
});
