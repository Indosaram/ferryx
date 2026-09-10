// Audit-only observations of 818b68d. PASS confirms the documented gap, not approval.
// Run temporarily from ui/src/remote/ZeroConfigFinalAudit.probe.test.tsx with Vitest.
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { remoteHostStore, remoteHostKey } from "../state/remoteHostStore";
import { RemoteApp } from "./RemoteApp";
vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({ transportUrl }: { transportUrl: string }) => <div data-testid="audit-terminal-base">{transportUrl}</div>,
}));
class Socket {
  static instances: Socket[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  close = vi.fn();
  constructor(readonly url: string) { Socket.instances.push(this); }
}
const workspace = () => new Response(JSON.stringify({
  activeContext: { workspaceId: "audit-workspace", sessionId: "audit-terminal", terminalTabs: [] }, projects: [], sessions: [],
}));
function fetcher() {
  return vi.fn(async (url: string, _init?: RequestInit) => {
    if (url.includes("socket-ticket")) return new Response(JSON.stringify({ ticket: "audit-ticket" }));
    if (url.includes("pair/exchange")) return new Response(JSON.stringify({ token: "audit-device", machineId: "audit-new", displayName: "New" }));
    if (url.endsWith("/api/v1/health")) return new Response("ok");
    return workspace();
  });
}
function paired(hints: {type: "lan"; url: string; priority: number}[] = []) {
  const origin = window.location.origin;
  remoteHostStore.upsertHost({ machineId: "audit-a", displayName: "A", relayOrigin: origin,
    deviceToken: "audit-device-a", lastSeenAt: 1, directHints: hints });
  remoteHostStore.setActiveHost(remoteHostKey(origin, "audit-a"));
}
function globals(fetch: ReturnType<typeof fetcher>) {
  vi.stubGlobal("fetch", fetch);
  vi.stubGlobal("WebSocket", Socket);
}
afterEach(() => {
  cleanup(); remoteHostStore.reset(); localStorage.clear();
  window.history.replaceState(null, "", "/"); Socket.instances = []; vi.unstubAllGlobals();
});
it("observes 128-bit QR capability ignored by the six-digit-only parser", async () => {
  window.history.replaceState(null, "", "/#pair=00112233445566778899aabbccddeeff&machine=audit-new");
  const fetch = fetcher(); globals(fetch);
  await act(async () => { render(<RemoteApp />); });
  expect(fetch.mock.calls.some(([url]) => url.includes("pair/exchange"))).toBe(false);
  expect(Object.keys(remoteHostStore.getState().hosts)).toHaveLength(0);
});
it("observes PIN request uses code and successful metadata does not create an inventory host", async () => {
  window.history.replaceState(null, "", "/#pair=123456&machine=audit-new");
  const fetch = fetcher(); globals(fetch);
  await act(async () => { render(<RemoteApp />); });
  const call = fetch.mock.calls.find(([url]) => url.includes("pair/exchange"));
  expect(call).toBeDefined();
  const body = JSON.parse(call![1]!.body as string);
  expect(body.code).toBe("123456"); expect(body.pin).toBeUndefined(); expect(body.pairingToken).toBeUndefined();
  expect(Object.keys(remoteHostStore.getState().hosts)).toHaveLength(0);
  expect(localStorage.getItem(`ferryx_remote_token_local:${window.location.origin}`)).toBe("audit-device");
});
it("observes new pairing link ignored when a host is already selected", async () => {
  paired(); window.history.replaceState(null, "", "/#pair=123456&machine=audit-b");
  const fetch = fetcher(); globals(fetch);
  await act(async () => { render(<RemoteApp />); });
  expect(fetch.mock.calls.some(([url]) => url.includes("pair/exchange"))).toBe(false);
});
it("observes terminal prop drops the machine prefix although events retain it", async () => {
  paired(); const fetch = fetcher(); globals(fetch);
  await act(async () => { render(<RemoteApp />); });
  expect(Socket.instances.some(socket => socket.url.includes("/host/audit-a/api/v1/events?ticket="))).toBe(true);
  expect(screen.getByTestId("audit-terminal-base").textContent).toBe(window.location.origin);
});
it("observes private health 200 is enough to receive a paired device credential", async () => {
  const direct = "https://192.168.1.99:8787";
  paired([{ type: "lan", url: direct, priority: 30 }]);
  const fetch = fetcher(); globals(fetch);
  await act(async () => { render(<RemoteApp />); });
  expect(fetch).toHaveBeenCalledWith(`${direct}/api/v1/workspace/state`, { headers: { Authorization: "Bearer audit-device-a" } });
  expect(Socket.instances.some(socket => socket.url.includes("192.168.1.99:8787/api/v1/events?token=audit-device-a"))).toBe(true);
});
