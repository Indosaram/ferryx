import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import React, { useState, useEffect } from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";
import { storeAccountSessionToken, clearStoredAccountSessionToken, storeAccountOrigin, clearStoredAccountOrigin } from "./accountSession";
import { remoteHostStore } from "../state/remoteHostStore";
import * as attachTunnelModule from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
import type { TunnelTransport, TunnelWebSocket, TunnelResponse, FetchLikeInit } from "./attachTunnel";

const { capturedProps, socketOpenCalls, socketErrors, deferredSignal } = vi.hoisted(() => {
  let resolveDeferred: (() => void) | null = null;
  const deferredPromise = new Promise<void>((resolve) => {
    resolveDeferred = resolve;
  });

  return {
    capturedProps: [] as Array<{
      sessionId?: string;
      createWebSocket?: (path: string) => Promise<any>;
    }>,
    socketOpenCalls: [] as string[],
    socketErrors: [] as unknown[],
    deferredSignal: {
      promise: deferredPromise,
      resolve: resolveDeferred,
      reset: () => {
        deferredSignal.promise = new Promise<void>((resolve) => {
          deferredSignal.resolve = resolve;
        });
      },
    },
  };
});

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: (props: { sessionId?: string; createWebSocket?: (path: string) => Promise<any> }) => {
    capturedProps.push({
      sessionId: props.sessionId,
      createWebSocket: props.createWebSocket,
    });

    useEffect(() => {
      if (props.createWebSocket) {
        socketOpenCalls.push(props.sessionId || "default");
        props.createWebSocket("/api/v1/terminal/test")
          .then(() => {
            deferredSignal.resolve?.();
          })
          .catch((err) => {
            socketErrors.push(err);
            deferredSignal.resolve?.();
          });
      }
    }, [props.createWebSocket]);

    return <div data-testid="mock-remote-terminal">{props.sessionId}</div>;
  },
}));

function machineFixture(origin: string) {
  return {
    machineRecordId: "rec-mach-1",
    machineId: "mach-1",
    displayName: "Machine 1",
    publicKey: "mach-pub-1",
    attachPublicKey: "mach-attach-pub-1",
    relayOrigin: origin,
    platform: "darwin",
    online: true,
    enrollmentEpoch: "1",
    lastSeenAt: Date.now(),
  };
}

const workspaceState = {
  projects: [
    {
      workspaceId: "ws-1",
      repoRoot: "/Users/dev/ferryx",
      worktrees: [{ slug: "main", label: "main" }],
    },
  ],
  activeContext: {
    workspaceId: "ws-1",
    worktreeSlug: "main",
    worktreeLabel: "main",
    sessionId: "term-1",
    activeTerminal: { sessionId: "term-1", title: "zsh", running: true },
  },
  sessions: [
    {
      sessionId: "term-1",
      title: "zsh",
      workspaceId: "ws-1",
      worktree: { wsId: "ws-1", slug: "main" },
      target: { machineId: "mach-1", sessionId: "term-1", daemonEpoch: "1790742255752" },
      running: true,
    },
  ],
};

function createFakeSocket(): TunnelWebSocket {
  return {
    readyState: 1,
    binaryType: "arraybuffer",
    onopen: null,
    onclose: null,
    onmessage: null,
    onerror: null,
    send: vi.fn(),
    close: vi.fn(),
  };
}

describe("RemoteApp createWebSocket factory stability regression", () => {
  beforeEach(() => {
    capturedProps.length = 0;
    socketOpenCalls.length = 0;
    socketErrors.length = 0;
    deferredSignal.reset();
    clearStoredAccountSessionToken();
    clearStoredAccountOrigin();
    remoteHostStore.reset();

    window.history.replaceState(null, "", "/");
    const origin = window.location.origin;
    storeAccountOrigin(origin);
    storeAccountSessionToken("session-token-1", origin);

    const mockFetch = vi.fn().mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return new Response(JSON.stringify([machineFixture(origin)]), { status: 200 });
      }
      if (url.includes("/grants")) {
        return new Response(
          JSON.stringify({
            grantId: "grant-mach-1",
            machineId: "mach-1",
            relayOrigin: origin,
            pairingToken: "pair-token-1",
            machineAttachPublicKey: "attach-key-1",
            grantScope: "machine",
            expiresAt: Date.now() + 600000,
          }),
          { status: 200 },
        );
      }
      if (url.endsWith("/api/v1/attach/session")) {
        return new Response(JSON.stringify({ sessionId: "sess-alloc-1" }), { status: 200 });
      }
      if (url.endsWith("/api/v1/health") || url.endsWith("/api/account/v1/health")) {
        return new Response(JSON.stringify({ status: "ok" }), { status: 200 });
      }
      return new Response("Not Found", { status: 404 });
    });

    vi.stubGlobal("fetch", mockFetch);

    const transport: TunnelTransport = {
      fetchLike: vi.fn(async (path: string, _init?: FetchLikeInit): Promise<TunnelResponse> => {
        if (path.startsWith("/api/v1/pair/exchange")) {
          return {
            status: 200,
            headers: { "Content-Type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({
                token: "device-token-1",
                device: { id: "dev-1", name: "Phone" },
                machineId: "mach-1",
                displayName: "Machine 1",
              }),
            ),
          };
        }
        if (path.startsWith("/api/v1/workspace/state")) {
          return {
            status: 200,
            headers: { "Content-Type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify(workspaceState)),
          };
        }
        if (path.startsWith("/api/v1/sessions")) {
          return {
            status: 200,
            headers: { "Content-Type": "application/json" },
            body: new TextEncoder().encode(
              JSON.stringify({
                revision: "1",
                completeness: "complete",
                sessions: workspaceState.sessions,
              }),
            ),
          };
        }
        if (path.startsWith("/api/v1/workspace/select")) {
          return {
            status: 200,
            headers: { "Content-Type": "application/json" },
            body: new TextEncoder().encode(JSON.stringify({ ok: true })),
          };
        }
        return { status: 404, headers: {}, body: new Uint8Array(0) };
      }),
      openWebSocket: vi.fn(async (_path: string): Promise<TunnelWebSocket> => {
        return createFakeSocket();
      }),
      close: vi.fn(),
    };

    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockResolvedValue({
      transport,
      close: vi.fn(),
    });

    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "phone-pub-base64",
      privateKey: "phone-priv-base64",
    });
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    clearStoredAccountSessionToken();
    clearStoredAccountOrigin();
    remoteHostStore.reset();
    vi.restoreAllMocks();
  });

  it("preserves createWebSocket factory identity across unrelated parent re-renders and avoids socket reconnection", async () => {
    let triggerParentRerender: (() => void) | null = null;

    const TestWrapper: React.FC = () => {
      const [, setTick] = useState(0);
      triggerParentRerender = () => setTick((t) => t + 1);

      return (
        <div data-testid="wrapper">
          <RemoteApp />
        </div>
      );
    };

    render(<TestWrapper />);

    const contextTrigger = await screen.findByRole("button", { name: /Change workspace context/i });
    act(() => {
      contextTrigger.click();
    });

    const worktreeOption = await screen.findByRole("button", { name: /main/i });
    act(() => {
      worktreeOption.click();
    });

    await screen.findByTestId("mock-remote-terminal");

    let timer: ReturnType<typeof setTimeout> | null = null;
    await Promise.race([
      deferredSignal.promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error("Timeout awaiting terminal socket creation")), 3000);
      }),
    ]).finally(() => {
      if (timer !== null) clearTimeout(timer);
    });

    expect(socketErrors).toEqual([]);
    expect(capturedProps.length).toBeGreaterThanOrEqual(1);
    const initialProps = capturedProps[capturedProps.length - 1];
    expect(typeof initialProps.createWebSocket).toBe("function");

    const initialSocketOpens = socketOpenCalls.length;
    expect(initialSocketOpens).toBe(1);

    const countBeforeRerender = capturedProps.length;

    act(() => {
      triggerParentRerender!();
    });

    expect(capturedProps.length).toBeGreaterThan(countBeforeRerender);
    const rerenderedProps = capturedProps[capturedProps.length - 1];

    expect(rerenderedProps.createWebSocket).toBe(initialProps.createWebSocket);
    expect(socketOpenCalls.length).toBe(initialSocketOpens);
  });
});
