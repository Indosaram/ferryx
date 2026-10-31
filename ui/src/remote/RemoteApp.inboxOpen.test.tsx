// Opening an account Inbox row from the phone home: a waiting session on another machine opens on
// this phone through that machine's own session list, never through /workspace/select and never by
// starting a terminal. Account transport is faked at the tunnel, one per machine.
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { remoteHostStore } from "../state/remoteHostStore";
import * as accountAttachModule from "./accountAttach";
import { clearStoredAccountOrigin, clearStoredAccountSessionToken, storeAccountOrigin, storeAccountSessionToken } from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import type { FetchLikeInit, TunnelResponse, TunnelTransport, TunnelWebSocket } from "./attachTunnel";
import * as inventoryModule from "./remoteSessionInventory";
import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({ RemoteTerminal: () => <div data-testid="mock-remote-terminal" /> }));

const MACHINES = [
  { machineId: "mach-1", record: "rec-1", name: "Studio", workspaceId: "ws-1", sessionId: "term-1" },
  { machineId: "mach-2", record: "rec-2", name: "Lab", workspaceId: "ws-2", sessionId: "term-2" },
] as const;

type Call = { machineId: string; method: string; path: string };

function json(value: unknown, status = 200): TunnelResponse {
  return { status, headers: { "Content-Type": "application/json" }, body: new TextEncoder().encode(JSON.stringify(value)) };
}

function fakeSocket(): TunnelWebSocket {
  return { readyState: 1, binaryType: "arraybuffer", onopen: null, onclose: null, onmessage: null, onerror: null, send: vi.fn(), close: vi.fn() };
}

function machineTransport(machine: (typeof MACHINES)[number], calls: Call[]): TunnelTransport {
  const session = {
    sessionId: machine.sessionId,
    title: "claude",
    workspaceId: machine.workspaceId,
    worktree: { wsId: machine.workspaceId, slug: "main" },
    target: { machineId: machine.machineId, sessionId: machine.sessionId, daemonEpoch: "1" },
    running: true,
    agentType: "claude",
  };
  return {
    fetchLike: vi.fn(async (path: string, init?: FetchLikeInit): Promise<TunnelResponse> => {
      calls.push({ machineId: machine.machineId, method: init?.method ?? "GET", path });
      if (path.startsWith("/api/v1/pair/exchange")) {
        return json({ token: `device-${machine.machineId}`, device: { id: "dev-1", name: "Phone" }, machineId: machine.machineId, displayName: machine.name });
      }
      if (path.startsWith("/api/v1/supervisor")) return json({ hub: { available: true, running: true }, lanes: [], watched: [], actions: [] });
      if (path === "/api/v1/workspace/projects") {
        return json({
          revision: 1,
          completeness: "complete",
          projects: [{ workspaceId: machine.workspaceId, repoRoot: `/Users/dev/${machine.workspaceId}`, availability: "ready", revision: 1 }],
          unavailableWorkspaceIds: [],
        });
      }
      if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
        return json({
          revision: 1,
          worktrees: [{
            workspaceId: machine.workspaceId, identity: { wsId: machine.workspaceId, slug: "main" }, path: `/Users/dev/${machine.workspaceId}`,
            head: "h1", branch: "refs/heads/main", bare: false, detached: false, locked: null, prunable: null, managed: false,
          }],
        });
      }
      if (path.startsWith("/api/v1/workspace/state")) {
        return json({ projects: [], activeContext: {}, activeWorkspaceId: machine.workspaceId, worktrees: [], sessions: [session] });
      }
      if (path.startsWith("/api/v1/sessions")) return json({ revision: "1", completeness: "complete", sessions: [session] });
      return { status: 404, headers: {}, body: new Uint8Array(0) };
    }),
    openWebSocket: vi.fn(async () => fakeSocket()),
    close: vi.fn(),
  };
}

let calls: Call[] = [];
let globalCalls: Array<{ url: string; method: string }> = [];

beforeEach(() => {
  calls = [];
  globalCalls = [];
  clearStoredAccountSessionToken();
  clearStoredAccountOrigin();
  remoteHostStore.reset();
  window.history.replaceState(null, "", "/");
  const origin = window.location.origin;
  storeAccountOrigin(origin);
  storeAccountSessionToken("session-token-1", origin);

  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      globalCalls.push({ url, method: (init?.method ?? "GET").toUpperCase() });
      if (url.endsWith("/api/account/v1/machines")) {
        return new Response(JSON.stringify(MACHINES.map((machine) => ({
          machineRecordId: machine.record, machineId: machine.machineId, displayName: machine.name, publicKey: "pub",
          attachPublicKey: "attach-pub", relayOrigin: origin, platform: "darwin", online: true, enrollmentEpoch: "1", lastSeenAt: Date.now(),
        }))), { status: 200 });
      }
      const grant = MACHINES.find((machine) => url.includes(`/machines/${machine.record}/grants`));
      if (grant) {
        return new Response(JSON.stringify({
          grantId: `grant-${grant.machineId}`, machineId: grant.machineId, relayOrigin: origin, pairingToken: "pair-1",
          machineAttachPublicKey: "attach-pub", grantScope: "machine", expiresAt: Date.now() + 600_000,
        }), { status: 200 });
      }
      if (url.endsWith("/api/v1/attach/session")) return new Response(JSON.stringify({ sessionId: "alloc-1" }), { status: 200 });
      return new Response("Not Found", { status: 404 });
    }),
  );

  const transports = Object.fromEntries(MACHINES.map((machine) => [machine.machineId, machineTransport(machine, calls)]));
  vi.spyOn(attachTunnelModule, "openAccountTunnel").mockImplementation(async (opts) => ({ transport: transports[opts.machineId], close: vi.fn() }));
  vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({ publicKey: "p", privateKey: "k" });
  // The machines' agents are waiting: their inventory rows carry that activity, as an observed
  // agent_state frame would have recorded it.
  const parse = inventoryModule.parseSessionsPayload;
  vi.spyOn(inventoryModule, "parseSessionsPayload").mockImplementation((machineId, raw, now) => {
    const parsed = parse(machineId, raw, now);
    if (!parsed.ok) return parsed;
    const entries = parsed.inventory.entries.map((entry) => ({ ...entry, activityState: "waiting" as const, activitySource: "agent_state" as const }));
    return { ...parsed, inventory: { ...parsed.inventory, entries } };
  });
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

describe("RemoteApp phone Inbox open", () => {
  it("opens another machine's waiting session through its own session list, with no select and no new terminal", async () => {
    render(<RemoteApp />);
    act(() => fireEvent.click(screen.getByTestId("mobile-home-tab-inbox")));
    const home = await screen.findByTestId("inbox-home");
    await waitFor(() => expect(within(home).getAllByTestId("attention-row")).toHaveLength(2), { timeout: 3000 });
    const row = within(home).getAllByTestId("attention-row").find((candidate) => candidate.getAttribute("aria-label")?.includes("Lab"));
    if (!row) throw new Error("Missing the Lab machine's Inbox row");
    const before = calls.length;

    await act(async () => {
      fireEvent.click(row);
    });

    await waitFor(() => expect(screen.queryByTestId("mobile-home")).toBeNull(), { timeout: 3000 });
    await waitFor(() => expect(calls.slice(before)).toContainEqual({ machineId: "mach-2", method: "GET", path: "/api/v1/sessions" }), { timeout: 3000 });
    expect(calls.some((call) => call.path.startsWith("/api/v1/workspace/select"))).toBe(false);
    expect(globalCalls.some((call) => call.url.includes("/workspace/select"))).toBe(false);
    expect(calls.some((call) => call.method === "POST" && call.path.startsWith("/api/v1/sessions"))).toBe(false);
  });
});
