import { invoke } from "@tauri-apps/api/core";

export interface RemoteDirectoryEntry {
  readonly name: string;
  readonly path: string;
  readonly hidden: boolean;
}

export interface RemoteDirectoryListing {
  readonly path: string;
  readonly parentPath: string | null;
  readonly homePath: string;
  readonly entries: readonly RemoteDirectoryEntry[];
  readonly truncated: boolean;
}

export function listRemoteDirectories(
  hostId: string,
  path: string | null,
): Promise<RemoteDirectoryListing> {
  return invoke<RemoteDirectoryListing>("cmd_ssh_list_directories", {
    request: { hostId, path },
  });
}
