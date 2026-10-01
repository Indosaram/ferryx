import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, renderHook } from "@testing-library/react";
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
import type { TunnelTransport, TunnelResponse, TunnelWebSocket } from "./attachTunnel";

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
    const workspaceStateDef = createDeferred<TunnelResponse>();

    vi.mocked(listMachines).mockReturnValue(listMachinesDef.promise);
    vi.mocked(requestGrant).mockReturnValue(requestGrantDef.promise);
    vi.mocked(allocateSession).mockReturnValue(allocateSessionDef.promise);

    const mockFetchLike = vi.fn().mockImplementation(async (path: string): Promise<TunnelResponse> => {
      if (path.startsWith("/api/v1/workspace/state")) {
        return workspaceStateDef.promise;
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
      workspaceStateDef.resolve({
        status: 200,
        headers: { "content-type": "application/json" },
        body: new TextEncoder().encode(
          JSON.stringify({
            projects: [
              {
                workspaceId: "ws-ferryx",
                repoRoot: "/test/main",
                worktrees: [
                  {
                    slug: "main",
                    label: "main",
                  },
                ],
              },
            ],
          }),
        ),
      });
    });

    expect(result.current.accountOptions).toHaveLength(1);
    expect(result.current.accountOptions[0].worktreeSlug).toBe("main");
    expect(result.current.accountOptions[0].workspaceId).toBe("ws-ferryx");
    expect(result.current.machineStatuses["mach-1"]?.status).toBe("ready");
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
