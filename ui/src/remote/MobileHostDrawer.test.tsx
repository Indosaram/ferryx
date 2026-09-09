import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { remoteHostStore, type HostEndpoint } from "../state/remoteHostStore";
import { hostAgentTotals, MobileHostDrawer, type HostWithAgentSummary } from "./MobileHostDrawer";

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

  it("selects local (null) via remoteHostStore.setActiveHost when the local option is tapped", () => {
    remoteHostStore.setHosts([makeHost({ hostId: "host-1", name: "Studio Mac" })]);
    remoteHostStore.setActiveHost("host-1");
    const onOpenChange = vi.fn();

    render(<MobileHostDrawer open onOpenChange={onOpenChange} />);

    fireEvent.click(screen.getByTestId("mobile-host-option-local"));

    expect(remoteHostStore.getState().activeHostId).toBeNull();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("closes when the backdrop is tapped", () => {
    const onOpenChange = vi.fn();
    render(<MobileHostDrawer open onOpenChange={onOpenChange} />);

    fireEvent.click(screen.getByTestId("mobile-host-drawer-backdrop"));

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
