// Permanent security regressions derived from the 2026-09-10 final audit probes.
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { remoteHostStore, remoteHostKey } from "../state/remoteHostStore";
import { RemoteApp } from "./RemoteApp";

class Socket {
  static instances: Socket[] = [];
  static readonly OPEN = 1;
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

function fetcher() {
  return vi.fn(async (url: string, _init?: RequestInit) => {
    if (url.includes("socket-ticket")) return Response.json({ ticket: "audit-ticket" });
    if (url.includes("pair/exchange")) return Response.json({
      token: "audit-device", device: { id: "audit-device-id", name: "Browser Device", permission: "control" },
      machineId: "audit-new", displayName: "New",
    });
    if (url.endsWith("/api/v1/health")) {
      return Response.json({ status: "ok", version: "0.1.0" });
    }
    return Response.json({
      activeContext: { workspaceId: "audit-workspace", sessionId: "audit-terminal", terminalTabs: [] },
      projects: [], sessions: [],
    });
  });
}

function paired(hints: { type: "lan"; url: string; priority: number }[] = [], machineId = "audit-a") {
  const origin = window.location.origin;
  remoteHostStore.upsertHost({ machineId, displayName: "A", relayOrigin: origin,
    deviceToken: "audit-device-a", lastSeenAt: 1, directHints: hints });
  remoteHostStore.setActiveHost(remoteHostKey(origin, machineId));
}

async function mount(fetch: ReturnType<typeof fetcher>) {
  vi.stubGlobal("fetch", fetch);
  vi.stubGlobal("WebSocket", Socket);
  await act(async () => { render(<RemoteApp />); });
  // Explicitly deliver the navigation signal; never depend on jsdom's queued hash event.
  await act(async () => { window.dispatchEvent(new HashChangeEvent("hashchange")); });
}

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const cell = this.hasAttribute("data-terminal-cell-measure");
    return { x: 0, y: 0, top: 0, left: 0, right: cell ? 10 : 800, bottom: cell ? 20 : 400,
      width: cell ? 10 : 800, height: cell ? 20 : 400, toJSON: () => ({}) };
  });
});
afterEach(() => {
  cleanup();
  remoteHostStore.reset();
  localStorage.clear();
  window.history.replaceState(null, "", "/");
  Socket.instances = [];
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it.each(["123456", "00112233445566778899aabbccddeeff"])(
  "parses pairing capability %s and persists exchange metadata in the host inventory", async (code) => {
    window.history.replaceState(null, "", `/#pair=${code}&machine=audit-new`);
    const fetch = fetcher();
    await mount(fetch);
    const call = fetch.mock.calls.find(([url]) => url.includes("pair/exchange"));
    expect(call).toBeDefined();
    expect(JSON.parse(call![1]!.body as string)).toMatchObject({ code, deviceName: expect.any(String) });
    const key = remoteHostKey(window.location.origin, "audit-new");
    expect(remoteHostStore.getState().hosts[key]).toMatchObject({
      machineId: "audit-new", displayName: "New", deviceToken: "audit-device", relayOrigin: window.location.origin,
    });
    expect(remoteHostStore.getState().activeHostId).toBe(key);
    expect(Object.keys(remoteHostStore.getState().hosts)).toHaveLength(1);
    expect(localStorage.getItem(`ferryx_remote_token_local:${window.location.origin}`)).toBeNull();
  },
);

it("processes a new pairing link while a host is active and upserts rather than duplicates", async () => {
  paired([], "audit-new");
  paired();
  const fetch = fetcher();
  await mount(fetch);
  await act(async () => {
    window.history.replaceState(null, "", "/#pair=123456&machine=audit-new");
    window.dispatchEvent(new HashChangeEvent("hashchange"));
  });
  await act(async () => { window.dispatchEvent(new HashChangeEvent("hashchange")); });
  expect(fetch.mock.calls.filter(([url]) => url.includes("pair/exchange"))).toHaveLength(1);
  const state = remoteHostStore.getState();
  expect(Object.keys(state.hosts)).toHaveLength(2);
  expect(state.activeHostId).toBe(remoteHostKey(window.location.origin, "audit-new"));
  expect(state.hosts[state.activeHostId!].deviceToken).toBe("audit-device");
  expect(state.hosts[remoteHostKey(window.location.origin, "audit-a")].deviceToken).toBe("audit-device-a");
});

it("retains the machine prefix, ticket and grid geometry in the real terminal socket URL", async () => {
  paired();
  const fetch = fetcher();
  await mount(fetch);
  const socket = Socket.instances.find(({ url }) => url.includes("/terminal/"));
  expect(socket).toBeDefined();
  const url = new URL(socket!.url);
  expect(url.pathname).toBe("/host/audit-a/api/v1/terminal/audit-terminal");
  expect(Object.fromEntries(url.searchParams)).toEqual({ ticket: "audit-ticket", render: "grid", cols: "80", rows: "20" });
  expect(fetch).toHaveBeenCalledWith(`${window.location.origin}/host/audit-a/api/v1/socket-ticket`, expect.objectContaining({
    headers: { Authorization: "Bearer audit-device-a", "Content-Type": "application/json" },
    body: JSON.stringify({ target: "/api/v1/terminal/audit-terminal" }),
  }));
});

it("never discloses credentials to an unverified direct candidate and stays on relay", async () => {
  const direct = "https://192.168.1.99:8787";
  paired([{ type: "lan", url: direct, priority: 30 }]);
  const fetch = fetcher();
  await mount(fetch);
  const probe = fetch.mock.calls.find(([url]) => url === `${direct}/api/v1/health`);
  expect(probe).toBeDefined();
  const [probeUrl, probeInit] = probe!;
  expect(new URL(probeUrl).search).toBe("");
  expect(new Headers(probeInit?.headers).has("Authorization")).toBe(false);
  expect(probeInit).toMatchObject({
    credentials: "omit", redirect: "error", cache: "no-store", mode: "cors",
  });
  expect(fetch.mock.calls.filter(([url]) => url.startsWith(direct) && url !== probeUrl)).toHaveLength(0);
  expect(Socket.instances.every(({ url }) => !url.includes("192.168.1.99"))).toBe(true);
});
