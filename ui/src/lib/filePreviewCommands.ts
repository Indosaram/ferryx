import { invoke } from "@tauri-apps/api/core";

export type FilePreviewResolved = {
  resolvedPath: string;
  exists: boolean;
  isDirectory: boolean;
};

export async function resolveFilePreviewPath(
  path: string,
  backendSessionId: string,
  workspaceId: string | null = null,
): Promise<FilePreviewResolved> {
  return invoke<FilePreviewResolved>("cmd_file_preview_resolve", {
    path,
    backendSessionId,
    workspaceId,
  });
}

export async function checkFilePreviewChanged(handle: string): Promise<boolean> {
  return invoke<boolean>("cmd_file_preview_changed", {
    handle,
  });
}
