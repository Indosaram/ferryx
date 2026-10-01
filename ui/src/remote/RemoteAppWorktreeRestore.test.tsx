import "@testing-library/jest-dom/vitest";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";
import {
  storeAccountSessionToken,
  getStoredAccountSessionToken,
  clearStoredAccountSessionToken,
  setAccountLastSelectedTarget,
  getAccountLastSelectedTarget,
} from "./accountSession";
import * as accountSessionModule from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
import * as useAccountWorktreesModule from "./useAccountWorktrees";
import { remoteHostStore } from "../state/remoteHostStore";

class MockTestWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static instances: MockTestWebSocket[] = [];
  url: string;
  sentMessages: (string | Uint8Array)[] = [];
  readyState: number = 1;
  binaryType: string = "arraybuffer";
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(url: string) {
    this.url = url;
    MockTestWebSocket.instances.push(this);
  }

  send(data: string | Uint8Array | ArrayBuffer) {
    if (typeof data === "string") {
      this.sentMessages.push(data);
    } else if (data instanceof Uint8Array) {
      this.sentMessages.push(data);
    } else {
      this.sentMessages.push(new Uint8Array(data));
    }
  }

  close() {
    this.readyState = 3;
    this.onclose?.();
  }
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

const encoder = new TextEncoder();

function jsonTunnelResponse(status: number, body: unknown) {
  return { status, headers: { "content-type": "application/json" }, body: encoder.encode(JSON.stringify(body)) };
}

function textTunnelResponse(status: number, body: string, contentType: string) {
  return { status, headers: { "content-type": contentType }, body: encoder.encode(body) };
}

class FakeTunnelSocket {
  readyState = 1;
  binaryType = "arraybuffer";
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: { code: number; reason: string; wasClean: boolean }) => void) | null = null;
  readonly sent: (string | Uint8Array)[] = [];

  constructor(readonly path: string) {}

  send(data: string | Uint8Array) {
    this.sent.push(data);
  }

  close() {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.onclose?.({ code: 1000, reason: "", wasClean: true });
  }
}

/**
 * In-memory stand-in for the paired desktop that mirrors the relay's real contract:
 * `GET /api/v1/workspace/state` returns projects plus the active context, `POST
 * /api/v1/workspace/select` acknowledges the selection, mutates the active context and
 * republishes the selection on the events socket.
 */
function createFakeDesktop() {
  const selectBodies: Record<string, unknown>[] = [];
  const sockets = new Set<FakeTunnelSocket>();
  const openedSocketPaths: string[] = [];
  let closeCalls = 0;
  let stateRequests = 0;
  let stateFailure: { status: number; body: string; contentType: string; fromRequest: number } | null = null;

  const state = {
    projects: [
      {
        workspaceId: "ws-ferryx",
        repoRoot: "/Users/dev/ferryx",
        worktrees: [
          { slug: "main", label: "main" },
          { slug: "feature-picker", label: "feature-picker" },
        ],
      },
    ],
    activeContext: null as null | {
      workspaceId: string;
      worktreeSlug: string;
      worktreeLabel: string;
      sessionId: string | null;
    },
    sessions: [] as {
      sessionId: string;
      daemonEpoch: string;
      running: boolean;
      title: string;
      workspaceId: string;
    }[],
  };

  const publishSelection = () => {
    const context = state.activeContext;
    const frame = JSON.stringify({
      event: "remote_active_selection_changed",
      payload: { workspaceId: context?.workspaceId, worktreeSlug: context?.worktreeSlug },
    });
    for (const socket of sockets) {
      if (socket.path.startsWith("/api/v1/events") && socket.readyState === 1) {
        socket.onmessage?.({ data: frame } as MessageEvent);
      }
    }
  };

  const fetchLike = async (
    path: string,
    init?: { method?: string; body?: unknown; headers?: Record<string, string> },
  ) => {
    if (path.startsWith("/api/v1/pair/exchange")) {
      return jsonTunnelResponse(200, {
        token: "tunnel-redeemed-device-bearer",
        device: { id: "dev-phone-1", name: "Phone" },
        machineId: "mach-phone-1",
        displayName: "Work MacBook Pro",
      });
    }

    if (path.startsWith("/api/v1/workspace/state")) {
      stateRequests += 1;
      if (stateFailure && stateRequests >= stateFailure.fromRequest) {
        return textTunnelResponse(stateFailure.status, stateFailure.body, stateFailure.contentType);
      }
      return jsonTunnelResponse(200, state);
    }

    if (path.startsWith("/api/v1/sessions")) {
      return jsonTunnelResponse(200, { revision: "1", completeness: "complete", sessions: state.sessions });
    }

    if (path.startsWith("/api/v1/workspace/select")) {
      const body = JSON.parse(String(init?.body ?? "{}")) as Record<string, unknown>;
      selectBodies.push(body);
      const project = state.projects.find((p) => p.workspaceId === body.workspaceId);
      if (!project) return jsonTunnelResponse(404, { code: "CONTEXT_NOT_FOUND" });
      const worktree = project.worktrees.find((w) => w.slug === (body.worktreeSlug ?? project.worktrees[0]?.slug));
      if (!worktree) return jsonTunnelResponse(404, { code: "CONTEXT_NOT_FOUND" });
      const sessionId = `sess-${project.workspaceId}-${worktree.slug}`;
      if (!state.sessions.some((s) => s.sessionId === sessionId)) {
        state.sessions.push({
          sessionId,
          daemonEpoch: "1790742255752",
          running: true,
          title: "zsh",
          workspaceId: project.workspaceId,
        });
      }
      state.activeContext = {
        workspaceId: project.workspaceId,
        worktreeSlug: worktree.slug,
        worktreeLabel: worktree.label,
        sessionId,
      };
      queueMicrotask(publishSelection);
      return jsonTunnelResponse(200, { ok: true });
    }

    return { status: 404, headers: {}, body: new Uint8Array(0) };
  };

  const openWebSocket = async (path: string) => {
    const socket = new FakeTunnelSocket(path);
    sockets.add(socket);
    openedSocketPaths.push(path);
    return socket;
  };

  const close = () => {
    closeCalls += 1;
    for (const socket of Array.from(sockets)) socket.close();
    sockets.clear();
  };

  return {
    transport: { fetchLike, openWebSocket, close } as unknown as attachTunnelModule.TunnelTransport,
    close,
    selectBodies,
    openedSocketPaths,
    get closeCalls() {
      return closeCalls;
    },
    get stateRequests() {
      return stateRequests;
    },
    /** Fail every `GET /api/v1/workspace/state` from `fromRequest` (1-based) onwards. */
    failStateFromRequest(status: number, body: string, contentType: string, fromRequest: number) {
      stateFailure = { status, body, contentType, fromRequest };
    },
  };
}

const grantResponseBody = {
  grantId: "grant-test-1",
  machineId: "mach-phone-1",
  relayOrigin: window.location.origin,
  pairingToken: "pair-tok-secret",
  machineAttachPublicKey: "machine-noise-pub-key",
  grantScope: "machine",
  expiresAt: Date.now() + 600000,
};

function responseText(status: number, body: string, contentType = "text/plain"): Response {
  return new Response(body, { status, headers: { "Content-Type": contentType } });
}

describe("RemoteApp Account Worktree Follow-ups: Sign-out and Worktree Reload Restore", () => {
  const originalFetch = globalThis.fetch;
  const originalWebSocket = globalThis.WebSocket;

  const sampleMachines = [
    {
      machineRecordId: "rec-mbp-1",
      machineId: "mach-phone-1",
      displayName: "Work MacBook Pro",
      publicKey: "pub-key-1",
      attachPublicKey: "attach-pub-1",
      relayOrigin: window.location.origin,
      platform: "macos",
      online: true,
      enrollmentEpoch: "1",
      lastSeenAt: Date.now(),
    },
  ];

  beforeEach(() => {
    vi.restoreAllMocks();
    MockTestWebSocket.instances = [];
    globalThis.WebSocket = MockTestWebSocket as unknown as typeof WebSocket;

    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      if (this.hasAttribute("data-terminal-cell-measure")) return rect(10, 20);
      if (this.getAttribute("data-testid") === "remote-terminal-grid") return rect(800, 400);
      return rect(0, 0);
    });

    clearStoredAccountSessionToken();
    localStorage.clear();
    remoteHostStore.reset();
  });

  afterEach(() => {
    cleanup();
    MockTestWebSocket.instances = [];
    globalThis.fetch = originalFetch;
    globalThis.WebSocket = originalWebSocket;
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    clearStoredAccountSessionToken();
    localStorage.clear();
    remoteHostStore.reset();
  });

  async function openWorkspaceContextPicker() {
    const trigger = await screen.findByRole("button", { name: "Change workspace context" });
    act(() => {
      fireEvent.click(trigger);
    });
    return screen.findByRole("dialog", { name: "Workspace context" });
  }

  async function openHostDrawerSignOut() {
    const picker = await openWorkspaceContextPicker();
    act(() => {
      fireEvent.click(within(picker).getByRole("button", { name: "Machines" }));
    });
    const drawer = await screen.findByRole("dialog", { name: "Switch host" });
    return {
      drawer,
      signOutButton: within(drawer).getByTestId("mobile-host-drawer-signout"),
    };
  }

  it("Item A: explicit Sign out in the host drawer calls logoutAccountSession and clears storage even if network fetch rejects", async () => {
    storeAccountSessionToken("test-account-session-token-xyz", window.location.origin);

    const logoutSpy = vi.spyOn(accountSessionModule, "logoutAccountSession");

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();

      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response(JSON.stringify(sampleMachines), { status: 200 }));
      }
      if (url.endsWith("/api/account/v1/logout")) {
        return Promise.reject(new TypeError("NetworkError when attempting to fetch resource."));
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    const { unmount } = render(<RemoteApp />);

    const { drawer, signOutButton } = await openHostDrawerSignOut();

    // Sign-out is reachable while signed in but before any machine tunnel exists.
    expect(within(drawer).getByTestId("mobile-host-drawer-disconnect")).toHaveTextContent("Disconnect machine");
    expect(signOutButton).toBeInTheDocument();

    act(() => {
      fireEvent.click(signOutButton);
    });

    await waitFor(() => {
      expect(logoutSpy).toHaveBeenCalledWith(window.location.origin, "test-account-session-token-xyz");
      expect(getStoredAccountSessionToken(window.location.origin)).toBeNull();
    });

    unmount();
  });

  it("Item A: confirmed account UNAUTHORIZED from the machine list clears the local session without calling logoutAccountSession", async () => {
    storeAccountSessionToken("expired-session-token", window.location.origin);

    setAccountLastSelectedTarget(window.location.origin, {
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });

    const logoutSpy = vi.spyOn(accountSessionModule, "logoutAccountSession");

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(
          new Response(JSON.stringify({ code: "UNAUTHORIZED", message: "Session expired" }), {
            status: 401,
            headers: { "Content-Type": "application/json" },
          })
        );
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    render(<RemoteApp />);

    await waitFor(() => {
      expect(getStoredAccountSessionToken(window.location.origin)).toBeNull();
      expect(getAccountLastSelectedTarget(window.location.origin)).toBeNull();
    });

    expect(screen.getByRole("heading", { name: "Sign In to Ferryx" })).toBeInTheDocument();
    expect(logoutSpy).not.toHaveBeenCalled();
  });

  it("Item C: reload with stored target triggers automatic worktree selection once machine is online", async () => {
    storeAccountSessionToken("active-session-token", window.location.origin);

    setAccountLastSelectedTarget(window.location.origin, {
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });

    const desktop = createFakeDesktop();

    // Hold the events socket's attach allocation so its refresh cannot invalidate the confirmation read.
    let releaseEventsSocketAttach = () => {};
    const eventsSocketAttachGate = new Promise<void>((resolve) => {
      releaseEventsSocketAttach = resolve;
    });
    let attachSessionCalls = 0;

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();

      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response(JSON.stringify(sampleMachines), { status: 200 }));
      }
      if (url.includes("/api/account/v1/machines/rec-mbp-1/grants")) {
        return Promise.resolve(new Response(JSON.stringify(grantResponseBody), { status: 200 }));
      }
      if (url.endsWith("/api/v1/attach/session")) {
        attachSessionCalls += 1;
        const allocated = new Response(JSON.stringify({ sessionId: `sess-alloc-${attachSessionCalls}` }), {
          status: 200,
        });
        if (attachSessionCalls === 1) return Promise.resolve(allocated);
        return eventsSocketAttachGate.then(() => allocated);
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "phone-initiator-pub-key-base64",
      privateKey: "phone-initiator-priv-key-base64",
    });

    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
      transport: desktop.transport,
      close: desktop.close,
    });

    render(<RemoteApp />);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Change workspace context" })).toHaveTextContent(
        "ws-ferryx / main",
      );
    });

    // The label comes from the relay's selection acknowledgement, not from a forced model.
    expect(desktop.selectBodies).toEqual([{ workspaceId: "ws-ferryx", worktreeSlug: "main" }]);
    expect(desktop.stateRequests).toBeGreaterThanOrEqual(2);

    expect(getAccountLastSelectedTarget(window.location.origin)).toEqual({
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });

    releaseEventsSocketAttach();
    await waitFor(() => {
      expect(desktop.openedSocketPaths).toContain("/api/v1/events");
    });
    expect(screen.getByLabelText("Current desktop context")).toHaveTextContent("ws-ferryx / main");
  });

  it("Item C: reload with stored target drops key when complete successful inventory confirms machine missing", async () => {
    storeAccountSessionToken("active-session-token", window.location.origin);

    setAccountLastSelectedTarget(window.location.origin, {
      machineId: "mach-gone-99",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
    });

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response(JSON.stringify(sampleMachines), { status: 200 }));
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    render(<RemoteApp />);

    await waitFor(() => {
      expect(getAccountLastSelectedTarget(window.location.origin)).toBeNull();
    });
  });

  it("Item C: reload with stored target drops key when complete successful inventory is empty []", async () => {
    storeAccountSessionToken("active-session-token", window.location.origin);

    setAccountLastSelectedTarget(window.location.origin, {
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
    });

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    render(<RemoteApp />);

    await waitFor(() => {
      expect(getAccountLastSelectedTarget(window.location.origin)).toBeNull();
    });
  });

  it("Item C: transient machine probe error or offline status PRESERVES stored target key", async () => {
    storeAccountSessionToken("active-session-token", window.location.origin);

    setAccountLastSelectedTarget(window.location.origin, {
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });

    const offlineMachine = [{ ...sampleMachines[0], online: false }];

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response(JSON.stringify(offlineMachine), { status: 200 }));
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    render(<RemoteApp />);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Change workspace context" })).toBeInTheDocument();
    });

    expect(getAccountLastSelectedTarget(window.location.origin)).toEqual({
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });
  });

  it("Item C: transient listMachines failure (e.g. 500) PRESERVES stored target key", async () => {
    storeAccountSessionToken("active-session-token", window.location.origin);

    setAccountLastSelectedTarget(window.location.origin, {
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response("Gateway 500 Error", { status: 500 }));
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    render(<RemoteApp />);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Change workspace context" })).toBeInTheDocument();
    });

    expect(getAccountLastSelectedTarget(window.location.origin)).toEqual({
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });
  });

  it("Item C: deferred /machines response preserves stored target key across initial render ticks", async () => {
    storeAccountSessionToken("active-session-token", window.location.origin);

    setAccountLastSelectedTarget(window.location.origin, {
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
    });

    let resolveMachines!: (res: Response) => void;
    const machinesPromise = new Promise<Response>((resolve) => {
      resolveMachines = resolve;
    });

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return machinesPromise;
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    render(<RemoteApp />);

    expect(getAccountLastSelectedTarget(window.location.origin)).not.toBeNull();

    resolveMachines(new Response(JSON.stringify(sampleMachines), { status: 200 }));

    await waitFor(() => {
      expect(getAccountLastSelectedTarget(window.location.origin)).not.toBeNull();
    });
  });

  it("Item C: project unavailable / pending preserves stored target key without dropping it", async () => {
    storeAccountSessionToken("active-session-token", window.location.origin);

    setAccountLastSelectedTarget(window.location.origin, {
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();

      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response(JSON.stringify(sampleMachines), { status: 200 }));
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "phone-initiator-pub-key-base64",
      privateKey: "phone-initiator-priv-key-base64",
    });

    const mockTunnelTransport = {
      fetchLike: vi.fn().mockImplementation((path: string) => {
        if (path.startsWith("/api/v1/pair/exchange")) {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: encoder.encode(
              JSON.stringify({
                token: "tunnel-redeemed-device-bearer",
                device: { id: "dev-phone-1", name: "Phone" },
                machineId: "mach-phone-1",
                displayName: "Work MacBook Pro",
              }),
            ),
          });
        }
        if (path === "/api/v1/workspace/projects") {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: encoder.encode(
              JSON.stringify({
                revision: 1,
                completeness: "complete",
                projects: [],
                unavailableWorkspaceIds: ["ws-ferryx"],
              }),
            ),
          });
        }
        if (path === "/api/v1/workspace/state") {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: encoder.encode(
              JSON.stringify({
                revision: 1,
                workspaces: [],
                unavailableWorkspaceIds: ["ws-ferryx"],
              }),
            ),
          });
        }
        if (path === "/api/v1/sessions") {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: encoder.encode(
              JSON.stringify({
                revision: "1",
                completeness: "complete",
                sessions: [],
              }),
            ),
          });
        }
        return Promise.resolve({
          status: 404,
          headers: {},
          body: new Uint8Array(0),
        });
      }),
      openWebSocket: vi.fn(),
    };

    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
      transport: mockTunnelTransport as unknown as attachTunnelModule.TunnelTransport,
      close: vi.fn(),
    });

    render(<RemoteApp />);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Change workspace context" })).toBeInTheDocument();
    });

    expect(getAccountLastSelectedTarget(window.location.origin)).toEqual({
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });
  });

  it("Item C: logout while acquireConnection is pending closes connection and cancels restore without repopulating storage", async () => {
    storeAccountSessionToken("active-session-token", window.location.origin);

    setAccountLastSelectedTarget(window.location.origin, {
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();

      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response(JSON.stringify(sampleMachines), { status: 200 }));
      }
      if (url.endsWith("/api/account/v1/logout")) {
        return Promise.resolve(new Response(null, { status: 204 }));
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    let resolveAcquireConnection!: (val: unknown) => void;
    const acquirePromise = new Promise((resolve) => {
      resolveAcquireConnection = resolve;
    });

    const mockClosedFn = vi.fn();
    const mockAcquireConn = {
      transport: {
        fetchLike: vi.fn(),
        openWebSocket: vi.fn(),
      },
      close: mockClosedFn,
      machine: sampleMachines[0],
      deviceToken: "acquired-device-bearer",
    };

    let acquireCalled = false;
    const origUseAccountWorktrees = useAccountWorktreesModule.useAccountWorktrees;
    vi.spyOn(useAccountWorktreesModule, "useAccountWorktrees").mockImplementation((...args) => {
      const hookRes = origUseAccountWorktrees(...args);
      return {
        ...hookRes,
        loading: false,
        initialized: true,
        machines: sampleMachines as unknown as ReturnType<typeof origUseAccountWorktrees>["machines"],
        machineStatuses: {
          "mach-phone-1": {
            machine: sampleMachines[0] as unknown as ReturnType<typeof origUseAccountWorktrees>["machines"][number],
            status: "ready",
            options: [
              {
                machineId: "mach-phone-1",
                machineDisplayName: "Work MacBook Pro",
                machinePlatform: "macos",
                machineOnline: true,
                workspaceId: "ws-ferryx",
                worktreeSlug: "main",
                worktreeLabel: "main",
              },
            ],
          },
        },
        accountOptions: [
          {
            machineId: "mach-phone-1",
            machineDisplayName: "Work MacBook Pro",
            machinePlatform: "macos",
            machineOnline: true,
            workspaceId: "ws-ferryx",
            worktreeSlug: "main",
            worktreeLabel: "main",
          },
        ],
        acquireConnection: vi.fn().mockImplementation(async () => {
          acquireCalled = true;
          return acquirePromise;
        }),
      };
    });

    render(<RemoteApp />);

    await waitFor(() => {
      expect(acquireCalled).toBe(true);
    });

    const { signOutButton } = await openHostDrawerSignOut();

    act(() => {
      fireEvent.click(signOutButton);
    });

    expect(getStoredAccountSessionToken(window.location.origin)).toBeNull();
    expect(getAccountLastSelectedTarget(window.location.origin)).toBeNull();

    act(() => {
      resolveAcquireConnection(mockAcquireConn);
    });

    await waitFor(() => {
      expect(mockClosedFn).toHaveBeenCalled();
    });

    expect(getStoredAccountSessionToken(window.location.origin)).toBeNull();
    expect(getAccountLastSelectedTarget(window.location.origin)).toBeNull();
  });

  it.each([
    { status: 401, body: "<html><body>401 Unauthorized</body></html>", contentType: "text/html" },
    { status: 403, body: "Forbidden", contentType: "text/plain" },
  ])(
    "workspace state untyped $status keeps the account session and only drops the machine attachment",
    async ({ status, body, contentType }) => {
      storeAccountSessionToken("active-session-token", window.location.origin);

      setAccountLastSelectedTarget(window.location.origin, {
        machineId: "mach-phone-1",
        workspaceId: "ws-ferryx",
        worktreeSlug: "main",
        worktreeLabel: "main",
      });

      const desktop = createFakeDesktop();
      // Read 1 enumerates worktrees; read 2 is the confirmation, answered untyped (edge/WAF style).
      desktop.failStateFromRequest(status, body, contentType, 2);

      globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
        const url = typeof input === "string" ? input : input.toString();
        if (url.endsWith("/api/account/v1/machines")) {
          return Promise.resolve(new Response(JSON.stringify(sampleMachines), { status: 200 }));
        }
        if (url.includes("/api/account/v1/machines/rec-mbp-1/grants")) {
          return Promise.resolve(new Response(JSON.stringify(grantResponseBody), { status: 200 }));
        }
        if (url.endsWith("/api/v1/attach/session")) {
          return Promise.resolve(new Response(JSON.stringify({ sessionId: "sess-alloc-front" }), { status: 200 }));
        }
        return Promise.resolve(new Response("Not Found", { status: 404 }));
      });

      vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
        publicKey: "phone-initiator-pub-key-base64",
        privateKey: "phone-initiator-priv-key-base64",
      });

      vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
        transport: desktop.transport,
        close: desktop.close,
      });

      const logoutSpy = vi.spyOn(accountSessionModule, "logoutAccountSession");
      const clearStoredSpy = vi.spyOn(accountSessionModule, "clearStoredAccountSessionToken");

      render(<RemoteApp />);

      await waitFor(() => {
        expect(desktop.stateRequests).toBeGreaterThanOrEqual(2);
        expect(desktop.closeCalls).toBeGreaterThan(0);
      });

      // The attachment is gone, but a device-scoped status is not account revocation.
      expect(desktop.closeCalls).toBeGreaterThan(0);
      expect(clearStoredSpy).not.toHaveBeenCalled();
      expect(logoutSpy).not.toHaveBeenCalled();
      expect(getStoredAccountSessionToken(window.location.origin)).toBe("active-session-token");
      expect(getAccountLastSelectedTarget(window.location.origin)).toEqual({
        machineId: "mach-phone-1",
        workspaceId: "ws-ferryx",
        worktreeSlug: "main",
        worktreeLabel: "main",
      });
    },
  );

  // Committed contract (accountSessionRetention.test.tsx scenario c): a grant-site denial is
  // machine-scoped, so every status class below must reach the machine error, not account invalidation.
  it.each([
    { kind: "untyped 401", status: 401, body: "Gateway 401 Proxy Error", contentType: "text/plain" },
    { kind: "untyped 403", status: 403, body: "<html><body>403 Forbidden</body></html>", contentType: "text/html" },
    {
      kind: "structured 401 UNAUTHORIZED",
      status: 401,
      body: JSON.stringify({ code: "UNAUTHORIZED", message: "Machine grant expired" }),
      contentType: "application/json",
    },
  ])(
    "machine-scoped grant rejection with $kind keeps the account session and reports the machine error",
    async ({ status, body, contentType }) => {
      storeAccountSessionToken("active-session-token", window.location.origin);

      setAccountLastSelectedTarget(window.location.origin, {
        machineId: "mach-phone-1",
        workspaceId: "ws-ferryx",
        worktreeSlug: "main",
        worktreeLabel: "main",
      });

      globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
        const url = typeof input === "string" ? input : input.toString();
        if (url.endsWith("/api/account/v1/machines")) {
          return Promise.resolve(new Response(JSON.stringify(sampleMachines), { status: 200 }));
        }
        if (url.includes("/grants")) {
          return Promise.resolve(responseText(status, body, contentType));
        }
        return Promise.resolve(new Response("Not Found", { status: 404 }));
      });

      const logoutSpy = vi.spyOn(accountSessionModule, "logoutAccountSession");
      const clearStoredSpy = vi.spyOn(accountSessionModule, "clearStoredAccountSessionToken");

      render(<RemoteApp />);

      const picker = await openWorkspaceContextPicker();
      const machineStatus = await within(picker).findByTestId("remote-account-machine-status-mach-phone-1");

      expect(machineStatus).toHaveAttribute("data-status", "error");
      expect(clearStoredSpy).not.toHaveBeenCalled();
      expect(logoutSpy).not.toHaveBeenCalled();
      expect(getStoredAccountSessionToken(window.location.origin)).toBe("active-session-token");
      expect(getAccountLastSelectedTarget(window.location.origin)).toEqual({
        machineId: "mach-phone-1",
        workspaceId: "ws-ferryx",
        worktreeSlug: "main",
        worktreeLabel: "main",
      });
    },
  );

});
