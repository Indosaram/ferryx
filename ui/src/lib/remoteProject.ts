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
