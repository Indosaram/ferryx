import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createRemoteHostStore, remoteHostKey, remoteHostStore, REMOTE_HOST_STORAGE_KEY } from "../state/remoteHostStore";
import { hostTransportUrl, remoteSocketUrl } from "./remoteClient";
import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({ transportUrl, onTransportFailure }: { transportUrl: string; onTransportFailure?: () => void }) => (
    <button data-testid="terminal-transport" onClick={onTransportFailure}>{transportUrl}</button>
  ),
}));

class EventSocket {
  static instances: EventSocket[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  close = vi.fn();
  constructor(readonly url: string) { EventSocket.instances.push(this); }
}

const workspace = () => new Response(JSON.stringify({
  activeContext: { workspaceId: "workspace", sessionId: "terminal", terminalTabs: [] },
  projects: [], sessions: [],
}));

function ticketed(inner: (input: any, init?: any) => any): typeof fetch {
  return vi.fn<typeof fetch>(async (input, init) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.includes("/api/v1/socket-ticket")) {
      return new Response(JSON.stringify({ ticket: "ui-test-ticket", expiresAt: 9999999999 }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    return inner(input, init);
  }) as unknown as typeof fetch;
}

afterEach(() => {
  cleanup();
  remoteHostStore.reset();
  localStorage.clear();
  window.history.replaceState(null, "", "/");
  EventSocket.instances = [];
  vi.unstubAllGlobals();
});

it("switches API and event sockets to the selected host without reusing local credentials", async () => {
  localStorage.setItem(`ferryx_remote_token_local:${window.location.origin}`, "local-token");
  localStorage.setItem("ferryx_remote_token_host-b", "host-b-token");
  remoteHostStore.setHosts([{ hostId: "host-b", name: "B", address: "http://192.168.1.9:8787", transport: "mdns", authStatus: "paired", online: true }]);
  const fetcher = vi.fn(async () => workspace());
  vi.stubGlobal("fetch", ticketed(fetcher));
  vi.stubGlobal("WebSocket", EventSocket);
  await act(async () => { render(<RemoteApp />); });
  const first = EventSocket.instances[0];
  await act(async () => { remoteHostStore.setActiveHost("host-b"); });
  expect(first.close).toHaveBeenCalledOnce();
  expect(fetcher).toHaveBeenLastCalledWith("http://192.168.1.9:8787/api/v1/workspace/state", { headers: { Authorization: "Bearer host-b-token" } });
  expect(EventSocket.instances.at(-1)?.url).toBe("ws://192.168.1.9:8787/api/v1/events?ticket=ui-test-ticket");
  expect(screen.getByTestId("terminal-transport").textContent).toBe("http://192.168.1.9:8787");
});

it("routes shared-relay hosts with isolated credentials and fresh event tickets", async () => {
  const relayOrigin = "https://relay.example";
  for (const machineId of ["machine-a", "machine-b"]) remoteHostStore.upsertHost({
    machineId, relayOrigin, displayName: machineId, deviceToken: `${machineId}-token`, lastSeenAt: 1, directHints: [],
  });
  const fetcher = vi.fn(async (url: string, init?: RequestInit) => {
    if (url.endsWith("socket-ticket")) {
      const machine = url.includes("machine-a") ? "machine-a" : "machine-b";
      expect(init?.headers).toEqual({ Authorization: `Bearer ${machine}-token`, "Content-Type": "application/json" });
      expect(JSON.parse(init?.body as string)).toEqual({ target: "/api/v1/events" });
      return new Response(JSON.stringify({ ticket: `${machine}-ticket` }));
    }
    return workspace();
  });
  vi.stubGlobal("fetch", fetcher);
  vi.stubGlobal("WebSocket", EventSocket);
  remoteHostStore.setActiveHost(remoteHostKey(relayOrigin, "machine-a"));
  await act(async () => { render(<RemoteApp />); });
  const first = EventSocket.instances[0];
  expect(first.url).toBe("wss://relay.example/host/machine-a/api/v1/events?ticket=machine-a-ticket");
  await act(async () => { remoteHostStore.setActiveHost(remoteHostKey(relayOrigin, "machine-b")); });
  expect(first.close).toHaveBeenCalledOnce();
  expect(fetcher).toHaveBeenCalledWith("https://relay.example/host/machine-b/api/v1/workspace/state", {
    headers: { Authorization: "Bearer machine-b-token" },
  });
  expect(EventSocket.instances.at(-1)?.url).toBe("wss://relay.example/host/machine-b/api/v1/events?ticket=machine-b-ticket");
  const restored = createRemoteHostStore().getState();
  expect(restored.activeHostId).toBe(remoteHostKey(relayOrigin, "machine-b"));
  expect(restored.hosts[remoteHostKey(relayOrigin, "machine-a")].deviceToken).toBe("machine-a-token");
  expect(restored.hosts[remoteHostKey(relayOrigin, "machine-b")].deviceToken).toBe("machine-b-token");
});

it("requests a new target-bound ticket for every terminal dial without exposing the token", async () => {
  const fetcher = vi.fn(async () => new Response(JSON.stringify({ ticket: `ticket-${fetcher.mock.calls.length}` })));
  vi.stubGlobal("fetch", fetcher);
  const host = { hostId: "a", name: "A", address: "https://relay.example", machineId: "machine-a", relayOrigin: "https://relay.example", transport: "relay" as const, authStatus: "paired" as const, online: true };
  const base = hostTransportUrl(host, host.relayOrigin);
  const first = await remoteSocketUrl(base, "/api/v1/terminal/session-a", "secret");
  const second = await remoteSocketUrl(base, "/api/v1/terminal/session-a", "secret");
  expect(first).toBe("wss://relay.example/host/machine-a/api/v1/terminal/session-a?ticket=ticket-1");
  expect(second).toContain("ticket=ticket-2");
  expect(fetcher).toHaveBeenCalledWith(`${base}/api/v1/socket-ticket`, {
    method: "POST", headers: { Authorization: "Bearer secret", "Content-Type": "application/json" },
    body: JSON.stringify({ target: "/api/v1/terminal/session-a" }), signal: undefined,
  });
});

it("never includes device token or token/access_token parameters in socket URLs for direct or relay transports", async () => {
  const fetcher = vi.fn(async (url: string | Request) => {
    const targetUrl = String(url instanceof Request ? url.url : url);
    if (targetUrl.includes("/api/v1/socket-ticket")) {
      return new Response(JSON.stringify({ ticket: "regression-ticket-123", expiresAt: 9999999999 }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    return new Response("not found", { status: 404 });
  });
  vi.stubGlobal("fetch", fetcher);

  const deviceToken = "super-secret-device-token-xyz";
  const transports = [
    { name: "relay transport", baseUrl: "https://relay.example/host/machine-a", target: "/api/v1/events" },
    { name: "direct transport", baseUrl: "http://192.168.1.9:8787", target: "/api/v1/events" },
    { name: "direct terminal", baseUrl: "http://192.168.1.9:8787", target: "/api/v1/terminal/session-1" },
    { name: "relay terminal", baseUrl: "https://relay.example/host/machine-a", target: "/api/v1/terminal/session-1" },
  ];

  for (const { name, baseUrl, target } of transports) {
    const socketUrl = await remoteSocketUrl(baseUrl, target, deviceToken);
    const parsed = new URL(socketUrl);

    expect(socketUrl, `${name} must not contain device token`).not.toContain(deviceToken);
    expect(parsed.searchParams.has("token"), `${name} must not contain token query param`).toBe(false);
    expect(parsed.searchParams.has("access_token"), `${name} must not contain access_token query param`).toBe(false);
    expect(parsed.searchParams.get("ticket"), `${name} must contain ticket query param`).toBe("regression-ticket-123");
  }
});

it("migrates a single-host inventory and its scoped token without borrowing an origin-wide token", () => {
  localStorage.setItem("ferryx_remote_token", "unrelated-token");
  localStorage.setItem("ferryx_remote_token_old-a", "a-token");
  localStorage.setItem(REMOTE_HOST_STORAGE_KEY, JSON.stringify({
    hostId: "old-a", machineId: "a", displayName: "A", relayOrigin: "https://relay.example",
  }));
  const store = createRemoteHostStore();
  const key = remoteHostKey("https://relay.example", "a");
  expect(store.getState().hosts[key].deviceToken).toBe("a-token");
  store.upsertHost({ machineId: "b", displayName: "B", relayOrigin: "https://relay.example", deviceToken: null, lastSeenAt: null, directHints: [] });
  expect(store.getState().hosts[remoteHostKey("https://relay.example", "b")].deviceToken).toBeNull();
  store.setHosts([]);
  expect(store.getState().hosts[key].deviceToken).toBe("a-token");
});

it("parses fragment PIN and hints separately and keeps transport on the relay", async () => {
  window.history.replaceState(null, "", "/#pair=123456&hints=" + encodeURIComponent("http://192.168.1.20:8787,https://evil.example"));
  // The real gateway health route is unauthenticated and returns only { status, version }.
  // It therefore cannot prove the responder owns the paired machine identity.
  const fetcher = vi.fn(async (url: string, _init?: RequestInit) => {
    if (url.includes("pair/exchange")) return new Response(JSON.stringify({ token: "paired" }));
    if (url.endsWith("/api/v1/health")) {
      return new Response(JSON.stringify({ status: "ok", version: "0.1.0" }), { status: 200 });
    }
    return workspace();
  });
  vi.stubGlobal("fetch", ticketed(fetcher));
  vi.stubGlobal("WebSocket", EventSocket);
  await act(async () => { render(<RemoteApp />); });
  const pairing = fetcher.mock.calls.find(([url]) => url.includes("pair/exchange"));
  expect(pairing).toBeDefined();
  const init = (pairing as unknown as [string, RequestInit])[1];
  expect(JSON.parse(init.body as string).code).toBe("123456");
  expect(fetcher.mock.calls.some(([url]) => url.includes("evil.example"))).toBe(false);
  expect(localStorage.getItem(`ferryx_remote_direct_candidates_local:${window.location.origin}`)).not.toContain("evil.example");

  // A reachable-but-unverified LAN hint must NOT capture the terminal transport: a
  // health 200 proves reachability only, so the credential-bearing transport stays
  // on the relay origin.
  expect(screen.getByTestId("terminal-transport").textContent).toBe(window.location.origin);

  // The probe itself must never carry the device credential to an unverified endpoint.
  const healthCalls = fetcher.mock.calls.filter(([url]) => String(url).endsWith("/api/v1/health"));
  expect(healthCalls.length).toBeGreaterThan(0);
  for (const [url, requestInit] of healthCalls as unknown as [string, RequestInit][]) {
    expect(new Headers(requestInit?.headers).get("Authorization")).toBeNull();
    expect(String(url)).not.toContain("paired");
  }

  await act(async () => { screen.getByTestId("terminal-transport").click(); });
  expect(screen.getByTestId("terminal-transport").textContent).toBe(window.location.origin);
  expect(EventSocket.instances.at(-1)?.url).toContain(window.location.host);
});
