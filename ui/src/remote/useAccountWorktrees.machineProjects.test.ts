import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
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

  it("uses advertised desktop scope and retains unavailable members on discovery failure", async () => {
    const machine: AccountMachineView = {
      machineId: "scoped", machineRecordId: "record", publicKey: "pub", attachPublicKey: "attach",
      relayOrigin: relayUrl, displayName: "Desktop", platform: "linux", online: true,
      enrollmentEpoch: "1", lastSeenAt: 1,
    };
    vi.mocked(listMachines).mockResolvedValue([machine]);
    vi.mocked(allocateSession).mockResolvedValue({ sessionId: "scope-session" });
    vi.mocked(requestGrant).mockResolvedValue({ grantId: "g", machineId: machine.machineId,
      relayOrigin: relayUrl, machineAttachPublicKey: "attach", pairingToken: "p", grantScope: "machine", expiresAt: 1 });
    vi.mocked(redeemInTunnel).mockResolvedValue({ token: "token", device: { id: "d", name: "D" },
      machineId: machine.machineId, displayName: "Desktop" });
    let fail = false;
    let discovered: () => void = () => undefined;
    const discovery = new Promise<void>((resolve) => { discovered = resolve; });
    const json = (value: unknown): TunnelResponse => ({ status: 200, headers: {}, body: new TextEncoder().encode(JSON.stringify(value)) });
    const fetchLike = vi.fn(async (path: string) => {
      if (fail) throw new Error("disconnected");
      if (path === "/api/v1/workspace/projects") return json({ desktopLocalSnapshotExists: true,
        projects: [{ workspaceId: "legacy", availability: "ready" }] });
      if (path === "/api/v1/workspace/projects?scope=desktopLocal") return json({ desktopLocalSnapshotExists: true,
        projects: [{ workspaceId: "member", availability: "ready" }, { workspaceId: "offline", availability: "missing" }] });
      if (path === "/api/v1/workspace/worktrees?workspaceId=member") return json({ worktrees: [{ workspaceId: "member", identity: null, branch: "main" }] });
      if (path === "/api/v1/sessions") { discovered(); return json({ revision: "1", completeness: "complete", sessions: [] }); }
      throw new Error(`Unexpected request ${path}`);
    });
    vi.mocked(openTunnel).mockImplementation(async () => ({ transport: { fetchLike, openWebSocket: vi.fn(), close: vi.fn() }, close: vi.fn() }));
    const { result } = renderHook(() => useAccountWorktrees(relayUrl, sessionToken, true));
    await act(async () => { await discovery; });
    expect(result.current.machineStatuses.scoped?.status).toBe("ready");
    expect(fetchLike).toHaveBeenCalledWith("/api/v1/workspace/projects?scope=desktopLocal", expect.anything());
    expect(result.current.accountOptions.map((option) => option.workspaceId).sort()).toEqual(["member", "offline"]);
    expect(result.current.accountOptions.find((option) => option.workspaceId === "offline")?.unavailable).toBe(true);
    expect(fetchLike).not.toHaveBeenCalledWith("/api/v1/workspace/worktrees?workspaceId=legacy", expect.anything());
    fail = true;
    await act(async () => { await result.current.retryMachine(machine.machineId); });
    expect(result.current.accountOptions).toHaveLength(2);
    expect(result.current.accountOptions.every((option) => option.unavailable)).toBe(true);
  });

  it("fetches at most four project worktree lists at once alongside sessions and isolates one failure", async () => {
    const machine: AccountMachineView = {
      machineId: "fanout", machineRecordId: "record", publicKey: "pub", attachPublicKey: "attach",
      relayOrigin: relayUrl, displayName: "Desktop", platform: "linux", online: true,
      enrollmentEpoch: "1", lastSeenAt: 1,
    };
    vi.mocked(listMachines).mockResolvedValue([machine]);
    vi.mocked(allocateSession).mockResolvedValue({ sessionId: "fanout-session" });
    vi.mocked(requestGrant).mockResolvedValue({ grantId: "g", machineId: machine.machineId,
      relayOrigin: relayUrl, machineAttachPublicKey: "attach", pairingToken: "p", grantScope: "machine", expiresAt: 1 });
    vi.mocked(redeemInTunnel).mockResolvedValue({ token: "token", device: { id: "d", name: "D" },
      machineId: machine.machineId, displayName: "Desktop" });
    const json = (value: unknown): TunnelResponse => ({ status: 200, headers: {}, body: new TextEncoder().encode(JSON.stringify(value)) });
    const ids = ["p1", "p2", "p3", "p4", "p5"];
    const pending = new Map<string, () => void>();
    let inFlight = 0;
    let peak = 0;
    let sessionsRequested = false;
    let fourStarted: () => void = () => undefined;
    const started = new Promise<void>((resolve) => { fourStarted = resolve; });
    const fetchLike = vi.fn(async (path: string) => {
      if (path === "/api/v1/workspace/projects") {
        return json({ projects: ids.map((workspaceId) => ({ workspaceId, availability: "ready" })) });
      }
      if (path === "/api/v1/sessions") {
        sessionsRequested = true;
        return json({ revision: "1", completeness: "complete", sessions: [] });
      }
      const wsId = new URLSearchParams(path.split("?")[1]).get("workspaceId") ?? "";
      inFlight += 1;
      peak = Math.max(peak, inFlight);
      if (inFlight === 4) fourStarted();
      await new Promise<void>((resolve) => pending.set(wsId, resolve));
      inFlight -= 1;
      if (wsId === "p2") return { status: 503, headers: {}, body: new Uint8Array() };
      return json({ worktrees: [{ workspaceId: wsId, identity: null, branch: "main" }] });
    });
    vi.mocked(openTunnel).mockImplementation(async () => ({ transport: { fetchLike, openWebSocket: vi.fn(), close: vi.fn() }, close: vi.fn() }));
    const { result } = renderHook(() => useAccountWorktrees(relayUrl, sessionToken, true));
    await act(async () => { await started; });
    expect([...pending.keys()]).toEqual(["p1", "p2", "p3", "p4"]);
    // Sessions are requested while every worktree reply is still unresolved.
    expect(sessionsRequested).toBe(true);
    await act(async () => { pending.get("p1")?.(); });
    await waitFor(() => expect(pending.has("p5")).toBe(true));
    await act(async () => { for (const id of ["p2", "p3", "p4", "p5"]) pending.get(id)?.(); });
    await waitFor(() => expect(result.current.machineStatuses.fanout?.status).toBe("ready"));
    expect(peak).toBe(4);
    const options = result.current.accountOptions.filter((option) => option.machineId === "fanout");
    expect(options.map((option) => option.workspaceId)).toEqual(ids);
    expect(options.find((option) => option.workspaceId === "p2")?.unavailable).toBe(true);
    expect(options.filter((option) => option.unavailable)).toHaveLength(1);
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

  const sshMachine = (machineId: string): AccountMachineView => ({
    machineId, machineRecordId: "record", publicKey: "pub", attachPublicKey: "attach",
    relayOrigin: relayUrl, displayName: "Desktop", platform: "macos", online: true,
    enrollmentEpoch: "1", lastSeenAt: 1,
  });
  const mockPairing = (machine: AccountMachineView) => {
    vi.mocked(listMachines).mockResolvedValue([machine]);
    vi.mocked(allocateSession).mockResolvedValue({ sessionId: "ssh-session" });
    vi.mocked(requestGrant).mockResolvedValue({ grantId: "g", machineId: machine.machineId,
      relayOrigin: relayUrl, machineAttachPublicKey: "attach", pairingToken: "p", grantScope: "machine", expiresAt: 1 });
    vi.mocked(redeemInTunnel).mockResolvedValue({ token: "token", device: { id: "d", name: "D" },
      machineId: machine.machineId, displayName: "Desktop" });
  };
  const sshJson = (value: unknown): TunnelResponse => ({ status: 200, headers: {}, body: new TextEncoder().encode(JSON.stringify(value)) });
  const SSH_ID = "ssh:f41f0c2a";

  it("lists SSH-host projects from workspace state and keeps their running sessions", async () => {
    const machine = sshMachine("ssh-desktop");
    mockPairing(machine);
    const fetchLike = vi.fn(async (path: string) => {
      if (path === "/api/v1/workspace/projects") return sshJson({ desktopLocalSnapshotExists: true, projects: [] });
      if (path === "/api/v1/workspace/projects?scope=desktopLocal") return sshJson({ desktopLocalSnapshotExists: true,
        projects: [{ workspaceId: "member", availability: "ready" }] });
      if (path === "/api/v1/workspace/worktrees?workspaceId=member") return sshJson({ worktrees: [{ workspaceId: "member", identity: null, branch: "main" }] });
      if (path === "/api/v1/workspace/state") return sshJson({
        context: { workspaceId: "member" },
        projects: [
          { workspaceId: "member", worktrees: [{ worktreeSlug: null, worktreeLabel: "main" }] },
          { workspaceId: SSH_ID, hostLabel: "omarchy", worktrees: [{ worktreeSlug: null, worktreeLabel: "mhc" }] },
        ],
      });
      if (path === "/api/v1/sessions") return sshJson({ revision: "1", completeness: "complete", sessions: [
        { workspaceId: SSH_ID, sessionId: "ssh-sess-1", running: true, title: "zsh",
          target: { machineId: machine.machineId, sessionId: "ssh-sess-1", daemonEpoch: 7 } },
      ] });
      throw new Error(`Unexpected request ${path}`);
    });
    vi.mocked(openTunnel).mockImplementation(async () => ({ transport: { fetchLike, openWebSocket: vi.fn(), close: vi.fn() }, close: vi.fn() }));
    const { result } = renderHook(() => useAccountWorktrees(relayUrl, sessionToken, true));
    await waitFor(() => expect(result.current.machineStatuses[machine.machineId]?.status).toBe("ready"));
    await waitFor(() => expect(result.current.accountOptions.some((option) => option.sessionId === "ssh-sess-1")).toBe(true));
    const root = result.current.accountOptions.find((option) => option.workspaceId === SSH_ID && !option.sessionId);
    expect(root).toMatchObject({ worktreeSlug: null, worktreeLabel: "mhc", projectLabel: "mhc (omarchy)" });
    expect(root?.unavailable).toBeFalsy();
    const session = result.current.accountOptions.find((option) => option.sessionId === "ssh-sess-1");
    expect(session).toMatchObject({ workspaceId: SSH_ID, worktreeLabel: "mhc", projectLabel: "mhc (omarchy)", running: true });
    expect(result.current.accountOptions.some((option) => option.workspaceId === "member" && option.worktreeLabel === "main")).toBe(true);
    expect(fetchLike).not.toHaveBeenCalledWith(expect.stringContaining(`/api/v1/workspace/worktrees?workspaceId=${encodeURIComponent(SSH_ID)}`), expect.anything());
    expect(fetchLike).not.toHaveBeenCalledWith(expect.stringContaining("workspaceId=ssh"), expect.anything());
  });

  it("keeps local rows and a ready machine when workspace state fails", async () => {
    const machine = sshMachine("ssh-state-fail");
    mockPairing(machine);
    const fetchLike = vi.fn(async (path: string) => {
      if (path === "/api/v1/workspace/projects") return sshJson({ desktopLocalSnapshotExists: true, projects: [] });
      if (path === "/api/v1/workspace/projects?scope=desktopLocal") return sshJson({ desktopLocalSnapshotExists: true,
        projects: [{ workspaceId: "member", availability: "ready" }] });
      if (path === "/api/v1/workspace/worktrees?workspaceId=member") return sshJson({ worktrees: [{ workspaceId: "member", identity: null, branch: "main" }] });
      if (path === "/api/v1/workspace/state") return { status: 500, headers: {}, body: new TextEncoder().encode("SSH inventory unavailable") };
      if (path === "/api/v1/sessions") return sshJson({ revision: "1", completeness: "complete", sessions: [
        { workspaceId: "member", sessionId: "local-sess", running: true, worktree: { wsId: "member", slug: null },
          target: { machineId: machine.machineId, sessionId: "local-sess", daemonEpoch: 7 } },
        { workspaceId: SSH_ID, sessionId: "ssh-sess-1", running: true,
          target: { machineId: machine.machineId, sessionId: "ssh-sess-1", daemonEpoch: 7 } },
      ] });
      throw new Error(`Unexpected request ${path}`);
    });
    vi.mocked(openTunnel).mockImplementation(async () => ({ transport: { fetchLike, openWebSocket: vi.fn(), close: vi.fn() }, close: vi.fn() }));
    const { result } = renderHook(() => useAccountWorktrees(relayUrl, sessionToken, true));
    await waitFor(() => expect(result.current.machineStatuses[machine.machineId]?.status).toBe("ready"));
    await waitFor(() => expect(result.current.accountOptions.some((option) => option.sessionId === "local-sess")).toBe(true));
    expect(fetchLike).toHaveBeenCalledWith("/api/v1/workspace/state", expect.anything());
    expect(result.current.machineStatuses[machine.machineId]?.error).toBeUndefined();
    expect(result.current.accountOptions.map((option) => option.workspaceId).sort()).toEqual(["member", "member"]);
    expect(result.current.accountOptions.some((option) => option.workspaceId === SSH_ID)).toBe(false);
  });
});
