import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { remoteHostStore } from "../state/remoteHostStore";
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
  vi.stubGlobal("fetch", fetcher);
  vi.stubGlobal("WebSocket", EventSocket);
  await act(async () => { render(<RemoteApp />); });
  const first = EventSocket.instances[0];
  await act(async () => { remoteHostStore.setActiveHost("host-b"); });
  expect(first.close).toHaveBeenCalledOnce();
  expect(fetcher).toHaveBeenLastCalledWith("http://192.168.1.9:8787/api/v1/workspace/state?token=host-b-token");
  expect(EventSocket.instances.at(-1)?.url).toBe("ws://192.168.1.9:8787/api/v1/events?token=host-b-token");
  expect(screen.getByTestId("terminal-transport").textContent).toBe("http://192.168.1.9:8787");
});

it("parses fragment PIN and hints separately and rolls a failed terminal back to relay", async () => {
  window.history.replaceState(null, "", "/#pair=123456&hints=" + encodeURIComponent("http://192.168.1.20:8787,https://evil.example"));
  const fetcher = vi.fn(async (url: string) => {
    if (url.includes("pair/exchange")) return new Response(JSON.stringify({ token: "paired" }));
    if (url.endsWith("/api/v1/health")) return new Response("ok");
    return workspace();
  });
  vi.stubGlobal("fetch", fetcher);
  vi.stubGlobal("WebSocket", EventSocket);
  await act(async () => { render(<RemoteApp />); });
  const pairing = fetcher.mock.calls.find(([url]) => url.includes("pair/exchange"));
  expect(pairing).toBeDefined();
  const init = (pairing as unknown as [string, RequestInit])[1];
  expect(JSON.parse(init.body as string).code).toBe("123456");
  expect(fetcher.mock.calls.some(([url]) => url.includes("evil.example"))).toBe(false);
  expect(screen.getByTestId("terminal-transport").textContent).toBe("http://192.168.1.20:8787");
  expect(localStorage.getItem(`ferryx_remote_direct_candidates_local:${window.location.origin}`)).not.toContain("evil.example");
  await act(async () => { screen.getByTestId("terminal-transport").click(); });
  expect(screen.getByTestId("terminal-transport").textContent).toBe(window.location.origin);
  expect(EventSocket.instances.at(-1)?.url).toContain(window.location.host);
});
