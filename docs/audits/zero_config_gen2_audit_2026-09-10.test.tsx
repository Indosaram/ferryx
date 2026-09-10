// Audit observations of 28032e3; PASS demonstrates the named credential-release decision.
// This uses the actual RemoteApp with controlled fetch/socket boundaries, not a real browser network.
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { RemoteApp } from "./RemoteApp";
import { remoteHostKey, remoteHostStore } from "../state/remoteHostStore";

class Socket {
  static readonly OPEN = 1;
  static instances: Socket[] = [];
  readyState = 0;
  binaryType = "arraybuffer";
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  close = vi.fn();
  send = vi.fn();
  constructor(readonly url: string) { Socket.instances.push(this); }
}

afterEach(() => {
  cleanup(); remoteHostStore.reset(); localStorage.clear();
  window.history.replaceState(null, "", "/");
  Socket.instances = []; vi.restoreAllMocks(); vi.unstubAllGlobals();
});

it.each(["wrong-machine", "unauthorized"])(
  "observes bearer released before a %s candidate can be rejected", async (result) => {
    const direct = "https://192.168.1.99:8787";
    const credential = "synthetic-victim-device-token";
    const origin = window.location.origin;
    remoteHostStore.upsertHost({ machineId: "expected-machine", displayName: "Expected machine",
      relayOrigin: origin, deviceToken: credential, lastSeenAt: 1,
      directHints: [{ type: "lan", url: direct, priority: 30 }] });
    remoteHostStore.setActiveHost(remoteHostKey(origin, "expected-machine"));
    let finishProbe!: (response: Response) => void;
    const probe = new Promise<Response>((resolve) => { finishProbe = resolve; });
    const fetcher = vi.fn((url: string, _init?: RequestInit) => {
      if (url === `${direct}/api/v1/health`) return probe;
      if (url.includes("socket-ticket")) return Promise.resolve(Response.json({ ticket: "synthetic-ticket" }));
      return Promise.resolve(Response.json({ activeContext: null, projects: [], sessions: [] }));
    });
    vi.stubGlobal("fetch", fetcher); vi.stubGlobal("WebSocket", Socket);
    await act(async () => { render(<RemoteApp />); });
    await waitFor(() => expect(fetcher.mock.calls.some(([url]) => url === `${direct}/api/v1/health`)).toBe(true));
    const request = fetcher.mock.calls.find(([url]) => url === `${direct}/api/v1/health`)!;
    // No response, identity proof, or success verdict has been delivered yet.
    expect(new Headers(request[1]?.headers).get("Authorization")).toBe(`Bearer ${credential}`);
    expect(request[1]).toMatchObject({ credentials: "omit", redirect: "error" });
    await act(async () => {
      finishProbe(result === "wrong-machine" ? Response.json({ machineId: "impostor" }) : new Response(null, { status: 401 }));
      await probe;
    });
    expect(fetcher.mock.calls.filter(([url]) => url.startsWith(direct) && !url.endsWith("/health"))).toHaveLength(0);
    expect(Socket.instances.every(({ url }) => !url.includes("192.168.1.99"))).toBe(true);
  },
);
