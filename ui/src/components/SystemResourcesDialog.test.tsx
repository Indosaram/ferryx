import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { HostResourceSnapshot } from "../lib/systemResources";
import { remoteHostStore, type HostEndpoint } from "../state/remoteHostStore";

const { fetchSystemResources, fetchPairedResources } = vi.hoisted(() => ({
  fetchSystemResources: vi.fn(),
  fetchPairedResources: vi.fn(),
}));

vi.mock("../lib/systemResources", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/systemResources")>();
  return { ...actual, fetchSystemResources, fetchPairedResources };
});

import { SystemResourcesDialog } from "./SystemResourcesDialog";

const snapshot: HostResourceSnapshot = {
  sampledAtMs: 1_700_000_000_000,
  platform: "macos",
  cpuCount: 12,
  cpuUtilization: 0.42,
  loadAverage1m: 3.5,
  memoryTotalBytes: 32 * 1024 ** 3,
  memoryUsedBytes: 16 * 1024 ** 3,
  swapTotalBytes: 4 * 1024 ** 3,
  swapUsedBytes: 1024 ** 3,
  uptimeSeconds: 3700,
  diskTotalBytes: 1000 * 1024 ** 3,
  diskFreeBytes: 250 * 1024 ** 3,
  processCount: 512,
  sessions: [
    {
      sessionId: "backend-1",
      pid: 4242,
      worktreePath: "/repo/.orca-worktrees/ferryx/pane-liveness",
      cpuPercent: 12.5,
      residentBytes: 512 * 1024 ** 2,
      processCount: 4,
    },
    {
      sessionId: "backend-2",
      pid: null,
      worktreePath: null,
      cpuPercent: null,
      residentBytes: null,
      processCount: null,
    },
  ],
  unavailable: [],
};

beforeEach(() => {
  fetchSystemResources.mockReset();
  fetchPairedResources.mockReset();
  remoteHostStore.reset();
});

afterEach(() => {
  cleanup();
  remoteHostStore.reset();
});

function pairedHost(hostId: string, name: string, overrides: Partial<HostEndpoint> = {}): HostEndpoint {
  return {
    hostId, name, displayName: name, address: "https://relay.ferryx.dev", transport: "relay",
    authStatus: "paired", online: true, machineId: `m-${hostId}`, generation: "3", grantScope: "machine",
    ...overrides,
  };
}

function withHosts(hosts: HostEndpoint[]) {
  remoteHostStore.setState((prev) => ({ ...prev, hosts: Object.fromEntries(hosts.map((host) => [host.hostId, host])) }));
}

describe("SystemResourcesDialog", () => {
  it("renders host metrics and one row per session", async () => {
    fetchSystemResources.mockResolvedValue(snapshot);
    render(<SystemResourcesDialog onClose={() => undefined} />);

    expect(await screen.findByRole("dialog", { name: "System resources" })).toBeInTheDocument();
    expect(await screen.findByText(/macos · 12 cores · up 1h 1m/)).toBeInTheDocument();
    expect(screen.getByText("16.0 GB / 32.0 GB")).toBeInTheDocument();
    expect(screen.getByText("1.00 GB / 4.00 GB")).toBeInTheDocument();

    const rows = await screen.findAllByTestId("resource-session-row");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("backend-1");
    expect(rows[0]).toHaveTextContent("4242");
    expect(rows[0]).toHaveTextContent("12.5%");
    expect(rows[1]).toHaveTextContent("backend-2");
    const groups = screen.getAllByTestId("resource-worktree-group");
    expect(groups.map((group) => group.textContent)).toEqual([
      expect.stringContaining("pane-liveness"),
      expect.stringContaining("Other sessions"),
    ]);
  });

  it("shows every machine-granted paired host beside this machine with its own status", async () => {
    fetchSystemResources.mockResolvedValue(snapshot);
    withHosts([
      pairedHost("h-omarchy", "omarchy"),
      pairedHost("h-win", "maho-win"),
      pairedHost("h-old", "old-mac"),
      pairedHost("h-mirror", "mirror-only", { grantScope: "mirror" }),
    ]);
    fetchPairedResources.mockImplementation(async (host: { hostId: string }) => {
      if (host.hostId === "h-omarchy") {
        return {
          status: "sampled",
          snapshot: { ...snapshot, platform: "linux", cpuCount: 16, sessions: [
            { sessionId: "remote-1", pid: 77, worktreePath: "feature-x", cpuPercent: 3, residentBytes: 1024 ** 2, processCount: 2 },
          ] },
        };
      }
      if (host.hostId === "h-win") return { status: "unreachable" };
      return { status: "update-required" };
    });
    render(<SystemResourcesDialog onClose={() => undefined} />);

    await waitFor(() => expect(screen.getAllByTestId("resource-session-row")).toHaveLength(3));
    const machines = screen.getAllByTestId("resource-machine");
    expect(machines.map((machine) => machine.getAttribute("aria-label"))).toEqual([
      "This machine", "maho-win", "old-mac", "omarchy",
    ]);
    expect(fetchPairedResources.mock.calls.map(([host]) => host.hostId).sort()).toEqual(["h-old", "h-omarchy", "h-win"]);
    expect(machines[1]).toHaveTextContent("Couldn't reach maho-win; its resources are unknown.");
    expect(machines[2]).toHaveTextContent("Update Ferryx on old-mac to see its resources.");
    expect(machines[3]).toHaveTextContent("linux · 16 cores");
    expect(machines[3]).toHaveTextContent("feature-x");
    expect(screen.getByText("4 machines")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Kill session remote-1" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Kill session backend-1" })).toBeInTheDocument();
  });

  it("sorts worktree groups by name, cpu and memory", async () => {
    fetchSystemResources.mockResolvedValue({
      ...snapshot,
      sessions: [
        { sessionId: "a", pid: 1, worktreePath: "/r/zeta", cpuPercent: 50, residentBytes: 10, processCount: 1 },
        { sessionId: "b", pid: 2, worktreePath: "/r/alpha", cpuPercent: 1, residentBytes: 900, processCount: 1 },
        { sessionId: "c", pid: 3, worktreePath: "/r/mid", cpuPercent: 20, residentBytes: 400, processCount: 1 },
      ],
    });
    render(<SystemResourcesDialog onClose={() => undefined} />);
    const labels = () => screen.getAllByTestId("resource-worktree-group").map((group) => group.textContent?.split("·")[0]);

    await waitFor(() => expect(labels()).toEqual(["alpha", "mid", "zeta"]));
    fireEvent.click(screen.getByRole("button", { name: "CPU" }));
    expect(labels()).toEqual(["zeta", "mid", "alpha"]);
    fireEvent.click(screen.getByRole("button", { name: "Name" }));
    expect(labels()).toEqual(["alpha", "mid", "zeta"]);
    fireEvent.click(screen.getByRole("button", { name: "Memory" }));
    expect(labels()).toEqual(["alpha", "mid", "zeta"]);
  });

  it("collapses a machine and a worktree group", async () => {
    fetchSystemResources.mockResolvedValue(snapshot);
    render(<SystemResourcesDialog onClose={() => undefined} />);
    await screen.findAllByTestId("resource-session-row");

    fireEvent.click(screen.getByRole("button", { name: "Collapse pane-liveness" }));
    expect(screen.getAllByTestId("resource-session-row")).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Collapse This machine" }));
    expect(screen.queryAllByTestId("resource-session-row")).toHaveLength(0);
    expect(screen.queryByTestId("meter-cpu")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand This machine" }));
    expect(screen.getAllByTestId("resource-session-row")).toHaveLength(1);
  });

  it("kills a local session only after confirmation", async () => {
    fetchSystemResources.mockResolvedValue(snapshot);
    const killSession = vi.fn(async () => undefined);
    render(<SystemResourcesDialog onClose={() => undefined} killSession={killSession} />);
    await screen.findAllByTestId("resource-session-row");

    fireEvent.click(screen.getByRole("button", { name: "Kill session backend-1" }));
    const confirm = screen.getByRole("alertdialog", { name: "Kill session" });
    expect(confirm).toHaveTextContent("Kill backend-1?");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(killSession).not.toHaveBeenCalled();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Kill session backend-1" }));
    fireEvent.click(screen.getAllByRole("button", { name: "Kill session" }).at(-1)!);
    await waitFor(() => expect(killSession).toHaveBeenCalledWith("backend-1"));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("keeps the confirmation open and shows the failure when the kill is refused", async () => {
    fetchSystemResources.mockResolvedValue(snapshot);
    const killSession = vi.fn(async () => { throw { code: "SESSION_NOT_FOUND", message: "session already gone" }; });
    render(<SystemResourcesDialog onClose={() => undefined} killSession={killSession} />);
    await screen.findAllByTestId("resource-session-row");

    fireEvent.click(screen.getByRole("button", { name: "Kill session backend-1" }));
    fireEvent.click(screen.getAllByRole("button", { name: "Kill session" }).at(-1)!);
    const confirm = await screen.findByRole("alertdialog", { name: "Kill session" });
    await waitFor(() => expect(confirm).toHaveTextContent("session already gone"));
  });

  it("draws cpu and memory history sparklines once samples arrive", async () => {
    fetchSystemResources.mockResolvedValue(snapshot);
    render(<SystemResourcesDialog onClose={() => undefined} />);
    await screen.findAllByTestId("resource-session-row");
    expect(screen.getByTestId("sparkline-cpu")).toBeInTheDocument();
    expect(screen.getByTestId("sparkline-mem")).toBeInTheDocument();
  });

  it("marks a metric the host could not report instead of showing zero", async () => {
    fetchSystemResources.mockResolvedValue({
      ...snapshot,
      cpuUtilization: null,
      loadAverage1m: null,
      swapTotalBytes: null,
      swapUsedBytes: null,
      unavailable: ["cpuUtilization", "swapTotalBytes"],
    });
    render(<SystemResourcesDialog onClose={() => undefined} />);

    await screen.findAllByTestId("resource-session-row");
    expect(screen.getByText("This host could not report: cpuUtilization, swapTotalBytes")).toBeInTheDocument();
    expect(screen.getAllByText("—").length).toBeGreaterThan(0);
  });

  it("shows the daemon's failure without discarding the last reading", async () => {
    fetchSystemResources.mockRejectedValue({ code: "DAEMON_UNAVAILABLE", message: "background service unreachable" });
    render(<SystemResourcesDialog onClose={() => undefined} />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("background service unreachable");
  });

  it("re-asks the daemon on its sample interval while open and stops after unmount", async () => {
    vi.useFakeTimers();
    try {
      fetchSystemResources.mockResolvedValue(snapshot);
      const view = render(<SystemResourcesDialog onClose={() => undefined} />);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(fetchSystemResources).toHaveBeenCalledTimes(1);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(fetchSystemResources).toHaveBeenCalledTimes(2);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(4000);
      });
      expect(fetchSystemResources).toHaveBeenCalledTimes(4);

      view.unmount();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(10_000);
      });
      expect(fetchSystemResources).toHaveBeenCalledTimes(4);
    } finally {
      vi.useRealTimers();
    }
  });

  it("closes on Escape and on a backdrop press, but not on a press inside the panel", async () => {
    fetchSystemResources.mockResolvedValue(snapshot);
    const onClose = vi.fn();
    render(<SystemResourcesDialog onClose={onClose} />);
    const dialog = await screen.findByRole("dialog", { name: "System resources" });

    fireEvent.mouseDown(dialog);
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);

    fireEvent.mouseDown(dialog.parentElement!);
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(2));
  });
});
