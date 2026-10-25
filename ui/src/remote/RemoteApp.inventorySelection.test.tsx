import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { findSidebarRow, SIDEBAR_PATH, sidebarBody } from "./remoteSidebarTestUtils";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";
import { openWorkspacePicker } from "./home/phoneHomeTestUtils";
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
/** When set, the machine advertises a desktop-local snapshot and answers the scoped query with it. */
let desktopScopedProjects: Array<{ workspaceId: string; availability: string }> | null = null;

type SessionRow = {
  workspaceId: string;
  worktree: { wsId: string; slug: string } | null;
  target: { machineId: string; sessionId: string; daemonEpoch: string };
  sessionId: string;
  running: boolean;
  title: string;
  agentType?: string;
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
  private messageHandler: ((event: TunnelMessageEvent) => void) | null = null;
  onMessageReady?: () => void;
  get onmessage() { return this.messageHandler; }
  set onmessage(handler: ((event: TunnelMessageEvent) => void) | null) {
    this.messageHandler = handler;
    if (handler) this.onMessageReady?.();
  }
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

async function mountAccountHarness(initial: SessionRow[], chat?: {
  readonly request: (path: string) => Promise<Response> | undefined;
  readonly terminalReady: (socket: RecordingSocket) => void;
}): Promise<Harness> {
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
      const chatResponse = chat?.request(url);
      if (chatResponse) return chatResponse;
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
    const chatResponse = chat?.request(path);
    if (chatResponse) {
      const response = await chatResponse;
      return respond(await response.json(), response.status);
    }
    if (path.startsWith("/api/v1/pair/exchange")) {
      return respond({
        token: "tunnel-device-token",
        device: { id: "dev-1", name: "Phone" },
        machineId: MACHINE_ID,
        displayName: "Parity Mac",
      });
    }
    if (path === SIDEBAR_PATH) {
      const workspaces = desktopScopedProjects?.map((project) => project.workspaceId) ?? ["ws-ferryx"];
      return respond(JSON.parse(sidebarBody(workspaces.map((workspaceId) => ({ workspaceId, slug: "main" })))));
    }
    if (path === "/api/v1/workspace/projects") {
      if (desktopScopedProjects) return respond({ revision: "1", completeness: "complete", desktopLocalSnapshotExists: true, projects: [], unavailableWorkspaceIds: [] });
      return respond({
        revision: "1",
        completeness: "complete",
        projects: [{ workspaceId: "ws-ferryx", repoRoot: "/srv/ferryx", availability: "ready", revision: "1" }],
        unavailableWorkspaceIds: [],
      });
    }
    if (path === "/api/v1/workspace/projects?scope=desktopLocal" && desktopScopedProjects) {
      return respond({
        revision: "2", completeness: "complete", desktopLocalSnapshotExists: true,
        projects: desktopScopedProjects.map((project) => ({ ...project, repoRoot: `/srv/${project.workspaceId}`, revision: "2" })),
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
    if (chat && path.startsWith("/api/v1/terminal/")) {
      socket.onMessageReady = () => chat.terminalReady(socket);
    }
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

  if (chat) {
    const ready = r25State(() => screen.queryByTestId("mobile-home-tab-worktrees") !== null);
    render(<RemoteApp />);
    await ready;
  } else {
    render(<RemoteApp />);
    await waitFor(() => expect(screen.getByTestId("mobile-home-tab-worktrees")).toBeDefined());
  }

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
  await ensureSessionRowsVisible();
}

/* Session rows live in the terminal tab strip of the attached worktree, so the worktree row is
   selected first; its sessions then render as strip tabs. */
async function ensureSessionRowsVisible(): Promise<void> {
  await openWorkspacePicker();
  if (terminalStripTabs().length > 0) return;
  const row = await findSidebarRow("ws-ferryx", "main");
  await act(async () => { fireEvent.click(row); });
}

function terminalStripTabs(): HTMLElement[] {
  const strip = screen.queryByRole("tablist", { name: "Terminal tabs" });
  return strip ? within(strip).queryAllByRole("tab") : [];
}

/* Every session the remote offers: the strip tabs of the attached worktree. */
function inventoryRowTexts(): string {
  return terminalStripTabs()
    .map((element) => `${element.getAttribute("data-session-id") ?? ""} ${element.getAttribute("aria-label") ?? ""} ${element.textContent ?? ""}`)
    .join(" | ");
}

function sessionRow(needle: RegExp): HTMLElement | null {
  return terminalStripTabs().find((element) =>
    needle.test(`${element.getAttribute("data-session-id") ?? ""} ${element.getAttribute("aria-label") ?? ""} ${element.textContent ?? ""}`)) ?? null;
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

const r25Cleanups = new Set<() => void>();

/* R25 waits are event-driven and carry no wall-clock deadline of their own: a transition that
   never happens leaves the test pending until vitest's per-test timeout fails it, and afterEach
   disposes every still-armed observer. A private 2000ms race made load, not behavior, the
   failure signal. */
function r25State(matches: () => boolean): Promise<void> {
  return new Promise<void>((resolve) => {
    const observer = new MutationObserver(() => { if (matches()) finish(); });
    const dispose = () => {
      observer.disconnect();
      r25Cleanups.delete(dispose);
    };
    const finish = () => {
      dispose();
      resolve();
    };
    r25Cleanups.add(dispose);
    observer.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    if (matches()) finish();
  });
}

function r25Signal<T>() {
  let publish: (value: T) => void = () => { throw new Error("R25 signal not initialized"); };
  const promise = new Promise<T>((resolve) => {
    const dispose = () => { r25Cleanups.delete(dispose); };
    r25Cleanups.add(dispose);
    publish = (value) => { dispose(); resolve(value); };
  });
  return { promise, resolve: (value: T) => publish(value) };
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
    desktopScopedProjects = null;
    for (const dispose of r25Cleanups) dispose();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    clearStoredAccountSessionToken();
    localStorage.clear();
    remoteHostStore.reset();
  });

  it.each([
    ["done", "working", "claude", "claude", "matching"],
    ["working", "done", "claude", "claude", "matching"],
    ["done", "done", "claude", "codex", "matching"],
    ["done", "working", "claude", "codex", "machine"],
    ["done", "working", "claude", "codex", "session"],
    ["done", "working", "claude", "codex", "epoch"],
    ["done", "done", "claude", "claude", "none"],
  ] as const)("r25-ui-m1: %s snapshot to %s observation, %s to %s, %s target", async (snapshot, observed, initialAgent, nextAgent, control) => {
    const reads: string[] = [];
    let promptRead: ReturnType<typeof r25Signal<string>> | null = null;
    let historyRead: ReturnType<typeof r25Signal<string>> | null = null;
    let terminalRead: ReturnType<typeof r25Signal<RecordingSocket>> | null = null;
    const request = (path: string): Promise<Response> | undefined => {
      if (path.includes("/api/v1/capabilities")) return Promise.resolve(new Response(JSON.stringify({
        apiVersion: 1, machineId: MACHINE_ID, daemonEpoch: EPOCH, referenceOwnerId: "r25-owner", platform: "linux",
      })));
      if (!path.includes("/reference-chat/")) return undefined;
      reads.push(path);
      if (path.includes("/prompt")) {
        promptRead?.resolve(path);
        return Promise.resolve(new Response(JSON.stringify({ prompt: null, screenRevision: null })));
      }
      if (path.includes("/history")) {
        historyRead?.resolve(path);
        return Promise.resolve(new Response(JSON.stringify({
          source: "claude-transcript", availability: "native", turns: [], cursor: null,
          hasMore: false, generation: "r25-generation", unavailableReason: null,
        })));
      }
      return Promise.resolve(new Response(JSON.stringify({ files: [] })));
    };
    const harness = await mountAccountHarness([session(SID_SECOND, "native-agent", { agentType: initialAgent })], {
      request,
      terminalReady: (socket) => terminalRead?.resolve(socket),
    });
    const rowsReady = r25State(() => sessionRow(SID_SECOND_ROW) !== null);
    await ensureSessionRowsVisible();
    await rowsReady;
    const row = sessionRow(SID_SECOND_ROW);
    if (!row) throw new Error("Missing R25 native session row");
    const selected = r25State(() => screen.queryByTestId("chat-composer-textarea") !== null);
    promptRead = r25Signal<string>();
    historyRead = r25Signal<string>();
    await act(async () => { fireEvent.click(row); });
    await Promise.all([promptRead.promise, historyRead.promise]);
    await selected;
    const terminalReady = r25Signal<RecordingSocket>();
    terminalRead = terminalReady;
    await act(async () => { fireEvent.click(screen.getByTestId("remote-view-mode-terminal")); });
    const socket = await terminalReady.promise;
    /* Complete the machine attach handshake the real daemon sends. Since c7fbfab3 an online wake
       redials any socket that has not reached readiness (RemoteTerminal recover()), and frames on
       the abandoned socket are ignored, so an unattached fixture socket loses every later frame. */
    await act(async () => socket.onmessage?.({ data: JSON.stringify({
      type: "attached", target: { machineId: MACHINE_ID, sessionId: SID_SECOND, daemonEpoch: EPOCH }, generation: "1",
    }) }));
    const frame = (state: string, agent: string, mismatch = "matching") => JSON.stringify({
      type: "agent_state", state, agent,
      target: {
        machineId: mismatch === "machine" ? OTHER_MACHINE_ID : MACHINE_ID,
        sessionId: mismatch === "session" ? "unreported-session" : SID_SECOND,
        daemonEpoch: mismatch === "epoch" ? "999" : EPOCH,
      },
    });
    if (control !== "none") {
      await act(async () => socket.onmessage?.({ data: frame(snapshot, initialAgent) }));
      const refresh = harness.expectSessionsQuery();
      const refreshArrived = r25Signal<void>();
      void refresh.arrived.then(() => refreshArrived.resolve());
      const releaseRefresh = () => refresh.answer(payload([session(SID_SECOND, "native-agent", { agentType: initialAgent })]));
      r25Cleanups.add(releaseRefresh);
      act(() => { window.dispatchEvent(new Event("online")); });
      await refreshArrived.promise;
      await act(async () => {
        releaseRefresh();
        await refresh.completed;
      });
      r25Cleanups.delete(releaseRefresh);
    }
    const queries = harness.sessionsQueryCount();
    const posts = harness.postPaths();
    const header = screen.getByTestId("mobile-top-bar");
    expect(within(header).queryByText("Working") !== null).toBe(snapshot === "working");
    const matches = control === "matching";
    const expectedState = matches ? observed : snapshot;
    const expectedAgent = matches ? nextAgent : initialAgent;
    const headerChanged = r25State(() =>
      (within(header).queryByText("Working") !== null) === (expectedState === "working")
      && header.querySelector("[data-agent-type]")?.getAttribute("data-agent-type") === expectedAgent);
    await act(async () => {
      if (control !== "none") socket.onmessage?.({ data: frame(observed, nextAgent, control) });
    });
    await headerChanged;
    expect(within(header).queryByText("Working") !== null).toBe(expectedState === "working");
    expect(header.querySelector("[data-agent-type]")?.getAttribute("data-agent-type")).toBe(expectedAgent);
    const controls = r25State(() => screen.queryByTestId(expectedState === "working" ? "stop-button" : "send-button") !== null);
    promptRead = r25Signal<string>();
    historyRead = r25Signal<string>();
    await act(async () => { fireEvent.click(screen.getByTestId("remote-view-mode-chat")); });
    const [prompt, history] = await Promise.all([promptRead.promise, historyRead.promise]);
    expect(new URL(prompt, window.location.origin).searchParams.get("registryId")).toBe(expectedAgent);
    expect(new URL(history, window.location.origin).searchParams.get("registryId")).toBe(expectedAgent);
    await controls;
    expect(screen.queryByTestId(expectedState === "working" ? "send-button" : "stop-button")).toBeNull();
    expect(harness.sessionsQueryCount()).toBe(queries);
    expect(harness.postPaths()).toEqual(posts);
    expect(reads.some((path) => path.includes("/prompt"))).toBe(true);
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
    fireEvent.click(await screen.findByTestId("remote-view-mode-terminal"));

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

  it("r28-ui keeps the attached session when its project becomes unavailable in the desktop scope", async () => {
    desktopScopedProjects = [
      { workspaceId: "ws-aaa", availability: "ready" },
      { workspaceId: "ws-ferryx", availability: "ready" },
    ];
    const harness = await mountAccountHarness([
      session(SID_FIRST, "alpha-shell", { workspaceId: "ws-aaa", worktree: null }),
      session(SID_SECOND, "beta-agent"),
    ]);
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
    fireEvent.click(await screen.findByTestId("remote-view-mode-terminal"));
    await waitFor(() => {
      expect(harness.terminalPaths().some((path) => path.includes(SID_SECOND))).toBe(true);
    });
    const attachedBefore = harness.terminalPaths().length;

    // The attached session's project goes offline (for example an unmounted volume).
    desktopScopedProjects = [
      { workspaceId: "ws-aaa", availability: "ready" },
      { workspaceId: "ws-ferryx", availability: "missing" },
    ];
    const refresh = harness.expectSessionsQuery();
    act(() => {
      harness.publishInventoryEvent({ completeness: "complete", sessions: payload([session(SID_FIRST, "alpha-shell", { workspaceId: "ws-aaa", worktree: null }), session(SID_SECOND, "beta-agent")]) });
    });
    await awaitSignal(refresh.arrived, "inventory refresh arrival");
    refresh.answer(payload([session(SID_FIRST, "alpha-shell", { workspaceId: "ws-aaa", worktree: null }), session(SID_SECOND, "beta-agent")]));
    await awaitSignal(refresh.completed, "inventory refresh answer completion");
    await waitFor(() => {
      expect(harness.timeline()).toContain("GET /api/v1/workspace/projects?scope=desktopLocal");
    });
    // Let any reselection the refresh triggers settle before asserting.
    await act(async () => { await Promise.resolve(); });
    const opened = harness.terminalPaths().slice(attachedBefore);
    expect(opened.some((path) => path.includes(SID_FIRST))).toBe(false);
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
    fireEvent.click(await screen.findByTestId("remote-view-mode-terminal"));
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
