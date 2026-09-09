import { invoke, isTauri } from "@tauri-apps/api/core";
import type { RegisteredProject } from "./tauri";

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

export function isRemoteWorkspaceId(workspaceId: string | null | undefined): boolean {
  return typeof workspaceId === "string" && workspaceId.startsWith("ssh:");
}

/**
 * Copies the clipboard image to the SSH host owning this workspace and resolves to the path the
 * remote agent can open. `null` means the clipboard held no image to send.
 */
export async function pasteClipboardImageToRemote(
  workspaceId: string,
): Promise<RemoteClipboardImagePaste | null> {
  if (!isTauri()) {
    return null;
  }
  const result = await invoke<RemoteClipboardImagePaste | null>("cmd_ssh_paste_clipboard_image", {
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
}): Promise<void> {
  if (!isTauri()) {
    throw new Error("Remote worktree deletion is available only in the Ferryx desktop runtime");
  }
  return invoke<void>("cmd_ssh_delete_remote_worktree", {
    workspaceId: options.workspaceId,
    path: options.path,
  });
}

