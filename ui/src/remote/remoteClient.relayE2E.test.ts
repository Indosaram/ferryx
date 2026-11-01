import { afterEach, describe, expect, it, vi } from "vitest";
import type { HostEndpoint } from "../state/remoteHostStore";
import type { OpenTunnelParams } from "./accountSession";
import type { TunnelTransport, TunnelWebSocket } from "./attachTunnel";
import {
  allocateE2ESession,
  CLIENT_BUILD_HEADER,
  openRelayE2ETransport,
  RELAY_E2E_AWARE_HEADER,
  RelayE2EError,
  RelayE2EUnsupportedError,
  registerRelayE2E,
  REPAIR_REQUIRED,
  RelayRepairRequiredError,
  relayE2EState,
  resetRelayE2EFallbacks,
} from "./relayE2E";
import { remoteFetch, remoteOpenSocket, remoteSocketUrl, RemoteRequestTimeoutError, REMOTE_REQUEST_TIMEOUT_MS } from "./remoteClient";

const RELAY = "https://relay.example.test";
const MACHINE = "machine-1";
const BASE = `${RELAY}/host/${MACHINE}`;

function fakeSocket(): TunnelWebSocket {
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
}

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}

describe("relay end-to-end encryption", () => {
  const unregister: Array<() => void> = [];

  afterEach(() => {
    for (const fn of unregister.splice(0)) fn();
    resetRelayE2EFallbacks();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("reaches /host/ APIs and sockets only inside the encrypted tunnel", async () => {
    const globalFetch = vi.fn(async () => jsonResponse(500, {}));
    const GlobalSocket = vi.fn();
    vi.stubGlobal("fetch", globalFetch);
    vi.stubGlobal("WebSocket", GlobalSocket);

    let sessions = 0;
    const allocate = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      jsonResponse(200, { sessionId: `sess-${++sessions}`, machineId: MACHINE, opaque: true }));
    const tunnelPaths: string[] = [];
    const openedSessions: string[] = [];
    const openTunnel = vi.fn(async (params: OpenTunnelParams) => {
      openedSessions.push(params.sessionId);
      const transport: TunnelTransport = {
        fetchLike: vi.fn(async (path: string) => {
          tunnelPaths.push(path);
          return { status: 200, headers: { "content-type": "application/json" }, body: new TextEncoder().encode("[]") };
        }),
        openWebSocket: vi.fn(async (path: string) => {
          tunnelPaths.push(path);
          return fakeSocket();
        }),
        close: vi.fn(),
        isClosed: false,
      };
      return { transport, close: vi.fn() };
    });

    const connection = openRelayE2ETransport({
      relayOrigin: RELAY,
      machineId: MACHINE,
      enrollmentEpoch: "7",
      machineAttachPublicKey: "bWFjaGluZS1wdWJsaWMta2V5LTMyLWJ5dGVzLWxvbmchIQ==",
      attachKey: { publicKey: "pub", privateKey: "priv" },
      bearer: "device-token",
      deps: { fetch: allocate as unknown as typeof fetch, openTunnel },
    });
    unregister.push(registerRelayE2E(BASE, connection), () => connection.close());

    const response = await remoteFetch(BASE, "/api/v1/sessions");
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual([]);
    const socket = await remoteOpenSocket(BASE, "/api/v1/events", "device-token");
    expect(socket.readyState).toBe(1);

    expect(globalFetch).not.toHaveBeenCalled();
    expect(GlobalSocket).not.toHaveBeenCalled();
    expect(tunnelPaths).toEqual(["/api/v1/sessions", "/api/v1/events"]);
    // HTTP shares one session; the WebSocket gets its own.
    expect(openedSessions).toEqual(["sess-1", "sess-2"]);
    expect(allocate).toHaveBeenCalledTimes(2);
    for (const [input, init] of allocate.mock.calls) {
      expect(String(input)).toBe(`${RELAY}/api/v1/attach/session?e2e=1`);
      expect(String(input)).not.toContain("/host/");
      expect(init?.method).toBe("POST");
      expect((init?.headers as Record<string, string>)[CLIENT_BUILD_HEADER]).toBeTruthy();
      expect(JSON.parse(String(init?.body))).toEqual({ machineId: MACHINE });
    }
  });

  it("reports REPAIR_REQUIRED for a relay host without attach info", () => {
    const host: HostEndpoint = {
      hostId: BASE,
      name: "Machine",
      address: RELAY,
      transport: "relay",
      machineId: MACHINE,
      relayOrigin: RELAY,
      online: true,
      authStatus: "paired",
    };
    expect(relayE2EState(host, RELAY)).toBe(REPAIR_REQUIRED);
    expect(relayE2EState({ ...host, attachPublicKey: "key", enrollmentEpoch: "7" }, RELAY)).toBe("e2e");
    expect(relayE2EState(host, "http://192.168.1.20:7777")).toBe("direct");
  });

  it("rejects a plaintext 426 E2E_REQUIRED answer with RelayRepairRequiredError", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse(426, {
      error: { code: "E2E_REQUIRED", message: "refused", retryable: false, requestId: "", details: {} },
    })));
    const failure = remoteFetch(BASE, "/api/v1/sessions");
    await expect(failure).rejects.toBeInstanceOf(RelayRepairRequiredError);
    await expect(failure).rejects.toMatchObject({ code: REPAIR_REQUIRED });
  });

  it.each([
    ["404", () => new Response("not found", { status: 404 })],
    ["405", () => new Response("method not allowed", { status: 405 })],
    ["401 without a JSON code from an old relay", () => new Response("unauthorized", { status: 401 })],
    ["401 UNAUTHORIZED from an old account relay", () => jsonResponse(401, { code: "UNAUTHORIZED" })],
  ])("falls back to plaintext when the relay answers %s to ?e2e=1", async (_label, answer) => {
    const globalFetch = vi.fn(async () => jsonResponse(200, { ok: true }));
    vi.stubGlobal("fetch", globalFetch);
    const allocate = vi.fn(async () => answer());
    const openTunnel = vi.fn();
    const connection = openRelayE2ETransport({
      relayOrigin: RELAY,
      machineId: MACHINE,
      enrollmentEpoch: "7",
      machineAttachPublicKey: "key",
      attachKey: { publicKey: "pub", privateKey: "priv" },
      bearer: "device-token",
      deps: { fetch: allocate as unknown as typeof fetch, openTunnel },
    });
    unregister.push(registerRelayE2E(BASE, connection), () => connection.close());

    expect((await remoteFetch(BASE, "/api/v1/health")).status).toBe(200);
    expect((await remoteFetch(BASE, "/api/v1/health")).status).toBe(200);
    expect(allocate).toHaveBeenCalledTimes(1);
    expect(openTunnel).not.toHaveBeenCalled();
    expect(globalFetch).toHaveBeenCalledTimes(2);
  });

  it("does not fall back when the relay requires E2E or the machine is offline", async () => {
    const globalFetch = vi.fn(async () => jsonResponse(200, { ok: true }));
    vi.stubGlobal("fetch", globalFetch);
    for (const [status, code] of [
      [426, "E2E_REQUIRED"],
      [409, "MACHINE_OFFLINE"],
      [402, "REMOTE_SUSPENDED"],
    ] as const) {
      const allocate = vi.fn(async () => jsonResponse(status, { code }));
      const connection = openRelayE2ETransport({
        relayOrigin: RELAY,
        machineId: MACHINE,
        enrollmentEpoch: "7",
        machineAttachPublicKey: "key",
        attachKey: { publicKey: "pub", privateKey: "priv" },
        bearer: "device-token",
        deps: { fetch: allocate as unknown as typeof fetch, openTunnel: vi.fn() },
      });
      const unregisterBase = registerRelayE2E(BASE, connection);
      await expect(remoteFetch(BASE, "/api/v1/health")).rejects.toMatchObject({
        code: status === 426 ? REPAIR_REQUIRED : code,
      });
      unregisterBase();
      connection.close();
    }
    expect(globalFetch).not.toHaveBeenCalled();
  });

  function connectionWith(allocate: () => Promise<Response>) {
    return openRelayE2ETransport({
      relayOrigin: RELAY,
      machineId: MACHINE,
      enrollmentEpoch: "7",
      machineAttachPublicKey: "key",
      attachKey: { publicKey: "pub", privateKey: "priv" },
      bearer: "device-token",
      deps: { fetch: vi.fn(allocate) as unknown as typeof fetch, openTunnel: vi.fn() },
    });
  }

  it.each([400, 401, 404, 405])(
    "never downgrades a relay that marks itself e2e-aware when it answers %s",
    async (status) => {
      const globalFetch = vi.fn(async () => jsonResponse(200, { ok: true }));
      vi.stubGlobal("fetch", globalFetch);
      const connection = connectionWith(async () => new Response(JSON.stringify({ code: status === 401 ? "UNAUTHORIZED" : "BAD_REQUEST" }), {
        status,
        headers: { "content-type": "application/json", [RELAY_E2E_AWARE_HEADER]: "1" },
      }));
      unregister.push(registerRelayE2E(BASE, connection), () => connection.close());

      const error = await remoteFetch(BASE, "/api/v1/health").then(() => null, (err: unknown) => err);
      expect(error).toBeInstanceOf(RelayE2EError);
      expect(error).not.toBeInstanceOf(RelayE2EUnsupportedError);
      expect(error).toMatchObject({ status });
      if (status === 401) expect(error).toMatchObject({ code: "UNAUTHORIZED" });
      expect(globalFetch).not.toHaveBeenCalled();
    },
  );

  it("reports and clears a plaintext fallback per registration", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse(200, { ok: true })));
    const host: HostEndpoint = {
      hostId: BASE,
      name: "Machine",
      address: RELAY,
      transport: "relay",
      machineId: MACHINE,
      relayOrigin: RELAY,
      online: true,
      authStatus: "paired",
      attachPublicKey: "key",
      enrollmentEpoch: "7",
    };
    const old = connectionWith(async () => new Response("not found", { status: 404 }));
    const unregisterOld = registerRelayE2E(BASE, old);
    expect(relayE2EState(host, RELAY)).toBe("e2e");
    expect((await remoteFetch(BASE, "/api/v1/health")).status).toBe(200);
    expect(relayE2EState(host, RELAY)).toBe("plaintext-fallback");
    unregisterOld();
    old.close();

    // When the host is registered again (e.g. after a token change)
    const fresh = connectionWith(async () => jsonResponse(409, { code: "MACHINE_OFFLINE" }));
    unregister.push(registerRelayE2E(BASE, fresh), () => fresh.close());

    // Then encryption is tried again instead of staying on plaintext
    expect(relayE2EState(host, RELAY)).toBe("e2e");
    await expect(remoteFetch(BASE, "/api/v1/health")).rejects.toMatchObject({ code: "MACHINE_OFFLINE" });
  });

  it("times out an allocation whose response body stalls", async () => {
    vi.useFakeTimers();
    try {
      // Given a relay that sends headers and then never finishes the body
      const allocate = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
        const body = new ReadableStream<Uint8Array>({
          start(controller) {
            init?.signal?.addEventListener("abort", () => controller.error(new DOMException("aborted", "AbortError")), { once: true });
          },
        });
        return new Response(body, { status: 200, headers: { "content-type": "application/json" } });
      });
      const result = expect(
        allocateE2ESession(RELAY, "device-token", MACHINE, allocate as unknown as typeof fetch),
      ).rejects.toMatchObject({ code: "REQUEST_TIMEOUT" });

      // When the allocation deadline passes
      await vi.advanceTimersByTimeAsync(15_000);

      // Then the stalled body is a typed timeout
      await result;
    } finally {
      vi.useRealTimers();
    }
  });

  it("closes a socket whose upgrade finished after the connection was closed", async () => {
    let finishUpgrade!: (socket: TunnelWebSocket) => void;
    const socket = fakeSocket();
    const tunnelClose = vi.fn();
    let upgradeStarted!: () => void;
    const started = new Promise<void>((resolve) => (upgradeStarted = resolve));
    const openTunnel = vi.fn(async () => ({
      transport: {
        fetchLike: vi.fn(),
        openWebSocket: vi.fn(() => {
          upgradeStarted();
          return new Promise<TunnelWebSocket>((resolve) => {
            finishUpgrade = resolve;
          });
        }),
        close: vi.fn(),
        isClosed: false,
      } as unknown as TunnelTransport,
      close: tunnelClose,
    }));
    const connection = openRelayE2ETransport({
      relayOrigin: RELAY,
      machineId: MACHINE,
      enrollmentEpoch: "7",
      machineAttachPublicKey: "key",
      attachKey: { publicKey: "pub", privateKey: "priv" },
      bearer: "device-token",
      deps: {
        fetch: vi.fn(async () => jsonResponse(200, { sessionId: "sess-1" })) as unknown as typeof fetch,
        openTunnel,
      },
    });
    const pending = connection.openWebSocket("/api/v1/events");
    await started;

    // When the connection closes mid-upgrade and the upgrade then completes
    connection.close();
    finishUpgrade(socket);

    // Then the late socket and its tunnel are released and the caller is told
    await expect(pending).rejects.toMatchObject({ code: "CONNECTION_CLOSED" });
    expect(socket.close).toHaveBeenCalled();
    expect(tunnelClose).toHaveBeenCalled();
  });
});

describe("plain host request deadlines", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  function hangingFetch() {
    return vi.fn((_input: RequestInfo | URL, init?: RequestInit) => new Promise<Response>((_resolve, reject) => {
      init?.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")), { once: true });
    }));
  }

  it.each([
    ["socket ticket", () => remoteSocketUrl(BASE, "/api/v1/events", "device-token", new AbortController().signal)],
    ["plaintext remoteFetch", () => remoteFetch(BASE, "/api/v1/workspace/state", { signal: new AbortController().signal })],
  ])("bounds a %s request that never answers", async (_label, start) => {
    vi.useFakeTimers();
    vi.stubGlobal("fetch", hangingFetch());
    const result = expect(start()).rejects.toBeInstanceOf(RemoteRequestTimeoutError);
    await vi.advanceTimersByTimeAsync(REMOTE_REQUEST_TIMEOUT_MS);
    await result;
  });

  it("still honours the caller's own abort before the deadline", async () => {
    vi.stubGlobal("fetch", hangingFetch());
    const controller = new AbortController();
    const pending = remoteFetch(BASE, "/api/v1/workspace/state", { signal: controller.signal });
    controller.abort();
    const error = await pending.then(() => null, (err: unknown) => err);
    expect(error).not.toBeInstanceOf(RemoteRequestTimeoutError);
    expect((error as Error).name).toBe("AbortError");
  });
});
