import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  requestLogin,
  consumeLogin,
  listMachines,
  requestGrant,
  allocateSession,
  openTunnel,
  redeemInTunnel,
  AccountSessionError,
  getStoredAccountSessionToken,
  clearStoredAccountSessionToken,
  type AccountMachineView,
} from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";

describe("accountSession client module", () => {
  const origin = "https://relay.example.com";
  const sessionToken = "account-token-secret-7788";
  const pairingToken = "pairing-token-secret-9944";
  let fetchCalls: Array<{ url: string; init?: RequestInit }> = [];

  beforeEach(() => {
    fetchCalls = [];
    clearStoredAccountSessionToken();

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL, init?: RequestInit) => {
      const url = typeof input === "string" ? input : input.toString();
      fetchCalls.push({ url, init });

      if (url.endsWith("/api/account/v1/login/request")) {
        return Promise.resolve(new Response(JSON.stringify({ status: "accepted" }), { status: 202 }));
      }

      if (url.endsWith("/api/account/v1/login/consume")) {
        const body = init?.body ? JSON.parse(init.body as string) : {};
        if (body.code === "valid-code") {
          return Promise.resolve(
            new Response(
              JSON.stringify({
                token: sessionToken,
                accountId: "acc-user-1",
                email: "user@example.com",
              }),
              { status: 200 },
            ),
          );
        }
        return Promise.resolve(
          new Response(
            JSON.stringify({ code: "UNAUTHORIZED", message: "Invalid or expired login code" }),
            { status: 401 },
          ),
        );
      }

      if (url.endsWith("/api/account/v1/machines")) {
        const auth = (init?.headers as Record<string, string>)?.Authorization;
        if (auth !== `Bearer ${sessionToken}`) {
          return Promise.resolve(
            new Response(JSON.stringify({ code: "UNAUTHORIZED" }), { status: 401 }),
          );
        }
        const sampleMachines: AccountMachineView[] = [
          {
            machineRecordId: "rec-mach-1",
            machineId: "mach-uuid-1",
            displayName: "Test Laptop",
            publicKey: "ed25519-pub-1",
            attachPublicKey: "x25519-pub-1",
            relayOrigin: origin,
            platform: "macos",
            online: true,
            enrollmentEpoch: "2",
            lastSeenAt: 1700000000,
          },
        ];
        return Promise.resolve(new Response(JSON.stringify(sampleMachines), { status: 200 }));
      }

      if (url.includes("/api/account/v1/machines/rec-mach-1/grants")) {
        const auth = (init?.headers as Record<string, string>)?.Authorization;
        if (auth !== `Bearer ${sessionToken}`) {
          return Promise.resolve(
            new Response(JSON.stringify({ code: "UNAUTHORIZED" }), { status: 401 }),
          );
        }
        return Promise.resolve(
          new Response(
            JSON.stringify({
              grantId: "grant-1",
              machineId: "mach-uuid-1",
              relayOrigin: origin,
              pairingToken: pairingToken,
              machineAttachPublicKey: "machine-attach-pub-key-1",
              grantScope: "mirror",
              expiresAt: 1700000600,
            }),
            { status: 200 },
          ),
        );
      }

      if (url.endsWith("/api/v1/attach/session")) {
        const auth = (init?.headers as Record<string, string>)?.Authorization;
        if (auth !== `Bearer ${sessionToken}`) {
          return Promise.resolve(
            new Response(JSON.stringify({ code: "UNAUTHORIZED" }), { status: 401 }),
          );
        }
        return Promise.resolve(
          new Response(JSON.stringify({ sessionId: "sess-alloc-123" }), { status: 200 }),
        );
      }

      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    clearStoredAccountSessionToken();
  });

  it("requestLogin sends POST to /api/account/v1/login/request with email body", async () => {
    await requestLogin(origin, "user@example.com");
    expect(fetchCalls.length).toBe(1);
    const call = fetchCalls[0];
    expect(call.url).toBe("https://relay.example.com/api/account/v1/login/request");
    expect(call.init?.method).toBe("POST");
    expect(call.init?.headers).toEqual({ "Content-Type": "application/json" });
    expect(JSON.parse(call.init?.body as string)).toEqual({ email: "user@example.com" });
    expect(call.url).not.toContain("token");
  });

  it("consumeLogin sends POST to /api/account/v1/login/consume and stores session token", async () => {
    const res = await consumeLogin(origin, "valid-code");
    expect(res.token).toBe(sessionToken);
    expect(res.accountId).toBe("acc-user-1");
    expect(res.email).toBe("user@example.com");

    expect(fetchCalls.length).toBe(1);
    const call = fetchCalls[0];
    expect(call.url).toBe("https://relay.example.com/api/account/v1/login/consume");
    expect(call.init?.method).toBe("POST");
    expect(JSON.parse(call.init?.body as string)).toEqual({ code: "valid-code" });
    expect(call.url).not.toContain(sessionToken);
    expect(call.url).not.toContain("valid-code");

    expect(getStoredAccountSessionToken()).toBe(sessionToken);
  });

  it("consumeLogin throws typed AccountSessionError on invalid code", async () => {
    await expect(consumeLogin(origin, "bad-code")).rejects.toThrowError(AccountSessionError);
    try {
      await consumeLogin(origin, "bad-code");
    } catch (err) {
      expect((err as AccountSessionError).code).toBe("UNAUTHORIZED");
      expect((err as AccountSessionError).status).toBe(401);
    }
  });

  it("listMachines sends GET to /api/account/v1/machines with bearer token", async () => {
    const machines = await listMachines(origin, sessionToken);
    expect(machines.length).toBe(1);
    expect(machines[0].machineRecordId).toBe("rec-mach-1");

    expect(fetchCalls.length).toBe(1);
    const call = fetchCalls[0];
    expect(call.url).toBe("https://relay.example.com/api/account/v1/machines");
    expect(call.init?.headers).toEqual({ Authorization: `Bearer ${sessionToken}` });
    expect(call.url).not.toContain(sessionToken);
  });

  it("listMachines throws typed AccountSessionError on 401 unauthorized", async () => {
    await expect(listMachines(origin, "invalid-token")).rejects.toThrowError(AccountSessionError);
    try {
      await listMachines(origin, "invalid-token");
    } catch (err) {
      expect((err as AccountSessionError).code).toBe("UNAUTHORIZED");
      expect((err as AccountSessionError).status).toBe(401);
    }
  });

  it("requestGrant sends POST to /api/account/v1/machines/{id}/grant with camelCase body", async () => {
    const machine: AccountMachineView = {
      machineRecordId: "rec-mach-1",
      machineId: "mach-uuid-1",
      displayName: "Test Laptop",
      publicKey: "pub",
      attachPublicKey: "attach-pub",
      relayOrigin: origin,
      platform: "macos",
      online: true,
      enrollmentEpoch: "2",
      lastSeenAt: 1700000000,
    };

    const grant = await requestGrant(origin, sessionToken, machine, "phone-attach-pub-key");
    expect(grant.grantId).toBe("grant-1");
    expect(grant.pairingToken).toBe(pairingToken);

    expect(fetchCalls.length).toBe(1);
    const call = fetchCalls[0];
    expect(call.url).toBe("https://relay.example.com/api/account/v1/machines/rec-mach-1/grants");
    expect(call.init?.method).toBe("POST");
    expect((call.init?.headers as Record<string, string>).Authorization).toBe(`Bearer ${sessionToken}`);

    const body = JSON.parse(call.init?.body as string);
    expect(body.machineRecordId).toBe("rec-mach-1");
    expect(body.enrollmentEpoch).toBe("2");
    expect(body.grantScope).toBe("mirror");
    expect(body.attachPublicKey).toBe("phone-attach-pub-key");
    expect(typeof body.deviceLabel).toBe("string");
    expect(typeof body.installationId).toBe("string");

    expect(call.url).not.toContain(sessionToken);
    expect(call.url).not.toContain(pairingToken);
  });

  it("requestGrant throws typed MACHINE_NOT_FOUND when machine returns 404", async () => {
    const missingMachine: AccountMachineView = {
      machineRecordId: "rec-mach-missing",
      machineId: "mach-missing",
      displayName: "Missing",
      publicKey: "pub",
      attachPublicKey: "attach",
      relayOrigin: origin,
      platform: "linux",
      online: false,
      enrollmentEpoch: "1",
      lastSeenAt: 0,
    };

    try {
      await requestGrant(origin, sessionToken, missingMachine, "key");
      expect.unreachable("should have thrown");
    } catch (err) {
      expect(err instanceof AccountSessionError).toBe(true);
      expect((err as AccountSessionError).code).toBe("MACHINE_NOT_FOUND");
      expect((err as AccountSessionError).status).toBe(404);
    }
  });

  it("allocateSession sends POST to /api/v1/attach/session returning sessionId", async () => {
    const alloc = await allocateSession(origin, sessionToken, "mach-uuid-1");
    expect(alloc.sessionId).toBe("sess-alloc-123");

    expect(fetchCalls.length).toBe(1);
    const call = fetchCalls[0];
    expect(call.url).toBe("https://relay.example.com/api/v1/attach/session");
    expect(call.init?.method).toBe("POST");
    expect((call.init?.headers as Record<string, string>).Authorization).toBe(`Bearer ${sessionToken}`);
    expect(JSON.parse(call.init?.body as string)).toEqual({ machineId: "mach-uuid-1" });

    expect(call.url).not.toContain(sessionToken);
  });

  it("allocateSession throws typed MACHINE_OFFLINE when machine is offline", async () => {
    globalThis.fetch = vi.fn().mockImplementation(() => {
      return Promise.resolve(
        new Response(
          JSON.stringify({ code: "MACHINE_OFFLINE", message: "Target machine is offline" }),
          { status: 503 },
        ),
      );
    });

    try {
      await allocateSession(origin, sessionToken, "mach-offline");
      expect.unreachable("should have thrown");
    } catch (err) {
      expect(err instanceof AccountSessionError).toBe(true);
      expect((err as AccountSessionError).code).toBe("MACHINE_OFFLINE");
      expect((err as AccountSessionError).status).toBe(503);
    }
  });

  it("allocateSession throws typed ALLOCATE_SESSION_NOT_FOUND on 404", async () => {
    globalThis.fetch = vi.fn().mockImplementation(() => {
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    try {
      await allocateSession(origin, sessionToken, "mach-any");
      expect.unreachable("should have thrown");
    } catch (err) {
      expect(err instanceof AccountSessionError).toBe(true);
      expect((err as AccountSessionError).code).toBe("ALLOCATE_SESSION_NOT_FOUND");
      expect((err as AccountSessionError).status).toBe(404);
    }
  });

  it("openTunnel delegates to openAccountTunnel with correct socketUrl and parameters", async () => {
    const mockTunnelTransport = {
      fetchLike: vi.fn(),
      openWebSocket: vi.fn(),
      close: vi.fn(),
    };
    const spy = vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
      transport: mockTunnelTransport as any,
      close: vi.fn(),
    });

    const localKeyPair = {
      publicKey: "local-pub-32-bytes",
      privateKey: "local-priv-32-bytes",
    };

    const res = await openTunnel({
      relayOrigin: origin,
      machineId: "mach-uuid-1",
      enrollmentEpoch: "2",
      machineAttachPublicKey: "machine-attach-pub-key-1",
      localKeyPair,
      sessionId: "sess-alloc-123",
    });

    expect(spy).toHaveBeenCalledTimes(1);
    const callArgs = spy.mock.calls[0][0];
    expect(callArgs.socketUrl).toBe("wss://relay.example.com/tunnel/opaque/sess-alloc-123");
    expect(callArgs.socketUrl).not.toContain("?");
    expect(callArgs.socketUrl).not.toContain("sessionId=");
    expect(callArgs.socketUrl).not.toContain("attachKey=");
    expect(callArgs.machineId).toBe("mach-uuid-1");
    expect(callArgs.enrollmentEpoch).toBe("2");
    expect(callArgs.machineAttachPublicKey).toBe("machine-attach-pub-key-1");
    expect(callArgs.sessionId).toBe("sess-alloc-123");
    expect(res.transport).toBe(mockTunnelTransport);
  });

  it("redeemInTunnel sends POST /api/v1/pair/exchange inside tunnel and receives device token", async () => {
    const mockTransport = {
      fetchLike: vi.fn().mockImplementation((path: string, init?: any) => {
        expect(path).toBe("/api/v1/pair/exchange");
        expect(init.method).toBe("POST");
        const body = JSON.parse(init.body);
        expect(body.code).toBe(pairingToken);
        expect(body.deviceName).toBe("My Phone");
        expect(body.installationId).toBe("install-uuid-9");

        const responsePayload = {
          token: "device-bearer-token-live-456",
          device: {
            id: "dev-1",
            name: "My Phone",
            accessScope: "mirror",
            permission: "control",
          },
          machineId: "mach-uuid-1",
          displayName: "Test Laptop",
        };
        const bytes = new TextEncoder().encode(JSON.stringify(responsePayload));
        return Promise.resolve({
          status: 200,
          headers: { "content-type": "application/json" },
          body: bytes,
        });
      }),
      openWebSocket: vi.fn(),
      close: vi.fn(),
    };

    const redeemed = await redeemInTunnel(
      mockTransport as any,
      pairingToken,
      "My Phone",
      "install-uuid-9",
    );

    expect(redeemed.token).toBe("device-bearer-token-live-456");
    expect(redeemed.machineId).toBe("mach-uuid-1");
    expect(redeemed.displayName).toBe("Test Laptop");
    expect(mockTransport.fetchLike).toHaveBeenCalledTimes(1);
  });

  it("pairing token and account session token never appear in any request URL", async () => {
    await requestLogin(origin, "user@example.com");
    await consumeLogin(origin, "valid-code");
    await listMachines(origin, sessionToken);
    await requestGrant(
      origin,
      sessionToken,
      {
        machineRecordId: "rec-mach-1",
        machineId: "mach-uuid-1",
        displayName: "Test",
        publicKey: "pub",
        attachPublicKey: "attach",
        relayOrigin: origin,
        platform: "macos",
        online: true,
        enrollmentEpoch: "1",
        lastSeenAt: 0,
      },
      "attach-key",
    );
    await allocateSession(origin, sessionToken, "mach-uuid-1");

    for (const call of fetchCalls) {
      expect(call.url).not.toContain(sessionToken);
      expect(call.url).not.toContain(pairingToken);
      expect(call.url).not.toContain("token=");
      expect(call.url).not.toContain("ticket=");
    }
  });
});
