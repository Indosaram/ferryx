import "./worktree-disk-test-dom";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorktreeDiskDialog, type DiskScanSnapshot, type WorktreeDiskServices } from "./WorktreeDiskDialog";

afterEach(cleanup);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function snapshot(scanId = "a", status: DiskScanSnapshot["status"] = "running", count = 0): DiskScanSnapshot {
  return {
    workspaceId: "ws-1", scanId, status,
    progress: { completedWorktrees: count, totalWorktrees: 4, currentPath: null, scannedBytes: count, scannedFiles: count, scannedEntries: count },
    rows: [], error: status === "cancelled" || status === "failed"
      ? { code: status === "cancelled" ? "SCAN_CANCELLED" : "IO_ERROR", message: status, details: {} } : null,
  };
}

function harness(delayedListener = false) {
  const listener = deferred<() => void>();
  const started = deferred<void>();
  const response = deferred<DiskScanSnapshot>();
  const unlisten = vi.fn();
  let emit!: (value: DiskScanSnapshot) => void;
  const services: WorktreeDiskServices = {
    onScanProgress: vi.fn((handler) => { emit = handler; return delayedListener ? listener.promise : Promise.resolve(unlisten); }),
    startScan: vi.fn(() => { started.resolve(); return response.promise; }),
    cancelScan: vi.fn(async () => true), getScanResult: vi.fn(async () => null),
    previewDelete: vi.fn(), deleteSafe: vi.fn(), deleteDestructive: vi.fn(),
  };
  return { services, listener, started, response, unlisten, emit: (value: DiskScanSnapshot) => emit(value) };
}

async function mount(h: ReturnType<typeof harness>) {
  const view = render(<WorktreeDiskDialog workspaceId="ws-1" services={h.services} onClose={() => {}} />);
  // The test timeout bounds this exact service-call signal, not a polling loop.
  await act(async () => { await h.started.promise; });
  return view;
}

describe("WorktreeDiskDialog lifecycle", () => {
  it("cancels its running scan and disposes the listener on unmount", async () => {
    const h = harness(); const view = await mount(h);
    await act(async () => h.response.resolve(snapshot()));
    view.unmount();
    expect(h.unlisten).toHaveBeenCalledTimes(1);
    expect(h.services.cancelScan).toHaveBeenCalledWith("ws-1", "a");
  });

  it("disposes a listener registered after unmount without starting", async () => {
    const h = harness(true);
    const view = render(<WorktreeDiskDialog workspaceId="ws-1" services={h.services} onClose={() => {}} />);
    view.unmount();
    await act(async () => h.listener.resolve(h.unlisten));
    expect(h.unlisten).toHaveBeenCalledTimes(1);
    expect(h.services.startScan).not.toHaveBeenCalled();
  });

  it("cancels a start response arriving after unmount", async () => {
    const h = harness(); const view = await mount(h); view.unmount();
    await act(async () => h.response.resolve(snapshot()));
    expect(h.services.cancelScan).toHaveBeenCalledWith("ws-1", "a");
  });

  it("keeps delayed old-workspace responses out of the replacement effect", async () => {
    const old = harness(); const view = await mount(old);
    const next = harness();
    await act(async () => {
      view.rerender(<WorktreeDiskDialog workspaceId="ws-2" services={next.services} onClose={() => {}} />);
    });
    await act(async () => {
      next.response.resolve({ ...snapshot("b", "running", 2), workspaceId: "ws-2" });
      old.response.resolve(snapshot());
      old.emit(snapshot("a", "failed"));
      next.emit(snapshot("b", "failed"));
    });
    expect(old.unlisten).toHaveBeenCalledTimes(1);
    expect(old.services.cancelScan).toHaveBeenCalledWith("ws-1", "a");
    expect(view.getByText(/2 of 4 worktrees/)).toBeTruthy();
    view.unmount();
    expect(next.services.cancelScan).toHaveBeenCalledWith("ws-2", "b");
  });

  it("serializes repeated refresh clicks while the start response is pending", async () => {
    const h = harness(); const view = await mount(h);
    await act(async () => {
      fireEvent.click(view.getByRole("button", { name: "Refresh scan" }));
      fireEvent.click(view.getByRole("button", { name: "Refresh scan" }));
    });
    expect(h.services.startScan).toHaveBeenCalledTimes(1);
    await act(async () => h.response.resolve(snapshot("a", "completed")));
  });

  for (const status of ["running", "completed", "cancelled", "failed"] as const) {
    it(`preserves ${status} events ahead of the start response`, async () => {
      const h = harness(); const view = await mount(h);
      await act(async () => { h.emit(snapshot("a", status, 3)); h.response.resolve(snapshot()); });
      expect(view.queryByRole("button", { name: "Cancel scan" }) !== null).toBe(status === "running");
      if (status === "running") expect(view.getByText(/3 of 4 worktrees/)).toBeTruthy();
      expect(view.queryByRole("button", { name: "Retry" }) !== null).toBe(status === "failed");
      expect(view.queryByRole("button", { name: "Restart scan" }) !== null).toBe(status === "cancelled");
      view.unmount();
      expect(h.services.cancelScan).toHaveBeenCalledTimes(status === "running" ? 1 : 0);
    });
  }

  it("does not classify a cancelled response as failure", async () => {
    const h = harness(); const view = await mount(h);
    await act(async () => h.response.resolve(snapshot("a", "cancelled")));
    expect(view.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(view.getByRole("button", { name: "Restart scan" })).toBeTruthy();
  });

  it("ignores regressive progress, terminal regressions and unrelated scan events", async () => {
    const h = harness(); const view = await mount(h);
    await act(async () => h.response.resolve(snapshot("a", "running", 3)));
    await act(async () => { h.emit(snapshot("a", "running", 1)); h.emit(snapshot("foreign", "failed")); });
    expect(view.getByText(/3 of 4 worktrees/)).toBeTruthy();
    await act(async () => { h.emit(snapshot("a", "completed", 4)); h.emit(snapshot()); });
    expect(view.queryByRole("button", { name: "Cancel scan" })).toBeNull();
  });

  it("ignores superseded events while refreshing and after the new response", async () => {
    const h = harness(); const view = await mount(h);
    await act(async () => h.response.resolve(snapshot("a", "completed", 4)));
    const next = deferred<DiskScanSnapshot>();
    h.services.startScan = vi.fn(() => next.promise);
    await act(async () => fireEvent.click(view.getByRole("button", { name: "Refresh scan" })));
    await act(async () => { h.emit(snapshot("b", "running", 2)); h.emit(snapshot("a", "cancelled")); next.resolve(snapshot("b")); });
    await act(async () => h.emit(snapshot("a", "failed")));
    expect(view.getByText(/2 of 4 worktrees/)).toBeTruthy();
    expect(view.queryByRole("button", { name: "Retry" })).toBeNull();
    view.unmount();
    expect(h.services.cancelScan).toHaveBeenCalledWith("ws-1", "b");
  });
});
