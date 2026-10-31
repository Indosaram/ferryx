import { afterEach, describe, expect, it, vi } from "vitest";

const open = vi.fn(async () => undefined);
const close = vi.fn(async () => undefined);
let status: "loading" | "failed" | "ready" = "loading";

vi.mock("./filePreview", () => ({
  createFilePreviewController: () => ({
    getState: () => ({ status }),
    subscribe: () => () => undefined,
    open,
    reload: vi.fn(async () => undefined),
    close,
    openExternal: vi.fn(async () => undefined),
    markdownCapability: () => null,
    reportFailure: vi.fn(),
  }),
}));

const source = {
  leafId: "preview-1",
  sessionId: "preview-1",
  backendSessionId: "back-1",
  workspaceId: null,
};
const request = {
  path: "/repo/readme.md",
  backendSessionId: "back-1",
  line: 1,
  col: 1,
};

describe("filePreviewTabRegistry", () => {
  afterEach(async () => {
    const registry = await import("./filePreviewTabRegistry");
    await registry.releaseFilePreview("preview-1");
    open.mockClear();
    close.mockClear();
    status = "loading";
  });

  it("notifies subscribers when a missing preview is retained", async () => {
    const registry = await import("./filePreviewTabRegistry");
    const seen: Array<string | null> = [];
    registry.subscribeFilePreviews(() => {
      seen.push(registry.getFilePreview("preview-1")?.getState().status ?? null);
    });
    registry.retainFilePreview("preview-1", source, request);
    expect(seen).toEqual(["loading"]);
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("retries open when the same request previously failed", async () => {
    const registry = await import("./filePreviewTabRegistry");
    status = "failed";
    registry.retainFilePreview("preview-1", source, request);
    open.mockClear();
    registry.retainFilePreview("preview-1", source, request);
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("does not reopen a live preview for the same request", async () => {
    const registry = await import("./filePreviewTabRegistry");
    status = "ready";
    registry.retainFilePreview("preview-1", source, request);
    open.mockClear();
    registry.retainFilePreview("preview-1", source, request);
    expect(open).not.toHaveBeenCalled();
  });

  it("re-resolves a relative request on every click, since the session cwd may have moved", async () => {
    const registry = await import("./filePreviewTabRegistry");
    status = "ready";
    const relative = { ...request, path: "readme.md" };
    registry.retainFilePreview("preview-1", source, relative);
    open.mockClear();
    registry.retainFilePreview("preview-1", source, relative);
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("re-reads a re-fetched remote file even though its cache path and request key are unchanged", async () => {
    const registry = await import("./filePreviewTabRegistry");
    status = "ready";
    const remoteSource = { ...source, workspaceId: "ssh:host-1" };
    const cached = { ...request, path: "/cache/remote-files/abc/readme.md" };
    registry.retainFilePreview("preview-1", remoteSource, registry.previewRequestForSource(remoteSource, cached));
    open.mockClear();
    registry.retainFilePreview("preview-1", remoteSource, registry.previewRequestForSource(remoteSource, cached));
    expect(open).toHaveBeenCalledTimes(1);
    // The one-shot flag never reaches the controller (and so never a stored request).
    expect(open.mock.lastCall as unknown[] | undefined).toEqual([remoteSource, cached]);
  });

  it("marks only SSH and paired-host previews for refresh", async () => {
    const registry = await import("./filePreviewTabRegistry");
    expect(registry.previewRequestForSource({ ...source, workspaceId: "ssh:a" }, request).refresh).toBe(true);
    expect(registry.previewRequestForSource({ ...source, workspaceId: "daemon:h:p" }, request).refresh).toBe(true);
    expect(registry.previewRequestForSource(source, request)).toBe(request);
    expect(registry.previewRequestForSource({ ...source, workspaceId: "ws-local" }, request)).toBe(request);
  });

  it("keeps reusing a live local preview, and a refetch landing at a new path still reopens", async () => {
    const registry = await import("./filePreviewTabRegistry");
    status = "ready";
    registry.retainFilePreview("preview-1", source, registry.previewRequestForSource(source, request));
    open.mockClear();
    registry.retainFilePreview("preview-1", source, registry.previewRequestForSource(source, request));
    expect(open).not.toHaveBeenCalled();
    // Windows fallback: the locked copy forces the refetch into a fresh sibling path.
    registry.retainFilePreview("preview-1", source, { ...request, path: "/repo/fresh/readme.md" });
    expect(open).toHaveBeenCalledTimes(1);
  });
});
