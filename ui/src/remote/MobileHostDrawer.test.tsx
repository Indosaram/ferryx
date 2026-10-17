import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { remoteHostKey, remoteHostStore, type HostEndpoint } from "../state/remoteHostStore";
import {
  hostAgentTotals,
  MobileHostDrawer,
  type DrawerTransport,
  type HostWithAgentSummary,
} from "./MobileHostDrawer";

function makeHost(overrides: Partial<HostWithAgentSummary> = {}): HostWithAgentSummary {
  return {
    hostId: "host-1",
    name: "Studio Mac",
    address: "100.64.0.12",
    transport: "tailscale",
    authStatus: "paired",
    online: true,
    ...overrides,
  };
}

/**
 * `remoteHostStore`'s `TransportType` union has not gained `"relay"` yet, so relay hosts enter
 * the store the same way they arrive over the wire: as a transport string the drawer widens.
 */
function makeTransportHost(hostId: string, name: string, transport: DrawerTransport): HostEndpoint {
  return { ...makeHost({ hostId, name }), transport } as HostEndpoint;
}

beforeEach(() => {
  remoteHostStore.reset();
});

afterEach(() => {
  cleanup();
  remoteHostStore.reset();
});

describe("MobileHostDrawer", () => {
  it("renders nothing when closed", () => {
    render(<MobileHostDrawer open={false} onOpenChange={vi.fn()} />);
    expect(screen.queryByTestId("mobile-host-drawer")).not.toBeInTheDocument();
  });

  it("renders the host list from remoteHostStore, including transport badge and online status", () => {
    remoteHostStore.setHosts([
      makeHost({ hostId: "host-1", name: "Studio Mac", transport: "tailscale", online: true }),
      makeHost({ hostId: "host-2", name: "Laptop", transport: "mdns", online: false }),
      makeHost({ hostId: "host-3", name: "Build Box", transport: "sshTunnel", online: true }),
    ]);

    render(<MobileHostDrawer open onOpenChange={vi.fn()} />);

    expect(screen.getByTestId("mobile-host-drawer")).toBeInTheDocument();
    expect(screen.getByTestId("mobile-host-option-local")).toBeInTheDocument();
    // The build identity of the client this phone is running; "dev" outside a stamped build.
    expect(screen.getByTestId("mobile-host-drawer-build")).toBeInTheDocument();
    expect(screen.getByTestId("mobile-host-drawer-build").textContent).not.toBe("");

    const host1 = screen.getByTestId("mobile-host-option-host-1");
    expect(host1).toHaveTextContent("Studio Mac");
    expect(host1).toHaveTextContent("Tailscale");

    const host2 = screen.getByTestId("mobile-host-option-host-2");
    expect(host2).toHaveTextContent("Laptop");
    expect(host2).toHaveTextContent("mDNS");
    expect(host2.querySelector('[data-testid="mobile-host-online-indicator"]')).toHaveAttribute("data-online", "false");

    const host3 = screen.getByTestId("mobile-host-option-host-3");
    expect(host3).toHaveTextContent("Build Box");
    expect(host3).toHaveTextContent("SSH");
    expect(host3.querySelector('[data-testid="mobile-host-online-indicator"]')).toHaveAttribute("data-online", "true");
  });

  it("renders a Relay transport badge for relay-reachable hosts", () => {
    remoteHostStore.setHosts([
      makeTransportHost("host-1", "Studio Mac", "tailscale"),
      makeTransportHost("host-4", "Relay Box", "relay"),
    ]);

    render(<MobileHostDrawer open onOpenChange={vi.fn()} />);

    const relayHost = screen.getByTestId("mobile-host-option-host-4");
    const badge = relayHost.querySelector('[data-testid="mobile-host-transport-badge"]');
    expect(badge).toHaveAttribute("data-transport", "relay");
    expect(badge).toHaveTextContent("Relay");
    expect(badge?.querySelector("svg")).toBeInTheDocument();

    const directHost = screen.getByTestId("mobile-host-option-host-1");
    expect(directHost.querySelector('[data-testid="mobile-host-transport-badge"]')).toHaveAttribute(
      "data-transport",
      "tailscale",
    );
  });

  it("falls back to the relay badge for an unrecognized transport instead of crashing", () => {
    remoteHostStore.setHosts([
      makeTransportHost("host-5", "Mystery Box", "quicWarp" as DrawerTransport),
    ]);

    render(<MobileHostDrawer open onOpenChange={vi.fn()} />);

    const badge = screen
      .getByTestId("mobile-host-option-host-5")
      .querySelector('[data-testid="mobile-host-transport-badge"]');
    expect(badge).toHaveAttribute("data-transport", "relay");
    expect(badge).toHaveTextContent("Relay");
  });

  it("shows an empty state when no hosts have been discovered", () => {
    render(<MobileHostDrawer open onOpenChange={vi.fn()} />);
    expect(screen.getByText("No remote hosts discovered yet.")).toBeInTheDocument();
  });

  it("displays per-host agent status counts accurately", () => {
    remoteHostStore.setHosts([
      makeHost({ hostId: "host-1", name: "Studio Mac", agentSummary: { running: 2, waiting: 1 } }),
      makeHost({ hostId: "host-2", name: "Laptop", agentSummary: { running: 0, waiting: 0 } }),
      makeHost({ hostId: "host-3", name: "Build Box" }),
    ]);

    render(<MobileHostDrawer open onOpenChange={vi.fn()} />);

    const host1 = screen.getByTestId("mobile-host-option-host-1");
    expect(host1.querySelector('[data-testid="mobile-host-agent-running"]')).toHaveTextContent("2 running");
    expect(host1.querySelector('[data-testid="mobile-host-agent-waiting"]')).toHaveTextContent("1 waiting");

    const host2 = screen.getByTestId("mobile-host-option-host-2");
    expect(host2.querySelector('[data-testid="mobile-host-agent-summary"]')).not.toBeInTheDocument();

    const host3 = screen.getByTestId("mobile-host-option-host-3");
    expect(host3.querySelector('[data-testid="mobile-host-agent-summary"]')).not.toBeInTheDocument();
  });

  it("aggregates agent totals across all hosts via hostAgentTotals", () => {
    remoteHostStore.setHosts([
      makeHost({ hostId: "host-1", name: "Studio Mac", agentSummary: { running: 2, waiting: 1 } }),
      makeHost({ hostId: "host-2", name: "Laptop", agentSummary: { running: 1, waiting: 3 } }),
    ]);

    expect(hostAgentTotals(remoteHostStore.getState())).toEqual({ running: 3, waiting: 4 });
  });

  it("selects a host via remoteHostStore.setActiveHost when tapped, and closes the drawer", () => {
    remoteHostStore.setHosts([
      makeHost({ hostId: "host-1", name: "Studio Mac" }),
      makeHost({ hostId: "host-2", name: "Laptop" }),
    ]);
    const onOpenChange = vi.fn();

    render(<MobileHostDrawer open onOpenChange={onOpenChange} />);

    fireEvent.click(screen.getByTestId("mobile-host-option-host-2"));

    expect(remoteHostStore.getState().activeHostId).toBe("host-2");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("switches between paired machines on one relay without losing their tokens", () => {
    const relayOrigin = "https://relay.example";
    for (const machineId of ["a", "b"]) remoteHostStore.upsertHost({
      machineId, relayOrigin, displayName: `Machine ${machineId}`, deviceToken: `token-${machineId}`,
      lastSeenAt: null, directHints: [],
    });
    render(<MobileHostDrawer open onOpenChange={vi.fn()} />);
    for (const machineId of ["a", "b", "a"]) {
      const key = remoteHostKey(relayOrigin, machineId);
      fireEvent.click(screen.getByTestId(`mobile-host-option-${key}`));
      expect(remoteHostStore.getState().activeHostId).toBe(key);
      expect(screen.getByTestId(`mobile-host-option-${key}`)).toHaveAttribute("aria-current", "true");
    }
    for (const machineId of ["a", "b"]) {
      expect(remoteHostStore.getState().hosts[remoteHostKey(relayOrigin, machineId)].deviceToken).toBe(`token-${machineId}`);
    }
  });

  it("selects local (null) via remoteHostStore.setActiveHost when the local option is tapped", () => {
    remoteHostStore.setHosts([makeHost({ hostId: "host-1", name: "Studio Mac" })]);
    remoteHostStore.setActiveHost("host-1");
    const onOpenChange = vi.fn();

    render(<MobileHostDrawer open onOpenChange={onOpenChange} />);

    fireEvent.click(screen.getByTestId("mobile-host-option-local"));

    expect(remoteHostStore.getState().activeHostId).toBeNull();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("exposes the active host with aria-current instead of aria-selected", () => {
    remoteHostStore.setHosts([
      makeHost({ hostId: "host-1", name: "Studio Mac" }),
      makeHost({ hostId: "host-2", name: "Laptop" }),
    ]);
    remoteHostStore.setActiveHost("host-2");

    render(<MobileHostDrawer open onOpenChange={vi.fn()} />);

    const active = screen.getByTestId("mobile-host-option-host-2");
    expect(active).toHaveAttribute("aria-current", "true");
    expect(active).not.toHaveAttribute("aria-selected");
    expect(screen.getByTestId("mobile-host-option-host-1")).not.toHaveAttribute("aria-current");
    expect(screen.getByTestId("mobile-host-option-local")).not.toHaveAttribute("aria-current");
    expect(screen.getByTestId("mobile-host-option-local")).not.toHaveAttribute("aria-selected");
  });

  it("ui-shell-sheets-r7-1: a double tap on Remove pairing never lands on Confirm disconnect", () => {
    const props = { onOpenChange: vi.fn(), onDisconnect: vi.fn() };
    render(<MobileHostDrawer open {...props} />);

    // both taps hit the same element under the finger
    const underFinger = screen.getByTestId("mobile-host-drawer-disconnect");
    fireEvent.click(underFinger);
    fireEvent.click(underFinger);

    expect(props.onDisconnect).not.toHaveBeenCalled();
    expect(screen.getByTestId("mobile-host-drawer-confirm-disconnect")).toBeInTheDocument();
  });

  it("ui-shell-sheets-r11-1: a double tap on Cancel never lands on Sign out", () => {
    const props = { onOpenChange: vi.fn(), onDisconnect: vi.fn(), onSignOut: vi.fn() };
    render(<MobileHostDrawer open isAccountSession {...props} />);

    fireEvent.click(screen.getByTestId("mobile-host-drawer-disconnect"));
    // both taps land on the same slot of the row, whatever button occupies it by then
    const cancel = screen.getByRole("button", { name: "Cancel" });
    const row = cancel.parentElement!;
    const slot = Array.from(row.children).indexOf(cancel);
    fireEvent.click(cancel);
    fireEvent.click(row.children[slot]!);

    expect(props.onSignOut).not.toHaveBeenCalled();
    expect(props.onDisconnect).not.toHaveBeenCalled();

    if (screen.queryByRole("button", { name: "Cancel" })) fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(screen.getByTestId("mobile-host-drawer-signout"));
    expect(props.onSignOut).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["pairing mode", { isAccountSession: false, withSignOut: false }],
    ["account session without onSignOut", { isAccountSession: true, withSignOut: false }],
    ["account session with Sign out", { isAccountSession: true, withSignOut: true }],
  ])("ui-shell-sheets-r17-1: the arm button keeps the slot Cancel takes, never Confirm (%s)", (_mode, mode) => {
    const props = {
      onOpenChange: vi.fn(),
      onDisconnect: vi.fn(),
      ...(mode.withSignOut ? { onSignOut: vi.fn() } : {}),
    };
    render(<MobileHostDrawer open isAccountSession={mode.isAccountSession} {...props} />);

    // jsdom has no geometry: the row's flex-1 slots stand in for where each button sits
    const arm = screen.getByTestId("mobile-host-drawer-disconnect");
    const row = arm.parentElement!;
    const armSlots = row.children.length;
    const armSlot = Array.from(row.children).indexOf(arm);

    fireEvent.click(arm);

    const cancel = screen.getByRole("button", { name: "Cancel" });
    const confirm = screen.getByTestId("mobile-host-drawer-confirm-disconnect");
    expect(cancel.parentElement).toBe(row);
    expect(row.children.length).toBe(armSlots);
    expect(Array.from(row.children).indexOf(cancel)).toBe(armSlot);
    expect(Array.from(row.children).indexOf(confirm)).not.toBe(armSlot);

    // a second tap on the arm button's slot cancels instead of disconnecting
    fireEvent.click(row.children[armSlot]!);
    expect(props.onDisconnect).not.toHaveBeenCalled();
  });

  it("ui-shell-sheets-r13-1: keyboard focus stays inside the sheet across the disconnect arm/cancel swap", () => {
    const props = { onOpenChange: vi.fn(), onDisconnect: vi.fn() };
    render(<MobileHostDrawer open {...props} />);

    // a keyboard user focuses the arm button and activates it
    const arm = screen.getByTestId("mobile-host-drawer-disconnect");
    arm.focus();
    fireEvent.click(arm);
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Cancel" }));

    // and Cancel hands focus back to the arm button that replaces it
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(document.activeElement).toBe(screen.getByTestId("mobile-host-drawer-disconnect"));
  });

  it("ui-shell-sheets-1: disarms the disconnect confirmation when the drawer closes", () => {
    const props = { onOpenChange: vi.fn(), onDisconnect: vi.fn() };
    const { rerender } = render(<MobileHostDrawer open {...props} />);

    fireEvent.click(screen.getByTestId("mobile-host-drawer-disconnect"));
    expect(screen.getByTestId("mobile-host-drawer-confirm-disconnect")).toBeInTheDocument();

    rerender(<MobileHostDrawer open={false} {...props} />);
    rerender(<MobileHostDrawer open {...props} />);

    expect(screen.getByTestId("mobile-host-drawer-disconnect")).toBeInTheDocument();
    expect(screen.queryByTestId("mobile-host-drawer-confirm-disconnect")).not.toBeInTheDocument();
    expect(props.onDisconnect).not.toHaveBeenCalled();
  });

  it("closes when the backdrop is tapped", () => {
    const onOpenChange = vi.fn();
    render(<MobileHostDrawer open onOpenChange={onOpenChange} />);

    fireEvent.click(screen.getByTestId("mobile-host-drawer-backdrop"));

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("closes when Escape is pressed", () => {
    const onOpenChange = vi.fn();
    render(<MobileHostDrawer open onOpenChange={onOpenChange} />);

    fireEvent.keyDown(window, { key: "Escape" });

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("renders both close buttons with aria-label 'Close host switcher' and clicking the X closes the drawer", () => {
    const onOpenChange = vi.fn();
    render(<MobileHostDrawer open onOpenChange={onOpenChange} />);

    const closeButtons = screen.getAllByLabelText("Close host switcher");
    expect(closeButtons).toHaveLength(2);

    // The X button inside the dialog header is the second close button (first is backdrop)
    const xButton = closeButtons.find((btn) => btn.querySelector("svg"));
    expect(xButton).toBeDefined();
    fireEvent.click(xButton!);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});

describe("getHostAgentSummary via drawer rendering", () => {
  it("treats a missing agentSummary as zero counts", () => {
    const host: HostEndpoint = {
      hostId: "host-1",
      name: "Studio Mac",
      address: "100.64.0.12",
      transport: "tailscale",
      authStatus: "paired",
      online: true,
    };
    remoteHostStore.setHosts([host]);

    render(<MobileHostDrawer open onOpenChange={vi.fn()} />);

    expect(
      screen.getByTestId("mobile-host-option-host-1").querySelector('[data-testid="mobile-host-agent-summary"]'),
    ).not.toBeInTheDocument();
  });
});
