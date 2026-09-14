import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { RemoteApp } from "./RemoteApp";
import { RemoteTerminal } from "./RemoteTerminal";
import { remoteHostKey, remoteHostStore } from "../state/remoteHostStore";
import { getRemoteAuthToken, setRemoteAuthToken } from "../lib/remoteClient";
import { FALLBACK_PREFERENCES, resetTerminalPreferencesCache, saveTerminalSettings } from "../lib/terminalSettings";

const runtime = vi.hoisted(() => ({ desktop: false, invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => runtime.desktop, invoke: runtime.invoke }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

// A timeout is only a failing deadline, never a synchronization delay.
async function bounded<T>(signal: Promise<T>): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([signal, new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error("Expected preferences signal did not arrive")), 1500);
    })]);
  } finally { clearTimeout(timer); }
}

function appearance(fontSize: number, background: string) {
  return { ...FALLBACK_PREFERENCES, fontFamily: `HostFont${fontSize}`, fontSize,
    theme: { ...FALLBACK_PREFERENCES.theme, background }, source: "ghostty", status: "imported" };
}

function expectAppearance(fontSize: number, background: string) {
  const terminal = screen.getByTestId("remote-terminal-grid");
  expect(terminal.style.fontSize).toBe(`${fontSize}px`);
  expect(terminal.style.backgroundColor).toBe(background);
}

class Socket {
  static OPEN = 1;
  readyState = 0;
  onopen = null;
  onclose = null;
  onmessage = null;
  close = vi.fn();
  send = vi.fn();
}

function transport() {
  const requests: Array<{ url: string; authorization: string | null }> = [];
  const gates = new Map<string, ReturnType<typeof deferred<Response>>>();
  const arrivals = new Map<string, ReturnType<typeof deferred<void>>>();
  const gate = (base: string) => {
    const response = deferred<Response>();
    const arrived = deferred<void>();
    gates.set(base, response);
    arrivals.set(base, arrived);
    return { response, arrived };
  };
  vi.stubGlobal("WebSocket", Socket);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/v1/terminal/preferences")) {
      const base = url.slice(0, -"/api/v1/terminal/preferences".length);
      requests.push({ url, authorization: new Headers(init?.headers).get("Authorization") });
      const pending = gates.get(base);
      if (!pending) throw new Error(`Unexpected preferences request: ${url}`);
      arrivals.get(base)!.resolve();
      return (await pending.promise).clone();
    }
    if (url.endsWith("/socket-ticket")) return Response.json({ ticket: "preference-contract-ticket" });
    if (url.endsWith("/workspace/state")) return Response.json({
      activeContext: { workspaceId: "workspace", sessionId: "terminal", terminalTabs: [] },
      projects: [], sessions: [],
    });
    throw new Error(`Unexpected transport request: ${url}`);
  }));
  return { gate, requests };
}

async function release(gate: ReturnType<ReturnType<typeof transport>["gate"]>, fontSize: number, background: string) {
  await act(async () => {
    gate.response.resolve(Response.json(appearance(fontSize, background)));
    await gate.response.promise;
  });
}

afterEach(() => {
  cleanup();
  remoteHostStore.reset();
  localStorage.clear();
  resetTerminalPreferencesCache();
  document.documentElement.style.removeProperty("--terminal");
  document.documentElement.style.removeProperty("--terminal-rgb");
  window.history.replaceState(null, "", "/");
  runtime.desktop = false;
  runtime.invoke.mockReset();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("migrates real scoped auth and loads only the selected host's preferences across gated host switches", async () => {
  const { gate, requests } = transport();
  const origin = window.location.origin;
  const local = gate(origin);
  const relay = "https://relay.example";
  const baseA = `${relay}/host/machine-a`;
  const baseB = `${relay}/host/machine-b`;
  const a = gate(baseA);
  const b = gate(baseB);
  setRemoteAuthToken("legacy-device");
  for (const machineId of ["machine-a", "machine-b"]) remoteHostStore.upsertHost({
    machineId, relayOrigin: relay, displayName: machineId, deviceToken: `${machineId}-device`, lastSeenAt: 1, directHints: [],
  });
  await act(async () => { render(<RemoteApp />); });
  await bounded(local.arrived.promise);
  expect(getRemoteAuthToken()).toBeNull();
  expect(getRemoteAuthToken(`local:${origin}`)).toBe("legacy-device");
  await release(local, 17, "#112233");
  expectAppearance(17, "rgb(17, 34, 51)");

  await act(async () => { remoteHostStore.setActiveHost(remoteHostKey(relay, "machine-a")); });
  await bounded(a.arrived.promise);
  expect(screen.getByTestId("remote-terminal-grid").style.fontSize).not.toBe("17px");
  await act(async () => { remoteHostStore.setActiveHost(remoteHostKey(relay, "machine-b")); });
  await bounded(b.arrived.promise);
  await release(b, 23, "#445566");
  expectAppearance(23, "rgb(68, 85, 102)");
  await release(a, 19, "#778899");
  expectAppearance(23, "rgb(68, 85, 102)");
  expect(document.documentElement.style.getPropertyValue("--terminal")).toBe("#445566");

  const returnedA = gate(baseA);
  await act(async () => { remoteHostStore.setActiveHost(remoteHostKey(relay, "machine-a")); });
  await bounded(returnedA.arrived.promise);
  expect(screen.getByTestId("remote-terminal-grid").style.fontSize).not.toBe("23px");
  await release(returnedA, 19, "#778899");
  expectAppearance(19, "rgb(119, 136, 153)");
  expect(requests.length).toBeGreaterThanOrEqual(4);
  for (const request of requests) {
    const expected = request.url.startsWith(baseA) ? "machine-a-device"
      : request.url.startsWith(baseB) ? "machine-b-device" : "legacy-device";
    expect(request.authorization).toBe(`Bearer ${expected}`);
    expect([origin, baseA, baseB].some((base) => request.url === `${base}/api/v1/terminal/preferences`)).toBe(true);
  }
});

it("rejects late preferences after an in-place credential change and retains local override precedence", async () => {
  const { gate, requests } = transport();
  const base = "https://relay.example/host/machine-a";
  const old = gate(base);
  const view = render(<RemoteTerminal sessionId="terminal" token="old-device" transportUrl={base} />);
  await bounded(old.arrived.promise);
  const current = gate(base);
  await act(async () => { view.rerender(<RemoteTerminal sessionId="terminal" token="new-device" transportUrl={base} />); });
  await bounded(current.arrived.promise);
  await release(current, 25, "#223344");
  expectAppearance(25, "rgb(34, 51, 68)");
  await release(old, 18, "#556677");
  expectAppearance(25, "rgb(34, 51, 68)");
  act(() => { saveTerminalSettings({ fontSize: 21, fontFamily: "LocalFont", scrollback: 10000 }); });
  expectAppearance(21, "rgb(34, 51, 68)");
  expect(screen.getByTestId("remote-terminal-grid").style.fontFamily).toBe("LocalFont");
  expect(requests.some((r) => r.authorization === "Bearer old-device")).toBe(true);
  expect(requests.at(-1)?.authorization).toBe("Bearer new-device");
});

it("keeps desktop-embedded appearance local even with remote transport credentials", async () => {
  const { requests } = transport();
  runtime.desktop = true;
  const invoked = deferred<void>();
  runtime.invoke.mockImplementation(async (command: string) => {
    if (command === "cmd_terminal_preferences") { invoked.resolve(); return appearance(22, "#334455"); }
    if (command === "cmd_terminal_apply_overrides") return;
    throw new Error(`Unexpected native command: ${command}`);
  });
  await act(async () => { render(<RemoteTerminal embedded sessionId="terminal" token="remote-device" transportUrl="https://relay.example/host/machine-a" />); });
  await bounded(invoked.promise);
  expectAppearance(22, "rgb(51, 68, 85)");
  expect(requests).toEqual([]);
});
