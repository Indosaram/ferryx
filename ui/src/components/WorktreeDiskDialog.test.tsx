import "./worktree-disk-test-dom";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it as test, vi } from "vitest";

import type { BranchDeletionPreview, Worktree } from "../lib/types";
import {
  WorktreeDiskDialog,
  type DiskScanSnapshot,
  type WorktreeDiskRow,
  type WorktreeDiskServices,
} from "./WorktreeDiskDialog";

// Bun shares React's module state across files. An unrelated timed-out async
// act scope leaves its private scope depth open, preventing subsequent renders
// from flushing. Run the unchanged cases in fresh Bun processes; Vitest already
// provides file isolation. Do not reset React internals or discard queued work.
const isolatedBun = process.versions.bun && !process.env.VITEST;
const childCase = process.env.WORKTREE_DISK_TEST_CASE;

function it(name: string, body: () => Promise<void>) {
  test(name, isolatedBun && childCase !== name ? () => {
    const fullName = `WorktreeDiskDialog ${name}`;
    const pattern = `^${fullName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`;
    const result = spawnSync(process.execPath, [
      "test", fileURLToPath(import.meta.url), "--test-name-pattern", pattern,
    ], {
      env: { ...process.env, WORKTREE_DISK_TEST_CASE: name },
      encoding: "utf8",
      timeout: 4000,
    });
    if (result.error) throw result.error;
    expect(result.status, result.stdout + result.stderr).toBe(0);
  } : body);
}

if (!isolatedBun || childCase) afterEach(cleanup);

const mockWorktrees: Worktree[] = [
  {
    path: "/repo/main",
    head: "commit111",
    branch: "refs/heads/main",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  },
  {
    path: "/repo/.orca-worktrees/wt-feature-a",
    head: "commit222",
    branch: "refs/heads/orca/ws-1/feature-a",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  },
  {
    path: "/repo/.orca-worktrees/wt-stale-b",
    head: "commit333",
    branch: "refs/heads/orca/ws-1/stale-b",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  },
  {
    path: "/repo/.orca-worktrees/wt-prunable-c",
    head: "commit444",
    branch: "refs/heads/orca/ws-1/prunable-c",
    bare: false,
    detached: false,
    locked: null,
    prunable: "git worktree prune candidate",
  },
];

const nowSeconds = Math.floor(Date.now() / 1000);

const sampleRows: WorktreeDiskRow[] = [
  {
    worktree: mockWorktrees[0], // primary root
    sizeBytes: 2 * 1024 * 1024 * 1024, // 2 GB
    lastCommitAt: nowSeconds - 3600, // 1 hour ago
    isDirty: false,
    dirtyFiles: [],
    error: null,
  },
  {
    worktree: mockWorktrees[1], // feature-a: active, clean, 500 MB
    sizeBytes: 500 * 1024 * 1024, // 500 MB
    lastCommitAt: nowSeconds - 86400 * 2, // 2 days ago
    isDirty: false,
    dirtyFiles: [],
    error: null,
  },
  {
    worktree: mockWorktrees[2], // stale-b: inactive > 14 days, dirty, 1 GB
    sizeBytes: 1024 * 1024 * 1024, // 1 GB
    lastCommitAt: nowSeconds - 86400 * 25, // 25 days ago
    isDirty: true,
    dirtyFiles: [{ statusCode: " M", path: "dirty-file.ts" }],
    error: null,
  },
  {
    worktree: mockWorktrees[3], // prunable-c: prunable badge, error reading size
    sizeBytes: null,
    lastCommitAt: nowSeconds - 86400 * 5,
    isDirty: null,
    dirtyFiles: [],
    error: {
      code: "IO_ERROR",
      message: "Permission denied reading path",
      details: { path: mockWorktrees[3].path },
    },
  },
];

const completedSnapshot: DiskScanSnapshot = {
  workspaceId: "ws-1",
  scanId: "scan-123",
  status: "completed",
  progress: {
    completedWorktrees: 4,
    totalWorktrees: 4,
    currentPath: null,
    scannedBytes: 3.5 * 1024 * 1024 * 1024,
    scannedFiles: 12400,
    scannedEntries: 15200,
  },
  rows: sampleRows,
  error: null,
};

const mockPreview: BranchDeletionPreview = {
  branch: "orca/ws-1/stale-b",
  head: "commit333",
  upstream: null,
  merged: false,
  ahead: 1,
  behind: 0,
};

function createMockServices(overrides: Partial<WorktreeDiskServices> = {}): {
  services: WorktreeDiskServices;
  callOrder: string[];
} {
  const callOrder: string[] = [];

  const services: WorktreeDiskServices = {
    startScan: vi.fn(async (_workspaceId: string, _refresh: boolean) => {
      callOrder.push("startScan");
      return completedSnapshot;
    }),
    cancelScan: vi.fn(async (_workspaceId: string, _scanId: string) => {
      callOrder.push("cancelScan");
      return true;
    }),
    getScanResult: vi.fn(async (_workspaceId: string) => {
      callOrder.push("getScanResult");
      return null;
    }),
    onScanProgress: vi.fn(async (_handler: (snapshot: DiskScanSnapshot) => void) => {
      callOrder.push("onScanProgress");
      return () => {};
    }),
    previewDelete: vi.fn(async (_workspaceId: string, _worktree: Worktree) => {
      callOrder.push("previewDelete");
      return mockPreview;
    }),
    deleteSafe: vi.fn(async (_workspaceId: string, _worktree: Worktree) => {
      callOrder.push("deleteSafe");
    }),
    deleteDestructive: vi.fn(async (_workspaceId: string, _worktree: Worktree) => {
      callOrder.push("deleteDestructive");
    }),
    ...overrides,
  };

  return { services, callOrder };
}

describe("WorktreeDiskDialog", () => {
  it("subscribes to scan progress BEFORE calling startScan", async () => {
    const { services, callOrder } = createMockServices();

    render(
      <WorktreeDiskDialog
        workspaceId="ws-1"
        projectName="Test Project"
        onClose={vi.fn()}
        services={services}
      />,
    );

    await waitFor(() => {
      expect(services.startScan).toHaveBeenCalledWith("ws-1", false);
    });

    const progressIndex = callOrder.indexOf("onScanProgress");
    const startIndex = callOrder.indexOf("startScan");
    expect(progressIndex).toBeGreaterThanOrEqual(0);
    expect(startIndex).toBeGreaterThanOrEqual(0);
    expect(progressIndex).toBeLessThan(startIndex);
  });

  it("displays scan progress while running and allows cancelling the scan", async () => {
    const runningSnapshot: DiskScanSnapshot = {
      workspaceId: "ws-1",
      scanId: "scan-999",
      status: "running",
      progress: {
        completedWorktrees: 2,
        totalWorktrees: 5,
        currentPath: "/repo/.orca-worktrees/wt-large",
        scannedBytes: 150 * 1024 * 1024,
        scannedFiles: 3200,
        scannedEntries: 4100,
      },
      rows: [],
      error: null,
    };

    const { services } = createMockServices({
      onScanProgress: vi.fn(async (_handler) => {
        return () => {};
      }),
      startScan: vi.fn(async () => runningSnapshot),
    });

    const view = render(
      <WorktreeDiskDialog
        workspaceId="ws-1"
        projectName="Test Project"
        onClose={vi.fn()}
        services={services}
      />,
    );

    const scanningText = await view.findByText(/Scanning worktrees/i);
    expect(scanningText).toBeTruthy();
    expect(view.getByText(/2 of 5 worktrees/i)).toBeTruthy();

    const cancelButton = view.getByRole("button", { name: /Cancel scan/i });
    expect(cancelButton).toBeTruthy();

    fireEvent.click(cancelButton);
    await waitFor(() => {
      expect(services.cancelScan).toHaveBeenCalledWith("ws-1", "scan-999");
    });
  });

  it("displays every worktree row including size, last commit date, dirty state, and per-row error", async () => {
    const { services } = createMockServices();

    const view = render(
      <WorktreeDiskDialog
        workspaceId="ws-1"
        projectName="Test Project"
        onClose={vi.fn()}
        services={services}
      />,
    );

    // Dialog title
    expect(await view.findByRole("dialog", { name: /Worktree Disk Management/i })).toBeTruthy();

    // Verify row contents
    expect(view.getByText("feature-a")).toBeTruthy();
    expect(view.getByText("stale-b")).toBeTruthy();
    expect(view.getByText("prunable-c")).toBeTruthy();

    // Sizes
    expect(view.getByText(/500(\.0)? MB/i)).toBeTruthy();
    expect(view.getByText(/1(\.0)? GB/i)).toBeTruthy();

    // Dirty state
    expect(view.getByText(/1 dirty file/i)).toBeTruthy();

    // Per-row error
    expect(view.getByText(/Permission denied reading path/i)).toBeTruthy();
  });

  it("identifies cleanup candidates based on prunable status and unused days threshold", async () => {
    const { services } = createMockServices();

    const view = render(
      <WorktreeDiskDialog
        workspaceId="ws-1"
        projectName="Test Project"
        onClose={vi.fn()}
        services={services}
      />,
    );

    await view.findByText("stale-b");

    // stale-b has commit 25 days ago (> 14 days default) -> candidate badge
    const staleCandidateBadge = view.getByTestId("candidate-badge-stale-b");
    expect(staleCandidateBadge).toBeTruthy();

    // prunable-c has git prunable record -> candidate badge
    const prunableCandidateBadge = view.getByTestId("candidate-badge-prunable-c");
    expect(prunableCandidateBadge).toBeTruthy();

    // feature-a was committed 2 days ago and not prunable -> no candidate badge
    expect(view.queryByTestId("candidate-badge-feature-a")).toBeNull();
  });

  it("surfaces structured errors including UNSUPPORTED for direct SSH projects", async () => {
    const unsupportedError = {
      code: "UNSUPPORTED",
      message: "Disk scans are only available for local workspaces",
      details: {},
    };

    const { services } = createMockServices({
      startScan: vi.fn(async () => {
        throw unsupportedError;
      }),
    });

    const view = render(
      <WorktreeDiskDialog
        workspaceId="ssh:remote-host"
        projectName="Remote SSH Project"
        onClose={vi.fn()}
        services={services}
      />,
    );

    expect(await view.findByText(/Disk scans are only available for local workspaces/i)).toBeTruthy();
    expect(view.getByText("UNSUPPORTED")).toBeTruthy();
  });

  it("wires cleanup flow using preview and warns before deleting a dirty worktree", async () => {
    const { services } = createMockServices();

    const view = render(
      <WorktreeDiskDialog
        workspaceId="ws-1"
        projectName="Test Project"
        onClose={vi.fn()}
        services={services}
      />,
    );

    await view.findByText("stale-b");

    // Click cleanup/delete button on the dirty row 'stale-b'
    const deleteButton = view.getByTestId("delete-btn-stale-b");
    fireEvent.click(deleteButton);

    // Delete confirmation dialog opens
    expect(await view.findByRole("dialog", { name: /Delete worktree/i })).toBeTruthy();
    expect(services.previewDelete).toHaveBeenCalled();

    // Dirty warning is displayed
    expect(await view.findByText(/Uncommitted changes/i)).toBeTruthy();
    const confirmBtn = view.getByRole("button", {
      name: /Delete worktree and discard changes permanently/i,
    });
    expect(confirmBtn).toBeTruthy();

    // Explicit confirmation triggers destructive deletion
    fireEvent.click(confirmBtn);
    await waitFor(() => {
      expect(services.deleteDestructive).toHaveBeenCalledWith("ws-1", mockWorktrees[2]);
    });
  });

  it("sorts worktree rows by apparent size descending by default", async () => {
    const { services } = createMockServices();

    const view = render(
      <WorktreeDiskDialog
        workspaceId="ws-1"
        projectName="Test Project"
        onClose={vi.fn()}
        services={services}
      />,
    );

    await view.findByText("feature-a");

    const rowNames = view.getAllByTestId(/worktree-disk-row-name-/i).map((el) => el.textContent);
    // mockWorktrees[0] (main) has 2 GB, stale-b has 1 GB, feature-a has 500 MB, prunable-c has null
    expect(rowNames[0]).toContain("main");
    expect(rowNames[1]).toContain("stale-b");
    expect(rowNames[2]).toContain("feature-a");
  });

  it("removes the deleted worktree row from the list after deletion completes", async () => {
    const { services } = createMockServices();

    const view = render(
      <WorktreeDiskDialog
        workspaceId="ws-1"
        projectName="Test Project"
        onClose={vi.fn()}
        services={services}
      />,
    );

    await view.findByText("stale-b");
    const before = view
      .getAllByTestId(/worktree-disk-row-name-/i)
      .map((el) => el.textContent);
    expect(before.some((name) => name?.includes("stale-b"))).toBe(true);

    fireEvent.click(view.getByTestId("delete-btn-stale-b"));
    await view.findByRole("dialog", { name: /Delete worktree/i });
    fireEvent.click(
      view.getByRole("button", {
        name: /Delete worktree and discard changes permanently/i,
      }),
    );

    // The scan snapshot must drop the deleted row without requiring a rescan.
    await waitFor(() => {
      const after = view
        .getAllByTestId(/worktree-disk-row-name-/i)
        .map((el) => el.textContent);
      expect(after.some((name) => name?.includes("stale-b"))).toBe(false);
      expect(after.length).toBe(before.length - 1);
    });

    // Surviving rows are untouched by the removal.
    const survivors = view
      .getAllByTestId(/worktree-disk-row-name-/i)
      .map((el) => el.textContent);
    expect(survivors.some((name) => name?.includes("main"))).toBe(true);
    expect(survivors.some((name) => name?.includes("feature-a"))).toBe(true);
  });
});
