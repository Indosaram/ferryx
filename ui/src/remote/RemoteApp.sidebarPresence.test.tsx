// An account machine's sidebar section says "can't reach" only when the machine is offline or
// its connect failed; an online machine whose tunnel is merely closed keeps listing worktrees.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";
import { openWorkspacePicker } from "./home/phoneHomeTestUtils";
import { requireSidebarRow } from "./remoteSidebarTestUtils";
import { clearStoredAccountSessionToken, storeAccountSessionToken } from "./accountSession";
import * as accountSessionModule from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
import type { FetchLikeInit, TunnelResponse, TunnelTransport, TunnelWebSocket } from "./attachTunnel";
import { remoteHostStore } from "../state/remoteHostStore";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: () => <div data-testid="remote-terminal" />,
}));

const OFFLINE_TEXT = "Can't reach this computer right now.";

/** `sidebarNeverAnswers`: the machine's sidebar read hangs, so only discovery has listed it. */
type Machine = { machineId: string; workspaceId: string; online: boolean; sidebarNeverAnswers?: boolean };
const A: Machine = { machineId: "mach-a", workspaceId: "ws-a", online: true };
const B: Machine = { machineId: "mach-b", workspaceId: "ws-b", online: true };

type FakeTransport = TunnelTransport & { isClosed: boolean; fetchLike: ReturnType<typeof vi.fn> };

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

/** One tunnel to a machine; closing it makes every later request fail like the relay does. */
function machineTransport(machine: Machine): FakeTransport {
  const transport = {
    isClosed: false,
    fetchLike: vi.fn(async (path: string, init?: FetchLikeInit): Promise<TunnelResponse> => {
      if (transport.isClosed) throw new Error("STREAM_CLOSED: cannot write to closed stream");
      if (path === "/api/v1/workspace/sidebar" && machine.sidebarNeverAnswers) return new Promise<TunnelResponse>(() => {});
      if (path.startsWith("/api/v1/pair/exchange")) {
        return respond({ token: `token-${machine.machineId}`, device: { id: "dev", name: "Phone" }, machineId: machine.machineId });
      }
      if (path === "/api/v1/workspace/projects") {
        return respond({
          revision: "1", completeness: "complete", unavailableWorkspaceIds: [],
          projects: [{ workspaceId: machine.workspaceId, repoRoot: `/srv/${machine.workspaceId}`, availability: "ready", revision: "1" }],
        });
      }
      if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
        return respond({
          revision: "1",
          worktrees: [{
            workspaceId: machine.workspaceId, identity: { wsId: machine.workspaceId, slug: "main" }, path: `/srv/${machine.workspaceId}`,
            head: "h1", branch: "refs/heads/main", bare: false, detached: false, locked: null, prunable: null, managed: false,
          }],
        });
      }
      if (path === "/api/v1/sessions" && (init?.method ?? "GET") === "GET") {
        return respond({ revision: "1", completeness: "complete", sessions: [] });
      }
      if (path.startsWith("/api/v1/workspace/state")) {
        return respond({ activeContext: { workspaceId: machine.workspaceId, worktreeSlug: "main" } });
      }
      return { status: 404, headers: {}, body: new Uint8Array(0) };
    }),
    openWebSocket: vi.fn(async () => new IdleSocket()),
    close: () => {
      transport.isClosed = true;
    },
  };
  return transport as unknown as FakeTransport;
}

function mountAccount(machines: Machine[]) {
  storeAccountSessionToken("presence-account-token", window.location.origin);
  const opened: Record<string, FakeTransport[]> = {};
  const open = (machineId: string) => {
    const machine = machines.find((m) => m.machineId === machineId);
    if (!machine) throw new Error(`unknown machine ${machineId}`);
    const transport = machineTransport(machine);
    (opened[machineId] ??= []).push(transport);
    return { transport, close: transport.close };
  };
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = typeof input === "string" ? input : input.toString();
    if (url.endsWith("/api/account/v1/machines")) {
      return new Response(JSON.stringify(machines.map((machine) => ({
        machineRecordId: `rec-${machine.machineId}`, machineId: machine.machineId, displayName: machine.machineId,
        publicKey: "pk", attachPublicKey: "apk", relayOrigin: window.location.origin, platform: "linux",
        online: machine.online, enrollmentEpoch: "1", lastSeenAt: Date.now(),
      }))), { status: 200 });
    }
    const grant = url.match(/\/machines\/rec-([^/]+)\/grants/);
    if (grant) {
      return new Response(JSON.stringify({
        grantId: `grant-${grant[1]}`, machineId: grant[1], relayOrigin: window.location.origin, pairingToken: "pairing-token",
        machineAttachPublicKey: "machine-attach-key", grantScope: "machine", expiresAt: Date.now() + 600_000,
      }), { status: 200 });
    }
    if (url.endsWith("/api/v1/attach/session")) return new Response(JSON.stringify({ sessionId: "alloc" }), { status: 200 });
    return new Response("Not Found", { status: 404 });
  }));
  vi.stubGlobal("WebSocket", IdleSocket);
  vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({ publicKey: "initiator-pk", privateKey: "initiator-sk" });
  vi.spyOn(accountSessionModule, "openTunnel").mockImplementation(async (params) => open(params.machineId));
  vi.spyOn(attachTunnelModule, "openAccountTunnel").mockImplementation(async (opts) => open(opts.machineId));
  render(<RemoteApp />);
  return opened;
}

function machineSection(picker: HTMLElement, machineId: string): HTMLElement {
  const section = picker.querySelector<HTMLElement>(`[data-testid="remote-sidebar-section"][data-machine-id="${machineId}"]`);
  if (!section) throw new Error(`section for ${machineId} not rendered`);
  return section;
}

describe("account sidebar presence", () => {
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

  it("keeps an online machine's worktrees listed, without offline wording, after another machine's selection closed its tunnel", async () => {
    const opened = mountAccount([A, B]);
    const home = await openWorkspacePicker({ timeout: 5000 });
    await waitFor(() => requireSidebarRow(B.workspaceId, "main", machineSection(home, B.machineId)), { timeout: 5000 });
    const rowA = await waitFor(() => requireSidebarRow(A.workspaceId, "main", machineSection(home, A.machineId)), { timeout: 5000 });

    await act(async () => {
      fireEvent.click(rowA);
    });

    // The selection owns a fresh tunnel to A and closed every exploratory one, B's included.
    await waitFor(() => {
      expect(opened[B.machineId]?.length).toBeGreaterThan(0);
      expect(opened[B.machineId]?.every((transport) => transport.isClosed)).toBe(true);
    }, { timeout: 5000 });
    const picker = await openWorkspacePicker({ timeout: 5000 });
    const sectionB = machineSection(picker, B.machineId);
    expect(requireSidebarRow(B.workspaceId, "main", sectionB)).toBeEnabled();
    expect(sectionB).not.toHaveTextContent(OFFLINE_TEXT);
  }, 20000);

  it("lists an online machine whose sidebar never loaded from its discovery once its tunnel is closed", async () => {
    const opened = mountAccount([A, { ...B, sidebarNeverAnswers: true }]);
    const home = await openWorkspacePicker({ timeout: 5000 });
    const rowA = await waitFor(() => requireSidebarRow(A.workspaceId, "main", machineSection(home, A.machineId)), { timeout: 5000 });

    await act(async () => {
      fireEvent.click(rowA);
    });

    await waitFor(() => {
      expect(opened[B.machineId]?.length).toBeGreaterThan(0);
      expect(opened[B.machineId]?.every((transport) => transport.isClosed)).toBe(true);
    }, { timeout: 5000 });
    const picker = await openWorkspacePicker({ timeout: 5000 });
    const sectionB = machineSection(picker, B.machineId);
    await waitFor(() => expect(requireSidebarRow(B.workspaceId, "main", sectionB)).toBeEnabled(), { timeout: 5000 });
    expect(sectionB).not.toHaveTextContent(OFFLINE_TEXT);
  }, 20000);

  it("says a machine the account reports offline cannot be reached", async () => {
    mountAccount([A, { ...B, online: false }]);
    const picker = await openWorkspacePicker({ timeout: 5000 });
    await waitFor(() => requireSidebarRow(A.workspaceId, "main", machineSection(picker, A.machineId)), { timeout: 5000 });

    const sectionB = machineSection(picker, B.machineId);
    expect(within(sectionB).getByRole("status")).toHaveTextContent(OFFLINE_TEXT);
    expect(machineSection(picker, A.machineId)).not.toHaveTextContent(OFFLINE_TEXT);
  }, 20000);
});
