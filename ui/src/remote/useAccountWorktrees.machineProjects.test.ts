import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useAccountWorktrees } from "./useAccountWorktrees";
import type { AccountMachineView, AccountGrantResponse, PairExchangeResponse, AllocateSessionResponse } from "./accountSession";
import type { TunnelResponse } from "./attachTunnel";

vi.mock("./accountSession", () => ({
  listMachines: vi.fn(),
  allocateSession: vi.fn(),
  openTunnel: vi.fn(),
  redeemInTunnel: vi.fn(),
  requestGrant: vi.fn(),
  AccountSessionError: class extends Error {
    code?: string;
  },
}));

vi.mock("./accountAttach", () => ({
  getOrCreateAttachKey: vi.fn().mockResolvedValue({
    publicKey: "test-pubkey",
    privateKey: "test-privkey",
  }),
}));

vi.mock("./deviceIdentity", () => ({
  suggestDeviceName: () => "Test Device",
}));

vi.mock("../lib/storageKeys", () => ({
  getOrCreateInstallationId: () => "test-install-id",
}));

import {
  listMachines,
  allocateSession,
  openTunnel,
  redeemInTunnel,
  requestGrant,
} from "./accountSession";
import { getOrCreateAttachKey } from "./accountAttach";

describe("useAccountWorktrees machine projects discovery", () => {
  const relayUrl = "https://relay.example.com";
  const sessionToken = "session-token-123";

  beforeEach(() => {
    vi.restoreAllMocks();
    vi.mocked(getOrCreateAttachKey).mockResolvedValue({
      publicKey: "test-pubkey",
      privateKey: "test-privkey",
    });
  });

  it("queries /api/v1/workspace/projects and /api/v1/workspace/worktrees strictly with machine grant scope", async () => {
    const mockMachine: AccountMachineView = {
      machineId: "2773ab38-d556-4a81-ae49-18a3b0fb83af",
      machineRecordId: "rec-1",
      publicKey: "pub-1",
      attachPublicKey: "attach-1",
      relayOrigin: relayUrl,
      displayName: "indo-remote",
      platform: "linux",
      online: true,
      enrollmentEpoch: "1",
      lastSeenAt: 12345,
    };

    vi.mocked(listMachines).mockResolvedValue([mockMachine]);
    const mockAllocResponse: AllocateSessionResponse = {
      sessionId: "session-1",
    };
    vi.mocked(allocateSession).mockResolvedValue(mockAllocResponse);

    const mockFetchLike = vi.fn(async (pathAndQuery: string): Promise<TunnelResponse> => {
      if (pathAndQuery === "/api/v1/workspace/projects") {
        return {
          status: 200,
          headers: { "content-type": "application/json" },
          body: new TextEncoder().encode(
            JSON.stringify({
              revision: 2,
              completeness: "complete",
              projects: [
                {
                  workspaceId: "project-2a54aface19a497491abc9ed791fd65d",
                  repoRoot: "/srv/repos/EclipticRD-Rewrite",
                  availability: "ready",
                  revision: 1,
                },
              ],
              unavailableWorkspaceIds: [],
            }),
          ),
        };
      }
      if (pathAndQuery.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
        return {
          status: 200,
          headers: { "content-type": "application/json" },
          body: new TextEncoder().encode(
            JSON.stringify({
              revision: 1,
              worktrees: [
                {
                  workspaceId: "project-2a54aface19a497491abc9ed791fd65d",
                  identity: null,
                  path: "/srv/repos/EclipticRD-Rewrite",
                  head: "abc1234",
                  branch: "refs/heads/main",
                  bare: false,
                  detached: false,
                  locked: null,
                  prunable: null,
                  managed: false,
                },
              ],
            }),
          ),
        };
      }
      if (pathAndQuery === "/api/v1/sessions") {
        return {
          status: 200,
          headers: { "content-type": "application/json" },
          body: new TextEncoder().encode(
            JSON.stringify({
              revision: "1",
              completeness: "complete",
              sessions: [],
            }),
          ),
        };
      }
      return { status: 404, headers: {}, body: new Uint8Array(0) };
    });

    vi.mocked(openTunnel).mockResolvedValue({
      transport: {
        fetchLike: mockFetchLike,
        openWebSocket: vi.fn(),
        close: vi.fn(),
      },
      close: vi.fn(),
    });

    const mockGrantResponse: AccountGrantResponse = {
      grantId: "grant-1",
      machineId: mockMachine.machineId,
      relayOrigin: relayUrl,
      machineAttachPublicKey: "attach-pub",
      pairingToken: "grant-token",
      grantScope: "machine",
      expiresAt: 99999,
    };
    vi.mocked(requestGrant).mockResolvedValue(mockGrantResponse);

    const mockPairResponse: PairExchangeResponse = {
      token: "device-token",
      device: { id: "dev-1", name: "Device" },
      machineId: mockMachine.machineId,
      displayName: "indo-remote",
    };
    vi.mocked(redeemInTunnel).mockResolvedValue(mockPairResponse);

    const { result } = renderHook(() =>
      useAccountWorktrees(relayUrl, sessionToken, true),
    );

    await waitFor(() => {
      const status = result.current.machineStatuses[mockMachine.machineId];
      expect(status?.error, `Machine probe failed: ${status?.error} (status: ${status?.status})`).toBeUndefined();
      expect(status?.status).toBe("ready");
    });

    expect(requestGrant).toHaveBeenCalledWith(
      relayUrl,
      sessionToken,
      mockMachine,
      "test-pubkey",
      { grantScope: "machine" },
    );

    const status = result.current.machineStatuses[mockMachine.machineId];
    expect(status?.options).toHaveLength(1);
    expect(status?.options[0].workspaceId).toBe("project-2a54aface19a497491abc9ed791fd65d");
    expect(status?.options[0].worktreeSlug).toBeNull();
    expect(status?.options[0].worktreeLabel).toBe("main");
    expect(mockFetchLike).toHaveBeenCalledWith("/api/v1/workspace/projects", expect.anything());
    expect(mockFetchLike).toHaveBeenCalledWith(
      "/api/v1/workspace/worktrees?workspaceId=project-2a54aface19a497491abc9ed791fd65d",
      expect.anything(),
    );
    expect(mockFetchLike).not.toHaveBeenCalledWith("/api/v1/workspace/state", expect.anything());
  });

  it("reuses the paired device token across retries, closes the replaced tunnel, and re-pairs only on 401", async () => {
    const machine: AccountMachineView = {
      machineId: "reuse-machine",
      machineRecordId: "rec-reuse",
      publicKey: "pub",
      attachPublicKey: "attach-1",
      relayOrigin: relayUrl,
      displayName: "reuse",
      platform: "linux",
      online: true,
      enrollmentEpoch: "1",
      lastSeenAt: 1,
    };
    vi.mocked(listMachines).mockResolvedValue([machine]);
    vi.mocked(allocateSession).mockResolvedValue({ sessionId: "s" });
    vi.mocked(requestGrant).mockResolvedValue({
      grantId: "g",
      machineId: machine.machineId,
      relayOrigin: relayUrl,
      machineAttachPublicKey: "attach-1",
      pairingToken: "pt",
      grantScope: "machine",
      expiresAt: 1,
    });
    let pairCount = 0;
    vi.mocked(redeemInTunnel).mockImplementation(async () => {
      pairCount += 1;
      return { token: `device-${pairCount}`, device: { id: "d", name: "D" }, machineId: machine.machineId, displayName: "reuse" };
    });
    let revoked: string | null = null;
    const json = (status: number, value: unknown): TunnelResponse => ({
      status,
      headers: {},
      body: new TextEncoder().encode(JSON.stringify(value)),
    });
    const tunnels: Array<{ close: ReturnType<typeof vi.fn> }> = [];
    vi.mocked(openTunnel).mockImplementation(async () => {
      const close = vi.fn();
      tunnels.push({ close });
      return {
        transport: {
          fetchLike: vi.fn(async (path: string, init?: { headers?: Record<string, string> }) => {
            if (revoked && init?.headers?.Authorization === `Bearer ${revoked}`) return json(401, { code: "UNAUTHORIZED" });
            if (path === "/api/v1/workspace/projects") return json(200, { projects: [] });
            if (path === "/api/v1/sessions") return json(200, { revision: "1", completeness: "complete", sessions: [] });
            return json(404, {});
          }),
          openWebSocket: vi.fn(),
          close: vi.fn(),
        },
        close,
      };
    });

    const { result } = renderHook(() => useAccountWorktrees(relayUrl, sessionToken, true));
    await waitFor(() => expect(result.current.machineStatuses[machine.machineId]?.status).toBe("ready"));
    expect(tunnels).toHaveLength(1);

    await result.current.retryMachine(machine.machineId);
    await waitFor(() => expect(result.current.machineStatuses[machine.machineId]?.status).toBe("ready"));
    expect(tunnels).toHaveLength(2);
    expect(requestGrant).toHaveBeenCalledTimes(1);
    expect(pairCount).toBe(1);
    expect(tunnels[0].close).toHaveBeenCalledTimes(1);
    expect(tunnels[1].close).not.toHaveBeenCalled();

    revoked = "device-1";
    await result.current.retryMachine(machine.machineId);
    await waitFor(() => expect(result.current.machineStatuses[machine.machineId]?.status).toBe("ready"));
    expect(requestGrant).toHaveBeenCalledTimes(2);
    expect(pairCount).toBe(2);
    const conn = await result.current.acquireConnection(machine.machineId);
    expect(conn?.deviceToken).toBe("device-2");
  });
});
