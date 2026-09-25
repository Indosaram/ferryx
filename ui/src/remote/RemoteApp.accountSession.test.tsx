import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import { RemoteApp } from "./RemoteApp";
import { storeAccountSessionToken, clearStoredAccountSessionToken } from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
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
    remoteHostStore.setActiveHost(null);
  });

  afterEach(() => {
    cleanup();
    MockTestWebSocket.instances = [];
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    clearStoredAccountSessionToken();
    localStorage.clear();
  });

  it("renders account machine list when signed in, then opens terminal pane on connect without Add Project or directory picker", async () => {
    storeAccountSessionToken("test-account-session-token-xyz");

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

    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL, init?: RequestInit) => {
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
              grantScope: "mirror",
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

        if (path.startsWith("/api/v1/workspace/state")) {
          const state = {
            projects: [],
            activeContext: {
              workspaceId: "ws-ferryx",
              sessionId: "term-sess-77",
            },
            sessions: [
              {
                sessionId: "term-sess-77",
                running: true,
                title: "zsh",
                workspaceId: "ws-ferryx",
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

    await vi.waitFor(() => {
      expect(screen.getByText("Account Machines")).toBeDefined();
      expect(screen.getByText("Work MacBook Pro")).toBeDefined();
    });

    expect(screen.queryByText(/Add Project/i)).toBeNull();
    expect(screen.queryByLabelText(/Add Project/i)).toBeNull();
    expect(screen.queryByTestId("directory-picker")).toBeNull();
    expect(screen.queryByText(/select directory|choose directory|browse directory/i)).toBeNull();

    const connectBtn = screen.getByTestId("connect-machine-mach-phone-1");
    act(() => {
      fireEvent.click(connectBtn);
    });

    await vi.waitFor(() => {
      expect(screen.getByTestId("remote-terminal-grid")).toBeDefined();
    });

    expect(screen.queryByText(/Add Project/i)).toBeNull();
    expect(screen.queryByLabelText(/Add Project/i)).toBeNull();
    expect(screen.queryByTestId("directory-picker")).toBeNull();
    expect(screen.queryByText(/select directory|choose directory|browse directory/i)).toBeNull();
  });
});
