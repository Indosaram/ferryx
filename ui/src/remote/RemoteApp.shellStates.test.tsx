import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RemoteApp } from "./RemoteApp";
import { openWorkspacePicker } from "./home/phoneHomeTestUtils";
import { requireSidebarRow } from "./remoteSidebarTestUtils";
import * as accountSessionModule from "./accountSession";
import { clearStoredAccountSessionToken, storeAccountSessionToken } from "./accountSession";
import * as attachTunnelModule from "./attachTunnel";
import { TunnelError, type TunnelResponse, type TunnelTransport } from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
import { remoteHostStore } from "../state/remoteHostStore";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({
    sessionId,
    showTitleBar = true,
    onBack,
  }: {
    sessionId: string;
    showTitleBar?: boolean;
    onBack?: () => void;
  }) => (
    <div data-testid="remote-terminal" data-session-id={sessionId}>
      {showTitleBar ? (
        <button type="button" data-testid="remote-terminal-back" onClick={onBack}>
          Back to chat
        </button>
      ) : null}
      Mirrored terminal {sessionId}
    </div>
  ),
}));

function jsonResponse(body: unknown, ok = true): Response {
  return {
    ok,
    status: ok ? 200 : 500,
    json: vi.fn(async () => body),
    text: vi.fn(async () => JSON.stringify(body)),
  } as unknown as Response;
}

function ticketed(inner: typeof fetch): typeof fetch {
  return vi.fn<typeof fetch>(async (input, init) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.includes("/api/v1/socket-ticket")) {
      return jsonResponse({ ticket: "ui-test-ticket", expiresAt: 9999999999 });
    }
    return inner(input, init);
  }) as unknown as typeof fetch;
}

class EventWebSocket {
  static latest: EventWebSocket | null = null;
  static instances: EventWebSocket[] = [];
  readonly url: string;
  close = vi.fn();
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    EventWebSocket.latest = this;
    EventWebSocket.instances.push(this);
  }
}

const baseState = {
  activeContext: {
    workspaceId: "ferryx",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTabId: "tab-1",
    activeTerminal: { sessionId: "sess-1", title: "Shell 1", running: true },
    terminalTabs: [
      {
        id: "tab-1",
        sessionId: "sess-1",
        label: "Shell 1",
        agentType: "shell",
        activityState: "working",
        worktreeLabel: "main",
      },
      {
        id: "tab-2",
        sessionId: "sess-2",
        label: "Agent Task",
        agentType: "claude",
        activityState: "waiting",
        worktreeLabel: "main",
      },
    ],
  },
};

describe("RemoteApp shell states", () => {
  beforeEach(() => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", EventWebSocket);
  });

  afterEach(() => {
    cleanup();
    localStorage.clear();
    vi.unstubAllGlobals();
    EventWebSocket.instances = [];
  });

  it("terminal mode with no running terminal offers New terminal instead of a blank body", async () => {
    const empty = { activeContext: { ...baseState.activeContext, activeTabId: null, activeTerminal: null, terminalTabs: [] } };
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(empty));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    render(<RemoteApp />);

    await screen.findByTestId("mobile-top-bar");
    fireEvent.click(screen.getByTestId("remote-view-mode-terminal"));

    const emptyState = await screen.findByTestId("remote-terminal-empty");
    fireEvent.click(within(emptyState).getByRole("button", { name: "New terminal" }));
    await waitFor(() => {
      const select = fetchMock.mock.calls.find(([input]) => String(input).includes("/api/v1/workspace/select"));
      expect(select).toBeDefined();
      expect(JSON.parse(String(select?.[1]?.body))).toMatchObject({ workspaceId: "ferryx", createTerminal: true });
    });
  });

  it("New worktree opens an in-app sheet instead of a browser prompt and posts the typed name as a slug", async () => {
    const promptSpy = vi.fn();
    vi.stubGlobal("prompt", promptSpy);
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(baseState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    render(<RemoteApp />);

    await screen.findByTestId("mobile-top-bar");
    fireEvent.click(screen.getByRole("button", { name: "More actions" }));
    const actions = await screen.findByRole("dialog", { name: "Actions" });
    fireEvent.click(within(actions).getByTestId("overflow-action-new-worktree"));

    const sheet = await screen.findByRole("dialog", { name: "New worktree" });
    const input = within(sheet).getByLabelText("Worktree name");
    expect(input).toHaveClass("text-base");
    fireEvent.change(input, { target: { value: "Fix Login Bug" } });
    expect(within(sheet).getByText("Will be created as fix-login-bug")).toBeInTheDocument();
    fireEvent.click(within(sheet).getByRole("button", { name: "Create worktree" }));

    await waitFor(() => {
      const create = fetchMock.mock.calls.find(([input]) => String(input).includes("/api/v1/workspace/worktrees"));
      expect(create).toBeDefined();
      expect(JSON.parse(String(create?.[1]?.body)).worktree).toEqual({ wsId: "ferryx", slug: "fix-login-bug" });
    });
    expect(promptSpy).not.toHaveBeenCalled();
  });

  it("a tapped alert for a session that never appears is cleared and reported", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      window.history.replaceState(null, "", "/#session=gone-session");
      vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(baseState))));
      render(<RemoteApp />);
      await screen.findByTestId("mobile-top-bar");

      await act(async () => {
        await vi.advanceTimersByTimeAsync(10_000);
      });

      expect(await screen.findByText("That session isn't available anymore")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
      window.history.replaceState(null, "", "/");
    }
  });

  it("a paired link whose events socket stays down offers Reconnect instead of a wordless reconnecting line", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      // Given a paired (non-account) shell whose events socket has been down past the strip delay
      const fetchMock = vi.fn<typeof fetch>(async () => jsonResponse(baseState));
      vi.stubGlobal("fetch", ticketed(fetchMock));
      render(<RemoteApp />);
      await screen.findByTestId("mobile-top-bar");
      const socket = EventWebSocket.latest;
      if (!socket) throw new Error("Missing event socket");
      await act(async () => { socket.onclose?.(); });
      await act(async () => { await vi.advanceTimersByTimeAsync(3_500); });

      // Then the strip reports the stalled link with no account reason, and its one action is reconnect
      const strip = await screen.findByTestId("remote-connection-strip");
      expect(strip).toHaveAttribute("data-health", "reconnecting");
      expect(strip).not.toHaveAttribute("data-reason");
      const action = within(strip).getByTestId("remote-connection-reconnect");
      expect(action).toHaveAttribute("data-action", "reconnect");

      // When Reconnect is pressed
      const stateReads = () => fetchMock.mock.calls.filter(([input]) => String(input).includes("/api/v1/workspace/state")).length;
      const exercised = () => stateReads() + EventWebSocket.instances.length;
      const before = exercised();
      await act(async () => { fireEvent.click(action); });

      // Then the events link is exercised again instead of the action being inert: a dead socket
      // is re-dialed, a live one makes the shell re-read its state.
      await waitFor(() => expect(exercised()).toBeGreaterThan(before));
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("RemoteApp computer drawer on a relay page", () => {
  beforeEach(() => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", EventWebSocket);
  });

  afterEach(() => {
    cleanup();
    localStorage.clear();
    vi.unstubAllGlobals();
    window.history.replaceState(null, "", "/");
    EventWebSocket.instances = [];
  });

  it.each([
    { page: "a relay page", url: "/?relay=1", localOffered: false },
    { page: "a direct gateway page", url: "/", localOffered: true },
  ])("offers This computer in the drawer only when $page is not the relay", async ({ url, localOffered }) => {
    // Given a paired shell opened on the page
    window.history.replaceState(null, "", url);
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(baseState))));
    render(<RemoteApp />);
    await screen.findByTestId("mobile-top-bar");

    // When the computer drawer is opened from the overflow menu
    fireEvent.click(screen.getByRole("button", { name: "More actions" }));
    fireEvent.click(await screen.findByTestId("overflow-action-machines"));
    await screen.findByTestId("mobile-host-drawer");

    // Then the page-origin computer is listed only where the page origin is a computer
    expect(screen.queryByTestId("mobile-host-option-local") !== null).toBe(localOffered);
  });
});

const ACCOUNT_MACHINE = "mach-shell";
const ACCOUNT_WS = "ws-shell";

function tunnelAnswer(value: unknown, status = 200): TunnelResponse {
  return { status, headers: { "content-type": "application/json" }, body: new TextEncoder().encode(JSON.stringify(value)) };
}

class IdleTunnelSocket {
  readyState = 1;
  binaryType = "arraybuffer";
  onopen = null;
  onmessage = null;
  onerror = null;
  onclose = null;
  send() {}
  close() {
    this.readyState = 3;
  }
}

type ShellTunnel = TunnelTransport & { isClosed: boolean };

/** One tunnel to the account machine; once closed, every request fails like a dropped relay stream. */
function shellTunnel(): ShellTunnel {
  const tunnel = {
    isClosed: false,
    fetchLike: vi.fn(async (path: string): Promise<TunnelResponse> => {
      if (tunnel.isClosed) throw new Error("STREAM_CLOSED: cannot write to closed stream");
      if (path.startsWith("/api/v1/pair/exchange")) return tunnelAnswer({ token: "device-token", machineId: ACCOUNT_MACHINE });
      if (path === "/api/v1/workspace/projects") {
        return tunnelAnswer({
          revision: "1", completeness: "complete", unavailableWorkspaceIds: [],
          projects: [{ workspaceId: ACCOUNT_WS, repoRoot: `/srv/${ACCOUNT_WS}`, availability: "ready", revision: "1" }],
        });
      }
      if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=")) {
        return tunnelAnswer({
          revision: "1",
          worktrees: [{
            workspaceId: ACCOUNT_WS, identity: { wsId: ACCOUNT_WS, slug: "main" }, path: `/srv/${ACCOUNT_WS}`,
            head: "h1", branch: "refs/heads/main", bare: false, detached: false, locked: null, prunable: null, managed: false,
          }],
        });
      }
      if (path === "/api/v1/sessions") return tunnelAnswer({ revision: "1", completeness: "complete", sessions: [] });
      if (path.startsWith("/api/v1/workspace/state")) return tunnelAnswer({ activeContext: { workspaceId: ACCOUNT_WS, worktreeSlug: "main" } });
      return { status: 404, headers: {}, body: new Uint8Array(0) };
    }),
    openWebSocket: vi.fn(async () => new IdleTunnelSocket()),
    close: () => {
      tunnel.isClosed = true;
    },
  };
  return tunnel as unknown as ShellTunnel;
}

/** Signs in one online account machine; `openShellTunnel` decides each tunnel the relay grants. */
function mountAccountShell(openShellTunnel: () => Promise<ShellTunnel>): void {
  storeAccountSessionToken("shell-account-token", window.location.origin);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = typeof input === "string" ? input : input.toString();
    if (url.endsWith("/api/account/v1/machines")) {
      return new Response(JSON.stringify([{
        machineRecordId: "rec-shell", machineId: ACCOUNT_MACHINE, displayName: "Shell desk", publicKey: "pk",
        attachPublicKey: "apk", relayOrigin: window.location.origin, platform: "linux", online: true,
        enrollmentEpoch: "1", lastSeenAt: Date.now(),
      }]), { status: 200 });
    }
    if (url.includes("/grants")) {
      return new Response(JSON.stringify({
        grantId: "grant-shell", machineId: ACCOUNT_MACHINE, relayOrigin: window.location.origin, pairingToken: "pairing-token",
        machineAttachPublicKey: "machine-attach-key", grantScope: "machine", expiresAt: Date.now() + 600_000,
      }), { status: 200 });
    }
    if (url.endsWith("/api/v1/attach/session")) return new Response(JSON.stringify({ sessionId: "alloc" }), { status: 200 });
    return new Response("Not Found", { status: 404 });
  }));
  vi.stubGlobal("WebSocket", IdleTunnelSocket);
  vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({ publicKey: "initiator-pk", privateKey: "initiator-sk" });
  const open = async () => {
    const transport = await openShellTunnel();
    return { transport, close: transport.close };
  };
  vi.spyOn(accountSessionModule, "openTunnel").mockImplementation(open);
  vi.spyOn(attachTunnelModule, "openAccountTunnel").mockImplementation(open);
  render(<RemoteApp />);
}

function machineSection(root: HTMLElement): HTMLElement {
  const section = root.querySelector<HTMLElement>(`[data-testid="remote-sidebar-section"][data-machine-id="${ACCOUNT_MACHINE}"]`);
  if (!section) throw new Error("account machine section not rendered");
  return section;
}

describe("RemoteApp account shell states", () => {
  beforeEach(() => {
    clearStoredAccountSessionToken();
    localStorage.clear();
    sessionStorage.clear();
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

  it("a machine section whose discovery failed offers Retry, which re-probes that machine", async () => {
    // Given an account machine whose tunnels the relay refuses during discovery
    let refusing = true;
    mountAccountShell(async () => {
      if (refusing) throw new TunnelError("TRANSPORT_ERROR", "relay refused the tunnel");
      return shellTunnel();
    });
    const picker = await openWorkspacePicker({ timeout: 5000 });
    const retry = await waitFor(() => within(machineSection(picker)).getByRole("button", { name: "Retry" }), { timeout: 5000 });

    // When the relay recovers and the section's Retry is pressed
    refusing = false;
    await act(async () => { fireEvent.click(retry); });

    // Then the machine is probed again and its worktrees are listed
    await waitFor(() => requireSidebarRow(ACCOUNT_WS, "main", machineSection(picker)), { timeout: 5000 });
  }, 20000);

  it("a resume refused as UNAUTHORIZED names that reason and offers sign-in", async () => {
    // Given a worktree open on a live account tunnel
    const tunnels: ShellTunnel[] = [];
    let refusal: TunnelError | null = null;
    mountAccountShell(async () => {
      if (refusal) throw refusal;
      const tunnel = shellTunnel();
      tunnels.push(tunnel);
      return tunnel;
    });
    const picker = await openWorkspacePicker({ timeout: 5000 });
    const row = await waitFor(() => requireSidebarRow(ACCOUNT_WS, "main", picker), { timeout: 5000 });
    await act(async () => { fireEvent.click(row); });
    await waitFor(() => expect(screen.getByTestId("remote-view-mode-terminal")).toBeInTheDocument(), { timeout: 5000 });

    // When the tunnel dies and the relay refuses the replacement because the session expired
    for (const tunnel of tunnels) tunnel.isClosed = true;
    refusal = new TunnelError("UNAUTHORIZED", "account session expired");
    await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });

    // Then the strip carries the reason and its one action is sign-in
    await waitFor(() => expect(screen.getByTestId("remote-connection-strip")).toHaveAttribute("data-reason", "UNAUTHORIZED"), { timeout: 5000 });
    expect(screen.getByTestId("remote-connection-reconnect")).toHaveAttribute("data-action", "sign-in");
  }, 20000);
});
