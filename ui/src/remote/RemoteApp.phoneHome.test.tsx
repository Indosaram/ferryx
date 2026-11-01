// The signed-in phone opens on the supervisor home; a lane opens its session screen, whose back
// control (and the platform back) returns to that home. Account transport is faked at the tunnel.
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { remoteHostStore } from "../state/remoteHostStore";
import * as accountAttachModule from "./accountAttach";
import { clearStoredAccountOrigin, clearStoredAccountSessionToken, storeAccountOrigin, storeAccountSessionToken } from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import type { FetchLikeInit, TunnelResponse, TunnelTransport, TunnelWebSocket } from "./attachTunnel";
import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({ RemoteTerminal: () => <div data-testid="mock-remote-terminal" /> }));

const SESSION = {
  sessionId: "term-1",
  title: "claude",
  workspaceId: "ws-1",
  worktree: { wsId: "ws-1", slug: "main" },
  target: { machineId: "mach-1", sessionId: "term-1", daemonEpoch: "1" },
  running: true,
  agentType: "claude",
};

const WORKSPACE_STATE = {
  projects: [{ workspaceId: "ws-1", repoRoot: "/Users/dev/ferryx", worktrees: [{ slug: "main", label: "main" }] }],
  activeContext: {
    workspaceId: "ws-1",
    worktreeSlug: "main",
    worktreeLabel: "main",
    sessionId: "term-1",
    activeTerminal: { sessionId: "term-1", title: "claude", running: true },
  },
  sessions: [SESSION],
};

const SUPERVISOR = {
  hub: { available: true, running: true },
  lanes: [
    {
      key: "lane-1",
      title: "Ship inbox",
      status: "running",
      sessionId: "term-1",
      cwd: "/Users/dev/ferryx",
      goal: "Land the inbox",
      paused: false,
      lastActivityAt: "2026-10-24T09:00:00Z",
    },
  ],
  watched: [],
  actions: [],
};

function json(value: unknown, status = 200): TunnelResponse {
  return { status, headers: { "Content-Type": "application/json" }, body: new TextEncoder().encode(JSON.stringify(value)) };
}

function fakeSocket(): TunnelWebSocket {
  return { readyState: 1, binaryType: "arraybuffer", onopen: null, onclose: null, onmessage: null, onerror: null, send: vi.fn(), close: vi.fn() };
}

let supervisorCalls: string[] = [];

beforeEach(() => {
  supervisorCalls = [];
  clearStoredAccountSessionToken();
  clearStoredAccountOrigin();
  remoteHostStore.reset();
  window.history.replaceState(null, "", "/");
  const origin = window.location.origin;
  storeAccountOrigin(origin);
  storeAccountSessionToken("session-token-1", origin);

  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/api/account/v1/machines")) {
        return new Response(
          JSON.stringify([
            {
              machineRecordId: "rec-1",
              machineId: "mach-1",
              displayName: "Studio",
              publicKey: "pub",
              attachPublicKey: "attach-pub",
              relayOrigin: origin,
              platform: "darwin",
              online: true,
              enrollmentEpoch: "1",
              lastSeenAt: Date.now(),
            },
          ]),
          { status: 200 },
        );
      }
      if (url.includes("/grants")) {
        return new Response(
          JSON.stringify({
            grantId: "grant-1",
            machineId: "mach-1",
            relayOrigin: origin,
            pairingToken: "pair-1",
            machineAttachPublicKey: "attach-pub",
            grantScope: "machine",
            expiresAt: Date.now() + 600_000,
          }),
          { status: 200 },
        );
      }
      if (url.endsWith("/api/v1/attach/session")) return new Response(JSON.stringify({ sessionId: "alloc-1" }), { status: 200 });
      return new Response("Not Found", { status: 404 });
    }),
  );

  const transport: TunnelTransport = {
    fetchLike: vi.fn(async (path: string, init?: FetchLikeInit): Promise<TunnelResponse> => {
      if (path.startsWith("/api/v1/pair/exchange")) {
        return json({ token: "device-1", device: { id: "dev-1", name: "Phone" }, machineId: "mach-1", displayName: "Studio" });
      }
      if (path.startsWith("/api/v1/supervisor")) {
        supervisorCalls.push(`${init?.method ?? "GET"} ${path}`);
        return path === "/api/v1/supervisor/pause" ? json({ laneKey: "lane-1", paused: true, changed: true }) : json(SUPERVISOR);
      }
      if (path.startsWith("/api/v1/workspace/state")) return json(WORKSPACE_STATE);
      if (path === "/api/v1/workspace/projects") {
        return json({
          revision: 1,
          completeness: "complete",
          projects: [{ workspaceId: "ws-1", repoRoot: "/Users/dev/ferryx", availability: "ready", revision: 1 }],
          unavailableWorkspaceIds: [],
        });
      }
      if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
        return json({
          revision: 1,
          worktrees: [
            {
              workspaceId: "ws-1",
              identity: { wsId: "ws-1", slug: "main" },
              path: "/Users/dev/ferryx",
              head: "h1",
              branch: "refs/heads/main",
              bare: false,
              detached: false,
              locked: null,
              prunable: null,
              managed: false,
            },
          ],
        });
      }
      if (path.startsWith("/api/v1/sessions")) return json({ revision: "1", completeness: "complete", sessions: [SESSION] });
      if (path.startsWith("/api/v1/workspace/select")) return json({ ok: true });
      return { status: 404, headers: {}, body: new Uint8Array(0) };
    }),
    openWebSocket: vi.fn(async () => fakeSocket()),
    close: vi.fn(),
  };
  vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({ transport, close: vi.fn() });
  vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({ publicKey: "p", privateKey: "k" });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  clearStoredAccountSessionToken();
  clearStoredAccountOrigin();
  remoteHostStore.reset();
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

describe("RemoteApp phone home", () => {
  it("opens on the Supervisor tab with the machine's lanes and no session chrome", async () => {
    render(<RemoteApp />);
    const home = await screen.findByTestId("mobile-home");
    expect(home).toHaveAttribute("data-destination", "supervisor");
    expect(screen.getByTestId("mobile-home-tab-supervisor")).toHaveAttribute("aria-selected", "true");
    const card = await screen.findByTestId("supervisor-lane-card", undefined, { timeout: 3000 });
    expect(card).toHaveAttribute("data-machine-id", "mach-1");
    expect(card).toHaveAttribute("data-lane-key", "lane-1");
    expect(supervisorCalls).toContain("GET /api/v1/supervisor");
    expect(screen.queryByTestId("mobile-top-bar")).toBeNull();
  });

  it("opens the lane's session without the tab bar and returns to the home with Back", async () => {
    render(<RemoteApp />);
    const open = await screen.findByTestId("supervisor-lane-open", undefined, { timeout: 3000 });
    // The lane opens only once the machine has listed the session among the account options.
    await waitFor(() => {
      act(() => fireEvent.click(open));
      expect(screen.queryByTestId("mobile-home")).toBeNull();
    }, { timeout: 3000 });
    expect(await screen.findByTestId("mobile-top-bar")).toBeInTheDocument();
    expect(screen.queryByTestId("mobile-home-tabs")).toBeNull();
    // Supervised session: the strip sits under the top bar.
    const strip = await screen.findByTestId("supervisor-session-strip", undefined, { timeout: 3000 });
    expect(strip).toHaveAttribute("data-state", "watching");

    // Back pops the pushed session entry with history.back(), whose popstate lands later; wait
    // for it here so it cannot arrive inside the next test and flip that test's tab.
    const popped = new Promise<void>((resolve) => window.addEventListener("popstate", () => resolve(), { once: true }));
    act(() => fireEvent.click(screen.getByTestId("mobile-session-back")));
    await act(async () => { await popped; });
    const home = await screen.findByTestId("mobile-home");
    expect(home).toHaveAttribute("data-destination", "supervisor");
    // The session stays open: the home offers a way back to it.
    expect(within(home).getByTestId("mobile-home-resume")).toBeInTheDocument();
  });

  it("switches destinations from the tab bar", async () => {
    render(<RemoteApp />);
    act(() => fireEvent.click(screen.getByTestId("mobile-home-tab-inbox")));
    expect(await screen.findByTestId("inbox-home")).toBeInTheDocument();
    act(() => fireEvent.click(screen.getByTestId("mobile-home-tab-worktrees")));
    expect(await screen.findByTestId("worktrees-home")).toBeInTheDocument();
    act(() => fireEvent.click(screen.getByTestId("account-open")));
    expect(await screen.findByTestId("account-delete-open")).toBeInTheDocument();
  });
});
