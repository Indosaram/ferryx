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
});
