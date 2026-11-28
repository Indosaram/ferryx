// The Remote Desktop Activity mode end to end: the drawer's machine list drives the global
// canvas, and the canvas is mounted once so its panes (and the native sessions behind them)
// survive every Activity Bar mode switch.
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState, useSyncExternalStore } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, channels } = vi.hoisted(() => ({
  invoke: vi.fn(),
  channels: [] as Array<{ onmessage: (payload: unknown) => void }>,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  isTauri: () => true,
  Channel: class {
    onmessage: (payload: unknown) => void;
    constructor(handler: (payload: unknown) => void) {
      this.onmessage = handler;
      channels.push(this);
    }
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));

import { DesktopActivityBar } from "../activity/DesktopActivityBar";
import { __resetRdSupportedForTests } from "../../lib/remoteDesktop";
import { loadSidebarWidthForMode, type ActivityMode } from "../../lib/sidebarPersistence";
import { sidebarWidthStorageKey } from "../../lib/storageKeys";
import { remoteDesktopStore } from "../../state/remoteDesktopStore";
import { remoteHostStore, type HostEndpoint } from "../../state/remoteHostStore";
import { GlobalRemoteDesktopCanvas, liveRemoteDesktopSessionCount } from "./GlobalRemoteDesktopCanvas";
import { RemoteDesktopActivitySidebar, remoteDesktopConnectBlock } from "./RemoteDesktopActivitySidebar";

function statusFrame(state: string, reason: string | null = null): Uint8Array {
  const body = new TextEncoder().encode(JSON.stringify({ state, reason }));
  const bytes = new Uint8Array(1 + body.length);
  bytes[0] = 0x02;
  bytes.set(body, 1);
  return bytes;
}

function host(overrides: Partial<HostEndpoint> & { hostId: string; name: string }): HostEndpoint {
  return {
    address: "https://relay.test",
    transport: "relay",
    authStatus: "paired",
    online: true,
    machineId: overrides.hostId,
    displayName: overrides.name,
    grantScope: "machine",
    generation: "1",
    ...overrides,
  };
}

/** The shell shape this mode must survive: rail + drawer unmount on switch, canvas never does. */
function Harness({ initialMode = "remoteDesktop" }: { readonly initialMode?: ActivityMode }) {
  const [mode, setMode] = useState<ActivityMode>(initialMode);
  const [activeHostId, setActiveHostId] = useState<string | null>(null);
  const sessionCount = useSyncExternalStore(
    remoteDesktopStore.subscribe,
    () => liveRemoteDesktopSessionCount(remoteDesktopStore.getState()),
    () => 0,
  );
  return (
    <div>
      <DesktopActivityBar
        mode={mode}
        onModeChange={setMode}
        remoteDesktopBadge={sessionCount > 0 ? sessionCount : null}
      />
      {mode === "remoteDesktop" ? (
        <RemoteDesktopActivitySidebar activeHostId={activeHostId} onSelectHost={setActiveHostId} />
      ) : null}
      <GlobalRemoteDesktopCanvas
        active={mode === "remoteDesktop"}
        activeHostId={activeHostId}
        onSelectHost={setActiveHostId}
      />
      <output data-testid="mode">{mode}</output>
    </div>
  );
}

function connectCalls(): unknown[][] {
  return invoke.mock.calls.filter(([command]) => command === "cmd_rd_connect");
}

/**
 * `remoteHostStore` is a generation-guarded singleton: it drops a host whose re-published
 * generation is not newer than the one it remembers, and a forgotten host for good. Tests
 * therefore publish strictly increasing generations instead of resetting the inventory.
 */
let generation = 0;

function seedHosts(hosts: HostEndpoint[]): void {
  generation += 1;
  remoteHostStore.setState((state) => ({
    ...state,
    hosts: Object.fromEntries(hosts.map((entry) => [entry.hostId, { ...entry, generation: String(generation) }])),
    nativeStatus: "ready",
    machineFeaturesEnabled: true,
  }));
}

beforeEach(() => {
  channels.length = 0;
  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    if (command === "cmd_rd_connect") return { rdSessionId: `rd-${String(args?.hostId)}` };
    if (command === "cmd_rd_supported") return true;
    if (command === "cmd_rd_host_state") {
      return { enabled: false, running: false, allowInput: false, allowClipboard: false, sessions: [], pending: [], error: null };
    }
    return undefined;
  });
  seedHosts([
    host({ hostId: "host-1", name: "Studio" }),
    host({ hostId: "host-2", name: "Laptop" }),
  ]);
});

afterEach(() => {
  cleanup();
  remoteDesktopStore.reset();
  invoke.mockReset();
  channels.length = 0;
  __resetRdSupportedForTests();
  localStorage.clear();
});

describe("Remote Desktop Activity mode", () => {
  it("keeps the live session across remoteDesktop -> worktrees -> remoteDesktop", async () => {
    render(<Harness />);

    fireEvent.click(await screen.findByTestId("rd-connect-host-1"));
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-host-1"));
    act(() => channels[0].onmessage(statusFrame("connected")));
    await waitFor(() => expect(screen.getByTestId("rd-tab-host-1")).toHaveAttribute("data-rd-tab-status", "connected"));
    const sessionId = remoteDesktopStore.getSession("host-1")?.rdSessionId;
    expect(sessionId).toBe("rd-host-1");

    // Away to Worktrees: the drawer unmounts, the canvas only hides.
    fireEvent.click(screen.getByRole("tab", { name: "Worktrees" }));
    expect(screen.getByTestId("mode")).toHaveTextContent("worktrees");
    expect(screen.queryByTestId("activity-drawer-remote-desktop")).toBeNull();
    expect(screen.getByTestId("activity-canvas-remote-desktop")).toHaveAttribute("data-active", "false");
    expect(screen.getByTestId("activity-canvas-remote-desktop")).toHaveAttribute("hidden");

    fireEvent.click(screen.getByRole("tab", { name: "Remote Desktop" }));
    expect(screen.getByTestId("mode")).toHaveTextContent("remoteDesktop");
    await waitFor(() => expect(screen.getByTestId("rd-tab-host-1")).toHaveAttribute("data-rd-tab-status", "connected"));

    // Same native session, one dial, no teardown: GAP-7.
    expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe(sessionId);
    expect(connectCalls()).toHaveLength(1);
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_disconnect", expect.anything());
  });

  it("runs two hosts concurrently, switches tabs and disconnects only the closed one", async () => {
    render(<Harness />);

    fireEvent.click(await screen.findByTestId("rd-connect-host-1"));
    fireEvent.click(screen.getByTestId("rd-connect-host-2"));
    await waitFor(() => expect(remoteDesktopStore.getSession("host-2")?.rdSessionId).toBe("rd-host-2"));
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-host-1"));
    act(() => channels[0].onmessage(statusFrame("connected")));
    act(() => channels[1].onmessage(statusFrame("connected")));

    const rdTabs = within(await screen.findByRole("tablist", { name: "Remote desktop hosts" })).getAllByRole("tab");
    expect(rdTabs.map((tab) => tab.textContent?.trim())).toEqual(["Computers", "Studio", "Laptop"]);
    // The newest connection is focused; both panes stay mounted.
    expect(screen.getByTestId("rd-tab-host-2")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("rd-tab-host-1")).toHaveAttribute("aria-selected", "false");
    expect(screen.getByTestId("rd-view-host-1")).toHaveAttribute("hidden");
    expect(screen.getByTestId("rd-view-host-2")).not.toHaveAttribute("hidden");
    expect(connectCalls()).toHaveLength(2);
    // The rail badge counts the live sessions, and drops back as they close.
    expect(screen.getByTestId("activity-mode-remoteDesktop-badge")).toHaveTextContent("2");

    fireEvent.click(screen.getByTestId("rd-tab-host-1"));
    await waitFor(() => expect(screen.getByTestId("rd-tab-host-1")).toHaveAttribute("aria-selected", "true"));
    expect(screen.getByTestId("rd-view-host-2")).toHaveAttribute("hidden");

    fireEvent.click(screen.getByTestId("rd-tab-close-host-1"));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_disconnect", { rdSessionId: "rd-host-1" }));
    expect(remoteDesktopStore.getSession("host-1")).toMatchObject({ status: "idle", rdSessionId: null });
    // The other host keeps running.
    expect(remoteDesktopStore.getSession("host-2")).toMatchObject({ status: "connected", rdSessionId: "rd-host-2" });
    expect(screen.getByTestId("activity-mode-remoteDesktop-badge")).toHaveTextContent("1");
  });

  it("disables Connect for an offline or ungranted machine and says why", async () => {
    seedHosts([
      host({ hostId: "host-1", name: "Studio" }),
      host({ hostId: "host-2", name: "Laptop", online: false }),
      host({ hostId: "host-3", name: "Old box", authStatus: "revoked" }),
    ]);
    render(<Harness />);

    const offline = await screen.findByTestId("rd-connect-host-2");
    expect(offline).toBeDisabled();
    expect(screen.getByTestId("rd-host-status-host-2")).toHaveTextContent("Offline");
    expect(screen.getByTestId("rd-connect-reason-host-2")).toHaveTextContent("offline or unreachable");
    expect(screen.getByTestId("rd-connect-reason-host-2")).toHaveAttribute("data-reason-code", "RD_HOST_OFFLINE");
    expect(offline).toHaveAttribute("aria-describedby", "rd-connect-reason-host-2");

    const revoked = screen.getByTestId("rd-connect-host-3");
    expect(revoked).toBeDisabled();
    expect(screen.getByTestId("rd-connect-reason-host-3")).toHaveAttribute("data-reason-code", "RD_PERMISSION_REQUIRED");
    expect(screen.getByTestId("rd-host-grant-host-3")).toHaveAttribute("data-granted", "false");

    expect(screen.getByTestId("rd-connect-host-1")).toBeEnabled();
    expect(connectCalls()).toHaveLength(0);
  });

  it("surfaces a structured connect failure by code instead of message text", async () => {
    invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === "cmd_rd_connect") {
        if (args?.hostId === "host-2") throw { code: "RD_HOST_DISABLED", message: "request timeout after 30000ms", details: {} };
        return { rdSessionId: `rd-${String(args?.hostId)}` };
      }
      if (command === "cmd_rd_supported") return true;
      if (command === "cmd_rd_host_state") {
        return { enabled: false, running: false, allowInput: false, allowClipboard: false, sessions: [], pending: [], error: null };
      }
      return undefined;
    });
    render(<Harness />);

    fireEvent.click(await screen.findByTestId("rd-connect-host-2"));

    const alert = await screen.findByTestId("rd-session-error-host-2");
    expect(alert).toHaveAttribute("data-error-code", "RD_HOST_DISABLED");
    expect(alert).toHaveTextContent("Remote desktop hosting is turned off on that machine.");
    expect(alert).not.toHaveTextContent("request timeout");
    // A failed attempt retains its main-area recovery UI, but is not a live session.
    expect(screen.getByTestId("rd-tab-host-2")).toHaveAttribute("data-rd-tab-status", "error");
    expect(screen.getByTestId("remote-desktop-status")).toHaveAttribute("role", "alert");
    expect(screen.queryByTestId("activity-mode-remoteDesktop-badge")).toBeNull();
    expect(screen.getByTestId("rd-connect-host-2")).toHaveTextContent("Reconnect");
  });

  it("renders the collapsible host-sharing section and persists the drawer width", async () => {
    render(<Harness />);
    await screen.findByTestId("rd-connect-host-1");

    expect(screen.queryByTestId("rd-host-sharing-panel")).toBeNull();
    fireEvent.click(screen.getByTestId("rd-host-sharing-toggle"));
    expect(await screen.findByTestId("rd-host-sharing-panel")).toBeInTheDocument();
    expect(screen.getByTestId("remote-desktop-host-section")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Allow remote desktop" })).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("rd-host-sharing-toggle"));
    expect(screen.queryByTestId("rd-host-sharing-panel")).toBeNull();

    // Resizing stores this mode's own width, not a shared one.
    const separator = screen.getByRole("separator", { name: "Resize remote desktop machines" });
    expect(loadSidebarWidthForMode("remoteDesktop")).toBe(280);
    fireEvent.keyDown(separator, { key: "ArrowRight" });
    expect(loadSidebarWidthForMode("remoteDesktop")).toBe(296);
    expect(localStorage.getItem(sidebarWidthStorageKey("remoteDesktop"))).toBe("296");

    const drawer = screen.getByTestId("activity-drawer-remote-desktop");
    expect(within(drawer).getByRole("list", { name: "Paired machines" })).toBeInTheDocument();
  });

  it("offers the computer library when no machine is connected and filters the available inventory", () => {
    seedHosts([host({ hostId: "host-1", name: "Studio" }), host({ hostId: "host-2", name: "Laptop", online: false })]);
    render(<Harness />);
    const library = within(screen.getByTestId("remote-desktop-library"));
    expect(library.getByRole("region", { name: "Available computers" })).toBeInTheDocument();
    expect(library.getByRole("region", { name: "Offline computers" })).toBeInTheDocument();
    fireEvent.click(library.getByRole("button", { name: "Available only" }));
    expect(library.queryByRole("region", { name: "Offline computers" })).toBeNull();
    fireEvent.change(library.getByRole("searchbox", { name: "Search computers" }), { target: { value: "missing" } });
    expect(library.getByTestId("remote-desktop-empty")).toBeInTheDocument();
    expect(connectCalls()).toHaveLength(0);
  });

  it("keeps a session mounted while visiting Computers, then restores its tab", async () => {
    render(<Harness />);
    fireEvent.click(screen.getByTestId("rd-connect-host-1"));
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-host-1"));
    act(() => channels[0].onmessage(statusFrame("connected")));
    fireEvent.click(screen.getByRole("tab", { name: "Computers" }));
    expect(screen.getByTestId("rd-view-host-1")).toHaveAttribute("hidden");
    expect(screen.getByTestId("remote-desktop-library")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("rd-tab-host-1"));
    expect(screen.getByTestId("rd-view-host-1")).not.toHaveAttribute("hidden");
    expect(connectCalls()).toHaveLength(1);
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_disconnect", expect.anything());
  });

  it("retains host-ended sessions for recovery without dialing again on mount", async () => {
    await remoteDesktopStore.connect("host-1", "Studio");
    act(() => channels[0].onmessage(statusFrame("ended", "Host disconnected")));
    render(<Harness />);
    fireEvent.click(screen.getByTestId("rd-tab-host-1"));
    expect(screen.getByTestId("remote-desktop-status")).toHaveTextContent("Host disconnected");
    expect(connectCalls()).toHaveLength(1);
    expect(liveRemoteDesktopSessionCount(remoteDesktopStore.getState())).toBe(0);
  });

  it("shows pairing guidance for an empty inventory", () => {
    seedHosts([]);
    render(<Harness />);
    expect(screen.getByTestId("remote-desktop-empty")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Computers" })).toHaveAttribute("aria-selected", "true");
  });

  it("treats an unpaired or ungranted machine as needing access", () => {
    expect(remoteDesktopConnectBlock({ authStatus: "paired", grantScope: "machine", online: true })).toBeNull();
    expect(remoteDesktopConnectBlock({ authStatus: "needsMachineGrant", grantScope: "mirror", online: true })?.code)
      .toBe("RD_PERMISSION_REQUIRED");
    expect(remoteDesktopConnectBlock({ authStatus: "paired", grantScope: "machine", online: false })?.code)
      .toBe("RD_HOST_OFFLINE");
  });
});
