import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";

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

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";
import { storeAccountSessionToken, clearStoredAccountSessionToken } from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
import { remoteHostStore } from "../state/remoteHostStore";

/**
 * Chat is the default surface at every width now (plan task 12), so a test that asserts the
 * terminal asks for it explicitly through the mode switch the header always offers.
 */
async function switchToTerminalMode(): Promise<void> {
  const toggle = screen.queryByTestId("remote-view-mode-terminal");
  if (!toggle) return;
  await act(async () => {
    fireEvent.click(toggle);
  });
}


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

    // The body is the preselection placeholder here, and the header's status cluster - which owns
    // the mode switch - is not rendered yet, so no terminal can be mounted whichever mode the app
    // will end up in.
    expect(screen.queryByTestId("remote-terminal-grid")).toBeNull();
    expect(screen.queryByTestId("remote-terminal")).toBeNull();
    expect(screen.queryByTestId("account-worktrees-container")).toBeNull();
    expect(screen.queryByRole("heading", { name: "Worktrees" })).toBeNull();

    expect(
      mockTunnelTransport.fetchLike.mock.calls.some(([path]) =>
        path.includes("/api/v1/workspace/select"),
      ),
    ).toBe(false);
    // The header's status cluster, which owns the mode switch, is only rendered once a worktree
    // is chosen (account preselection renders the collapsed picker alone). Asking for the
    // terminal before that is a silent no-op - the switch does not exist yet - so the request is
    // made below, after the selection has landed.
    act(() => {
      fireEvent.click(topContextTrigger);
    });
    expect(topContextTrigger.getAttribute("aria-expanded")).toBe("true");

    const optionBtn = await waitFor(() =>
      screen.getByRole("button", { name: /main/i })
    );

    act(() => {
      fireEvent.click(optionBtn);
    });

    await waitFor(() => {
      expect(mockTunnelTransport.fetchLike).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          method: "POST",
          body: expect.stringContaining('"worktreeSlug":"main"'),
        }),
      );
    });

    // The selection has landed, so the switch exists now: wait for it rather than assume it, then
    // ask for the terminal. A switch that never appears fails here instead of silently leaving the
    // chat surface up and reporting only a missing grid.
    await waitFor(() => {
      expect(screen.getByTestId("remote-view-mode-terminal")).toBeDefined();
    });
    await switchToTerminalMode();

    try {
      await waitFor(() => {
        expect(screen.getByTestId("remote-terminal-grid")).toBeDefined();
      });
    } catch (error) {
      emitFailureTrace(
        "accountSession: grid after worktree selection",
        globalThis.fetch,
        mockTunnelTransport.fetchLike,
      );
      throw error;
    }
  });

  it("does not prematurely mount old terminal when chosen target is wsB while host state remains wsA", async () => {
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

    let currentHostWorkspace = "wsA";
    let currentHostSlug = "main";
    let currentHostSessionId = "sess-old-wsA";

    const selectPostDeferred = deferred<Response>();

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

        if (path.startsWith("/api/v1/workspace/select")) {
          return selectPostDeferred.promise.then(async (res) => ({
            status: res.status,
            headers: { "content-type": "application/json" },
            body: new TextEncoder().encode(await res.text()),
          }));
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
    await switchToTerminalMode();

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
      expect(mockTunnelTransport.fetchLike).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.anything(),
      );
    });

    expect(screen.queryByTestId("remote-terminal-grid")).toBeNull();

    await switchToTerminalMode();

    currentHostWorkspace = "wsB";
    currentHostSlug = "feature-b";
    currentHostSessionId = "sess-new-wsB";

    await act(async () => {
      selectPostDeferred.resolve(new Response(JSON.stringify({ ok: true }), { status: 200 }));
    });

    act(() => {
      mockEventWs.onmessage?.({
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
    });
  });

  it("confirms the selection when the desktop event lands while the post-select state read is in flight", async () => {
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
        return Promise.resolve(new Response(JSON.stringify({ sessionId: "sess-alloc-test-42" }), { status: 200 }));
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });

    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "phone-initiator-pub-key-base64",
      privateKey: "phone-initiator-priv-key-base64",
    });

    const socket = () => ({
      readyState: 1,
      send: vi.fn(),
      close: vi.fn(),
      onopen: null as any,
      onclose: null as any,
      onmessage: null as any,
      onerror: null as any,
    });
    const mockTerminalWs = socket();
    const mockEventWs = socket();

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
    const ok = (body: unknown) => ({
      status: 200,
      headers: { "content-type": "application/json" },
      body: new TextEncoder().encode(JSON.stringify(body)),
    });

    const mockTunnelTransport = {
      fetchLike: vi.fn().mockImplementation(async (path: string) => {
        if (path.startsWith("/api/v1/pair/exchange")) {
          return ok({
            token: "tunnel-redeemed-device-bearer",
            device: { id: "dev-phone-1", name: "Phone" },
            machineId: "mach-phone-1",
            displayName: "Work MacBook Pro",
          });
        }
        if (path.startsWith("/api/v1/workspace/select")) {
          await selectRelease.promise;
          hostWorkspace = "wsB";
          hostSlug = "feature-b";
          hostSessionId = "sess-new-wsB";
          selectAccepted = true;
          return ok({ ok: true });
        }
        if (path.startsWith("/api/v1/workspace/state")) {
          const body = stateBody();
          if (selectAccepted && ++stateReadsAfterSelect === 1) {
            heldStateReadStarted?.();
            await heldStateRead.promise;
          }
          return ok(body);
        }
        return { status: 404, headers: {}, body: new Uint8Array(0) };
      }),
      openWebSocket: vi.fn().mockImplementation((path: string) =>
        Promise.resolve(path.startsWith("/api/v1/terminal/") ? mockTerminalWs : mockEventWs)),
      close: vi.fn(),
    };

    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
      transport: mockTunnelTransport as any,
      close: vi.fn(),
    });

    render(<RemoteApp />);
    await switchToTerminalMode();

    const trigger = await waitFor(() => screen.getByRole("button", { name: /Change workspace context/i }));
    act(() => {
      fireEvent.click(trigger);
    });
    fireEvent.click(await waitFor(() => screen.getByRole("button", { name: /feature-b/i })));

    await waitFor(() => {
      expect(mockEventWs.onmessage).toBeTypeOf("function");
      expect(mockTunnelTransport.fetchLike.mock.calls.some(([path]) => path.startsWith("/api/v1/workspace/select"))).toBe(true);
    });
    await act(async () => {
      selectRelease.resolve();
    });
    await heldStateReadIssued;
    act(() => {
      mockEventWs.onmessage?.({
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: { workspaceId: "wsB", worktreeSlug: "feature-b" },
        }),
      } as MessageEvent);
    });
    await act(async () => {
      heldStateRead.resolve();
    });

    await switchToTerminalMode();

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
    await switchToTerminalMode();

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
      expect(alert.textContent).toMatch(/Selection failed \(500\)|Selection request failed/i);
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
    await switchToTerminalMode();

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
});
