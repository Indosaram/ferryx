import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteApp, desktopContextSyncDecision } from "./RemoteApp";
import { normalizeRemoteWorkspaceState } from "./RemoteSessionList";
import { clearStoredAccountSessionToken, storeAccountSessionToken } from "./accountSession";
import * as accountSessionModule from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
import type {
  FetchLikeInit,
  TunnelCloseEvent,
  TunnelErrorEvent,
  TunnelMessageEvent,
  TunnelResponse,
  TunnelTransport,
} from "./attachTunnel";
import { remoteHostStore } from "../state/remoteHostStore";

const MACHINE_ID = "f47ac10b-58cc-4372-a567-0e02b2c3d479";
const TOKEN = "acct-session-91b";
const SID_FIRST = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1";
const SID_SECOND = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb2";
const SID_DESKTOP = "cccccccc-cccc-4ccc-8ccc-ccccccccccc3";
const EPOCH = "1790742255752";

function machine() {
  return {
    machineRecordId: "rec-1",
    machineId: MACHINE_ID,
    displayName: "Desktop Mac",
    publicKey: "pk",
    attachPublicKey: "apk",
    relayOrigin: window.location.origin,
    platform: "linux",
    online: true,
    enrollmentEpoch: "1",
    lastSeenAt: Date.now(),
  };
}

function session(sessionId: string, title: string, extra: Record<string, unknown> = {}) {
  return {
    sessionId,
    workspaceId: "ws-ferryx",
    title,
    shell: "/bin/zsh",
    running: true,
    pid: 4321,
    exitCode: null,
    worktree: { wsId: "ws-ferryx", slug: "main" },
    target: { machineId: MACHINE_ID, sessionId: sessionId, daemonEpoch: EPOCH },
    ...extra,
  };
}

class MockSocket {
  static instances: MockSocket[] = [];
  url: string;
  readyState = 1;
  binaryType = "arraybuffer";
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: TunnelMessageEvent) => void) | null = null;
  onclose: ((event: TunnelCloseEvent) => void) | null = null;
  onerror: ((event: TunnelErrorEvent) => void) | null = null;
  send = vi.fn();
  close = vi.fn(() => {
    this.readyState = 3;
    this.onclose?.({ code: 1000, reason: "", wasClean: true });
  });
  constructor(url: string) {
    this.url = url;
    MockSocket.instances.push(this);
  }
}

let timeline: string[] = [];
let fetchImpl: typeof fetch;
let stateBody: unknown;
let sessionsPayload: unknown;
let socketsFactory: () => any;
let deferStateResponse = false;
let stateDeferreds: Array<{ promise: Promise<TunnelResponse>; resolve: (value: TunnelResponse) => void }> = [];
let openedSocketPaths: string[] = [];
let eventSockets: MockSocket[] = [];
let worktreeIdentitySlug: string | null = "main";

function addPath(input: RequestInfo | URL, init?: RequestInit) {
  const raw =
    typeof input === "string"
      ? input
      : input instanceof Request
        ? input.url
        : String((input as URL).href ?? input);
  let path: string;
  try {
    path = raw.startsWith("http") ? new URL(raw).pathname + new URL(raw).search : raw;
  } catch {
    path = raw;
  }
  const method = String(init?.method ?? "GET").toUpperCase();
  timeline.push(`${method} ${path}`);
}

function jsonResponse(body: unknown, ok = true): Response {
  return { ok, status: ok ? 200 : 500, json: vi.fn(async () => body) } as unknown as Response;
}

function respond(value: unknown, status = 200): TunnelResponse {
  return {
    status,
    headers: {},
    body: new TextEncoder().encode(JSON.stringify(value ?? null)),
  };
}

function freshActiveContext(worktreeSlug: string | null = "main") {
  return {
    activeContext: {
      workspaceId: "ws-ferryx",
      worktreeSlug,
      worktreeLabel: worktreeSlug,
      sessionId: SID_DESKTOP,
      tabId: "tab-desktop-active",
      terminalTabs: [
        { id: "tab-desktop-active", label: "desktop tab", sessionId: SID_DESKTOP },
      ],
    },
    daemonEpoch: EPOCH,
  };
}

function armState(body: unknown = freshActiveContext()) {
  stateBody = body;
}

function stateGetCount() {
  return timeline.filter(
    (entry) => entry.startsWith("GET ") && entry.includes("/api/v1/workspace/state"),
  ).length;
}

function sessionsGetCount() {
  return timeline.filter(
    (entry) => entry.startsWith("GET ") && entry.includes("/api/v1/sessions"),
  ).length;
}

function postSelectPaths() {
  return timeline.filter((entry) => entry.includes("/api/v1/workspace/select"));
}

function terminalPaths() {
  return openedSocketPaths.filter((path) => path.startsWith("/api/v1/terminal/"));
}

function sessionRow(sessionId: string): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-session-id="${sessionId}"]`);
}

function marked(element: HTMLElement | null): boolean {
  return (
    element !== null &&
    (element.getAttribute("aria-current") === "true" ||
      element.getAttribute("aria-selected") === "true")
  );
}

async function mountAccountApp() {
  storeAccountSessionToken(TOKEN, window.location.origin);
  render(<RemoteApp />);
  await waitFor(() => {
    expect(screen.getByRole("button", { name: /Change workspace context/i })).toBeInTheDocument();
  });
  await waitFor(() => {
    expect(
      timeline.some((entry) => entry.startsWith("GET ") && entry.endsWith("/api/v1/sessions")),
    ).toBe(true);
  });
}

function inventoryRows(): HTMLElement[] {
  return [...screen.queryAllByTestId("remote-session-row"), ...screen.queryAllByRole("tab")];
}

async function ensureSessionRowsVisible(): Promise<void> {
  const trigger = await waitFor(() =>
    screen.getByRole("button", { name: /Change workspace context/i }),
  );
  if (inventoryRows().length === 0 && trigger.getAttribute("aria-expanded") !== "true") {
    act(() => {
      fireEvent.click(trigger);
    });
  }
}

async function pickSession(sessionId: string) {
  await ensureSessionRowsVisible();
  const row = await waitFor(() => {
    const found = document.querySelector<HTMLElement>(`[data-session-id="${sessionId}"]`);
    if (!found) throw new Error(`session row ${sessionId} not rendered`);
    return found;
  });
  act(() => {
    fireEvent.click(row);
  });
  fireEvent.click(await screen.findByTestId("remote-view-mode-terminal"));
}

async function pickWorktree() {
  await ensureSessionRowsVisible();
  const matches = await waitFor(() => {
    const found = screen.queryAllByRole("button", { name: "ws-ferryx / main" });
    if (found.length === 0) throw new Error("worktree row not rendered");
    return found;
  });
  act(() => {
    fireEvent.click(matches[0]);
  });
  fireEvent.click(await screen.findByTestId("remote-view-mode-terminal"));
}

async function eventsSocket(): Promise<MockSocket> {
  await waitFor(() => {
    expect(eventSockets.length).toBeGreaterThan(0);
  });
  return eventSockets[eventSockets.length - 1];
}

function inventoryBoundary() {
  return JSON.stringify({
    type: "inventoryInvalidated",
    sequence: "9001",
    reason: "session_started",
    payload: {
      completeness: "complete",
      sessions: {
        revision: "9001",
        completeness: "complete",
        sessions: sessionsPayload,
      },
    },
  });
}

beforeEach(() => {
  clearStoredAccountSessionToken();
  localStorage.clear();
  remoteHostStore.reset();
  timeline = [];
  MockSocket.instances = [];
  deferStateResponse = false;
  stateDeferreds = [];
  openedSocketPaths = [];
  eventSockets = [];
  worktreeIdentitySlug = "main";
  sessionsPayload = [
    session(SID_FIRST, "first shell"),
    session(SID_SECOND, "second shell"),
    session(SID_DESKTOP, "desktop active shell"),
  ];
  armState(freshActiveContext());
  socketsFactory = () => new MockSocket("ws://stub");
  vi.stubGlobal("WebSocket", socketsFactory as unknown as typeof WebSocket);
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query === "(min-width: 768px)",
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent: () => false,
  }));
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    x: 0, y: 0, top: 0, left: 0, right: 800, bottom: 400, width: 800, height: 400,
    toJSON: () => ({}),
  } as DOMRect);

  fetchImpl = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    addPath(input, init);
    const raw =
      typeof input === "string"
        ? input
        : input instanceof Request
          ? input.url
          : String((input as URL).href ?? input);
    const path = raw.startsWith("http") ? new URL(raw).pathname + new URL(raw).search : raw;
    if (path.endsWith("/api/account/v1/machines") || path.startsWith("/api/v1/machines")) {
      return jsonResponse([machine()]);
    }
    if (path.includes("/grants")) {
      return jsonResponse({
        grantId: "grant-1",
        machineId: MACHINE_ID,
        relayOrigin: window.location.origin,
        pairingToken: "pairing-token",
        machineAttachPublicKey: "machine-attach-key",
        grantScope: "machine",
        expiresAt: Date.now() + 600_000,
      });
    }
    if (path.includes("/api/v1/attach/session")) {
      return jsonResponse({ sessionId: "alloc-1" });
    }
    if (path.includes("/api/v1/socket-ticket")) {
      return jsonResponse({ ticket: "ui-test-ticket", expiresAt: Date.now() + 60_000 });
    }
    return jsonResponse({}, false);
  });
  vi.stubGlobal("fetch", fetchImpl);

  const fetchLike = vi.fn(
    async (path: string, init?: FetchLikeInit): Promise<TunnelResponse> => {
      timeline.push(`${init?.method ?? "GET"} ${path}`);
      if (path.startsWith("/api/v1/pair/exchange")) {
        return respond({
          token: "tunnel-device-token",
          device: { id: "dev-1", name: "Phone" },
          machineId: MACHINE_ID,
          displayName: "Desktop Mac",
        });
      }
      if (path === "/api/v1/workspace/projects") {
        return respond({
          revision: "1",
          completeness: "complete",
          projects: [
            { workspaceId: "ws-ferryx", repoRoot: "/srv/ferryx", availability: "ready", revision: "1" },
          ],
          unavailableWorkspaceIds: [],
        });
      }
      if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
        return respond({
          revision: "1",
          worktrees: [
            {
              workspaceId: "ws-ferryx",
              identity: worktreeIdentitySlug === null ? null : { wsId: "ws-ferryx", slug: worktreeIdentitySlug },
              path: "/srv/ferryx",
              head: "h1",
              branch: "refs/heads/main",
              bare: false,
              detached: false,
              locked: null,
              prunable: null,
              managed: false,
            },
          ],
        });
      }
      if (path === "/api/v1/sessions") {
        if ((init?.method ?? "GET") === "POST") {
          return respond({ revision: "1", ...session(SID_FIRST, "spawned session") });
        }
        return respond({ revision: "1", completeness: "complete", sessions: sessionsPayload });
      }
      if (path.startsWith("/api/v1/workspace/state")) {
        if (deferStateResponse) {
          let resolve!: (value: TunnelResponse) => void;
          const promise = new Promise<TunnelResponse>((r) => {
            resolve = r;
          });
          stateDeferreds.push({ promise, resolve });
          return promise;
        }
        return respond(stateBody);
      }
      if (path.startsWith("/api/v1/agent/")) {
        return respond({ messages: [] });
      }
      return respond({}, 404);
    },
  );

  const transport: TunnelTransport = {
    fetchLike,
    openWebSocket: async (pathAndQuery: string) => {
      const socket = new MockSocket(`ws://tunnel${pathAndQuery}`);
      openedSocketPaths.push(pathAndQuery);
      if (!pathAndQuery.startsWith("/api/v1/terminal/")) eventSockets.push(socket);
      return socket;
    },
    close: () => {},
  };

  vi.spyOn(accountSessionModule, "openTunnel").mockResolvedValue({ transport, close: () => {} });
  vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({ transport, close: () => {} });
  vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
    publicKey: "initiator-pk",
    privateKey: "initiator-sk",
  });
});

afterEach(() => {
  cleanup();
  clearStoredAccountSessionToken();
  localStorage.clear();
  remoteHostStore.reset();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("desktop context sync policy", () => {
  const fresh = {
    hasToken: true,
    connectionMachineId: "mach-omarchy-01",
    contextMachineId: "mach-omarchy-01",
    targetInFlight: false,
    independentSessionChosen: false,
  };

  it("accepts the fresh chosen machine context", () => {
    expect(desktopContextSyncDecision(fresh)).toBe(true);
  });

  it("refuses a connection for a machine other than the chosen context machine", () => {
    expect(
      desktopContextSyncDecision({ ...fresh, connectionMachineId: "mach-maho-root-00" }),
    ).toBe(false);
    expect(
      desktopContextSyncDecision({ ...fresh, connectionMachineId: null, contextMachineId: null }),
    ).toBe(false);
  });

  it("refuses while a stored target is still in flight", () => {
    expect(desktopContextSyncDecision({ ...fresh, targetInFlight: true })).toBe(false);
  });

  it("refuses once a session was chosen independently", () => {
    expect(desktopContextSyncDecision({ ...fresh, independentSessionChosen: true })).toBe(false);
  });

  it("refuses without a token", () => {
    expect(desktopContextSyncDecision({ ...fresh, hasToken: false })).toBe(false);
  });
});

describe("read-only desktop context sync", () => {
  it("keeps the initial picker blank until an explicit choice", async () => {
    await mountAccountApp();
    expect(terminalPaths()).toHaveLength(0);
    expect(postSelectPaths()).toHaveLength(0);
    expect(stateGetCount()).toBe(0);
  });

  it("follows the desktop active session inside a freshly chosen worktree, not the first row", async () => {
    armState(freshActiveContext("main"));
    const normalized = normalizeRemoteWorkspaceState(stateBody);
    expect(normalized.context.workspaceId).toBe("ws-ferryx");
    expect(normalized.context.worktreeSlug).toBe("main");
    expect(normalized.context.activeTerminal?.sessionId).toBe(SID_DESKTOP);
    expect(normalized.context.activeTabId).toBe("tab-desktop-active");
    expect(normalized.context.daemonEpoch).toBe(EPOCH);
    await mountAccountApp();
    expect(stateGetCount()).toBe(0);

    await pickWorktree();

    try {
      await waitFor(() => {
        expect(stateGetCount()).toBeGreaterThanOrEqual(1);
      });
      await waitFor(() => {
        expect(terminalPaths().some((entry) => entry.includes(SID_DESKTOP))).toBe(true);
      });
    } catch {
      const rows = [SID_FIRST, SID_SECOND, SID_DESKTOP]
        .map((sid) => {
          const row = sessionRow(sid);
          return `${sid}=${row ? (marked(row) ? "marked" : "row") : "absent"}`;
        })
        .join(" ");
      throw new Error(
        `positive attach missing: stateReads=${stateGetCount()} dials=${JSON.stringify(openedSocketPaths)} rows=[${rows}] timeline=${JSON.stringify(timeline)}`,
      );
    }

    expect(postSelectPaths()).toHaveLength(0);
    expect(stateGetCount()).toBeGreaterThanOrEqual(1);

    await ensureSessionRowsVisible();
    expect(marked(sessionRow(SID_DESKTOP))).toBe(true);
    expect(marked(sessionRow(SID_FIRST))).toBe(false);
  });

  it("never replaces an explicitly chosen session with the desktop active one", async () => {
    armState(freshActiveContext("main"));
    await mountAccountApp();
    await pickSession(SID_SECOND);
    await waitFor(() => {
      expect(terminalPaths().some((entry) => entry.includes(SID_SECOND))).toBe(true);
    });
    await waitFor(() => {
      expect(marked(sessionRow(SID_SECOND))).toBe(true);
    });

    expect(terminalPaths().some((entry) => entry.includes(SID_DESKTOP))).toBe(false);
    expect(postSelectPaths()).toHaveLength(0);
    expect(marked(sessionRow(SID_DESKTOP))).toBe(false);
  });

  it("fences an active context whose worktree is null against the chosen worktree", async () => {
    armState(freshActiveContext(null));
    deferStateResponse = true;
    await mountAccountApp();
    await pickWorktree();

    await waitFor(() => {
      expect(stateDeferreds.length).toBeGreaterThanOrEqual(1);
    });
    const sessionsBefore = sessionsGetCount();
    await act(async () => {
      for (const deferred of stateDeferreds.splice(0)) deferred.resolve(respond(stateBody));
    });
    // act flush guarantees the deferred read was consumed; a valid context whose
    // worktree is exactly null must be fenced before it is ever applied. The follow path
    // may spend one validation read on /api/v1/sessions, but it must reject the row.
    expect(normalizeRemoteWorkspaceState(stateBody).context.worktreeSlug).toBeNull();
    expect(sessionsGetCount()).toBeLessThanOrEqual(sessionsBefore + 1);
    expect(stateGetCount()).toBeGreaterThanOrEqual(1);

    await waitFor(() => {
      expect(marked(sessionRow(SID_DESKTOP))).toBe(false);
    });
    expect(terminalPaths().some((entry) => entry.includes(SID_DESKTOP))).toBe(false);
    expect(postSelectPaths()).toHaveLength(0);
  });

  it("follows the desktop active session when the desktop context and the choice are both the repository root", async () => {
    armState({
      activeContext: {
        workspaceId: "ws-ferryx",
        worktreeSlug: null,
        worktreeLabel: "main",
        sessionId: SID_DESKTOP,
        tabId: "tab-desktop-active",
        terminalTabs: [
          { id: "tab-desktop-active", label: "desktop tab", sessionId: SID_DESKTOP },
        ],
      },
      daemonEpoch: EPOCH,
    });
    worktreeIdentitySlug = null;
    sessionsPayload = [
      session(SID_FIRST, "first shell", { worktree: null }),
      session(SID_SECOND, "second shell", { worktree: null }),
      session(SID_DESKTOP, "desktop active shell", { worktree: null }),
    ];
    await mountAccountApp();
    const normalized = normalizeRemoteWorkspaceState(stateBody);
    expect(normalized.context.workspaceId).toBe("ws-ferryx");
    expect(normalized.context.worktreeSlug).toBeNull();
    expect(normalized.context.activeTerminal?.sessionId).toBe(SID_DESKTOP);
    expect(normalized.context.daemonEpoch).toBe(EPOCH);

    await pickWorktree();

    await waitFor(() => {
      expect(stateGetCount()).toBeGreaterThanOrEqual(1);
    });
    await waitFor(() => {
      expect(terminalPaths().some((entry) => entry.includes(SID_DESKTOP))).toBe(true);
    });
    expect(marked(sessionRow(SID_DESKTOP))).toBe(true);
    expect(marked(sessionRow(SID_FIRST))).toBe(false);
    expect(postSelectPaths()).toHaveLength(0);
  });

  it("re-reads the desktop context after a same-machine inventory event", async () => {
    armState(freshActiveContext("main"));
    await mountAccountApp();
    await pickWorktree();

    await waitFor(() => {
      expect(terminalPaths().some((entry) => entry.includes(SID_DESKTOP))).toBe(true);
    });
    const baseline = stateGetCount();
    expect(baseline).toBeGreaterThanOrEqual(1);

    const socket = await eventsSocket();
    await act(async () => {
      socket.onmessage?.({ data: inventoryBoundary() });
    });

    await waitFor(() => {
      expect(stateGetCount()).toBeGreaterThan(baseline);
    });
    expect(postSelectPaths()).toHaveLength(0);
  });
});

describe("always-follow desktop focus", () => {
  function focusOn(sessionId: string) {
    return {
      activeContext: {
        workspaceId: "ws-ferryx",
        worktreeSlug: "main",
        worktreeLabel: "main",
        sessionId,
        tabId: `tab-${sessionId}`,
        terminalTabs: [{ id: `tab-${sessionId}`, label: "tab", sessionId }],
      },
      daemonEpoch: EPOCH,
    };
  }
  function focusChanged() {
    return JSON.stringify({
      type: "desktopSelectionChanged",
      sequence: "9100",
      revision: "9100",
      workspaceId: "ws-ferryx",
      sessionId: SID_DESKTOP,
      payload: { workspaceId: "ws-ferryx", worktreeSlug: "main" },
    });
  }

  it("replaces a manual pick when the desktop publishes a new focus, without selecting desktop focus", async () => {
    armState(focusOn(SID_FIRST));
    await mountAccountApp();
    await pickSession(SID_SECOND);
    await waitFor(() => {
      expect(terminalPaths().some((entry) => entry.includes(SID_SECOND))).toBe(true);
    });
    // The focus the manual pick connected under is only a baseline: it never overrides the pick.
    expect(terminalPaths().some((entry) => entry.includes(SID_DESKTOP))).toBe(false);

    armState(focusOn(SID_DESKTOP));
    const socket = await eventsSocket();
    await act(async () => {
      socket.onmessage?.({ data: focusChanged() });
    });

    await waitFor(() => {
      expect(terminalPaths().some((entry) => entry.includes(SID_DESKTOP))).toBe(true);
    });
    await ensureSessionRowsVisible();
    await waitFor(() => {
      expect(marked(sessionRow(SID_DESKTOP))).toBe(true);
    });
    expect(postSelectPaths()).toHaveLength(0);
  });

  it("does not follow a desktop session the machine inventory cannot confirm", async () => {
    armState(focusOn(SID_FIRST));
    await mountAccountApp();
    await pickSession(SID_SECOND);
    await waitFor(() => {
      expect(terminalPaths().some((entry) => entry.includes(SID_SECOND))).toBe(true);
    });

    armState(focusOn("dddddddd-dddd-4ddd-8ddd-dddddddddddd"));
    const reads = sessionsGetCount();
    const socket = await eventsSocket();
    await act(async () => {
      socket.onmessage?.({ data: focusChanged() });
    });

    await waitFor(() => {
      expect(sessionsGetCount()).toBeGreaterThan(reads);
    });
    expect(terminalPaths().every((entry) => !entry.includes("dddddddd"))).toBe(true);
    expect(marked(sessionRow(SID_SECOND))).toBe(true);
    expect(postSelectPaths()).toHaveLength(0);
  });
});
