import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";
import { openWorkspacePicker } from "./home/phoneHomeTestUtils";
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
import { continuityRestoreMessages } from "./accountContinuityRestore";
import type { RemoteSessionInventory, RemoteSessionInventoryEntry } from "./remoteSessionInventory";
import type { AccountMachineView } from "./accountSession";
import type {
  AccountWorktreeOption,
  ActiveMachineConnection,
  MachineDiscoveryStatus,
  UseAccountWorktreesResult,
} from "./useAccountWorktrees";
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
    // A successful empty continuity read permits this suite's device-local restore scenarios.
    vi.spyOn(accountSessionModule, "getAccountContinuity").mockResolvedValue(null);
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

    const picker = await openWorkspacePicker();
    // Sign-out lives only in the Account sheet on the phone home (H-18).
    expect(within(picker).queryByRole("button", { name: "Sign out" })).toBeNull();
    act(() => {
      fireEvent.click(screen.getByTestId("account-open"));
    });
    const signOutBtn = await screen.findByTestId("account-sign-out");
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
      expect(screen.getByTestId("phone-panel-start-session")).toBeInTheDocument();
      expect(screen.getByText("ws-ferryx / main")).toBeInTheDocument();
    });

    expect(getAccountLastSelectedTarget(window.location.origin)).toEqual({
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    });
  });

  it("preserves the device target through a failed account read; only a successful empty result permits fallback", async () => {
    const origin = window.location.origin;
    storeAccountSessionToken("active-session-token", origin);
    setAccountLastSelectedTarget(origin, { machineId: "unlisted", workspaceId: "stored", worktreeSlug: null });
    const readPointer = vi.mocked(accountSessionModule.getAccountContinuity);
    let readable = false;
    readPointer.mockImplementation(async () => {
      if (!readable) throw new Error("offline");
      return null;
    });
    globalThis.fetch = vi.fn(async () => new Response("[]", { status: 200 }));
    render(<RemoteApp />);
    await openWorkspacePicker();
    const retry = await screen.findByRole("button", { name: "Retry session restore" });
    expect(getAccountLastSelectedTarget(origin)?.machineId).toBe("unlisted");
    readable = true;
    await act(async () => { fireEvent.click(retry); });
    await waitFor(() => expect(getAccountLastSelectedTarget(origin)).toBeNull());
    expect(readPointer.mock.calls.length).toBeGreaterThan(1);
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
      expect(screen.getByTestId("phone-panel-start-session")).toBeInTheDocument();
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
      expect(screen.getByTestId("phone-panel-start-session")).toBeInTheDocument();
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
      expect(screen.getByTestId("phone-panel-start-session")).toBeInTheDocument();
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

    const picker = await openWorkspacePicker();
    // Sign-out lives only in the Account sheet on the phone home (H-18).
    expect(within(picker).queryByRole("button", { name: "Sign out" })).toBeNull();
    act(() => {
      fireEvent.click(screen.getByTestId("account-open"));
    });
    const signOutBtn = await screen.findByTestId("account-sign-out");

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

  /* The continuity pointer names a session the discovery inventory still reports as running,
     so the restore reaches its own selection; that selection's /api/v1/sessions answer is what
     the two tests below vary. The worktree hook is replaced so acquireConnection hands back a
     connection whose close is reachable only from disconnect(), never from discovery pooling. */
  async function mountContinuityRestore(sessionsAnswer: () => attachTunnelModule.TunnelResponse) {
    storeAccountSessionToken("active-session-token", window.location.origin);

    vi.spyOn(accountSessionModule, "getAccountContinuity").mockResolvedValue({
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      sessionId: "sess-123",
      revision: 3,
      updatedAt: Date.now(),
    });

    const json = (value: unknown) => ({
      status: 200,
      headers: { "content-type": "application/json" },
      body: new TextEncoder().encode(JSON.stringify(value)),
    });

    let sessionsFetches = 0;
    const transport = {
      fetchLike: vi.fn().mockImplementation((path: string) => {
        if (path === "/api/v1/sessions") {
          sessionsFetches += 1;
          return Promise.resolve(sessionsAnswer());
        }
        if (path === "/api/v1/workspace/projects") {
          return Promise.resolve(json({
            revision: 1,
            completeness: "complete",
            projects: [{ workspaceId: "ws-ferryx", repoRoot: "/Users/dev/ferryx", availability: "ready", revision: 1 }],
            unavailableWorkspaceIds: [],
          }));
        }
        if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
          return Promise.resolve(json({
            revision: 1,
            worktrees: [{
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
            }],
          }));
        }
        return Promise.resolve({ status: 404, headers: {}, body: new Uint8Array(0) });
      }),
      openWebSocket: vi.fn(),
      close: vi.fn(),
      isClosed: false,
    };

    // The close spy resolves a promise so a test registers the signal before the restore can run.
    let closeObserved: (() => void) | null = null;
    const closedOnce = new Promise<void>((resolve) => {
      closeObserved = resolve;
    });
    const connectionClose = vi.fn(() => {
      closeObserved?.();
    });
    const acquired: ActiveMachineConnection = {
      transport,
      close: connectionClose,
      machine: sampleMachines[0],
      deviceToken: "acquired-device-bearer",
    };

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response(JSON.stringify(sampleMachines), { status: 200 }));
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    const runningEntry: RemoteSessionInventoryEntry = {
      machineId: "mach-phone-1",
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
      sessionId: "sess-123",
      daemonEpoch: "1790742255752",
      title: "zsh",
      running: true,
      agentType: null,
      providerSessionId: null,
      activityState: null,
    };
    const accountOption: AccountWorktreeOption = {
      machineId: "mach-phone-1",
      machineDisplayName: "Work MacBook Pro",
      machinePlatform: "macos",
      machineOnline: true,
      workspaceId: "ws-ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
    };
    const inventory: RemoteSessionInventory = {
      machineId: "mach-phone-1",
      revision: "1",
      completeness: "complete",
      unavailableWorkspaceIds: [],
      fetchedAt: Date.now(),
      rejectedRowCount: 0,
      entries: [runningEntry],
    };
    const machines: AccountMachineView[] = [sampleMachines[0]];
    const machineStatuses: Record<string, MachineDiscoveryStatus> = {
      "mach-phone-1": { machine: machines[0], status: "ready", options: [accountOption] },
    };
    const sessionInventories: Record<string, RemoteSessionInventory> = { "mach-phone-1": inventory };

    const origUseAccountWorktrees = useAccountWorktreesModule.useAccountWorktrees;
    vi.spyOn(useAccountWorktreesModule, "useAccountWorktrees").mockImplementation((...args) => {
      const hookRes = origUseAccountWorktrees(...args);
      const patched: UseAccountWorktreesResult = {
        ...hookRes,
        loading: false,
        initialized: true,
        error: null,
        machines,
        machineStatuses,
        accountOptions: [accountOption],
        sessionInventories,
        acquireConnection: vi.fn().mockResolvedValue(acquired),
      };
      return patched;
    });

    /* The picker's status node is mounted only once its surface is open, so the observer is armed
       before the picker is opened: the test registers this promise, then triggers, then awaits it
       before reading the DOM. It settles on the expected copy, on timeout, or on disposal. */
    const readStatus = () =>
      document.querySelector('[data-testid="remote-account-inventory-status"]')?.textContent ?? null;
    const observed: string[] = [];
    let observer: MutationObserver | null = null;
    let pendingTimer: ReturnType<typeof setTimeout> | null = null;
    let cancelPending: (() => void) | null = null;

    /* Disposal resolves a still-armed promise instead of rejecting it: when the trigger throws
       before anything awaits the arm, a rejection here would surface as an unhandled rejection,
       and leaving it pending would dangle. Settling first keeps both paths quiet. */
    const disposeStatusWatch = () => {
      observer?.disconnect();
      observer = null;
      if (pendingTimer !== null) clearTimeout(pendingTimer);
      pendingTimer = null;
      const cancel = cancelPending;
      cancelPending = null;
      cancel?.();
    };

    const armStatus = (expected: string, timeoutMs = 2000): Promise<string> =>
      new Promise<string>((resolve, reject) => {
        disposeStatusWatch();
        let done = false;
        const stopWatch = () => {
          observer?.disconnect();
          observer = null;
          if (pendingTimer !== null) clearTimeout(pendingTimer);
          pendingTimer = null;
        };
        const settle = () => {
          if (done) return false;
          done = true;
          stopWatch();
          cancelPending = null;
          return true;
        };
        cancelPending = () => {
          if (settle()) resolve(observed[observed.length - 1] ?? "");
        };
        const check = () => {
          if (done) return;
          const text = readStatus();
          if (text === null) return;
          if (observed[observed.length - 1] !== text) observed.push(text);
          if (text === expected && settle()) resolve(expected);
        };
        pendingTimer = setTimeout(() => {
          if (!settle()) return;
          const last = observed.length > 0 ? observed[observed.length - 1] : null;
          reject(new Error(
            `status never read ${JSON.stringify(expected)}; last observed ${JSON.stringify(last)}`,
          ));
        }, timeoutMs);
        check();
        if (done) return;
        observer = new MutationObserver(check);
        observer.observe(document.body, { childList: true, subtree: true, characterData: true });
      });

    // closeWithin registers on the arm-time promise, so a close that already happened still
    // settles it; the timeout only detects the absence of a close and never outlives the test.
    const closeWithin = (timeoutMs = 2000): Promise<void> =>
      new Promise<void>((resolve, reject) => {
        let settled = false;
        const timer = setTimeout(
          () => {
            if (settled) return;
            settled = true;
            reject(new Error(`connection close not observed within ${timeoutMs}ms`));
          },
          timeoutMs,
        );
        void closedOnce.then(() => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          resolve();
        });
      });

    render(<RemoteApp />);
    return {
      connectionClose,
      armStatus,
      disposeStatusWatch,
      closeWithin,
      sessionsFetches: () => sessionsFetches,
    };
  }

  it("Item C: reuses the tunnel and confirms the exact session after a transient refusal", async () => {
    const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
    let available = false;
    const { connectionClose, armStatus, disposeStatusWatch, sessionsFetches } = await mountContinuityRestore(() => ({
      status: available ? 200 : 503,
      headers: { "content-type": "application/json" },
      body: new TextEncoder().encode(JSON.stringify(available ? {
        revision: "2", completeness: "complete", sessions: ["sibling", "sess-123"].map((id) => ({
          target: { machineId: "mach-phone-1", sessionId: id, daemonEpoch: "1790742255752" },
          workspaceId: "ws-ferryx", worktree: { slug: "main" }, running: true,
        })),
      } : { error: "unavailable" })),
    }));

    const retryableNotice = armStatus(continuityRestoreMessages["network-error"]);
    try {
      await openWorkspacePicker();
      expect(await retryableNotice).toBe(continuityRestoreMessages["network-error"]);
      expect(screen.getByTestId("remote-account-inventory-status").textContent)
        .toBe(continuityRestoreMessages["network-error"]);
    } finally {
      disposeStatusWatch();
    }

    /* Two listings, each with a distinct job: the post-attach workspace load
       (loadWorkspace -> /api/v1/sessions) and the exact-target confirmation that
       decides whether this restore can resume. Anything higher would mean a retry
       loop, anything lower would mean the confirmation never ran. */
    expect(sessionsFetches()).toBe(2);
    expect(warning).toHaveBeenCalledWith("Selection failed", expect.anything());

    expect(connectionClose).not.toHaveBeenCalled();
    const preferred = () => accountSessionModule.getAccountPreferredSessionId(window.location.origin, "mach-phone-1", "ws-ferryx", "main");
    expect(preferred()).toBeNull();
    available = true;
    await act(async () => { window.dispatchEvent(new Event("online")); });
    expect(preferred()).toBe("sess-123");
    expect(connectionClose).not.toHaveBeenCalled();
  });

  it("Item C: a listing that confirms the exact session is gone disconnects", async () => {
    const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { connectionClose, armStatus, disposeStatusWatch, closeWithin, sessionsFetches } = await mountContinuityRestore(
      () => ({
        status: 200,
        headers: { "content-type": "application/json" },
        body: new TextEncoder().encode(
          JSON.stringify({ revision: "2", completeness: "complete", sessions: [] }),
        ),
      }),
    );

    // The named session is absent from a complete listing, so nothing is left to resume on this
    // connection and the restore revokes it instead of leaving a stale tunnel up. The close is
    // the observable difference: a transient refusal above leaves this same spy uncalled.
    await closeWithin();
    expect(connectionClose).toHaveBeenCalledTimes(1);
    // Same two-listing contract as the refusal case above: workspace load, then confirmation.
    expect(sessionsFetches()).toBe(2);
    expect(warning).toHaveBeenCalledWith("Selection failed", expect.anything());

    const goneNotice = armStatus(continuityRestoreMessages["session-gone"]);
    try {
      await openWorkspacePicker();
      expect(await goneNotice).toBe(continuityRestoreMessages["session-gone"]);
      expect(screen.getByTestId("remote-account-inventory-status").textContent)
        .toBe(continuityRestoreMessages["session-gone"]);
    } finally {
      disposeStatusWatch();
    }
  });
});
