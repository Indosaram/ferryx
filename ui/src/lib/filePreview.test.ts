import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  createFilePreviewController,
  type FilePreviewDeps,
  type FilePreviewExternalRequest,
} from "./filePreview";
import {
  FILE_PREVIEW_COMMANDS,
  FILE_PREVIEW_LIMITS,
  type FilePreviewChildAsset,
  type FilePreviewOpenRequest,
  type FilePreviewPayload,
  type FilePreviewSource,
} from "./filePreviewTypes";

type Deferred<T> = {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
  readonly reject: (error: unknown) => void;
};

/** Deterministic completion control: every async edge is resolved by the test, never by time. */
function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const SOURCE: FilePreviewSource = {
  leafId: "leaf-1",
  sessionId: "frontend-session-1",
  backendSessionId: "backend-session-1",
  workspaceId: "workspace-1",
};

const REQUEST: FilePreviewOpenRequest = {
  path: "/tmp/notes.txt",
  backendSessionId: "backend-session-1",
  line: 12,
  col: 3,
};

function textPayload(handle: string): FilePreviewPayload {
  return {
    handle,
    displayName: `${handle}.txt`,
    kind: "text",
    byteLength: 42,
    encoding: "utf-8",
    mediaType: null,
    mediaUrl: null,
    text: "hello",
    lineCount: 1,
    target: { line: 1, col: null },
  };
}

function markdownPayload(handle: string): FilePreviewPayload {
  return {
    ...textPayload(handle),
    displayName: `${handle}.md`,
    kind: "markdown",
  };
}

function childAsset(handle: string): FilePreviewChildAsset {
  return {
    handle,
    displayName: `${handle}.png`,
    kind: "image",
    byteLength: 10,
    mediaType: "image/png",
    mediaUrl: `http://127.0.0.1:1/${handle}`,
  };
}

type Harness = {
  readonly controller: ReturnType<typeof createFilePreviewController>;
  readonly invoke: ReturnType<typeof vi.fn>;
  readonly openExternalFile: ReturnType<typeof vi.fn>;
  readonly openExternalUrl: ReturnType<typeof vi.fn>;
  readonly closedHandles: () => readonly string[];
};

function harness(overrides: Partial<FilePreviewDeps> = {}): Harness {
  const invoke = vi.fn(async (command: string, _args: Record<string, unknown> = {}) => {
    if (command === FILE_PREVIEW_COMMANDS.close) return null;
    throw new Error(`unexpected command ${command}`);
  });
  // Released handles are read from the recorded close calls, so a test that
  // supplies its own implementation cannot silently lose the receipt.
  const closed = () =>
    invoke.mock.calls
      .filter((call) => call[0] === FILE_PREVIEW_COMMANDS.close)
      .map((call) => String((call[1] ?? {}).handle));
  const openExternalFile = vi.fn(async (_request: FilePreviewExternalRequest) => {});
  const openExternalUrl = vi.fn(async (_url: string) => {});
  const controller = createFilePreviewController({
    invoke: invoke as unknown as FilePreviewDeps["invoke"],
    openExternalFile,
    openExternalUrl,
    ...overrides,
  });
  return { controller, invoke, openExternalFile, openExternalUrl, closedHandles: closed };
}

describe("filePreview controller", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("starts closed and reports a ready payload after open resolves", async () => {
    const h = harness();
    const open = deferred<FilePreviewPayload>();
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return open.promise;
      return null;
    });

    expect(h.controller.getState().status).toBe("closed");
    const pending = h.controller.open(SOURCE, REQUEST);
    expect(h.controller.getState().status).toBe("loading");

    open.resolve(textPayload("handle-a"));
    await pending;

    const state = h.controller.getState();
    expect(state.status).toBe("ready");
    if (state.status !== "ready") throw new Error("expected ready");
    expect(state.payload.handle).toBe("handle-a");
    expect(state.source).toEqual(SOURCE);
    expect(h.invoke).toHaveBeenCalledWith(FILE_PREVIEW_COMMANDS.open, {
      path: REQUEST.path,
      backendSessionId: REQUEST.backendSessionId,
      line: REQUEST.line,
      col: REQUEST.col,
    });
  });

  it("keeps the newer request when an older open resolves late and revokes the stale handle", async () => {
    const h = harness();
    const first = deferred<FilePreviewPayload>();
    const second = deferred<FilePreviewPayload>();
    const opens = [first, second];
    let index = 0;
    h.invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return opens[index++]!.promise;
      if (command === FILE_PREVIEW_COMMANDS.close) return null;
      throw new Error(`unexpected ${command} ${JSON.stringify(args)}`);
    });

    const pendingA = h.controller.open(SOURCE, REQUEST);
    const pendingB = h.controller.open(SOURCE, { ...REQUEST, path: "/tmp/second.txt" });

    second.resolve(textPayload("handle-b"));
    await pendingB;
    first.resolve(textPayload("handle-a"));
    await pendingA;

    const state = h.controller.getState();
    if (state.status !== "ready") throw new Error("expected ready");
    expect(state.payload.handle).toBe("handle-b");
    expect(state.request.path).toBe("/tmp/second.txt");
    // The superseded capability is released exactly once and never displayed.
    expect(h.closedHandles()).toEqual(["handle-a"]);
  });

  it("closes a handle that resolves after the modal was closed and stays closed", async () => {
    const h = harness();
    const open = deferred<FilePreviewPayload>();
    h.invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return open.promise;
      if (command === FILE_PREVIEW_COMMANDS.close) return null;
      throw new Error(`unexpected ${command} ${JSON.stringify(args)}`);
    });

    const pending = h.controller.open(SOURCE, REQUEST);
    await h.controller.close();
    open.resolve(textPayload("handle-late"));
    await pending;

    expect(h.controller.getState().status).toBe("closed");
    expect(h.closedHandles()).toEqual(["handle-late"]);
  });

  it("closes each acquired handle exactly once across repeated close calls", async () => {
    const h = harness();
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return textPayload("handle-once");
      return null;
    });

    await h.controller.open(SOURCE, REQUEST);
    await h.controller.close();
    await h.controller.close();

    expect(h.closedHandles()).toEqual(["handle-once"]);
  });

  it("reload releases the previous handle once and reacquires the original request", async () => {
    const h = harness();
    const handles = ["handle-1", "handle-2"];
    let index = 0;
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return textPayload(handles[index++]!);
      return null;
    });

    await h.controller.open(SOURCE, REQUEST);
    const opened = h.controller.getState();
    if (opened.status !== "ready") throw new Error("expected ready");
    const generationBefore = opened.generation;
    await h.controller.reload();

    const state = h.controller.getState();
    if (state.status !== "ready") throw new Error("expected ready");
    expect(state.payload.handle).toBe("handle-2");
    expect(state.request).toEqual(REQUEST);
    expect(state.generation).toBeGreaterThan(generationBefore);
    expect(h.closedHandles()).toEqual(["handle-1"]);
  });

  it("surfaces the machine failure reason without parsing prose", async () => {
    const h = harness();
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) {
        throw {
          code: "UNSUPPORTED",
          message: "file is larger than the preview limit",
          details: { reason: "TooLarge", byteLength: 3_000_000, limit: FILE_PREVIEW_LIMITS.textMaxBytes },
        };
      }
      return null;
    });

    await h.controller.open(SOURCE, REQUEST);

    const state = h.controller.getState();
    if (state.status !== "failed") throw new Error("expected failed");
    expect(state.failure.reason).toBe("TooLarge");
    expect(state.failure.details?.limit).toBe(FILE_PREVIEW_LIMITS.textMaxBytes);
    expect(state.failure.message).toBe("file is larger than the preview limit");
  });

  it("opens the external editor with the original path, session and caret, never the preview handle", async () => {
    const h = harness();
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) {
        return { ...textPayload("handle-x"), target: { line: 1, col: null } };
      }
      return null;
    });

    await h.controller.open(SOURCE, REQUEST);
    const openCallsBefore = h.invoke.mock.calls.filter((call) => call[0] === FILE_PREVIEW_COMMANDS.open).length;
    await h.controller.openExternal();

    expect(h.openExternalFile).toHaveBeenCalledWith({
      path: REQUEST.path,
      backendSessionId: SOURCE.backendSessionId,
      line: REQUEST.line,
      col: REQUEST.col,
    });
    // External opening must never recurse into another preview request.
    const openCallsAfter = h.invoke.mock.calls.filter((call) => call[0] === FILE_PREVIEW_COMMANDS.open).length;
    expect(openCallsAfter).toBe(openCallsBefore);
  });

  it("offers a Markdown capability only for markdown payloads", async () => {
    const h = harness();
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return textPayload("handle-text");
      return null;
    });
    await h.controller.open(SOURCE, REQUEST);
    expect(h.controller.markdownCapability()).toBeNull();

    const md = harness();
    md.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return markdownPayload("handle-md");
      return null;
    });
    await md.controller.open(SOURCE, { ...REQUEST, path: "/tmp/readme.md" });
    const capability = md.controller.markdownCapability();
    expect(capability).not.toBeNull();
    expect(capability?.remainingChildHandles).toBe(FILE_PREVIEW_LIMITS.maxChildHandles);
  });

  it("refuses child image requests past the per-parent handle budget without calling the backend", async () => {
    const h = harness();
    let childIndex = 0;
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return markdownPayload("handle-md");
      if (command === FILE_PREVIEW_COMMANDS.openChild) return childAsset(`child-${childIndex++}`);
      return null;
    });
    await h.controller.open(SOURCE, { ...REQUEST, path: "/tmp/readme.md" });

    for (let i = 0; i < FILE_PREVIEW_LIMITS.maxChildHandles; i++) {
      await h.controller.markdownCapability()!.requestImage(`img-${i}.png`);
    }
    expect(h.controller.markdownCapability()!.remainingChildHandles).toBe(0);

    const childCallsBefore = h.invoke.mock.calls.filter(
      (call) => call[0] === FILE_PREVIEW_COMMANDS.openChild,
    ).length;
    await expect(h.controller.markdownCapability()!.requestImage("overflow.png")).rejects.toThrow();
    const childCallsAfter = h.invoke.mock.calls.filter(
      (call) => call[0] === FILE_PREVIEW_COMMANDS.openChild,
    ).length;
    expect(childCallsAfter).toBe(childCallsBefore);
  });

  it("releases every child handle when the preview closes", async () => {
    const h = harness();
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return markdownPayload("handle-md");
      if (command === FILE_PREVIEW_COMMANDS.openChild) return childAsset("child-0");
      return null;
    });
    await h.controller.open(SOURCE, { ...REQUEST, path: "/tmp/readme.md" });
    await h.controller.markdownCapability()!.requestImage("a.png");
    await h.controller.close();

    expect([...h.closedHandles()].sort()).toEqual(["child-0", "handle-md"]);
  });

  it("revokes a child capability that resolves after the preview was replaced", async () => {
    const h = harness();
    const child = deferred<FilePreviewChildAsset>();
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return markdownPayload("handle-md");
      if (command === FILE_PREVIEW_COMMANDS.openChild) return child.promise;
      return null;
    });
    await h.controller.open(SOURCE, { ...REQUEST, path: "/tmp/readme.md" });

    const pendingChild = h.controller.markdownCapability()!.requestImage("late.png");
    await h.controller.close();
    child.resolve(childAsset("child-late"));

    await expect(pendingChild).rejects.toThrow();
    expect([...h.closedHandles()].sort()).toEqual(["child-late", "handle-md"]);
  });

  it("reports the missing child-document command instead of opening an unbounded path", async () => {
    const h = harness();
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return markdownPayload("handle-md");
      return null;
    });
    await h.controller.open(SOURCE, { ...REQUEST, path: "/tmp/readme.md" });
    const openCallsBefore = h.invoke.mock.calls.filter((call) => call[0] === FILE_PREVIEW_COMMANDS.open).length;

    h.controller.markdownCapability()!.requestDocument("./nested/guide.md");

    const state = h.controller.getState();
    if (state.status !== "ready") throw new Error("expected ready");
    expect(state.notice).toEqual({ kind: "child-document-unsupported", relativePath: "./nested/guide.md" });
    const openCallsAfter = h.invoke.mock.calls.filter((call) => call[0] === FILE_PREVIEW_COMMANDS.open).length;
    expect(openCallsAfter).toBe(openCallsBefore);
  });

  it("only forwards http(s) anchors to the external browser", async () => {
    const h = harness();
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return markdownPayload("handle-md");
      return null;
    });
    await h.controller.open(SOURCE, { ...REQUEST, path: "/tmp/readme.md" });

    h.controller.markdownCapability()!.openExternalUrl("https://example.com/doc");
    expect(h.openExternalUrl).toHaveBeenCalledWith("https://example.com/doc");

    h.openExternalUrl.mockClear();
    for (const hostile of ["javascript:alert(1)", "data:text/html,<script>", "file:///etc/passwd", "ferryx://x"]) {
      h.controller.markdownCapability()!.openExternalUrl(hostile);
    }
    expect(h.openExternalUrl).not.toHaveBeenCalled();
  });

  it("ignores renderer failures reported from a superseded generation", async () => {
    const h = harness();
    const handles = ["handle-1", "handle-2"];
    let index = 0;
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return textPayload(handles[index++]!);
      return null;
    });

    await h.controller.open(SOURCE, REQUEST);
    const first = h.controller.getState();
    if (first.status !== "ready") throw new Error("expected ready");
    const staleGeneration = first.generation;
    await h.controller.open(SOURCE, { ...REQUEST, path: "/tmp/other.txt" });

    h.controller.reportFailure(staleGeneration, { reason: "UnsupportedFormat", message: "stale", details: null });
    expect(h.controller.getState().status).toBe("ready");

    const current = h.controller.getState();
    if (current.status !== "ready") throw new Error("expected ready");
    h.controller.reportFailure(current.generation, { reason: "UnsupportedFormat", message: "live", details: null });
    const after = h.controller.getState();
    if (after.status !== "failed") throw new Error("expected failed");
    expect(after.failure.message).toBe("live");
  });

  it("notifies subscribers on each state transition and stops after unsubscribe", async () => {
    const h = harness();
    h.invoke.mockImplementation(async (command: string) => {
      if (command === FILE_PREVIEW_COMMANDS.open) return textPayload("handle-sub");
      return null;
    });
    const listener = vi.fn();
    const unsubscribe = h.controller.subscribe(listener);

    await h.controller.open(SOURCE, REQUEST);
    expect(listener.mock.calls.length).toBeGreaterThanOrEqual(2);

    unsubscribe();
    const before = listener.mock.calls.length;
    await h.controller.close();
    expect(listener.mock.calls.length).toBe(before);
  });
});
