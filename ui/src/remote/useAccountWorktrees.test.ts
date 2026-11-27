import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as accountAttachModule from "./accountAttach";
import { useAccountWorktrees } from "./useAccountWorktrees";
import {
  listMachines,
  allocateSession,
  openTunnel,
  redeemInTunnel,
  requestGrant,
  AccountSessionError,
  PLAN_LIMIT_REACHED,
  REMOTE_SUSPENDED,
  type AccountMachineView,
  type PlanLimitState,
} from "./accountSession";
import { AttachTunnelRefusedError, TunnelError, type TunnelTransport, type TunnelResponse, type TunnelWebSocket } from "./attachTunnel";

vi.mock("./accountSession", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./accountSession")>();
  return {
    ...actual,
    listMachines: vi.fn(),
    allocateSession: vi.fn(),
    openTunnel: vi.fn(),
    redeemInTunnel: vi.fn(),
    requestGrant: vi.fn(),
  };
});

vi.mock("./deviceIdentity", () => ({
  suggestDeviceName: () => "Test Device",
}));

vi.mock("../lib/storageKeys", () => ({
  getOrCreateInstallationId: () => "test-install-id",
}));

interface Deferred<T> {
  readonly promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (err: unknown) => void;
}

function createDeferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (err: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function awaitBoundedSignal<T>(
  signal: Deferred<T>,
  timeoutMs = 1000,
  label = "signal",
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error(`Timed out waiting for ${label} after ${timeoutMs}ms`));
    }, timeoutMs);
  });
  try {
    return await Promise.race([signal.promise, timeoutPromise]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

describe("useAccountWorktrees machine discovery and plan limits", () => {
  const sampleMachine: AccountMachineView = {
    machineRecordId: "rec-1",
    machineId: "mach-1",
    displayName: "Machine 1",
    publicKey: "pk-1",
    attachPublicKey: "apk-1",
    relayOrigin: "https://relay.test",
    platform: "linux",
    online: true,
    enrollmentEpoch: "1",
    lastSeenAt: Date.now(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "test-pubkey",
      privateKey: "test-privkey",
    });
  });

  it("discovers machines and populates account options on ready", async () => {
    const listMachinesDef = createDeferred<AccountMachineView[]>();
    const requestGrantDef = createDeferred<{
      grantId: string;
      machineId: string;
      relayOrigin: string;
      pairingToken: string;
      machineAttachPublicKey: string;
      grantScope: "machine";
      expiresAt: number;
    }>();
    const allocateSessionDef = createDeferred<{ sessionId: string }>();
    const redeemInTunnelDef = createDeferred<{
      token: string;
      device: { id: string; name: string };
      machineId: string;
      displayName: string;
    }>();
    const workspaceProjectsDef = createDeferred<TunnelResponse>();
    const jsonResponse = (value: unknown): TunnelResponse => ({
      status: 200,
      headers: { "content-type": "application/json" },
      body: new TextEncoder().encode(JSON.stringify(value)),
    });

    vi.mocked(listMachines).mockReturnValue(listMachinesDef.promise);
    vi.mocked(requestGrant).mockReturnValue(requestGrantDef.promise);
    vi.mocked(allocateSession).mockReturnValue(allocateSessionDef.promise);

    const mockFetchLike = vi.fn().mockImplementation(async (path: string): Promise<TunnelResponse> => {
      if (path === "/api/v1/workspace/projects") {
        return workspaceProjectsDef.promise;
      }
      if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
        return jsonResponse({
          revision: 1,
          worktrees: [
            {
              workspaceId: "ws-ferryx",
              identity: { wsId: "ws-ferryx", slug: "main" },
              path: "/test/main",
              head: "abc1234",
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
      if (path === "/api/v1/sessions") {
        return jsonResponse({ revision: "1", completeness: "complete", sessions: [] });
      }
      return { status: 404, headers: {}, body: new Uint8Array(0) };
    });

    const mockOpenWebSocket = vi.fn().mockImplementation(async (): Promise<TunnelWebSocket> => {
      return {
        send: vi.fn(),
        close: vi.fn(),
        readyState: 1,
        binaryType: "arraybuffer",
        onopen: null,
        onmessage: null,
        onerror: null,
        onclose: null,
      };
    });

    const mockClose = vi.fn();
    const mockTransport: TunnelTransport = {
      fetchLike: mockFetchLike,
      openWebSocket: mockOpenWebSocket,
      close: mockClose,
    };

    vi.mocked(openTunnel).mockResolvedValue({
      transport: mockTransport,
      close: mockClose,
    });
    vi.mocked(redeemInTunnel).mockReturnValue(redeemInTunnelDef.promise);

    const { result } = renderHook(() =>
      useAccountWorktrees("https://relay.test", "token-xyz", true),
    );

    expect(result.current.loading).toBe(true);
    expect(result.current.accountOptions).toHaveLength(0);

    await act(async () => {
      listMachinesDef.resolve([sampleMachine]);
    });

    expect(result.current.machines).toHaveLength(1);
    expect(result.current.loading).toBe(false);

    await act(async () => {
      requestGrantDef.resolve({
        grantId: "g-1",
        machineId: "mach-1",
        relayOrigin: "https://relay.test",
        pairingToken: "pair-1",
        machineAttachPublicKey: "apk-1",
        grantScope: "machine",
        expiresAt: Date.now() + 60000,
      });
    });

    await act(async () => {
      allocateSessionDef.resolve({ sessionId: "sess-1" });
    });

    await act(async () => {
      redeemInTunnelDef.resolve({
        token: "tok-1",
        device: { id: "dev-1", name: "Dev" },
        machineId: "mach-1",
        displayName: "Machine 1",
      });
    });

    await act(async () => {
      workspaceProjectsDef.resolve(
        jsonResponse({
          revision: 1,
          completeness: "complete",
          projects: [
            {
              workspaceId: "ws-ferryx",
              repoRoot: "/test/main",
              availability: "ready",
              revision: 1,
            },
          ],
          unavailableWorkspaceIds: [],
        }),
      );
    });

    await waitFor(() => {
      expect(result.current.machineStatuses["mach-1"]?.status).toBe("ready");
    });
    expect(result.current.accountOptions).toHaveLength(1);
    expect(result.current.accountOptions[0].worktreeSlug).toBe("main");
    expect(result.current.accountOptions[0].workspaceId).toBe("ws-ferryx");
    expect(result.current.machineStatuses["mach-1"]?.status).toBe("ready");
  });

  it.each([
    { failure: new AccountSessionError("UNAUTHORIZED", "expired", 401), code: "UNAUTHORIZED" },
    { failure: new AccountSessionError("MACHINE_KEY_CHANGED", "key mismatch"), code: "MACHINE_KEY_CHANGED" },
    { failure: new AccountSessionError("MACHINE_OFFLINE", "offline"), code: "MACHINE_OFFLINE" },
    { failure: new AccountSessionError("MACHINE_NOT_FOUND", "removed", 404), code: "MISSING_MACHINE" },
    { failure: new TunnelError("UNAUTHORIZED", "device rejected", 401), code: "UNAUTHORIZED" },
    { failure: new TunnelError("MACHINE_KEY_CHANGED", "attach key mismatch"), code: "MACHINE_KEY_CHANGED" },
    { failure: new TunnelError("MACHINE_OFFLINE", "relay offline"), code: "MACHINE_OFFLINE" },
    { failure: new TunnelError("STREAM_CLOSED", "socket gone"), code: "TRANSPORT_ERROR" },
    { failure: new TypeError("network unavailable"), code: "TRANSPORT_ERROR" },
    { failure: new AccountSessionError(PLAN_LIMIT_REACHED, "limit", 402), code: "PLAN_LIMIT" },
    { failure: new AttachTunnelRefusedError(REMOTE_SUSPENDED), code: "PLAN_LIMIT" },
  ])("returns $code when acquisition fails with $failure.name / $failure.message", async ({ failure, code }) => {
    // Given a discovered machine whose initial probe failed before opening a tunnel.
    const listed = createDeferred<AccountMachineView[]>();
    vi.mocked(listMachines).mockReturnValue(listed.promise);
    vi.mocked(requestGrant).mockRejectedValueOnce(new AccountSessionError("NETWORK", "probe failed"));
    const onPlanLimit = vi.fn();
    const { result } = renderHook(() =>
      useAccountWorktrees("https://relay.test", "token-xyz", true, undefined, onPlanLimit),
    );
    await act(async () => { listed.resolve([sampleMachine]); });
    expect(result.current.machineStatuses[sampleMachine.machineId]?.status).toBe("error");
    if (failure instanceof TunnelError || failure instanceof AttachTunnelRefusedError) {
      vi.mocked(requestGrant).mockResolvedValueOnce({
        grantId: "g", machineId: sampleMachine.machineId, relayOrigin: "https://relay.test",
        pairingToken: "pair", machineAttachPublicKey: "apk-1", grantScope: "machine", expiresAt: 1,
      });
      vi.mocked(allocateSession).mockResolvedValueOnce({ sessionId: "acquisition-session" });
      vi.mocked(openTunnel).mockRejectedValueOnce(failure);
    } else {
      vi.mocked(requestGrant).mockRejectedValueOnce(failure);
    }

    // When the caller requests a detailed connection.
    const outcome = await result.current.acquireConnectionDetailed(sampleMachine.machineId);

    // Then the structured reason survives without exposing transport diagnostics.
    expect(outcome).toMatchObject({ ok: false, code, message: expect.any(String) });
    expect(onPlanLimit).toHaveBeenCalledTimes(code === "PLAN_LIMIT" ? 1 : 0);
  });

  it("returns MACHINE_OFFLINE without opening a tunnel when presence is offline", async () => {
    // Given an offline machine in the account inventory.
    const listed = createDeferred<AccountMachineView[]>();
    vi.mocked(listMachines).mockReturnValue(listed.promise);
    const { result } = renderHook(() => useAccountWorktrees("https://relay.test", "token-xyz", true));
    await act(async () => { listed.resolve([{ ...sampleMachine, online: false }]); });

    // When a detailed connection is requested.
    const outcome = await result.current.acquireConnectionDetailed(sampleMachine.machineId);

    // Then presence is reported without attempting a grant or tunnel.
    expect(outcome).toMatchObject({ ok: false, code: "MACHINE_OFFLINE" });
    expect(requestGrant).not.toHaveBeenCalled();
    expect(openTunnel).not.toHaveBeenCalled();
  });

  it("returns UNAUTHORIZED when no account session exists", async () => {
    // Given discovery without a session token.
    const { result } = renderHook(() => useAccountWorktrees("https://relay.test", null, false));

    // When a detailed connection is requested.
    const outcome = await result.current.acquireConnectionDetailed(sampleMachine.machineId);

    // Then callers can route to sign-in instead of generic retry.
    expect(outcome).toMatchObject({ ok: false, code: "UNAUTHORIZED" });
  });

  it("returns MISSING_MACHINE when the requested machine is absent", async () => {
    // Given a loaded empty inventory.
    const listed = createDeferred<AccountMachineView[]>();
    vi.mocked(listMachines).mockReturnValue(listed.promise);
    const { result } = renderHook(() => useAccountWorktrees("https://relay.test", "token-xyz", true));
    await act(async () => { listed.resolve([]); });

    // When a removed machine is requested.
    const outcome = await result.current.acquireConnectionDetailed("removed-machine");

    // Then the absent machine has its own outcome.
    expect(outcome).toMatchObject({ ok: false, code: "MISSING_MACHINE" });
  });

  it("keeps the legacy null result when detailed acquisition fails", async () => {
    // Given discovery without a session token.
    const { result } = renderHook(() => useAccountWorktrees("https://relay.test", null, false));

    // When an existing caller uses the legacy API.
    const connection = await result.current.acquireConnection(sampleMachine.machineId);

    // Then failure remains backward compatible.
    expect(connection).toBeNull();
  });

  it("fires onPlanLimit callback when listMachines fails with PLAN_LIMIT_REACHED", async () => {
    const planLimitSignal = createDeferred<PlanLimitState>();
    const onPlanLimit = vi.fn().mockImplementation((state: PlanLimitState) => {
      planLimitSignal.resolve(state);
    });

    const listMachinesDef = createDeferred<AccountMachineView[]>();
    vi.mocked(listMachines).mockReturnValue(listMachinesDef.promise);

    renderHook(() =>
      useAccountWorktrees("https://relay.test", "token-xyz", true, undefined, onPlanLimit),
    );

    await act(async () => {
      listMachinesDef.reject(
        new AccountSessionError(PLAN_LIMIT_REACHED, "Plan limit reached", 402, {
          plan: "free",
          limit: 1,
          used: 2,
        }),
      );
    });

    const captured = await awaitBoundedSignal(planLimitSignal, 1000, "planLimitSignal");
    expect(captured).toEqual(
      expect.objectContaining({
        code: PLAN_LIMIT_REACHED,
        plan: "free",
        limit: 1,
        used: 2,
      }),
    );
    expect(onPlanLimit).toHaveBeenCalledTimes(1);
  });

  it("fires onPlanLimit callback when machine probe fails with REMOTE_SUSPENDED", async () => {
    const planLimitSignal = createDeferred<PlanLimitState>();
    const onPlanLimit = vi.fn().mockImplementation((state: PlanLimitState) => {
      planLimitSignal.resolve(state);
    });

    const listMachinesDef = createDeferred<AccountMachineView[]>();
    const requestGrantDef = createDeferred<{
      grantId: string;
      machineId: string;
      relayOrigin: string;
      pairingToken: string;
      machineAttachPublicKey: string;
      grantScope: "machine";
      expiresAt: number;
    }>();

    vi.mocked(listMachines).mockReturnValue(listMachinesDef.promise);
    vi.mocked(requestGrant).mockReturnValue(requestGrantDef.promise);

    renderHook(() =>
      useAccountWorktrees("https://relay.test", "token-xyz", true, undefined, onPlanLimit),
    );

    await act(async () => {
      listMachinesDef.resolve([sampleMachine]);
    });

    await act(async () => {
      requestGrantDef.reject(
        new AccountSessionError(REMOTE_SUSPENDED, "Remote access suspended", 402, {
          plan: "pro_monthly",
          status: "stopped",
        }),
      );
    });

    const captured = await awaitBoundedSignal(planLimitSignal, 1000, "planLimitSignal");
    expect(captured).toEqual(
      expect.objectContaining({
        code: REMOTE_SUSPENDED,
        plan: "pro_monthly",
        status: "stopped",
      }),
    );
    expect(onPlanLimit).toHaveBeenCalledTimes(1);
  });
});
