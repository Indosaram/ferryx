import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";
import { openWorkspacePicker } from "./home/phoneHomeTestUtils";
import { requireSidebarRow } from "./remoteSidebarTestUtils";
import { clearStoredAccountSessionToken, storeAccountSessionToken } from "./accountSession";
import * as accountSessionModule from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
import type { FetchLikeInit, TunnelResponse, TunnelTransport, TunnelWebSocket } from "./attachTunnel";
import { remoteHostStore } from "../state/remoteHostStore";
import { referenceDrafts } from "./chat/referenceDraft";
import type { ReferenceTargetRef } from "./chat/referenceTypes";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: () => <div data-testid="remote-terminal" />,
}));

interface MachineFixture {
  machineId: string;
  workspaceId: string;
  sessionId: string;
  epoch: string;
  ownerId: string;
  deviceToken: string;
}

const M1: MachineFixture = {
  machineId: "mach-one",
  workspaceId: "ws-one",
  sessionId: "sess-one",
  epoch: "111",
  ownerId: "owner-one",
  deviceToken: "device-token-one",
};
const M2: MachineFixture = {
  machineId: "mach-two",
  workspaceId: "ws-two",
  sessionId: "sess-two",
  epoch: "222",
  ownerId: "owner-two",
  deviceToken: "device-token-two",
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

function machineTransport(machine: MachineFixture, historyPaths: string[]): TunnelTransport {
  const fetchLike = vi.fn(async (path: string, init?: FetchLikeInit): Promise<TunnelResponse> => {
    if (path.startsWith("/api/v1/pair/exchange")) {
      return respond({ token: machine.deviceToken, device: { id: "dev", name: "Phone" }, machineId: machine.machineId });
    }
    if (path === "/api/v1/workspace/projects") {
      return respond({
        revision: "1",
        completeness: "complete",
        projects: [{ workspaceId: machine.workspaceId, repoRoot: `/srv/${machine.workspaceId}`, availability: "ready", revision: "1" }],
        unavailableWorkspaceIds: [],
      });
    }
    if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
      return respond({
        revision: "1",
        worktrees: [
          {
            workspaceId: machine.workspaceId,
            identity: { wsId: machine.workspaceId, slug: "main" },
            path: `/srv/${machine.workspaceId}`,
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
    if (path === "/api/v1/sessions" && (init?.method ?? "GET") === "GET") {
      return respond({
        revision: "1",
        completeness: "complete",
        sessions: [
          {
            workspaceId: machine.workspaceId,
            worktree: { wsId: machine.workspaceId, slug: "main" },
            target: { machineId: machine.machineId, sessionId: machine.sessionId, daemonEpoch: machine.epoch },
            sessionId: machine.sessionId,
            running: true,
            title: "claude",
            // A pane with no agent type has no transcript reader, so the chat would read nothing.
            agentType: "claude",
          },
        ],
      });
    }
    if (path.startsWith("/api/v1/workspace/state")) {
      return respond({ activeContext: { workspaceId: machine.workspaceId, worktreeSlug: "main" } });
    }
    if (path.includes("/reference-chat/") && path.includes("/history")) {
      historyPaths.push(path);
      return respond({
        source: "claude-transcript",
        availability: "native",
        turns: [{ role: "user", parts: [{ kind: "text", text: `hello from ${machine.machineId}` }], startedAt: null }],
        cursor: null,
        hasMore: false,
        generation: "gen-1",
        unavailableReason: null,
      });
    }
    // Every other host route rides the encrypted tunnel too (account mode registers it for the
    // relay base): answer it from the test's relay fixture at the same `/host/<machine>` URL.
    const relayed = await fetch(`${window.location.origin}/host/${machine.machineId}${path}`, {
      method: init?.method,
      headers: init?.headers,
      body: init?.body as BodyInit | undefined,
    });
    const responseHeaders: Record<string, string> = {};
    relayed.headers.forEach((value, key) => {
      responseHeaders[key] = value;
    });
    return { status: relayed.status, headers: responseHeaders, body: new Uint8Array(await relayed.arrayBuffer()) };
  });
  return {
    fetchLike,
    openWebSocket: vi.fn(async () => new IdleSocket()),
    close: () => {},
  };
}

describe("reference chat gateway identity across a machine switch", () => {
  beforeEach(() => {
    clearStoredAccountSessionToken();
    localStorage.clear();
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

  it("ui-workspace-wiring-r13-2: the previous machine's owner and epoch never build a target on the next machine", async () => {
    storeAccountSessionToken("switch-account-token", window.location.origin);
    const historyByMachine: Record<string, string[]> = { [M1.machineId]: [], [M2.machineId]: [] };
    const transports: Record<string, TunnelTransport> = {
      [M1.machineId]: machineTransport(M1, historyByMachine[M1.machineId]),
      [M2.machineId]: machineTransport(M2, historyByMachine[M2.machineId]),
    };
    // M2's capabilities never answer: the window before its identity is where M1's must not be used.
    const never = new Promise<Response>(() => {});

    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url = typeof input === "string" ? input : input.toString();
        if (url.endsWith("/api/account/v1/machines")) {
          return new Response(
            JSON.stringify(
              [M1, M2].map((machine) => ({
                machineRecordId: `rec-${machine.machineId}`,
                machineId: machine.machineId,
                displayName: machine.machineId,
                publicKey: "pk",
                attachPublicKey: "apk",
                relayOrigin: window.location.origin,
                platform: "linux",
                online: true,
                enrollmentEpoch: "1",
                lastSeenAt: Date.now(),
              })),
            ),
            { status: 200 },
          );
        }
        const grant = url.match(/\/machines\/rec-([^/]+)\/grants/);
        if (grant) {
          return new Response(
            JSON.stringify({
              grantId: `grant-${grant[1]}`,
              machineId: grant[1],
              relayOrigin: window.location.origin,
              pairingToken: "pairing-token",
              machineAttachPublicKey: "machine-attach-key",
              grantScope: "machine",
              expiresAt: Date.now() + 600_000,
            }),
            { status: 200 },
          );
        }
        if (url.endsWith("/api/v1/attach/session")) {
          return new Response(JSON.stringify({ sessionId: "alloc" }), { status: 200 });
        }
        if (url.includes(`/host/${M1.machineId}/api/v1/capabilities`)) {
          return new Response(
            JSON.stringify({ apiVersion: 1, machineId: M1.machineId, daemonEpoch: M1.epoch, referenceOwnerId: M1.ownerId }),
            { status: 200 },
          );
        }
        if (url.includes(`/host/${M2.machineId}/api/v1/capabilities`)) return never;
        return new Response("Not Found", { status: 404 });
      }),
    );
    vi.stubGlobal("WebSocket", IdleSocket);
    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "initiator-pk",
      privateKey: "initiator-sk",
    });
    vi.spyOn(accountSessionModule, "openTunnel").mockImplementation(async (params) => ({
      transport: transports[params.machineId],
      close: () => {},
    }));
    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockImplementation(async (opts) => ({
      transport: transports[opts.machineId],
      close: () => {},
    }));

    render(<RemoteApp />);
    const pick = async (workspaceId: string) => {
      const picker = await openWorkspacePicker();
      const option = await waitFor(() => requireSidebarRow(workspaceId, "main", picker), { timeout: 5000 });
      act(() => {
        fireEvent.click(option);
      });
    };

    await pick(M1.workspaceId);
    await screen.findByText(`hello from ${M1.machineId}`, undefined, { timeout: 5000 });
    expect(historyByMachine[M1.machineId].every((path) => path.includes(`ownerId=${M1.ownerId}`))).toBe(true);

    await pick(M2.workspaceId);
    await waitFor(() => {
      expect(screen.queryByText(`hello from ${M1.machineId}`)).toBeNull();
    });
    await act(async () => {
      await new Promise<void>((done) => setTimeout(done, 0));
    });

    const foreign = historyByMachine[M2.machineId].filter(
      (path) => path.includes(`ownerId=${M1.ownerId}`) || path.includes(`epoch=${M1.epoch}`),
    );
    expect(foreign).toEqual([]);
  });

  /** SHA-256 of "abc": the staging lane refuses a receipt it cannot prove. */
  const NOTES_SHA256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

  interface DeleteCall {
    url: string;
    authorization: string | null;
  }

  /**
   * Two machines whose identities both answer, with a file staged on M1 and a worktree pick that
   * moves the lane to M2. `deleteAnswer` is what M1 says to the release.
   */
  async function stageOnFirstMachineThenSwitch(deleteAnswer: () => Response): Promise<DeleteCall[]> {
    storeAccountSessionToken("switch-account-token", window.location.origin);
    const transports: Record<string, TunnelTransport> = {
      [M1.machineId]: machineTransport(M1, []),
      [M2.machineId]: machineTransport(M2, []),
    };
    const deletes: DeleteCall[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = typeof input === "string" ? input : input.toString();
        const method = (init?.method ?? "GET").toUpperCase();
        if (url.endsWith("/api/account/v1/machines")) {
          return new Response(
            JSON.stringify(
              [M1, M2].map((machine) => ({
                machineRecordId: `rec-${machine.machineId}`,
                machineId: machine.machineId,
                displayName: machine.machineId,
                publicKey: "pk",
                attachPublicKey: "apk",
                relayOrigin: window.location.origin,
                platform: "linux",
                online: true,
                enrollmentEpoch: "1",
                lastSeenAt: Date.now(),
              })),
            ),
            { status: 200 },
          );
        }
        const grant = url.match(/\/machines\/rec-([^/]+)\/grants/);
        if (grant) {
          return new Response(
            JSON.stringify({
              grantId: `grant-${grant[1]}`,
              machineId: grant[1],
              relayOrigin: window.location.origin,
              pairingToken: "pairing-token",
              machineAttachPublicKey: "machine-attach-key",
              grantScope: "machine",
              expiresAt: Date.now() + 600_000,
            }),
            { status: 200 },
          );
        }
        if (url.endsWith("/api/v1/attach/session")) {
          return new Response(JSON.stringify({ sessionId: "alloc" }), { status: 200 });
        }
        for (const machine of [M1, M2]) {
          if (url.includes(`/host/${machine.machineId}/api/v1/capabilities`)) {
            return new Response(
              JSON.stringify({
                apiVersion: 1,
                machineId: machine.machineId,
                daemonEpoch: machine.epoch,
                referenceOwnerId: machine.ownerId,
              }),
              { status: 200 },
            );
          }
        }
        if (url.includes("/reference-chat/") && url.includes("/files")) {
          if (method === "DELETE") {
            const headers = new Headers(init?.headers);
            deletes.push({ url, authorization: headers.get("Authorization") });
            return deleteAnswer();
          }
          if (method === "POST") {
            return new Response(
              JSON.stringify({
                ok: true,
                data: {
                  receipt: { hostId: "local", attachmentId: "att-1", sha256: NOTES_SHA256, sizeBytes: 3, mediaType: "text/plain" },
                  displayName: "notes.txt",
                  mentionText: "@notes.txt ",
                },
                requestId: "stage",
              }),
              { status: 200 },
            );
          }
          return new Response(JSON.stringify({ ok: true, data: { files: [] }, requestId: "list" }), { status: 200 });
        }
        return new Response("Not Found", { status: 404 });
      }),
    );
    vi.stubGlobal("WebSocket", IdleSocket);
    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "initiator-pk",
      privateKey: "initiator-sk",
    });
    vi.spyOn(accountSessionModule, "openTunnel").mockImplementation(async (params) => ({
      transport: transports[params.machineId],
      close: () => {},
    }));
    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockImplementation(async (opts) => ({
      transport: transports[opts.machineId],
      close: () => {},
    }));

    render(<RemoteApp />);
    const pick = async (workspaceId: string) => {
      const picker = await openWorkspacePicker();
      const option = await waitFor(() => requireSidebarRow(workspaceId, "main", picker), { timeout: 5000 });
      act(() => {
        fireEvent.click(option);
      });
    };

    await pick(M1.workspaceId);
    await screen.findByText(`hello from ${M1.machineId}`, undefined, { timeout: 5000 });
    const file = new File(["abc"], "notes.txt", { type: "text/plain" });
    Object.defineProperty(file, "arrayBuffer", {
      configurable: true,
      value: async () => new TextEncoder().encode("abc").buffer,
    });
    await act(async () => {
      fireEvent.change(screen.getByTestId("file-upload-input"), { target: { files: [file] } });
    });
    await screen.findByTestId("attachment-preview-att-1");
    expect(referenceDrafts.read(firstMachineTarget).text).toContain("@notes.txt");

    await pick(M2.workspaceId);
    await screen.findByText(`hello from ${M2.machineId}`, undefined, { timeout: 5000 });
    await waitFor(() => {
      expect(deletes.length).toBeGreaterThan(0);
    });
    await act(async () => {
      await new Promise<void>((done) => setTimeout(done, 0));
    });
    return deletes;
  }

  const firstMachineTarget: ReferenceTargetRef = {
    target: { hostId: "local", ownerId: M1.ownerId, epoch: M1.epoch, backendSessionId: M1.sessionId },
  };

  it("ui-workspace-wiring-rfinal-1: a cross-machine switch releases the staged file on its own host, then drops its mention", async () => {
    const deletes = await stageOnFirstMachineThenSwitch(
      () => new Response(JSON.stringify({ ok: true }), { status: 200 }),
    );

    expect(deletes).toEqual([
      {
        url: expect.stringContaining(`/host/${M1.machineId}/api/v1/reference-chat/${M1.sessionId}/files/att-1`),
        authorization: `Bearer ${M1.deviceToken}`,
      },
    ]);
    expect(deletes[0].url).toContain(`ownerId=${M1.ownerId}`);
    expect(referenceDrafts.read(firstMachineTarget).text).not.toContain("@notes.txt");
  });

  it("ui-workspace-wiring-rfinal-1: a refused release keeps the mention in the previous machine's draft", async () => {
    await stageOnFirstMachineThenSwitch(
      () =>
        new Response(JSON.stringify({ ok: false, error: { code: "TARGET_EXPIRED", message: "expired" } }), {
          status: 409,
        }),
    );

    expect(referenceDrafts.read(firstMachineTarget).text).toContain("@notes.txt");
  });
});
