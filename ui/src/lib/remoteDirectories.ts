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

/** A captured owner/generation; changing sources cancels UI adoption of pending results. */
export interface DirectorySource {
  readonly key: string;
  readonly directories: (path: string | null, includeHidden: boolean) => Promise<RemoteDirectoryListing>;
}

export function listRemoteDirectories(
  hostId: string,
  path: string | null,
): Promise<RemoteDirectoryListing> {
  return invoke<RemoteDirectoryListing>("cmd_ssh_list_directories", {
    request: { hostId, path },
  });
}
