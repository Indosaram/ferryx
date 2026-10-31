import { createFilePreviewController, type FilePreviewController } from "./filePreview";
import type { FilePreviewOpenRequest, FilePreviewSource } from "./filePreviewTypes";
import { isPairedWorkspaceId, isRemoteWorkspaceId } from "./remoteProject";

type Entry = {
  controller: FilePreviewController;
  requestKey: string;
};

const entries = new Map<string, Entry>();
const listeners = new Set<() => void>();

function notify() {
  for (const listener of listeners) listener();
}

export function subscribeFilePreviews(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function requestKey(request: FilePreviewOpenRequest): string {
  const identity = request.remote
    ? `${request.remote.workspaceId}\0${request.remote.path}`
    : `${request.path}\0${request.backendSessionId}`;
  return `${identity}\0${request.line ?? ""}\0${request.col ?? ""}`;
}

/**
 * A preview opened from an SSH or paired-host terminal shows a file that was just fetched
 * into the local cache. Its cache path is the same on every fetch, so the request is marked
 * `refresh` to re-read the new bytes instead of reusing the live preview of the old ones.
 */
export function previewRequestForSource(
  source: FilePreviewSource,
  request: FilePreviewOpenRequest,
): FilePreviewOpenRequest {
  const remote = isRemoteWorkspaceId(source.workspaceId) || isPairedWorkspaceId(source.workspaceId);
  return remote ? { ...request, refresh: true } : request;
}

/** A relative path is resolved against the session's cwd at open time, so the same token can
 * name a different file after a "cd"; only an absolute request may reuse a live preview. */
function isAbsoluteRequestPath(path: string): boolean {
  return path.startsWith("/") || path.startsWith("\\\\") || /^[a-zA-Z]:[\\/]/.test(path);
}

export function retainFilePreview(
  tabId: string,
  source: FilePreviewSource,
  request: FilePreviewOpenRequest,
): FilePreviewController {
  // `refresh` is a one-shot instruction, never part of the identity or the stored request.
  const { refresh, ...openRequest } = request;
  const key = requestKey(openRequest);
  const existing = entries.get(tabId);
  if (existing) {
    const failed = existing.controller.getState().status === "failed";
    if (refresh === true || existing.requestKey !== key || failed || !isAbsoluteRequestPath(openRequest.path)) {
      existing.requestKey = key;
      void existing.controller.open(source, openRequest);
    }
    return existing.controller;
  }
  const controller = createFilePreviewController({ ownerId: tabId });
  entries.set(tabId, { controller, requestKey: key });
  notify();
  void controller.open(source, openRequest);
  return controller;
}

export function listFilePreviewIds(): string[] {
  return [...entries.keys()];
}

export function getFilePreview(tabId: string): FilePreviewController | null {
  return entries.get(tabId)?.controller ?? null;
}

export async function releaseFilePreview(tabId: string): Promise<void> {
  const entry = entries.get(tabId);
  if (!entry) return;
  entries.delete(tabId);
  notify();
  await entry.controller.close();
}

// A hot reload re-creates this module with an empty registry; release the old controllers so
// their backend capabilities do not outlive the panes that re-open fresh ones.
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    for (const id of [...entries.keys()]) void releaseFilePreview(id);
  });
}
