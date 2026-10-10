import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { HostResourceSnapshot } from "../lib/systemResources";

const { fetchSystemResources } = vi.hoisted(() => ({ fetchSystemResources: vi.fn() }));

vi.mock("../lib/systemResources", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/systemResources")>();
  return { ...actual, fetchSystemResources };
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
});

afterEach(cleanup);

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
    expect(rows[0]).toHaveTextContent("pane-liveness");
    expect(rows[0]).toHaveTextContent("4242");
    expect(rows[0]).toHaveTextContent("12.5%");
    expect(rows[1]).toHaveTextContent("backend-2");
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
