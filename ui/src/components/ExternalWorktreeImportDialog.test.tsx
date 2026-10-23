import React from "react";
import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({ registerProject: vi.fn(), listExternalWorktrees: vi.fn() }));
vi.mock("../lib/tauri", () => native);

import { ExternalWorktreeImportDialog } from "./ExternalWorktreeImportDialog";
import { loadPromptedExternalWorktrees, pendingExternalWorktrees } from "../lib/externalWorktrees";
import { EXTERNAL_WORKTREE_PROMPTED_STORAGE_KEY } from "../lib/storageKeys";
import { useExternalWorktreePrompt } from "../state/useExternalWorktreePrompt";
import type { ExternalWorktree, RegisteredProject } from "../lib/tauri";

const orcaWorktree: ExternalWorktree = {
  path: "/Users/me/orca/workspaces/lectures/feat",
  head: "abc1234def",
  branch: "refs/heads/feat",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
  createdByOrca: true,
};
const otherWorktree: ExternalWorktree = { ...orcaWorktree, path: "/tmp/other", branch: "refs/heads/other", createdByOrca: false };
const project: RegisteredProject = { workspaceId: "lectures", repoRoot: "/Users/me/lectures", gitRoot: "/Users/me/lectures" };

beforeEach(() => {
  window.localStorage.clear();
  native.registerProject.mockReset();
  native.listExternalWorktrees.mockReset();
});
afterEach(cleanup);

describe("pendingExternalWorktrees", () => {
  it("skips worktrees already added as local projects or already answered", () => {
    const pending = pendingExternalWorktrees(
      [orcaWorktree, otherWorktree],
      [project, { workspaceId: "feat", repoRoot: `${orcaWorktree.path}/` }],
      new Set(),
    );
    expect(pending.map((worktree) => worktree.path)).toEqual([otherWorktree.path]);
    expect(pendingExternalWorktrees([otherWorktree], [project], new Set([otherWorktree.path]))).toEqual([]);
  });

  it("still offers a path that only an SSH project shares", () => {
    const ssh: RegisteredProject = { workspaceId: "ssh:x", repoRoot: otherWorktree.path, target: { kind: "ssh", hostId: "h" } };
    expect(pendingExternalWorktrees([otherWorktree], [ssh], new Set())).toHaveLength(1);
  });
});

describe("ExternalWorktreeImportDialog", () => {
  it("imports only the checked worktrees and remembers the answer", async () => {
    native.registerProject.mockImplementation(async ({ workspaceId, repoPath }: { workspaceId: string; repoPath: string }) => ({
      workspaceId, repoRoot: repoPath, gitRoot: repoPath,
    }));
    const onImported = vi.fn();
    const onClose = vi.fn();
    render(<ExternalWorktreeImportDialog worktrees={[orcaWorktree, otherWorktree]} projects={[project]}
      onImported={onImported} onLater={vi.fn()} onClose={onClose} />);

    expect(screen.getByRole("dialog", { name: "Orca worktrees found" })).toBeTruthy();
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Import selected" })); });

    expect(native.registerProject).toHaveBeenCalledExactlyOnceWith({ workspaceId: "feat", repoPath: orcaWorktree.path });
    expect(onImported).toHaveBeenCalledExactlyOnceWith([{ workspaceId: "feat", repoRoot: orcaWorktree.path, gitRoot: orcaWorktree.path }]);
    expect(onClose).toHaveBeenCalledOnce();
    expect(loadPromptedExternalWorktrees()).toEqual(new Set([orcaWorktree.path, otherWorktree.path]));
  });

  it("declining imports nothing and stops asking", () => {
    const onClose = vi.fn();
    render(<ExternalWorktreeImportDialog worktrees={[orcaWorktree]} projects={[project]}
      onImported={vi.fn()} onLater={vi.fn()} onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: "Don't import" }));
    expect(native.registerProject).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
    expect(JSON.parse(window.localStorage.getItem(EXTERNAL_WORKTREE_PROMPTED_STORAGE_KEY) ?? "[]")).toEqual([orcaWorktree.path]);
  });

  it("keeps a failed import open and unanswered", async () => {
    native.registerProject.mockRejectedValue({ code: "INVALID_REPO_ROOT", message: "gone" });
    const onClose = vi.fn();
    const onImported = vi.fn();
    render(<ExternalWorktreeImportDialog worktrees={[orcaWorktree]} projects={[project]}
      onImported={onImported} onLater={vi.fn()} onClose={onClose} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Import selected" })); });
    expect(screen.getByRole("alert").textContent).toBe("gone");
    expect(onClose).not.toHaveBeenCalled();
    expect(onImported).not.toHaveBeenCalled();
    expect(loadPromptedExternalWorktrees().size).toBe(0);
  });
});

describe("useExternalWorktreePrompt", () => {
  it("asks only after the local project is registered, once per session", async () => {
    native.listExternalWorktrees.mockResolvedValue([orcaWorktree]);
    const services = { listExternalWorktrees: native.listExternalWorktrees };
    const { result, rerender } = renderHook(
      ({ ready }: { ready: string | null }) => useExternalWorktreePrompt(project, ready, [project], services),
      { initialProps: { ready: null as string | null } },
    );
    expect(native.listExternalWorktrees).not.toHaveBeenCalled();

    rerender({ ready: "lectures" });
    await waitFor(() => expect(result.current.prompt?.worktrees).toEqual([orcaWorktree]));
    act(() => result.current.later());
    expect(result.current.prompt).toBeNull();

    rerender({ ready: null });
    rerender({ ready: "lectures" });
    expect(native.listExternalWorktrees).toHaveBeenCalledOnce();
  });

  it("never checks SSH projects", () => {
    const ssh: RegisteredProject = { ...project, workspaceId: "ssh:x", target: { kind: "ssh", hostId: "h" } };
    renderHook(() => useExternalWorktreePrompt(ssh, "ssh:x", [ssh], { listExternalWorktrees: native.listExternalWorktrees }));
    expect(native.listExternalWorktrees).not.toHaveBeenCalled();
  });
});

void React;
