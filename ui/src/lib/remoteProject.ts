import { invoke, isTauri } from "@tauri-apps/api/core";
import type { RegisteredProject } from "./tauri";
import type { BranchDeletionPreview, Worktree } from "./types";

export interface RegisterRemoteProjectRequest {
  workspaceId: string;
  hostId: string;
  repoPath: string;
}

export interface RegisteredRemoteProject {
  workspaceId: string;
  repoRoot: string;
  gitRoot: string | null;
  gitRemote?: string | null;
  gitCommonDir?: string | null;
  gitBranch?: string | null;
  gitHead?: string | null;
  hostId: string;
  hostLabel: string;
}

export interface RemoteClipboardImagePaste {
  remotePath: string;
  byteLength: number;
}

export interface LocalClipboardImagePaste {
  localPath: string;
  byteLength: number;
}

export function isPairedWorkspaceId(workspaceId: string | null | undefined): boolean {
  return typeof workspaceId === "string" && workspaceId.startsWith("daemon:");
}

export function isRemoteWorkspaceId(workspaceId: string | null | undefined): boolean {
  return typeof workspaceId === "string" && workspaceId.startsWith("ssh:");
}

/**
 * Saves the clipboard image to a private temporary file on this machine and resolves to the
 * path local agents can open. `null` means the clipboard held no image.
 */
export async function pasteClipboardImageLocally(): Promise<LocalClipboardImagePaste | null> {
  if (!isTauri()) {
    return null;
  }
  const result = await invoke<LocalClipboardImagePaste | null>("cmd_local_paste_clipboard_image");
  return result ?? null;
}

/**
 * Copies the clipboard image to the SSH host owning this workspace and resolves to the path the
 * remote agent can open. `null` means the clipboard held no image to send.
 */
export async function pasteClipboardImageToRemote(
  workspaceId: string,
): Promise<RemoteClipboardImagePaste | null> {
  let command: string | null = null;
  if (typeof workspaceId === "string") {
    if (workspaceId.startsWith("ssh:")) {
      command = "cmd_ssh_paste_clipboard_image";
    } else if (workspaceId.startsWith("daemon:")) {
      command = "cmd_daemon_paste_clipboard_image";
    }
  }
  if (!command) {
    throw { code: "UNSUPPORTED_CAPABILITY", message: "Clipboard image upload is not supported for this target." };
  }
  if (!isTauri()) {
    return null;
  }
  const result = await invoke<RemoteClipboardImagePaste | null>(command, {
    workspaceId,
  });
  return result ?? null;
}

export function toRegisteredProject(remote: RegisteredRemoteProject): RegisteredProject {
  return {
    workspaceId: remote.workspaceId,
    repoRoot: remote.repoRoot,
    gitRoot: remote.gitRoot ?? null,
    ...(remote.gitRemote ? { gitRemote: remote.gitRemote } : {}),
    ...(remote.gitCommonDir ? { gitCommonDir: remote.gitCommonDir } : {}),
    gitBranch: remote.gitBranch ?? null,
    gitHead: remote.gitHead ?? null,
    hostLabel: remote.hostLabel,
    target: {
      kind: "ssh",
      hostId: remote.hostId,
    },
  };
}

export async function registerRemoteProject(
  request: RegisterRemoteProjectRequest,
): Promise<RegisteredRemoteProject> {
  if (!isTauri()) {
    throw new Error("Remote project registration is available only in the Ferryx desktop runtime");
  }
  return invoke<RegisteredRemoteProject>("cmd_project_register_remote", { request });
}

export interface RemoteWorktree {
  path: string;
  head: string | null;
  branch: string | null;
  bare: boolean;
  detached: boolean;
}

export async function listRemoteWorktrees(workspaceId: string): Promise<RemoteWorktree[]> {
  if (!isTauri()) {
    return [];
  }
  return invoke<RemoteWorktree[]>("cmd_ssh_list_remote_worktrees", { workspaceId });
}

export async function createRemoteWorktree(options: {
  workspaceId: string;
  slug: string;
  baseRef?: string | null;
}): Promise<RemoteWorktree> {
  if (!isTauri()) {
    throw new Error("Remote worktree creation is available only in the Ferryx desktop runtime");
  }
  return invoke<RemoteWorktree>("cmd_ssh_create_remote_worktree", {
    workspaceId: options.workspaceId,
    slug: options.slug,
    baseRef: options.baseRef ?? null,
  });
}

export async function deleteRemoteWorktree(options: {
  workspaceId: string;
  path: string;
  force?: boolean;
}): Promise<void> {
  if (!isTauri()) {
    throw new Error("Remote worktree deletion is available only in the Ferryx desktop runtime");
  }
  return invoke<void>("cmd_ssh_delete_remote_worktree", {
    workspaceId: options.workspaceId,
    path: options.path,
    force: options.force ?? false,
  });
}

export type WorktreeDeleteServices = {
  previewDelete: (worktree: Worktree) => Promise<BranchDeletionPreview>;
  deleteSafe: (worktree: Worktree) => Promise<void>;
  deleteDestructive: (worktree: Worktree) => Promise<void>;
};

export function createRemoteWorktreeDeleteServices(workspaceId: string): WorktreeDeleteServices {
  return {
    previewDelete: async (worktree) => {
      const branchName = (worktree.branch ?? "").replace(/^refs\/heads\//, "");
      return {
        dirtyState: { isDirty: false, files: [] },
        missing: false,
        branch: branchName || (worktree.path.split(/[/\\]/).pop() ?? "worktree"),
        head: worktree.head ?? "",
        upstream: null,
        merged: true,
        ahead: null,
        behind: null,
      };
    },
    deleteSafe: async (worktree) => {
      await deleteRemoteWorktree({
        workspaceId,
        path: worktree.path,
        force: false,
      });
    },
    deleteDestructive: async (worktree) => {
      await deleteRemoteWorktree({
        workspaceId,
        path: worktree.path,
        force: true,
      });
    },
  };
}

