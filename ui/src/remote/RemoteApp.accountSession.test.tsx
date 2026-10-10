import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";
import { storeAccountSessionToken, clearStoredAccountSessionToken } from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
import { remoteHostStore } from "../state/remoteHostStore";
import { setRemoteAuthToken } from "../lib/remoteClient";

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

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (err: unknown) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (err: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("RemoteApp - Account Session Phone Flow", () => {
  beforeEach(() => {
    MockTestWebSocket.instances = [];
    vi.stubGlobal("WebSocket", MockTestWebSocket);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
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
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    clearStoredAccountSessionToken();
    localStorage.clear();
    remoteHostStore.reset();
  });

  it("renders top context selector with empty initial body after login, then opens terminal pane on selecting worktree", async () => {
    storeAccountSessionToken("test-account-session-token-xyz", window.location.origin);

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

    const mockEventWs = {
      readyState: 1,
      send: vi.fn(),
      close: vi.fn(),
      onopen: null as any,
      onclose: null as any,
      onmessage: null as any,
      onerror: null as any,
    };

    const mockTerminalWs = {
      readyState: 1,
      send: vi.fn(),
      close: vi.fn(),
      onopen: null as any,
      onclose: null as any,
      onmessage: null as any,
      onerror: null as any,
    };

    let currentHostWorkspace = "ws-ferryx";
    let currentHostSlug = "main";
    let currentHostSessionId = "term-sess-77";

    const mockTunnelTransport = {
      fetchLike: vi.fn().mockImplementation((path: string, init?: RequestInit) => {
        if (path.startsWith("/api/v1/pair/exchange")) {
          const body = {
            token: "tunnel-redeemed-device-bearer",
            device: { id: "dev-phone-1", name: "Phone" },
            machineId: "mach-phone-1",
            displayName: "Work MacBook Pro",
          };
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify(body)),
          });
        }

        if (path.startsWith("/api/v1/workspace/select")) {
          if (init?.body && typeof init.body === "string") {
            try {
              const parsed = JSON.parse(init.body);
              if (parsed.workspaceId) currentHostWorkspace = parsed.workspaceId;
              if (parsed.worktreeSlug) currentHostSlug = parsed.worktreeSlug;
            } catch {}
          }
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify({ ok: true })),
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
                    workspaceId: currentHostWorkspace,
                    worktree: { wsId: currentHostWorkspace, slug: currentHostSlug },
                    target: {
                      machineId: "mach-phone-1",
                      sessionId: currentHostSessionId,
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

        if (path.startsWith("/api/v1/workspace/state")) {
          const state = {
            projects: [
              {
                workspaceId: "ws-ferryx",
                repoRoot: "/Users/dev/ferryx",
                worktrees: [
                  { slug: "main", label: "main" },
                ],
              },
            ],
            activeContext: {
              workspaceId: currentHostWorkspace,
              worktreeSlug: currentHostSlug,
              worktreeLabel: currentHostSlug,
              sessionId: currentHostSessionId,
              activeTerminal: {
                sessionId: currentHostSessionId,
                title: "zsh",
                running: true,
              },
            },
            sessions: [
              {
                sessionId: currentHostSessionId,
                running: true,
                title: "zsh",
                workspaceId: currentHostWorkspace,
              },
            ],
          };
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify(state)),
          });
        }

        return Promise.resolve({
          status: 404,
          headers: {},
          body: new Uint8Array(0),
        });
      }),
      openWebSocket: vi.fn().mockImplementation((path: string) => {
        if (path.startsWith("/api/v1/terminal/")) {
          return Promise.resolve(mockTerminalWs);
        }
        return Promise.resolve(mockEventWs);
      }),
      close: vi.fn(),
    };

    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
      transport: mockTunnelTransport as any,
      close: vi.fn(),
    });

    render(<RemoteApp />);

    const topContextTrigger = await waitFor(() =>
      screen.getByRole("button", { name: /Change workspace context/i })
    );
    expect(topContextTrigger).toBeDefined();
    expect(topContextTrigger.getAttribute("aria-expanded")).toBe("false");

    expect(screen.queryByTestId("remote-terminal-grid")).toBeNull();
    expect(screen.queryByTestId("remote-terminal")).toBeNull();
    expect(screen.queryByTestId("account-worktrees-container")).toBeNull();
    expect(screen.queryByRole("heading", { name: "Worktrees" })).toBeNull();

    expect(
      mockTunnelTransport.fetchLike.mock.calls.some(([path]) =>
        path.includes("/api/v1/workspace/select"),
      ),
    ).toBe(false);

    act(() => {
      fireEvent.click(topContextTrigger);
    });
    expect(topContextTrigger.getAttribute("aria-expanded")).toBe("true");

    const optionBtn = await waitFor(() =>
      screen.getByRole("button", { name: "ws-ferryx / main" })
    );
    // Desktop-style worktree list only: no machine names, machine groups, or machine choice.
    const picker = screen.getByRole("dialog", { name: "Workspace context" });
    expect(picker.textContent).not.toContain("Work MacBook Pro");
    expect(picker.textContent).not.toContain("mach-phone-1");
    expect(within(picker).queryByRole("button", { name: /Work MacBook Pro|mach-phone-1/i })).toBeNull();
    expect(screen.queryByText(/Loading machines/i)).toBeNull();
    // Account mode has no machine chooser entry point; sign-out remains available.
    expect(within(picker).queryByRole("button", { name: "Machines" })).toBeNull();
    expect(within(picker).getByRole("button", { name: "Sign out" })).toBeDefined();

    act(() => {
      fireEvent.click(optionBtn);
    });

    await waitFor(() => {
      expect(
        mockTunnelTransport.fetchLike.mock.calls.some(([path]) => {
          return path.includes("/api/v1/sessions");
        }),
      ).toBe(true);
      expect(
        mockTunnelTransport.fetchLike.mock.calls.some(([path]) => {
          return path.includes("/api/v1/workspace/select");
        }),
      ).toBe(false);
      expect(
        mockTunnelTransport.fetchLike.mock.calls.some(([path, init]) => {
          return path.includes("/api/v1/sessions") && init?.method === "POST";
        }),
      ).toBe(false);
    });
    // The hidden machine identity routed the tunnel to the owning machine.
    expect(attachTunnelModule.openAccountTunnel).toHaveBeenCalledWith(
      expect.objectContaining({ machineId: "mach-phone-1" }),
    );

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal-grid")).toBeDefined();
    });
  });

  it("does not prematurely mount old terminal when chosen target is wsB while host state remains wsA", async () => {
    // Non-account desktop mode: bearer device token without accountSessionToken
    clearStoredAccountSessionToken();
    localStorage.setItem("ferryx_remote_token", "desktop-test-token-xyz");

    let currentHostWorkspace = "wsA";
    let currentHostSlug = "main";
    let currentHostSessionId = "sess-old-wsA";

    const selectPostDeferred = deferred<Response>();

    const fetchMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);

      if (url.includes("/api/v1/workspace/select")) {
        return selectPostDeferred.promise;
      }

      if (url.includes("/api/v1/workspace/state")) {
        const state = {
          projects: [
            {
              workspaceId: "wsA",
              repoRoot: "/srv/repoA",
              worktrees: [{ slug: "main", label: "main" }],
            },
            {
              workspaceId: "wsB",
              repoRoot: "/srv/repoB",
              worktrees: [{ slug: "feature-b", label: "feature-b" }],
            },
          ],
          activeContext: {
            workspaceId: currentHostWorkspace,
            worktreeSlug: currentHostSlug,
            worktreeLabel: currentHostSlug,
            sessionId: currentHostSessionId,
            activeTerminal: {
              sessionId: currentHostSessionId,
              title: "terminal",
              running: true,
            },
          },
          sessions: [
            {
              sessionId: currentHostSessionId,
              running: true,
              title: "terminal",
              workspaceId: currentHostWorkspace,
            },
          ],
        };
        return new Response(JSON.stringify(state), { status: 200 });
      }

      return new Response("Not Found", { status: 404 });
    });

    const ticketedFetch = (inner: typeof fetch): typeof fetch => {
      return vi.fn<typeof fetch>(async (input, init) => {
        const url = String(input instanceof Request ? input.url : input);
        if (url.includes("/api/v1/socket-ticket")) {
          return new Response(JSON.stringify({ ticket: "ui-test-ticket", expiresAt: 9999999999 }), { status: 200 });
        }
        return inner(input, init);
      }) as unknown as typeof fetch;
    };

    vi.stubGlobal("fetch", ticketedFetch(fetchMock));
    vi.stubGlobal("WebSocket", MockTestWebSocket);

    render(<RemoteApp />);

    const topContextTrigger = await waitFor(() =>
      screen.getByRole("button", { name: /Change workspace context/i })
    );

    act(() => {
      fireEvent.click(topContextTrigger);
    });

    const selectWsBBtn = await waitFor(() =>
      screen.getByRole("button", { name: /feature-b/i })
    );
    fireEvent.click(selectWsBBtn);

    await waitFor(() => {
      expect(
        fetchMock.mock.calls.some(([input]) => String(input instanceof Request ? input.url : input).includes("/api/v1/workspace/select")),
      ).toBe(true);
    });

    // In desktop mode with existing active terminal, verify current desktop context and active socket target remain old session
    expect(screen.getByTestId("remote-terminal-grid")).toBeDefined();
    expect(screen.getByLabelText("Current desktop context").textContent).toBe("wsA / main");
    expect(MockTestWebSocket.instances.some((s) => s.url.includes("sess-new-wsB"))).toBe(false);

    currentHostWorkspace = "wsB";
    currentHostSlug = "feature-b";
    currentHostSessionId = "sess-new-wsB";

    await act(async () => {
      selectPostDeferred.resolve(new Response(JSON.stringify({ ok: true }), { status: 200 }));
    });

    const eventWs = MockTestWebSocket.instances.find((s) => s.url.includes("/api/v1/events"));
    act(() => {
      eventWs?.onmessage?.({
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: {
            workspaceId: "wsB",
            worktreeSlug: "feature-b",
          },
        }),
      } as MessageEvent);
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal-grid")).toBeDefined();
      expect(screen.getByLabelText("Current desktop context").textContent).toBe("wsB / feature-b");
      expect(MockTestWebSocket.instances.some((s) => s.url.includes("sess-new-wsB"))).toBe(true);
    });
  });

  it("confirms the selection when the desktop event lands while the post-select state read is in flight", async () => {
    // Non-account desktop mode: bearer device token without accountSessionToken
    clearStoredAccountSessionToken();
    localStorage.setItem("ferryx_remote_token", "desktop-test-token-xyz");

    let hostWorkspace = "wsA";
    let hostSlug = "main";
    let hostSessionId = "sess-old-wsA";
    let selectAccepted = false;
    // The first state read issued after the select POST is held open so the selection
    // event can arrive while it is in flight.
    const heldStateRead = deferred<void>();
    let heldStateReadStarted: (() => void) | null = null;
    const heldStateReadIssued = new Promise<void>((resolve) => { heldStateReadStarted = resolve; });
    let stateReadsAfterSelect = 0;
    // The select response is held until the event socket is live, matching the desktop order.
    const selectRelease = deferred<void>();

    const stateBody = () => ({
      projects: [
        { workspaceId: "wsA", repoRoot: "/srv/repoA", worktrees: [{ slug: "main", label: "main" }] },
        { workspaceId: "wsB", repoRoot: "/srv/repoB", worktrees: [{ slug: "feature-b", label: "feature-b" }] },
      ],
      activeContext: {
        workspaceId: hostWorkspace,
        worktreeSlug: hostSlug,
        worktreeLabel: hostSlug,
        sessionId: hostSessionId,
        activeTerminal: { sessionId: hostSessionId, title: "terminal", running: true },
      },
      sessions: [{ sessionId: hostSessionId, running: true, title: "terminal", workspaceId: hostWorkspace }],
    });

    const fetchMock = vi.fn<typeof fetch>(async (input: RequestInfo | URL) => {
      const url = String(input instanceof Request ? input.url : input);

      if (url.includes("/api/v1/workspace/select")) {
        await selectRelease.promise;
        hostWorkspace = "wsB";
        hostSlug = "feature-b";
        hostSessionId = "sess-new-wsB";
        selectAccepted = true;
        return new Response(JSON.stringify({ ok: true }), { status: 200 });
      }

      if (url.includes("/api/v1/workspace/state")) {
        const body = stateBody();
        if (selectAccepted && ++stateReadsAfterSelect === 1) {
          heldStateReadStarted?.();
          await heldStateRead.promise;
        }
        return new Response(JSON.stringify(body), { status: 200 });
      }

      return new Response("Not Found", { status: 404 });
    });

    const ticketedFetch = (inner: typeof fetch): typeof fetch => {
      return vi.fn<typeof fetch>(async (input, init) => {
        const url = String(input instanceof Request ? input.url : input);
        if (url.includes("/api/v1/socket-ticket")) {
          return new Response(JSON.stringify({ ticket: "ui-test-ticket", expiresAt: 9999999999 }), { status: 200 });
        }
        return inner(input, init);
      }) as unknown as typeof fetch;
    };

    vi.stubGlobal("fetch", ticketedFetch(fetchMock));
    vi.stubGlobal("WebSocket", MockTestWebSocket);

    render(<RemoteApp />);

    const trigger = await waitFor(() => screen.getByRole("button", { name: /Change workspace context/i }));
    act(() => {
      fireEvent.click(trigger);
    });
    fireEvent.click(await waitFor(() => screen.getByRole("button", { name: /feature-b/i })));

    await waitFor(() => {
      expect(
        fetchMock.mock.calls.some(([input]) => String(input instanceof Request ? input.url : input).includes("/api/v1/workspace/select")),
      ).toBe(true);
    });

    const eventWs = MockTestWebSocket.instances.find((s) => s.url.includes("/api/v1/events"));
    // Send event while select POST is held so confirmSelection proceeds into held post-select state read
    act(() => {
      eventWs?.onmessage?.({
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: { workspaceId: "wsB", worktreeSlug: "feature-b" },
        }),
      } as MessageEvent);
    });

    await act(async () => {
      selectRelease.resolve();
    });
    await heldStateReadIssued;
    act(() => {
      eventWs?.onmessage?.({
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: { workspaceId: "wsB", worktreeSlug: "feature-b" },
        }),
      } as MessageEvent);
    });
    await act(async () => {
      heldStateRead.resolve();
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal-grid")).toBeDefined();
    });
    expect(screen.getByLabelText("Current desktop context").textContent).toBe("wsB / feature-b");
  });

  it("displays error and allows retry without mounting old terminal if context selection POST fails", async () => {
    storeAccountSessionToken("test-account-session-token-xyz", window.location.origin);

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

    const mockTerminalWs = {
      readyState: 1,
      send: vi.fn(),
      close: vi.fn(),
      onopen: null as any,
      onclose: null as any,
      onmessage: null as any,
      onerror: null as any,
    };
    const mockEventWs = {
      readyState: 1,
      send: vi.fn(),
      close: vi.fn(),
      onopen: null as any,
      onclose: null as any,
      onmessage: null as any,
      onerror: null as any,
    };
    let discoverySessionsResolved = false;

    const mockTunnelTransport = {
      fetchLike: vi.fn().mockImplementation((path: string) => {
        if (path.startsWith("/api/v1/pair/exchange")) {
          const body = {
            token: "tunnel-redeemed-device-bearer",
            device: { id: "dev-phone-1", name: "Phone" },
            machineId: "mach-phone-1",
            displayName: "Work MacBook Pro",
          };
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify(body)),
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
                    workspaceId: "wsA",
                    repoRoot: "/srv/repoA",
                    availability: "ready",
                    revision: 1,
                  },
                  {
                    workspaceId: "wsB",
                    repoRoot: "/srv/repoB",
                    availability: "ready",
                    revision: 1,
                  },
                ],
                unavailableWorkspaceIds: [],
              }),
            ),
          });
        }

        if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
          const wsId = new URL(path, "http://localhost").searchParams.get("workspaceId");
          const slug = wsId === "wsB" ? "feature-b" : "main";
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({
                revision: 1,
                worktrees: [
                  {
                    workspaceId: wsId,
                    identity: { wsId, slug },
                    path: `/srv/${slug}`,
                    head: "h1",
                    branch: `refs/heads/${slug}`,
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
          if (!discoverySessionsResolved) {
            discoverySessionsResolved = true;
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
            status: 500,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify({ error: "Context switch rejected" })),
          });
        }

        if (path.startsWith("/api/v1/workspace/select")) {
          return Promise.resolve({
            status: 500,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify({ error: "Context switch rejected" })),
          });
        }

        if (path.startsWith("/api/v1/workspace/state")) {
          const state = {
            projects: [
              {
                workspaceId: "wsA",
                repoRoot: "/srv/repoA",
                worktrees: [{ slug: "main", label: "main" }],
              },
              {
                workspaceId: "wsB",
                repoRoot: "/srv/repoB",
                worktrees: [{ slug: "feature-b", label: "feature-b" }],
              },
            ],
            activeContext: {
              workspaceId: "wsA",
              worktreeSlug: "main",
              worktreeLabel: "main",
              sessionId: "sess-old-wsA",
            },
            sessions: [
              {
                sessionId: "sess-old-wsA",
                running: true,
                title: "terminal",
                workspaceId: "wsA",
              },
            ],
          };
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify(state)),
          });
        }

        return Promise.resolve({ status: 404, headers: {}, body: new Uint8Array(0) });
      }),
      openWebSocket: vi.fn().mockImplementation((path: string) => {
        if (path.startsWith("/api/v1/terminal/")) return Promise.resolve(mockTerminalWs);
        return Promise.resolve(mockEventWs);
      }),
      close: vi.fn(),
    };

    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
      transport: mockTunnelTransport as any,
      close: vi.fn(),
    });

    render(<RemoteApp />);

    const topContextTrigger = await waitFor(() =>
      screen.getByRole("button", { name: /Change workspace context/i })
    );

    act(() => {
      fireEvent.click(topContextTrigger);
    });

    const selectWsBBtn = await waitFor(() =>
      screen.getByRole("button", { name: /feature-b/i })
    );
    fireEvent.click(selectWsBBtn);

    await waitFor(() => {
      const alert = screen.getByRole("alert");
      expect(alert).toBeDefined();
      expect(alert.textContent).toMatch(/Selection failed \(500\)|Machine sessions query failed \(500\)|Selection request failed/i);
    });

    expect(screen.queryByTestId("remote-terminal-grid")).toBeNull();
  });

  it("releases pending selection and shows retry/back error when desktop confirmation times out", async () => {
    storeAccountSessionToken("test-account-session-token-xyz", window.location.origin);

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

    const mockTerminalWs = {
      readyState: 1,
      send: vi.fn(),
      close: vi.fn(),
      onopen: null as any,
      onclose: null as any,
      onmessage: null as any,
      onerror: null as any,
    };
    const mockEventWs = {
      readyState: 1,
      send: vi.fn(),
      close: vi.fn(),
      onopen: null as any,
      onclose: null as any,
      onmessage: null as any,
      onerror: null as any,
    };

    const postSelectReceived = deferred<void>();
    let discoverySessionsResolved = false;

    const mockTunnelTransport = {
      fetchLike: vi.fn().mockImplementation((path: string) => {
        if (path.startsWith("/api/v1/pair/exchange")) {
          const body = {
            token: "tunnel-redeemed-device-bearer",
            device: { id: "dev-phone-1", name: "Phone" },
            machineId: "mach-phone-1",
            displayName: "Work MacBook Pro",
          };
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify(body)),
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
                    workspaceId: "wsA",
                    repoRoot: "/srv/repoA",
                    availability: "ready",
                    revision: 1,
                  },
                  {
                    workspaceId: "wsB",
                    repoRoot: "/srv/repoB",
                    availability: "ready",
                    revision: 1,
                  },
                ],
                unavailableWorkspaceIds: [],
              }),
            ),
          });
        }

        if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
          const wsId = new URL(path, "http://localhost").searchParams.get("workspaceId");
          const slug = wsId === "wsB" ? "feature-b" : "main";
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({
                revision: 1,
                worktrees: [
                  {
                    workspaceId: wsId,
                    identity: { wsId, slug },
                    path: `/srv/${slug}`,
                    head: "h1",
                    branch: `refs/heads/${slug}`,
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
          if (!discoverySessionsResolved) {
            discoverySessionsResolved = true;
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
          postSelectReceived.resolve();
          return new Promise(() => {}); // hang selection request to trigger confirmation / completion timeout
        }

        if (path.startsWith("/api/v1/workspace/select")) {
          postSelectReceived.resolve();
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify({ ok: true })),
          });
        }

        if (path.startsWith("/api/v1/workspace/state")) {
          const state = {
            projects: [
              {
                workspaceId: "wsA",
                repoRoot: "/srv/repoA",
                worktrees: [{ slug: "main", label: "main" }],
              },
              {
                workspaceId: "wsB",
                repoRoot: "/srv/repoB",
                worktrees: [{ slug: "feature-b", label: "feature-b" }],
              },
            ],
            activeContext: {
              workspaceId: "wsA",
              worktreeSlug: "main",
              worktreeLabel: "main",
              sessionId: "sess-old-wsA",
            },
            sessions: [
              {
                sessionId: "sess-old-wsA",
                running: true,
                title: "terminal",
                workspaceId: "wsA",
              },
            ],
          };
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify(state)),
          });
        }

        return Promise.resolve({ status: 404, headers: {}, body: new Uint8Array(0) });
      }),
      openWebSocket: vi.fn().mockImplementation((path: string) => {
        if (path.startsWith("/api/v1/terminal/")) return Promise.resolve(mockTerminalWs);
        return Promise.resolve(mockEventWs);
      }),
      close: vi.fn(),
    };

    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
      transport: mockTunnelTransport as any,
      close: vi.fn(),
    });

    render(<RemoteApp />);

    const topContextTrigger = await waitFor(() =>
      screen.getByRole("button", { name: /Change workspace context/i })
    );

    act(() => {
      fireEvent.click(topContextTrigger);
    });

    const selectWsBBtn = await waitFor(() =>
      screen.getByRole("button", { name: /feature-b/i })
    );

    vi.useFakeTimers();
    try {
      await act(async () => {
        fireEvent.click(selectWsBBtn);
      });

      await postSelectReceived.promise;

      await act(async () => {
        await vi.advanceTimersByTimeAsync(6000);
      });

      const alert = screen.getByRole("alert");
      expect(alert).toBeDefined();
      expect(alert.textContent).toMatch(/did not confirm|retry/i);
      expect(screen.queryByTestId("remote-terminal-grid")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  const pickerMachine = (online: boolean) => ({
    machineRecordId: "rec-mbp-1",
    machineId: "mach-phone-1",
    displayName: "Work MacBook Pro",
    publicKey: "pub-key-1",
    attachPublicKey: "attach-pub-1",
    relayOrigin: window.location.origin,
    platform: "macos",
    online,
    enrollmentEpoch: "1",
    lastSeenAt: Date.now(),
  });

  async function openPickerWith(machines: unknown[], grantResponse?: Response) {
    storeAccountSessionToken("test-account-session-token-xyz", window.location.origin);
    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response(JSON.stringify(machines), { status: 200 }));
      }
      if (url.includes("/grants") && grantResponse) return Promise.resolve(grantResponse);
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });
    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "phone-initiator-pub-key-base64",
      privateKey: "phone-initiator-priv-key-base64",
    });
    render(<RemoteApp />);
    const trigger = await waitFor(() => screen.getByRole("button", { name: /Change workspace context/i }));
    act(() => {
      fireEvent.click(trigger);
    });
    return screen.getByRole("dialog", { name: "Workspace context" });
  }

  it("names an offline desktop instead of the generic empty picker line", async () => {
    const picker = await openPickerWith([pickerMachine(false)]);
    await waitFor(() => {
      expect(within(picker).getByText("Desktop is offline. Open Ferryx on your computer.")).toBeDefined();
    });
    expect(picker.textContent).not.toContain("No selectable desktop worktrees");
    expect(picker.textContent).not.toContain("Work MacBook Pro");
  });

  it("says no desktops are linked when the account has no machines", async () => {
    const picker = await openPickerWith([]);
    await waitFor(() => {
      expect(within(picker).getByText("No desktops linked to this account.")).toBeDefined();
    });
    expect(picker.textContent).not.toContain("No selectable desktop worktrees");
  });

  it("selects worktree and creates machine session via POST /api/v1/sessions without legacy select and survives refresh", async () => {
    let postSessionsPayload: any = null;
    const postSessionCreated = deferred<void>();
    let activeSessions: any[] = [];

    const mockTunnelTransport = {
      fetchLike: vi.fn().mockImplementation((path: string, init?: any) => {
        if (path === "/api/v1/pair/exchange") {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({ token: "device-token-123", displayName: "omarchy" }),
            ),
          });
        }

        if (path === "/api/v1/workspace/projects") {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({
                revision: 2,
                completeness: "complete",
                projects: [
                  {
                    workspaceId: "project-2a54aface19a497491abc9ed791fd65d",
                    repoRoot: "/srv/repos/EclipticRD-Rewrite",
                    availability: "ready",
                    revision: 1,
                  },
                ],
                unavailableWorkspaceIds: [],
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
                    workspaceId: "project-2a54aface19a497491abc9ed791fd65d",
                    identity: null,
                    path: "/srv/repos/EclipticRD-Rewrite",
                    head: "abc1234",
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
          if (!init || init.method === "GET" || !init.method) {
            return Promise.resolve({
              status: 200,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(
                JSON.stringify({
                  revision: "1",
                  completeness: "complete",
                  sessions: activeSessions,
                }),
              ),
            });
          }
          if (init?.method === "POST") {
            postSessionsPayload = JSON.parse(init.body);
            const created = {
              workspaceId: "project-2a54aface19a497491abc9ed791fd65d",
              worktree: null,
              target: {
                machineId: "2773ab38-d556-4a81-ae49-18a3b0fb83af",
                sessionId: "machine-sess-999",
                daemonEpoch: "1790742255752",
              },
              running: true,
              title: "main",
            };
            // Put an earlier session for the SAME workspace and worktree BEFORE the created one
            // to catch priority bugs where an earlier session would erroneously be picked
            activeSessions = [
              {
                workspaceId: "project-2a54aface19a497491abc9ed791fd65d",
                worktree: null,
                target: {
                  machineId: "2773ab38-d556-4a81-ae49-18a3b0fb83af",
                  sessionId: "machine-sess-wrong-earlier",
                  daemonEpoch: "1790742255752",
                },
                running: true,
                title: "earlier-session",
              },
              created,
            ];
            postSessionCreated.resolve();
            return Promise.resolve({
              status: 201,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(JSON.stringify(created)),
            });
          }
        }

        return Promise.resolve({ status: 404, headers: {}, body: new Uint8Array(0) });
      }),
      openWebSocket: vi.fn().mockImplementation((path: string) => {
        return Promise.resolve(new MockTestWebSocket(path));
      }),
      close: vi.fn(),
    };

    storeAccountSessionToken("test-account-session-token-xyz", window.location.origin);
    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(
          new Response(
            JSON.stringify([
              {
                machineId: "2773ab38-d556-4a81-ae49-18a3b0fb83af",
                machineRecordId: "rec-1",
                publicKey: "pub-1",
                attachPublicKey: "attach-1",
                relayOrigin: "https://relay.example.com",
                displayName: "omarchy",
                platform: "linux",
                online: true,
                enrollmentEpoch: "1",
                lastSeenAt: Date.now(),
              },
            ]),
            { status: 200 },
          ),
        );
      }
      if (url.includes("/grants")) {
        return Promise.resolve(
          new Response(
            JSON.stringify({
              grantId: "grant-1",
              pairingToken: "grant-pairing-token",
              relayOrigin: "https://relay.example.com",
              machineAttachPublicKey: "attach-pub",
            }),
            { status: 200 },
          ),
        );
      }
      if (url.endsWith("/api/v1/attach/session")) {
        return Promise.resolve(
          new Response(JSON.stringify({ sessionId: "sess-alloc-test-8" }), { status: 200 }),
        );
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
      transport: mockTunnelTransport as any,
      close: vi.fn(),
    });

    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "phone-initiator-pub-key-base64",
      privateKey: "phone-initiator-priv-key-base64",
    });

    const { unmount } = render(<RemoteApp />);

    const trigger = await waitFor(() =>
      screen.getByRole("button", { name: /Change workspace context/i }),
    );
    act(() => {
      fireEvent.click(trigger);
    });

    const worktreeOptionBtn = await waitFor(() => {
      const dialog = screen.getByRole("dialog", { name: "Workspace context" });
      return within(dialog).getByRole("button", {
        name: (content, element) =>
          element?.getAttribute("aria-label") === "project-2a54aface19a497491abc9ed791fd65d / main" ||
          content.trim() === "project-2a54aface19a497491abc9ed791fd65d / main",
      });
    });

    await act(async () => {
      fireEvent.click(worktreeOptionBtn);
    });

    await postSessionCreated.promise;

    expect(postSessionsPayload).toMatchObject({
      workspaceId: "project-2a54aface19a497491abc9ed791fd65d",
      worktree: null,
      cols: 80,
      rows: 24,
      startup: { kind: "shell" },
    });
    expect(typeof postSessionsPayload.requestId).toBe("string");

    const calledSelect = mockTunnelTransport.fetchLike.mock.calls.some((call: any[]) =>
      String(call[0]).includes("/api/v1/workspace/select"),
    );
    expect(calledSelect).toBe(false);

    const chatBtn = await waitFor(() => screen.getByTestId("remote-view-mode-chat"));
    act(() => {
      fireEvent.click(chatBtn);
    });

    await waitFor(() => {
      const openedTerminal = mockTunnelTransport.openWebSocket.mock.calls.some((call: any[]) =>
        String(call[0]).includes("/api/v1/terminal/machine-sess-999?daemonEpoch=1790742255752"),
      );
      expect(openedTerminal).toBe(true);
    });

    // Remount/refresh proof: remounting RemoteApp begins with intentional blank/picker top state.
    // User selects the same worktree again after reload.
    // It must discover and attach to the existing session (machine-sess-999),
    // and must NOT trigger another duplicate POST /api/v1/sessions.
    unmount();
    const callsBefore = mockTunnelTransport.fetchLike.mock.calls.filter(
      (c: any[]) => c[0] === "/api/v1/sessions" && c[1]?.method === "POST",
    ).length;

    const freshRemountDeferred = deferred<string>();
    mockTunnelTransport.openWebSocket.mockImplementation((path: string) => {
      if (path.includes("/api/v1/terminal/")) {
        freshRemountDeferred.resolve(path);
      }
      return Promise.resolve(new MockTestWebSocket(path));
    });

    render(<RemoteApp />);

    const remountTrigger = await waitFor(() =>
      screen.getByRole("button", { name: /Change workspace context/i }),
    );
    act(() => {
      fireEvent.click(remountTrigger);
    });

    const remountWorktreeBtn = await waitFor(() => {
      const dialog = screen.getByRole("dialog", { name: "Workspace context" });
      return within(dialog).getByRole("button", {
        name: (content, element) =>
          element?.getAttribute("aria-label") === "project-2a54aface19a497491abc9ed791fd65d / main" ||
          content.trim() === "project-2a54aface19a497491abc9ed791fd65d / main",
      });
    });

    await act(async () => {
      fireEvent.click(remountWorktreeBtn);
    });

    const remountChatBtn = await waitFor(() => screen.getByTestId("remote-view-mode-chat"));
    act(() => {
      fireEvent.click(remountChatBtn);
    });

    const freshTerminalPath = await freshRemountDeferred.promise;
    expect(freshTerminalPath).toContain("machine-sess-999");

    const callsAfter = mockTunnelTransport.fetchLike.mock.calls.filter(
      (c: any[]) => c[0] === "/api/v1/sessions" && c[1]?.method === "POST",
    ).length;
    expect(callsAfter).toBe(callsBefore);
  });

  it("does not create a session when GET /api/v1/sessions returns non-2xx status or 404", async () => {
    let postAttempted = false;
    const selectFailedDeferred = deferred<void>();

    let discoverySessionsResolved = false;

    const mockTunnelTransport = {
      fetchLike: vi.fn().mockImplementation((path: string, init?: any) => {
        if (path === "/api/v1/pair/exchange") {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({ token: "device-token-123", displayName: "omarchy" }),
            ),
          });
        }
        if (path === "/api/v1/workspace/projects") {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({
                revision: 2,
                completeness: "complete",
                projects: [
                  {
                    workspaceId: "project-2a54aface19a497491abc9ed791fd65d",
                    repoRoot: "/srv/repos/EclipticRD-Rewrite",
                    availability: "ready",
                    revision: 1,
                  },
                ],
                unavailableWorkspaceIds: [],
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
                    workspaceId: "project-2a54aface19a497491abc9ed791fd65d",
                    identity: null,
                    path: "/srv/repos/EclipticRD-Rewrite",
                    head: "abc1234",
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
          if (!discoverySessionsResolved) {
            discoverySessionsResolved = true;
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
          if (!init || init.method === "GET" || !init.method) {
            selectFailedDeferred.resolve();
            return Promise.resolve({
              status: 500,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(JSON.stringify({ error: "Internal Error" })),
            });
          }
          if (init?.method === "POST") {
            postAttempted = true;
            return Promise.resolve({ status: 201, headers: {}, body: new Uint8Array(0) });
          }
        }
        return Promise.resolve({ status: 404, headers: {}, body: new Uint8Array(0) });
      }),
      openWebSocket: vi.fn().mockImplementation((path: string) => {
        return Promise.resolve(new MockTestWebSocket(path));
      }),
      close: vi.fn(),
    };

    storeAccountSessionToken("test-account-session-token-xyz", window.location.origin);
    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(
          new Response(
            JSON.stringify([
              {
                machineId: "2773ab38-d556-4a81-ae49-18a3b0fb83af",
                machineRecordId: "rec-1",
                publicKey: "pub-1",
                attachPublicKey: "attach-1",
                relayOrigin: "https://relay.example.com",
                displayName: "omarchy",
                platform: "linux",
                online: true,
                enrollmentEpoch: "1",
                lastSeenAt: Date.now(),
              },
            ]),
            { status: 200 },
          ),
        );
      }
      if (url.includes("/grants")) {
        return Promise.resolve(
          new Response(
            JSON.stringify({
              grantId: "grant-1",
              pairingToken: "grant-pairing-token",
              relayOrigin: "https://relay.example.com",
              machineAttachPublicKey: "attach-pub",
            }),
            { status: 200 },
          ),
        );
      }
      if (url.endsWith("/api/v1/attach/session")) {
        return Promise.resolve(
          new Response(JSON.stringify({ sessionId: "sess-alloc-test-9" }), { status: 200 }),
        );
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
      transport: mockTunnelTransport as any,
      close: vi.fn(),
    });

    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "phone-initiator-pub-key-base64",
      privateKey: "phone-initiator-priv-key-base64",
    });

    render(<RemoteApp />);

    const trigger = await waitFor(() =>
      screen.getByRole("button", { name: /Change workspace context/i }),
    );
    act(() => {
      fireEvent.click(trigger);
    });

    const worktreeOptionBtn = await waitFor(() =>
      screen.getByRole("button", { name: /main/i }),
    );

    await act(async () => {
      fireEvent.click(worktreeOptionBtn);
    });

    await selectFailedDeferred.promise;

    expect(postAttempted).toBe(false);
    expect(
      mockTunnelTransport.fetchLike.mock.calls.some(
        (call: any[]) => call[0] === "/api/v1/sessions" && call[1]?.method === "POST",
      ),
    ).toBe(false);
  });

  it("sends binary bytes for terminal input over active account machine WebSocket and does not leak token", async () => {
    let sentData: any = null;

    const mockTerminalWsInstance = {
      readyState: 1,
      send: vi.fn((data: any) => {
        sentData = data;
      }),
      close: vi.fn(),
      onopen: null as any,
      onclose: null as any,
      onmessage: null as any,
      onerror: null as any,
    };

    const mockTunnelTransport = {
      fetchLike: vi.fn().mockImplementation((path: string) => {
        if (path === "/api/v1/pair/exchange") {
          return Promise.resolve({
            status: 200,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({ token: "secret-device-token", displayName: "omarchy" }),
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
                    workspaceId: "ws-chat",
                    repoRoot: "/srv/chat",
                    availability: "ready",
                    revision: 1,
                  },
                ],
                unavailableWorkspaceIds: [],
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
                    workspaceId: "ws-chat",
                    identity: null,
                    path: "/srv/chat",
                    head: "c1",
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
                    workspaceId: "ws-chat",
                    worktree: null,
                    target: {
                      machineId: "2773ab38-d556-4a81-ae49-18a3b0fb83af",
                      sessionId: "machine-chat-sess",
                      daemonEpoch: "1790742255752",
                    },
                    running: true,
                    title: "main",
                  },
                ],
              }),
            ),
          });
        }
        return Promise.resolve({ status: 404, headers: {}, body: new Uint8Array(0) });
      }),
      openWebSocket: vi.fn().mockImplementation((path: string) => {
        if (path.startsWith("/api/v1/terminal/")) {
          return Promise.resolve(mockTerminalWsInstance);
        }
        return Promise.resolve(new MockTestWebSocket(path));
      }),
      close: vi.fn(),
    };

    storeAccountSessionToken("test-account-session-token-xyz", window.location.origin);
    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(
          new Response(
            JSON.stringify([
              {
                machineId: "2773ab38-d556-4a81-ae49-18a3b0fb83af",
                machineRecordId: "rec-1",
                publicKey: "pub-1",
                attachPublicKey: "attach-1",
                relayOrigin: "https://relay.example.com",
                displayName: "omarchy",
                platform: "linux",
                online: true,
                enrollmentEpoch: "1",
                lastSeenAt: Date.now(),
              },
            ]),
            { status: 200 },
          ),
        );
      }
      if (url.includes("/grants")) {
        return Promise.resolve(
          new Response(
            JSON.stringify({
              grantId: "grant-1",
              pairingToken: "grant-pairing-token",
              relayOrigin: "https://relay.example.com",
              machineAttachPublicKey: "attach-pub",
            }),
            { status: 200 },
          ),
        );
      }
      if (url.endsWith("/api/v1/attach/session")) {
        return Promise.resolve(
          new Response(JSON.stringify({ sessionId: "sess-alloc-test-10" }), { status: 200 }),
        );
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
      transport: mockTunnelTransport as any,
      close: vi.fn(),
    });

    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "phone-initiator-pub-key-base64",
      privateKey: "phone-initiator-priv-key-base64",
    });

    render(<RemoteApp />);

    const trigger = await waitFor(() =>
      screen.getByRole("button", { name: /Change workspace context/i }),
    );
    act(() => {
      fireEvent.click(trigger);
    });

    const worktreeOptionBtn = await waitFor(() => {
      const dialog = screen.getByRole("dialog", { name: "Workspace context" });
      return within(dialog).getByRole("button", {
        name: (content, element) =>
          element?.getAttribute("aria-label") === "ws-chat / main" ||
          content.trim() === "ws-chat / main",
      });
    });

    await act(async () => {
      fireEvent.click(worktreeOptionBtn);
    });

    const chatBtn = await waitFor(() => screen.getByTestId("remote-view-mode-chat"));
    act(() => {
      fireEvent.click(chatBtn);
    });

    await waitFor(() => {
      expect(mockTunnelTransport.openWebSocket).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/terminal/machine-chat-sess?daemonEpoch=1790742255752"),
        { Authorization: "Bearer secret-device-token" },
      );
    });

    const composerTextarea = await waitFor(() =>
      screen.getByRole("textbox", { name: "Ask the repo agent" }),
    );

    fireEvent.change(composerTextarea, { target: { value: "hello agent" } });

    const sendBtn = screen.getByRole("button", { name: /Send message/i });
    act(() => {
      fireEvent.click(sendBtn);
    });

    expect(ArrayBuffer.isView(sentData)).toBe(true);
    expect(Object.prototype.toString.call(sentData)).toBe("[object Uint8Array]");
    const decoded = new TextDecoder().decode(sentData);
    expect(decoded).toBe("hello agent\n");
    expect(decoded).not.toContain("secret-device-token");
    expect(decoded).not.toContain("test-account-session-token");
  });

  it("shows the structured relay error code and message with retry when discovery fails", async () => {
    const picker = await openPickerWith(
      [pickerMachine(true)],
      new Response(
        JSON.stringify({ code: "GRANT_DELIVERY_FAILED", message: "Desktop did not accept the grant" }),
        { status: 502, headers: { "content-type": "application/json" } },
      ),
    );
    const alert = await waitFor(() => within(picker).getByRole("alert"));
    expect(alert.textContent).toContain("GRANT_DELIVERY_FAILED");
    expect(within(picker).getByRole("button", { name: "Retry loading worktrees" })).toBeDefined();
    expect(picker.textContent).not.toContain("No selectable desktop worktrees");
  });
});
