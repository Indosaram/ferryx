// The signed-in phone opens on the Supervisor panel (the setup card while no hub runs); "+" opens
// the Worktrees screen. Account transport is faked at the tunnel.
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
let hubRunning = true;

beforeEach(() => {
  supervisorCalls = [];
  hubRunning = true;
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
        if (path === "/api/v1/supervisor/start") {
          hubRunning = true;
          return json({ started: true });
        }
        if (path === "/api/v1/supervisor/pause") return json({ laneKey: "lane-1", paused: true, changed: true });
        return json({ ...SUPERVISOR, hub: { available: true, running: hubRunning } });
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
  it("opens on the Supervisor panel with no tab bar, Inbox or lanes list", async () => {
    render(<RemoteApp />);
    const home = await screen.findByTestId("mobile-home");
    expect(home).toHaveAttribute("data-home", "panel");
    expect(screen.queryByTestId("mobile-top-bar")).toBeNull();
    // Nothing from the retired home is reachable.
    expect(screen.queryByTestId("mobile-home-tabs")).toBeNull();
    expect(screen.queryByTestId("inbox-home")).toBeNull();
    expect(screen.queryByTestId("supervisor-lane-card")).toBeNull();
    expect(screen.queryByTestId("supervisor-home")).toBeNull();
  });

  it("shows the one setup card, not the previous home, while no computer runs the supervisor", async () => {
    render(<RemoteApp />);
    // This machine's snapshot names no supervisor identity and serves no panel route (404).
    const card = await screen.findByTestId("setup-card", undefined, { timeout: 3000 });
    expect(card).toHaveTextContent(/Turn on the supervisor on /);
    for (const section of ["Needs you", "Handling", "Sessions"]) expect(screen.queryByRole("region", { name: section })).toBeNull();
    expect(supervisorCalls).toContain("GET /api/v1/supervisor");
  });

  it("turns the supervisor on through the hub start route on the one computer that can run it", async () => {
    // The machine reports a hub that is available but not running.
    hubRunning = false;
    render(<RemoteApp />);
    const card = await screen.findByTestId("setup-card", undefined, { timeout: 3000 });
    await waitFor(() => expect(supervisorCalls).toContain("GET /api/v1/supervisor"));
    await act(async () => fireEvent.click(within(card).getByRole("button", { name: "Turn on" })));
    await waitFor(() => expect(supervisorCalls).toContain("POST /api/v1/supervisor/start"));
  });

  it("opens the Worktrees screen from + and returns to the panel", async () => {
    render(<RemoteApp />);
    act(() => fireEvent.click(await screen.findByTestId("phone-panel-start-session")));
    const start = await screen.findByTestId("mobile-home");
    expect(start).toHaveAttribute("data-destination", "worktrees");
    act(() => fireEvent.click(screen.getByTestId("phone-panel-start-back")));
    expect(await screen.findByTestId("phone-supervisor-home")).toBeInTheDocument();
    act(() => fireEvent.click(screen.getByTestId("account-open")));
    expect(await screen.findByTestId("account-delete-open")).toBeInTheDocument();
  });
});
