import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteApp } from "./RemoteApp";
import { openWorkspacePicker } from "./home/phoneHomeTestUtils";
import { requireSidebarRow } from "./remoteSidebarTestUtils";
import { clearStoredAccountSessionToken, storeAccountSessionToken } from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
import { remoteHostStore } from "../state/remoteHostStore";

const MACHINE = "2773ab38-d556-4a81-ae49-18a3b0fb83af";
const WS = "project-2a54aface19a497491abc9ed791fd65d";

class MockSocket {
  static readonly OPEN = 1;
  readyState = 1;
  binaryType = "arraybuffer";
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(public url: string) {}
  send() {}
  close() {
    this.readyState = 3;
    this.onclose?.();
  }
}

function body(value: unknown) {
  return { status: 200, headers: {}, body: new TextEncoder().encode(JSON.stringify(value)) };
}

type FakeTunnel = {
  isClosed: boolean;
  fetchLike: ReturnType<typeof vi.fn>;
  openWebSocket: ReturnType<typeof vi.fn>;
  close: ReturnType<typeof vi.fn>;
};

function makeTunnel(): FakeTunnel {
  const tunnel: FakeTunnel = {
    isClosed: false,
    fetchLike: vi.fn(),
    openWebSocket: vi.fn(),
    close: vi.fn(() => {
      tunnel.isClosed = true;
    }),
  };
  tunnel.fetchLike.mockImplementation((path: string) => {
    if (tunnel.isClosed) return Promise.reject(new Error("STREAM_CLOSED: transport has been closed"));
    if (path === "/api/v1/pair/exchange") return Promise.resolve(body({ token: "device-token-123", displayName: "omarchy" }));
    if (path === "/api/v1/workspace/projects") {
      return Promise.resolve(body({ revision: 2, completeness: "complete", projects: [{ workspaceId: WS, repoRoot: "/srv/repo", availability: "ready", revision: 1 }], unavailableWorkspaceIds: [] }));
    }
    if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
      return Promise.resolve(body({ revision: 1, worktrees: [{ workspaceId: WS, identity: null, path: "/srv/repo", head: "abc1234", branch: "refs/heads/main", bare: false, detached: false, locked: null, prunable: null, managed: false }] }));
    }
    if (path === "/api/v1/sessions") {
      return Promise.resolve(body({ revision: "1", completeness: "complete", sessions: [{ workspaceId: WS, worktree: null, target: { machineId: MACHINE, sessionId: "machine-sess-1", daemonEpoch: "1" }, running: true, title: "main" }] }));
    }
    return Promise.resolve({ status: 404, headers: {}, body: new Uint8Array(0) });
  });
  tunnel.openWebSocket.mockImplementation((path: string) =>
    tunnel.isClosed ? Promise.reject(new Error("STREAM_CLOSED: transport has been closed")) : Promise.resolve(new MockSocket(path)),
  );
  return tunnel;
}

describe("RemoteApp account tunnel resume", () => {
  beforeEach(() => {
    vi.stubGlobal("WebSocket", MockSocket);
    clearStoredAccountSessionToken();
    localStorage.clear();
    remoteHostStore.reset();
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    clearStoredAccountSessionToken();
    localStorage.clear();
    remoteHostStore.reset();
  });

  it("replaces a tunnel the relay dropped while the phone was in the background as soon as it is visible again", async () => {
    const tunnels: FakeTunnel[] = [];
    storeAccountSessionToken("test-account-session-token-xyz", window.location.origin);
    globalThis.fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.endsWith("/api/account/v1/machines")) {
        return Promise.resolve(new Response(JSON.stringify([{ machineId: MACHINE, machineRecordId: "rec-1", publicKey: "pub-1", attachPublicKey: "attach-pub", relayOrigin: "https://relay.example.com", displayName: "omarchy", platform: "linux", online: true, enrollmentEpoch: "1", lastSeenAt: Date.now() }]), { status: 200 }));
      }
      if (url.includes("/grants")) {
        return Promise.resolve(new Response(JSON.stringify({ grantId: "grant-1", pairingToken: "grant-pairing-token", relayOrigin: "https://relay.example.com", machineAttachPublicKey: "attach-pub" }), { status: 200 }));
      }
      if (url.endsWith("/api/v1/attach/session")) {
        return Promise.resolve(new Response(JSON.stringify({ sessionId: `sess-alloc-${tunnels.length}` }), { status: 200 }));
      }
      return Promise.resolve(new Response("Not Found", { status: 404 }));
    });
    vi.spyOn(attachTunnelModule, "openAccountTunnel").mockImplementation(async () => {
      const tunnel = makeTunnel();
      tunnels.push(tunnel);
      return { transport: tunnel as never, close: tunnel.close };
    });
    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "phone-initiator-pub-key-base64",
      privateKey: "phone-initiator-priv-key-base64",
    });

    // Given a worktree open on a live tunnel
    render(<RemoteApp />);
    const picker = await openWorkspacePicker();
    const row = await waitFor(() => requireSidebarRow(WS, null, picker));
    await act(async () => { fireEvent.click(row); });
    await waitFor(() => expect(screen.getByTestId("remote-view-mode-terminal")).toBeInTheDocument());

    // When the relay drops every tunnel while hidden and the page becomes visible again
    for (const tunnel of tunnels) tunnel.isClosed = true;
    const openedBefore = tunnels.length;
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });

    // Then a fresh tunnel is acquired without any user action, and no raw transport error shows
    await waitFor(() => expect(tunnels.length).toBeGreaterThan(openedBefore));
    await waitFor(() => {
      const fresh = tunnels.slice(openedBefore);
      expect(fresh.some((tunnel) => !tunnel.isClosed)).toBe(true);
    });
    expect(screen.queryByText(/STREAM_CLOSED/)).toBeNull();
  });
});
