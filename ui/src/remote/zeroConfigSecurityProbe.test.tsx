// Permanent security regressions derived from the 2026-09-10 final audit probes.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, onTestFailed, vi } from "vitest";

/**
 * Failure-only diagnostics for the post-merge run: the fetch order (method + PATHNAME only) and the
 * terminal-related selector state at the point of failure. Pathnames only - query strings can carry
 * tickets and tokens - and no header, body or DOM dump is ever read or printed. Bounded to 40 calls
 * per mock. Registered through onTestFailed, so a passing test prints nothing and the original
 * assertion error is untouched.
 */
/**
 * Writes a diagnostic line straight to the process stdout. The JSON reporter does not implement
 * onUserConsoleLog, so a console.log never reaches a --reporter=json receipt; process.stdout does.
 */
function emitLine(line: string): void {
  try {
    process.stdout.write(`${line}\n`);
  } catch {
    /* a diagnostic must never change the outcome of the test it reports on */
  }
}

/**
 * The report a failure should carry: the fetch order (method + PATHNAME only) and the presence of
 * the selectors these suites look for. Pathnames only - query strings can carry tickets and tokens
 * - and no header, body or DOM dump is ever read or printed. Bounded to 40 calls per mock.
 */
function buildFailureTrace(label: string, ...mocks: unknown[]): string[] {
  const lines: string[] = [];
  try {
    mocks.forEach((mock, mockIndex) => {
      const calls = (mock as { mock?: { calls?: unknown[][] } })?.mock?.calls ?? [];
      const shown = calls.slice(0, 40).map((args, index) => {
        const raw = String(args[0] instanceof Request ? args[0].url : args[0]);
        let pathname = "(unparseable-url)";
        try {
          pathname = new URL(raw, "http://localhost").pathname;
        } catch {
          /* never print the raw value */
        }
        const init = args[1] as RequestInit | undefined;
        return `${index + 1} ${(init?.method ?? "GET").toUpperCase()} ${pathname}`;
      });
      const more = calls.length > 40 ? ` (+${calls.length - 40} more)` : "";
      lines.push(
        `[ui-diag] ${label} | mock${mockIndex + 1} order (${calls.length}): ${shown.join(" | ") || "(none)"}${more}`,
      );
    });
    const selectors = ["remote-view-mode-terminal", "remote-terminal", "remote-terminal-grid", "mobile-chat-workspace"]
      .map((id) => `${id}=${document.querySelector(`[data-testid="${id}"]`) ? "present" : "absent"}`)
      .join(", ");
    const trigger = document.querySelector('button[aria-label="Change workspace context"]') ? "present" : "absent";
    lines.push(`[ui-diag] ${label} | selectors: ${selectors}, context-trigger=${trigger}`);
  } catch {
    /* a diagnostic must never change the outcome of the test it reports on */
  }
  return lines;
}

/**
 * Emit the report NOW. Call it from a catch block at the assertion boundary: vitest runs
 * onTestFailed AFTER afterEach (which here does cleanup() plus the configured
 * clearMocks/restoreMocks), so a report built inside the hook sees an empty document and zero
 * recorded calls. A report frozen before an await can also miss requests that arrive while waiting.
 */
function emitFailureTrace(label: string, ...mocks: unknown[]): void {
  for (const line of buildFailureTrace(label, ...mocks)) emitLine(line);
}

/** Freeze the report now and emit it only if the test fails - for boundaries that normally pass. */
function captureOnFailure(label: string, ...mocks: unknown[]): void {
  const frozen = buildFailureTrace(label, ...mocks);
  onTestFailed(() => {
    for (const line of frozen) emitLine(line);
  });
}

import { remoteHostStore, remoteHostKey } from "../state/remoteHostStore";
import { RemoteApp } from "./RemoteApp";

/** A deadline that fails loudly and names what never arrived; never a synchronization delay. */
async function bounded<T>(signal: Promise<T>, what: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      signal,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(`Timed out waiting for ${what}`)), 2000);
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

class Socket {
  static instances: Socket[] = [];
  /**
   * Waiters for the next terminal socket. The terminal is loaded lazily and the socket is opened
   * asynchronously, so a caller must subscribe BEFORE the action that opens it and await the event,
   * instead of reading Socket.instances immediately afterwards.
   */
  static waiters: Array<(socket: Socket) => void> = [];
  static readonly OPEN = 1;
  readyState = 0;
  binaryType = "arraybuffer";
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  close = vi.fn();
  send = vi.fn();
  constructor(readonly url: string) {
    Socket.instances.push(this);
    if (url.includes("/terminal/")) {
      const waiters = Socket.waiters;
      Socket.waiters = [];
      for (const resolve of waiters) resolve(this);
    }
  }

  /** Resolves with the next terminal socket, whenever it opens. Subscribe before triggering. */
  static whenTerminalOpened(): Promise<Socket> {
    return new Promise<Socket>((resolve) => { Socket.waiters.push(resolve); });
  }
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
  Socket.waiters = [];
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
  // Chat is the default surface: the real terminal socket is only opened in terminal mode. The
  // open is asynchronous, so subscribe to the exact event BEFORE triggering the switch and await
  // it - reading Socket.instances straight after the click races the lazy terminal chunk.
  const socketOpened = Socket.whenTerminalOpened();
  await act(async () => {});
  await act(async () => {
    fireEvent.click(screen.getByTestId("remote-view-mode-terminal"));
  });
  await bounded(socketOpened, "the terminal socket to open");
  captureOnFailure("zero-config terminal socket URL", fetch);
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

// F09b: a permanent device token must never appear in any request URL. URLs are
// recorded in browser history, proxy/server access logs and Referer headers, so a
// credential placed there outlives the request and escapes the client's control.
it("never places the device token in an HTTP or WebSocket request URL", async () => {
  const fetch = fetcher();
  paired();
  await mount(fetch);

  const tokens = ["audit-device-a", "audit-device"];
  const httpUrls = fetch.mock.calls.map(([url]) => String(url));
  expect(httpUrls.length).toBeGreaterThan(0);
  for (const url of httpUrls) {
    for (const secret of tokens) {
      expect(url).not.toContain(secret);
    }
    expect(url).not.toMatch(/[?&](token|access_token)=/);
  }

  // Authenticated calls must carry the credential in the Authorization header instead.
  const authenticated = fetch.mock.calls.filter(([, init]) =>
    new Headers((init as RequestInit | undefined)?.headers).has("Authorization"));
  expect(authenticated.length).toBeGreaterThan(0);

  for (const socket of Socket.instances) {
    for (const secret of tokens) {
      expect(socket.url).not.toContain(secret);
    }
    expect(socket.url).not.toMatch(/[?&](token|access_token)=/);
  }
});

// The direct-gateway terminal transport is not reached by the probe above, because
// that test only inspects sockets the mounted app opens. It is the one remaining
// place that put a PERMANENT device token in a WebSocket URL, so it needs its own
// regression: the URL must carry a single-use ticket minted over HTTP instead.
it("attaches the direct terminal socket with a single-use ticket, not the device token", async () => {
  const { WebSocketTerminalTransport } = await import("../lib/terminalTransport/remoteTransport");
  const token = "audit-device";
  const fetch = vi.fn(async (_url: RequestInfo | URL, _init?: RequestInit) =>
    new Response(JSON.stringify({ ticket: "one-shot-ticket-123", expiresAt: 99 }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    }));
  vi.stubGlobal("fetch", fetch);
  vi.stubGlobal("WebSocket", Socket);

  const transport = new WebSocketTerminalTransport("https://gateway.example", token);
  await transport.attach("session-42");

  const socket = Socket.instances.at(-1);
  expect(socket, "the transport must open a socket").toBeDefined();
  expect(socket!.url).not.toContain(token);
  expect(socket!.url).not.toMatch(/[?&](token|access_token)=/);
  expect(socket!.url).toContain("ticket=one-shot-ticket-123");

  // The bearer is spent on the ticket request, in the header rather than the URL.
  const [url, init] = fetch.mock.calls.at(-1)!;
  expect(String(url)).toContain("/api/v1/socket-ticket");
  expect(String(url)).not.toContain(token);
  expect(new Headers(init?.headers).get("Authorization")).toBe(`Bearer ${token}`);
});
