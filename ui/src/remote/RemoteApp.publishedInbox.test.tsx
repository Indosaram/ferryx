// Opening a desktop-published Inbox row from the account remote attaches to what the machine already
// runs and never starts a terminal: no POST /api/v1/sessions, even when the row's backend session is
// gone from the machine's inventory.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";
import { sidebarModelFixture } from "./remoteSidebarTestUtils";
import { clearStoredAccountSessionToken, storeAccountSessionToken } from "./accountSession";
import * as accountSessionModule from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
import type { FetchLikeInit, TunnelResponse, TunnelTransport, TunnelWebSocket } from "./attachTunnel";
import { remoteHostStore } from "../state/remoteHostStore";
import type { PublishedAttentionRow } from "../components/sidebar/sidebarSessionSignals";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({ sessionId }: { sessionId: string }) => <div data-testid="remote-terminal" data-session-id={sessionId} />,
}));

const MACHINE = "mach-a";
const WS = "ws-a";
const BACKEND_ID = "pty-backend-9";
const EPOCH = "11";

/** The row the desktop publishes: its local pane id, plus the pane's backend session and worktree. */
const ROW: PublishedAttentionRow = {
  id: "activity:1", revision: 1, workspaceId: WS, sessionId: "session:local-uuid", state: "needs-you", who: "codex",
  location: "ws-a / feature", at: 1, backendSessionId: BACKEND_ID, worktreeSlug: "feature",
};

function respond(value: unknown, status = 200): TunnelResponse {
  return { status, headers: { "content-type": "application/json" }, body: new TextEncoder().encode(JSON.stringify(value)) };
}

class IdleSocket implements TunnelWebSocket {
  readyState = 1;
  binaryType = "arraybuffer";
  onopen = null;
  onmessage = null;
  onerror = null;
  onclose = null;
  send() {}
  close() {
    this.readyState = 3;
  }
}

type Calls = Array<{ path: string; method: string }>;

/** One machine whose desktop published ROW; `running` lists the sessions its inventory reports. */
function machineTransport(running: readonly string[], calls: Calls): TunnelTransport {
  const worktree = (slug: string, path: string) => ({
    workspaceId: WS, identity: { wsId: WS, slug }, path, head: "h1", branch: `refs/heads/${slug}`,
    bare: false, detached: false, locked: null, prunable: null, managed: false,
  });
  const sidebar = sidebarModelFixture([{ workspaceId: WS, slug: "main" }, { workspaceId: WS, slug: "feature" }]);
  return {
    fetchLike: vi.fn(async (path: string, init?: FetchLikeInit): Promise<TunnelResponse> => {
      const method = init?.method ?? "GET";
      calls.push({ path, method });
      if (path.startsWith("/api/v1/pair/exchange")) return respond({ token: "token-a", device: { id: "dev", name: "Phone" }, machineId: MACHINE });
      if (path === "/api/v1/workspace/sidebar") {
        return respond({ model: { ...sidebar, signals: { unreadWorktreePaths: {}, agentStateByPath: {}, attentionRows: [ROW] } } });
      }
      if (path === "/api/v1/workspace/projects") {
        return respond({ revision: "1", completeness: "complete", unavailableWorkspaceIds: [],
          projects: [{ workspaceId: WS, repoRoot: "/srv/ws-a", availability: "ready", revision: "1" }] });
      }
      if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
        return respond({ revision: "1", worktrees: [worktree("main", "/srv/ws-a"), worktree("feature", "/srv/ws-a-feature")] });
      }
      if (path === "/api/v1/sessions" && method === "GET") {
        return respond({ revision: "1", completeness: "complete", sessions: running.map((sessionId) => ({
          workspaceId: WS, worktree: { wsId: WS, slug: "feature" }, target: { machineId: MACHINE, sessionId, daemonEpoch: EPOCH },
          sessionId, daemonEpoch: EPOCH, running: true, title: "codex",
        })) });
      }
      if (path === "/api/v1/sessions" && method === "POST") {
        return respond({ sessionId: "spawned", target: { machineId: MACHINE, sessionId: "spawned", daemonEpoch: EPOCH }, running: true });
      }
      if (path.startsWith("/api/v1/workspace/state")) return respond({ activeContext: { workspaceId: WS, worktreeSlug: "main" } });
      return { status: 404, headers: {}, body: new Uint8Array(0) };
    }),
    openWebSocket: vi.fn(async () => new IdleSocket()),
    close: () => undefined,
  } as unknown as TunnelTransport;
}

function mountAccount(running: readonly string[]): { calls: Calls; relayFetch: ReturnType<typeof vi.fn> } {
  storeAccountSessionToken("inbox-account-token", window.location.origin);
  const calls: Calls = [];
  const open = () => {
    const transport = machineTransport(running, calls);
    return { transport, close: transport.close };
  };
  const relayFetch = vi.fn(async (input: RequestInfo | URL) => {
    const url = typeof input === "string" ? input : input.toString();
    if (url.endsWith("/api/account/v1/machines")) {
      return new Response(JSON.stringify([{
        machineRecordId: `rec-${MACHINE}`, machineId: MACHINE, displayName: MACHINE, publicKey: "pk", attachPublicKey: "apk",
        relayOrigin: window.location.origin, platform: "linux", online: true, enrollmentEpoch: "1", lastSeenAt: Date.now(),
      }]), { status: 200 });
    }
    if (/\/machines\/rec-[^/]+\/grants/.test(url)) {
      return new Response(JSON.stringify({
        grantId: "grant-a", machineId: MACHINE, relayOrigin: window.location.origin, pairingToken: "pairing-token",
        machineAttachPublicKey: "machine-attach-key", grantScope: "machine", expiresAt: Date.now() + 600_000,
      }), { status: 200 });
    }
    if (url.endsWith("/api/v1/attach/session")) return new Response(JSON.stringify({ sessionId: "alloc" }), { status: 200 });
    return new Response("Not Found", { status: 404 });
  });
  vi.stubGlobal("fetch", relayFetch);
  vi.stubGlobal("WebSocket", IdleSocket);
  // The wide layout: the picker is the always-open sidebar, Inbox tab included.
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query === "(min-width: 768px)", media: query, onchange: null,
    addEventListener() {}, removeEventListener() {}, dispatchEvent: () => false,
  }));
  vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({ publicKey: "initiator-pk", privateKey: "initiator-sk" });
  vi.spyOn(accountSessionModule, "openTunnel").mockImplementation(async () => open());
  vi.spyOn(attachTunnelModule, "openAccountTunnel").mockImplementation(async () => open());
  render(<RemoteApp />);
  return { calls, relayFetch };
}

/** Opens the picker's Inbox, clicks the one published row, and shows the terminal view. */
async function clickPublishedRow() {
  const picker = await screen.findByTestId("remote-desktop-sidebar", {}, { timeout: 5000 });
  const tab = await waitFor(() => within(picker).getByRole("tab", { name: /^Inbox \(1\)/ }), { timeout: 5000 });
  await act(async () => {
    fireEvent.click(tab);
  });
  const row = await waitFor(() => within(picker).getByTestId("attention-row"), { timeout: 5000 });
  await act(async () => {
    fireEvent.click(row);
  });
  await act(async () => {
    fireEvent.click(screen.getByTestId("remote-view-mode-terminal"));
  });
}

/** Every request that would start a terminal, through the tunnel or the relay's fetch. */
function spawnRequests(calls: Calls, relayFetch: ReturnType<typeof vi.fn>) {
  const tunnel = calls.filter((call) => call.path.startsWith("/api/v1/sessions") && call.method === "POST");
  const relay = relayFetch.mock.calls.filter(([input, init]) =>
    String(input).includes("/api/v1/sessions") && ((init as RequestInit | undefined)?.method ?? "GET").toUpperCase() === "POST");
  return [...tunnel, ...relay];
}

describe("account remote: opening a published Inbox row", () => {
  beforeEach(() => {
    clearStoredAccountSessionToken();
    localStorage.clear();
    sessionStorage.clear();
    remoteHostStore.reset();
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    clearStoredAccountSessionToken();
    localStorage.clear();
    remoteHostStore.reset();
  });

  it("attaches to the row's backend session without starting a terminal", async () => {
    const { calls, relayFetch } = mountAccount([BACKEND_ID]);
    await clickPublishedRow();

    expect(await screen.findByTestId("remote-terminal", {}, { timeout: 5000 })).toHaveAttribute("data-session-id", BACKEND_ID);
    expect(screen.getByRole("button", { name: /Change workspace context/i })).toHaveTextContent("feature");
    expect(spawnRequests(calls, relayFetch)).toEqual([]);
  }, 20000);

  it("opens the row's worktree with no terminal when the machine no longer lists the session", async () => {
    const { calls, relayFetch } = mountAccount([]);
    await clickPublishedRow();

    await waitFor(() => expect(screen.getByRole("button", { name: /Change workspace context/i })).toHaveTextContent("feature"), { timeout: 5000 });
    expect(spawnRequests(calls, relayFetch)).toEqual([]);
    expect(screen.queryByTestId("remote-terminal")).toBeNull();
  }, 20000);
});
