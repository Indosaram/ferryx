import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";
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
  TunnelWebSocket,
} from "./attachTunnel";
import { remoteHostStore } from "../state/remoteHostStore";

const MACHINE_ID = "mach-parity-1";
const OTHER_MACHINE_ID = "mach-other-9";
const EPOCH = "101";
const SID_FIRST = "s-1a-alpha";
const SID_SECOND = "s-1b-beta";

type SessionRow = {
  workspaceId: string;
  worktree: { wsId: string; slug: string } | null;
  target: { machineId: string; sessionId: string; daemonEpoch: string };
  sessionId: string;
  running: boolean;
  title: string;
};

type SessionsPayload = { revision: string; completeness: string; sessions: SessionRow[] };

function session(sessionId: string, title: string, overrides: Partial<SessionRow> = {}): SessionRow {
  return {
    workspaceId: "ws-ferryx",
    worktree: { wsId: "ws-ferryx", slug: "main" },
    target: { machineId: MACHINE_ID, sessionId, daemonEpoch: EPOCH },
    sessionId,
    running: true,
    title,
    ...overrides,
  };
}

function payload(rows: SessionRow[], completeness = "complete"): SessionsPayload {
  return { revision: String(rows.length + 1), completeness, sessions: rows };
}

function encode(value: unknown): Uint8Array {
  return new TextEncoder().encode(JSON.stringify(value));
}

function respond(value: unknown, status = 200): TunnelResponse {
  return { status, headers: { "content-type": "application/json" }, body: encode(value) };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function rect(width: number, height: number): DOMRect {
  return {
    x: 0,
    y: 0,
    width,
    height,
    top: 0,
    right: width,
    bottom: height,
    left: 0,
    toJSON: () => ({}),
  } as DOMRect;
}

class RecordingSocket implements TunnelWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  sent: (string | Uint8Array)[] = [];
  readyState = 1;
  binaryType = "arraybuffer";
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: TunnelMessageEvent) => void) | null = null;
  onerror: ((event: TunnelErrorEvent) => void) | null = null;
  onclose: ((event: TunnelCloseEvent) => void) | null = null;

  constructor(readonly path: string) {}

  send(data: Uint8Array | string) {
    this.sent.push(data);
  }

  close() {
    this.readyState = 3;
  }
}

type PendingSessionsQuery = {
  arrived: Promise<void>;
  completed: Promise<void>;
  answer: (body: SessionsPayload) => void;
};

type Harness = {
  openedSocketPaths: string[];
  timeline: () => string;
  terminalPaths: () => string[];
  sessionsQueryCount: () => number;
  postPaths: () => string[];
  sessionCreatePosts: () => string[];
  setSessions: (rows: SessionRow[], completeness?: string) => void;
  expectSessionsQuery: () => PendingSessionsQuery;
  publishInventoryEvent: (body: unknown) => void;
};

async function mountAccountHarness(initial: SessionRow[]): Promise<Harness> {
  storeAccountSessionToken("parity-account-token", window.location.origin);

  let currentPayload = payload(initial);
  let sessionsQueries = 0;
  const openedSocketPaths: string[] = [];
  const eventSockets: RecordingSocket[] = [];
  let armed: { slot: { arrival: ReturnType<typeof deferred<void>>; completion: ReturnType<typeof deferred<TunnelResponse>> } } | null = null;
  const timeline: string[] = [];

  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return new Response(
          JSON.stringify([
            {
              machineRecordId: "rec-parity-1",
              machineId: MACHINE_ID,
              displayName: "Parity Mac",
              publicKey: "pk",
              attachPublicKey: "apk",
              relayOrigin: window.location.origin,
              platform: "darwin",
              online: true,
              enrollmentEpoch: "1",
              lastSeenAt: Date.now(),
            },
          ]),
          { status: 200 },
        );
      }
      if (url.includes("/grants")) {
        return new Response(
          JSON.stringify({
            grantId: "grant-1",
            machineId: MACHINE_ID,
            relayOrigin: window.location.origin,
            pairingToken: "pairing-token",
            machineAttachPublicKey: "machine-attach-key",
            grantScope: "machine",
            expiresAt: Date.now() + 600_000,
          }),
          { status: 200 },
        );
      }
      if (url.endsWith("/api/v1/attach/session")) {
        return new Response(JSON.stringify({ sessionId: "alloc-1" }), { status: 200 });
      }
      return new Response("Not Found", { status: 404 });
    }),
  );

  vi.stubGlobal("WebSocket", RecordingSocket);

  vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
    publicKey: "initiator-pk",
    privateKey: "initiator-sk",
  });

  const fetchLike = vi.fn(async (path: string, init?: FetchLikeInit): Promise<TunnelResponse> => {
    timeline.push(`${init?.method ?? "GET"} ${path}`);
    if (path.startsWith("/api/v1/pair/exchange")) {
      return respond({
        token: "tunnel-device-token",
        device: { id: "dev-1", name: "Phone" },
        machineId: MACHINE_ID,
        displayName: "Parity Mac",
      });
    }
    if (path === "/api/v1/workspace/projects") {
      return respond({
        revision: "1",
        completeness: "complete",
        projects: [{ workspaceId: "ws-ferryx", repoRoot: "/srv/ferryx", availability: "ready", revision: "1" }],
        unavailableWorkspaceIds: [],
      });
    }
    if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
      return respond({
        revision: "1",
        worktrees: [
          {
            workspaceId: "ws-ferryx",
            identity: { wsId: "ws-ferryx", slug: "main" },
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
      if (init?.method === "POST") {
        return respond({ ...session("spawned-should-not-happen", "spawned") });
      }
      sessionsQueries += 1;
      if (armed) {
        const slot = armed.slot;
        armed = null;
        slot.arrival.resolve();
        timeline.push(`  (armed GET /api/v1/sessions deferred)`);
        return slot.completion.promise;
      }
      timeline.push(
        `  -> sessions completeness=${currentPayload.completeness} ids=[${currentPayload.sessions
          .map((row) => row.sessionId)
          .join(",")}]`,
      );
      return respond(currentPayload);
    }
    if (path.startsWith("/api/v1/workspace/state")) {
      return respond({ activeContext: { workspaceId: "ws-ferryx", worktreeSlug: "main" } });
    }
    if (path.startsWith("/api/v1/agent/")) {
      return respond({ messages: [] });
    }
    return { status: 404, headers: {}, body: new Uint8Array(0) };
  });

  const openWebSocket = vi.fn(async (path: string): Promise<TunnelWebSocket> => {
    openedSocketPaths.push(path);
    timeline.push(`WS ${path}`);
    const socket = new RecordingSocket(path);
    if (!path.startsWith("/api/v1/terminal/")) eventSockets.push(socket);
    return socket;
  });

  const transport: TunnelTransport = {
    fetchLike,
    openWebSocket,
    close: () => {},
  };

  vi.spyOn(accountSessionModule, "openTunnel").mockResolvedValue({ transport, close: () => {} });
  vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({ transport, close: () => {} });

  render(<RemoteApp />);
  await waitFor(() => expect(screen.getByRole("button", { name: /Change workspace context/i })).toBeDefined());

  return {
    openedSocketPaths,
    timeline: () => timeline.join("\n    "),
    terminalPaths: () => openedSocketPaths.filter((path) => path.startsWith("/api/v1/terminal/")),
    sessionsQueryCount: () => sessionsQueries,
    postPaths: () =>
      fetchLike.mock.calls
        .filter(([, init]) => init?.method === "POST")
        .map(([path]) => String(path)),
    sessionCreatePosts: () =>
      fetchLike.mock.calls
        .filter(([, init]) => init?.method === "POST")
        .map(([path]) => String(path))
        .filter((path) => path === "/api/v1/sessions"),
    setSessions: (rows, completeness = "complete") => {
      currentPayload = payload(rows, completeness);
    },
    expectSessionsQuery: () => {
      const arrival = deferred<void>();
      const completion = deferred<TunnelResponse>();
      armed = { slot: { arrival, completion } };
      return {
        arrived: arrival.promise,
        completed: completion.promise.then(() => undefined),
        answer: (body: SessionsPayload) => {
          timeline.push(
            `  (answered armed GET: completeness=${body.completeness} ids=[${body.sessions
              .map((row) => row.sessionId)
              .join(",")}])`,
          );
          completion.resolve(respond(body));
        },
      };
    },
    publishInventoryEvent: (body: unknown) => {
      const socket = eventSockets[eventSockets.length - 1];
      if (!socket) throw new Error("no events socket was opened");
      socket.onmessage?.({
        data: JSON.stringify({ sequence: "7", revision: "7", type: "inventoryInvalidated", reason: "change", payload: body }),
      });
    },
  };
}

async function openContextPicker(): Promise<void> {
  const trigger = await waitFor(() => screen.getByRole("button", { name: /Change workspace context/i }));
  act(() => {
    fireEvent.click(trigger);
  });
}

async function ensureSessionRowsVisible(): Promise<void> {
  const trigger = await waitFor(() => screen.getByRole("button", { name: /Change workspace context/i }));
  if (inventoryRows().length === 0 && trigger.getAttribute("aria-expanded") !== "true") {
    act(() => {
      fireEvent.click(trigger);
    });
  }
}

/* The sidebar represents a session either as a session row (a still-existing session) or as
   a pane row (the session this client is attached to). Both are inventory truth. */
function inventoryRows(): HTMLElement[] {
  return [...screen.queryAllByTestId("remote-session-row"), ...screen.queryAllByRole("tab")];
}

function inventoryRowTexts(): string {
  const containers = screen.queryAllByTestId("remote-session-rows").map((element) => element.textContent ?? "");
  const rows = inventoryRows().map(
    (element) => `${element.getAttribute("aria-label") ?? ""} ${element.textContent ?? ""}`,
  );
  return [...containers, ...rows].join(" | ");
}


function sessionRow(needle: RegExp): HTMLElement | null {
  const byTestId = screen
    .queryAllByTestId("remote-session-row")
    .find((element) => needle.test(element.textContent ?? ""));
  if (byTestId) return byTestId;
  return (
    screen
      .queryAllByRole("button")
      .find((element) => needle.test(`${element.getAttribute("aria-label") ?? ""} ${element.textContent ?? ""}`)) ?? null
  );
}

/* Rows are identified by their session id suffix: the title is display data that can
   change or be replaced by "Terminal", the id cannot. */
const SID_SECOND_ROW = new RegExp(SID_SECOND.slice(0, 8));
const SID_FOREIGN_TEXT = "s-shared".slice(0, 8);

function awaitSignal<T>(promise: Promise<T>, description: string, timeoutMs = 2000): Promise<T> {
  return Promise.race([
    promise,
    new Promise<never>((_, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`Timed out after ${timeoutMs}ms waiting for ${description}`));
      }, timeoutMs);
      promise.finally(() => clearTimeout(timer));
    }),
  ]);
}

describe("RemoteApp machine inventory selection", () => {
  beforeEach(() => {
    clearStoredAccountSessionToken();
    localStorage.clear();
    remoteHostStore.reset();
    /* jsdom reports every rect as 0x0, which suppresses the terminal's own geometry
       measurement and therefore its socket. These are the same measurements the
       existing account-session suite supplies; the component still measures normally. */
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.hasAttribute("data-terminal-cell-measure")) return rect(10, 20);
      if (this.getAttribute("data-testid") === "remote-terminal-grid") return rect(800, 400);
      return rect(0, 0);
    });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    clearStoredAccountSessionToken();
    localStorage.clear();
    remoteHostStore.reset();
  });

  it("attaches to the second existing session by exact id without creating one", async () => {
    const harness = await mountAccountHarness([session(SID_FIRST, "alpha-shell"), session(SID_SECOND, "beta-agent")]);
    await waitFor(() => {
      expect(harness.sessionsQueryCount()).toBeGreaterThan(0);
    });

    await openContextPicker();
    const row = await waitFor(() => {
      const found = sessionRow(SID_SECOND_ROW);
      if (!found) throw new Error("second-session row not rendered");
      return found;
    });
    act(() => {
      fireEvent.click(row);
    });

    await waitFor(() => {
      expect(harness.terminalPaths().length).toBeGreaterThan(0);
    });
    const terminalPaths = harness.terminalPaths();
    const attached = terminalPaths.filter((path) => path.startsWith(`/api/v1/terminal/${SID_SECOND}`));
    expect(attached).toHaveLength(1);
    expect(attached[0]).toContain(`daemonEpoch=${EPOCH}`);
    expect(terminalPaths.some((path) => path.includes(SID_FIRST))).toBe(false);
    /* Pairing is an authorized POST on this boundary; only session creation is forbidden. */
    expect(harness.sessionCreatePosts()).toEqual([]);
  });

  it("refuses a session whose daemon epoch changed and never spawns a replacement", async () => {
    const harness = await mountAccountHarness([session(SID_FIRST, "alpha-shell"), session(SID_SECOND, "beta-agent")]);
    await waitFor(() => {
      expect(harness.sessionsQueryCount()).toBeGreaterThan(0);
    });

    await openContextPicker();
    const row = await waitFor(() => {
      const found = sessionRow(SID_SECOND_ROW);
      if (!found) throw new Error("second-session row not rendered");
      return found;
    });

    const query = harness.expectSessionsQuery();
    harness.setSessions([
      session(SID_FIRST, "alpha-shell", {
        target: { machineId: MACHINE_ID, sessionId: SID_FIRST, daemonEpoch: "202" },
      }),
      session(SID_SECOND, "beta-agent", {
        target: { machineId: MACHINE_ID, sessionId: SID_SECOND, daemonEpoch: "202" },
      }),
    ]);
    act(() => {
      fireEvent.click(row);
    });
    await awaitSignal(query.arrived, "GET /api/v1/sessions arrival");
    query.answer(
      payload([
        session(SID_FIRST, "alpha-shell", {
          target: { machineId: MACHINE_ID, sessionId: SID_FIRST, daemonEpoch: "202" },
        }),
        session(SID_SECOND, "beta-agent", {
          target: { machineId: MACHINE_ID, sessionId: SID_SECOND, daemonEpoch: "202" },
        }),
      ]),
    );
    await awaitSignal(query.completed, "GET /api/v1/sessions answer completion");

    await waitFor(() => {
      const alerts = screen.queryAllByRole("alert");
      if (alerts.length === 0) {
        throw new Error(`no accessible error surfaced after the epoch refusal\n  timeline:\n    ${harness.timeline()}`);
      }
      expect(alerts.length).toBeGreaterThan(0);
    });
    expect(harness.terminalPaths().some((path) => path.includes(SID_SECOND))).toBe(false);
    expect(harness.sessionCreatePosts()).toEqual([]);
  });

  it("keeps the selected session when a partial inventory answer omits it", async () => {
    const harness = await mountAccountHarness([session(SID_FIRST, "alpha-shell"), session(SID_SECOND, "beta-agent")]);
    await waitFor(() => {
      expect(harness.sessionsQueryCount()).toBeGreaterThan(0);
    });

    await openContextPicker();
    const row = await waitFor(() => {
      const found = sessionRow(SID_SECOND_ROW);
      if (!found) throw new Error("second-session row not rendered");
      return found;
    });
    act(() => {
      fireEvent.click(row);
    });
    await waitFor(() => {
      expect(harness.terminalPaths().some((path) => path.includes(SID_SECOND))).toBe(true);
    });
    const queriesBeforeEvent = harness.sessionsQueryCount();
    const terminalsBeforeEventPaths = harness.terminalPaths();

    const refresh = harness.expectSessionsQuery();
    act(() => {
      harness.publishInventoryEvent({
        completeness: "partial",
        sessions: payload([session(SID_FIRST, "alpha-shell")], "partial"),
      });
    });

    await awaitSignal(refresh.arrived, "partial inventory refresh arrival");
    expect(harness.sessionsQueryCount()).toBe(queriesBeforeEvent + 1);
    refresh.answer(payload([session(SID_FIRST, "alpha-shell")], "partial"));
    await awaitSignal(refresh.completed, "partial inventory refresh answer completion");

    await waitFor(() => {
      expect(harness.sessionsQueryCount()).toBeGreaterThan(queriesBeforeEvent);
    });
    await ensureSessionRowsVisible();
    await waitFor(() => {
      const selectedSession = document.querySelector(`[data-session-id="${SID_SECOND}"]`);
      if (!selectedSession) {
        throw new Error(`selected session ${SID_SECOND} missing exact data-session-id element\n  timeline:\n    ${harness.timeline()}`);
      }
      expect(selectedSession).not.toBeNull();
    });
    const selectedSession = document.querySelector(`[data-session-id="${SID_SECOND}"]`);
    expect(selectedSession).not.toBeNull();
    const otherSession = document.querySelector(`[data-session-id="${SID_FIRST}"]`);
    expect(otherSession).not.toBeNull();
    expect(harness.terminalPaths()).toEqual(terminalsBeforeEventPaths);
    expect(harness.sessionCreatePosts()).toEqual([]);
  });

  it("rejects a row the machine payload attributes to another machine", async () => {
    const harness = await mountAccountHarness([
      session(SID_FIRST, "alpha-shell"),
      session("s-shared", "foreign-shell", {
        target: { machineId: OTHER_MACHINE_ID, sessionId: "s-shared", daemonEpoch: "55" },
      }),
    ]);
    await waitFor(() => {
      expect(harness.sessionsQueryCount()).toBeGreaterThan(0);
    });

    await openContextPicker();
    await ensureSessionRowsVisible();
    await waitFor(() => {
      const rendered = inventoryRowTexts();
      if (!rendered.includes(SID_FIRST.slice(0, 8))) {
        throw new Error(`first session row missing from the picker\n  timeline:\n    ${harness.timeline()}`);
      }
      expect(rendered).toContain(SID_FIRST.slice(0, 8));
    });
    const rendered = inventoryRowTexts();
    expect(rendered).toContain(SID_FIRST.slice(0, 8));
    expect(rendered).not.toContain(SID_FOREIGN_TEXT);
  });
});
