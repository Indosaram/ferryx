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

  it("Item A: explicit Sign out calls logoutAccountSession and clears storage even if network fetch rejects", async () => {
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

    const topContextTrigger = await screen.findByRole("button", {
      name: "Change workspace context",
    });

    act(() => {
      fireEvent.click(topContextTrigger);
    });

    const picker = await screen.findByRole("dialog", { name: "Workspace context" });
    const signOutBtn = within(picker).getByRole("button", { name: "Sign out" });
    expect(signOutBtn).toBeDefined();

    act(() => {
      fireEvent.click(signOutBtn);
    });

    await waitFor(() => {
      expect(logoutSpy).toHaveBeenCalledWith(window.location.origin, "test-account-session-token-xyz");
      expect(getStoredAccountSessionToken(window.location.origin)).toBeNull();
    });

    unmount();
  });

  it("Item A: onUnauthorized callback does NOT call logoutAccountSession and only clears locally", async () => {
    storeAccountSessionToken("expired-session-token", window.location.origin);

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
    });

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

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();

      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response(JSON.stringify(sampleMachines), { status: 200 }));
      }
      if (url.includes("/api/account/v1/machines/rec-mbp-1/grants")) {
        return Promise.resolve(
          new Response(
            JSON.stringify({
              grantId: "grant-test-1",
              machineId: "mach-phone-1",
              relayOrigin: window.location.origin,
              pairingToken: "pair-tok-secret",
              machineAttachPublicKey: "machine-noise-pub-key",
              grantScope: "machine",
              expiresAt: Date.now() + 600000,
            }),
            { status: 200 },
          ),
        );
      }
      if (url.endsWith("/api/v1/attach/session")) {
        return Promise.resolve(
          new Response(JSON.stringify({ sessionId: "sess-alloc-test-42" }), { status: 200 }),
        );
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
            body: new TextEncoder().encode(
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
            body: new TextEncoder().encode(
              JSON.stringify({
                revision: 1,
                completeness: "complete",
                projects: [
                  {
                    workspaceId: "ws-ferryx",
                    repoRoot: "/Users/dev/ferryx",
                    availability: "ready",
                    revision: 1,
                  },
                ],
                unavailableWorkspaceIds: [],
              }),
            ),
          });
        }
        if (path === "/api/v1/workspace/state") {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({
                revision: 1,
                workspaces: [
                  {
                    id: "ws-ferryx",
                    repoPath: "/Users/dev/ferryx",
                    worktrees: [
                      {
                        slug: "main",
                        branch: "main",
                        path: "/Users/dev/ferryx",
                      },
                    ],
                  },
                ],
              }),
            ),
          });
        }
        if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({
                revision: 1,
                worktrees: [
                  {
                    workspaceId: "ws-ferryx",
                    identity: { wsId: "ws-ferryx", slug: "main" },
                    path: "/Users/dev/ferryx",
                    head: "h1",
                    branch: "refs/heads/main",
                    bare: false,
                    detached: false,
                    locked: null,
                    prunable: null,
                    managed: false,
                  },
                ],
              }),
            ),
          });
        }
        if (path === "/api/v1/sessions") {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({
                revision: "1",
                completeness: "complete",
                sessions: [
                  {
                    workspaceId: "ws-ferryx",
                    worktree: { wsId: "ws-ferryx", slug: "main" },
                    target: {
                      machineId: "mach-phone-1",
                      sessionId: "sess-123",
                      daemonEpoch: "1790742255752",
                    },
                    running: true,
                    title: "zsh",
                  },
                ],
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
      expect(screen.getByText("ws-ferryx / main")).toBeInTheDocument();
    });

    expect(getAccountLastSelectedTarget(window.location.origin)).toEqual({
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });
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
            body: new TextEncoder().encode(
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
            body: new TextEncoder().encode(
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
            body: new TextEncoder().encode(
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
            body: new TextEncoder().encode(
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

    let resolveAcquireConnection!: (val: any) => void;
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
        machines: sampleMachines as any,
        machineStatuses: {
          "mach-phone-1": {
            machine: sampleMachines[0] as any,
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

    const topContextTrigger = await screen.findByRole("button", {
      name: "Change workspace context",
    });

    act(() => {
      fireEvent.click(topContextTrigger);
    });

    const picker = await screen.findByRole("dialog", { name: "Workspace context" });
    const signOutBtn = within(picker).getByRole("button", { name: "Sign out" });

    act(() => {
      fireEvent.click(signOutBtn);
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
});
