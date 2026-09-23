import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  createFilePreviewController,
  type FilePreviewDeps,
} from "../lib/filePreview";
import {
  FILE_PREVIEW_COMMANDS,
  type FilePreviewOpenRequest,
  type FilePreviewPayload,
  type FilePreviewSource,
  type FilePreviewTextProps,
} from "../lib/filePreviewTypes";
import { FilePreviewAudio } from "./FilePreviewAudio";
import { FilePreviewDialog } from "./FilePreviewDialog";
import { FilePreviewPdf } from "./FilePreviewPdf";
import { FilePreviewVideo } from "./FilePreviewVideo";

type Deferred<T> = {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
  readonly reject: (error: unknown) => void;
};

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
  path: "/tmp/노트.txt",
  backendSessionId: "backend-session-1",
  line: 4,
  col: null,
};

function payload(overrides: Partial<FilePreviewPayload> = {}): FilePreviewPayload {
  return {
    handle: "handle-a",
    displayName: "노트.txt",
    kind: "text",
    byteLength: 128,
    encoding: "utf-8",
    mediaType: null,
    mediaUrl: null,
    text: "첫 줄\n두 번째 줄",
    lineCount: 2,
    target: { line: 4, col: null },
    ...overrides,
  };
}

type Controller = ReturnType<typeof createFilePreviewController>;

function makeController(handlers: {
  open?: (request: FilePreviewOpenRequest) => Promise<FilePreviewPayload>;
  openExternalFile?: FilePreviewDeps["openExternalFile"];
} = {}): Controller {
  const invoke = (async (command: string, args: Record<string, unknown>) => {
    if (command === FILE_PREVIEW_COMMANDS.open) {
      return handlers.open
        ? handlers.open(args as unknown as FilePreviewOpenRequest)
        : payload();
    }
    return null;
  }) as unknown as FilePreviewDeps["invoke"];
  return createFilePreviewController({
    invoke,
    openExternalFile: handlers.openExternalFile ?? (async () => {}),
    openExternalUrl: async () => {},
  });
}

/** Drives a controller transition inside React's commit window, with no timers involved. */
async function openPreview(
  controller: Controller,
  source: FilePreviewSource,
  request: FilePreviewOpenRequest,
): Promise<void> {
  await act(async () => {
    await controller.open(source, request);
  });
}

/** A stand-in for a mounted terminal pane leaf with its own focus sink. */
function mountLeaf(leafId: string, label: string): HTMLInputElement {
  const leaf = document.createElement("div");
  leaf.setAttribute("data-leaf-id", leafId);
  const input = document.createElement("input");
  input.setAttribute("aria-label", label);
  leaf.appendChild(input);
  document.body.appendChild(leaf);
  return input;
}

function mountChrome(): HTMLElement {
  const chrome = document.createElement("div");
  chrome.setAttribute("data-workspace-chrome", "true");
  chrome.tabIndex = -1;
  document.body.appendChild(chrome);
  return chrome;
}

function StubTextRenderer(props: FilePreviewTextProps) {
  return (
    <div data-testid="stub-text">
      <span data-testid="stub-generation">{props.generation}</span>
      <span data-testid="stub-text-body">{props.payload.text}</span>
      <span data-testid="stub-markdown">{props.markdown ? "capability" : "none"}</span>
      <button type="button" onClick={() => props.onFailure({ reason: "UnsupportedEncoding", message: "decode", details: null })}>
        fail
      </button>
    </div>
  );
}

const MEDIA_RENDERERS = {
  audio: FilePreviewAudio,
  pdf: FilePreviewPdf,
  video: FilePreviewVideo,
} as const;

function expectReadOnlyPreview(testId: "file-preview-audio-element" | "file-preview-pdf-frame"): HTMLElement {
  const dialog = screen.getByRole("dialog");
  const element = screen.getByTestId(testId);
  expect(dialog.contains(element)).toBe(true);
  expect(element.getAttribute("contenteditable")).toBeNull();
  expect(dialog.querySelector("[contenteditable]")).toBeNull();
  expect(dialog.querySelector("input, textarea, select")).toBeNull();
  expect(screen.queryByRole("button", { name: /^save$/i })).toBeNull();
  expect(screen.queryByTestId("file-preview-video-element")).toBeNull();
  expect(screen.queryByTestId("file-preview-video")).toBeNull();
  return element;
}

describe("FilePreviewDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
    document.body.innerHTML = "";
  });

  it("renders nothing while the controller is closed", () => {
    const controller = makeController();
    render(<FilePreviewDialog controller={controller} />);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it("presents a modal dialog while loading and moves focus into it", async () => {
    const open = deferred<FilePreviewPayload>();
    const controller = makeController({ open: () => open.promise });
    render(<FilePreviewDialog controller={controller} />);

    let pending!: Promise<void>;
    await act(async () => {
      pending = controller.open(SOURCE, REQUEST);
    });

    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(screen.getByTestId("file-preview-loading")).toBeInTheDocument();
    expect(dialog.contains(document.activeElement)).toBe(true);

    await act(async () => {
      open.resolve(payload());
      await pending;
    });
    expect(screen.queryByTestId("file-preview-loading")).toBeNull();
  });

  it("closes on Escape and returns focus to the originating pane leaf", async () => {
    const sink = mountLeaf(SOURCE.leafId, "pane input");
    sink.focus();
    const controller = makeController();
    render(<FilePreviewDialog controller={controller} />);
    await openPreview(controller, SOURCE, REQUEST);

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    expect(controller.getState().status).toBe("closed");
    expect(document.activeElement).toBe(sink);
  });

  it("falls back to workspace chrome when the originating leaf is gone, never another pane", async () => {
    const sink = mountLeaf(SOURCE.leafId, "pane input");
    const otherSink = mountLeaf("leaf-2", "other pane input");
    const chrome = mountChrome();
    sink.focus();
    const controller = makeController();
    render(<FilePreviewDialog controller={controller} />);
    await openPreview(controller, SOURCE, REQUEST);

    sink.closest("[data-leaf-id]")!.remove();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    expect(document.activeElement).toBe(chrome);
    expect(document.activeElement).not.toBe(otherSink);
  });

  it("keeps modal keystrokes out of the terminal sink", async () => {
    const controller = makeController();
    render(<FilePreviewDialog controller={controller} />);
    await openPreview(controller, SOURCE, REQUEST);
    const ptyWrite = vi.fn();
    document.addEventListener("keydown", ptyWrite);
    document.addEventListener("keyup", ptyWrite);

    try {
      const dialog = screen.getByRole("dialog");
      fireEvent.keyDown(dialog, { key: "a" });
      fireEvent.keyDown(dialog, { key: "가" });
      fireEvent.keyUp(dialog, { key: "가" });
      expect(ptyWrite).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener("keydown", ptyWrite);
      document.removeEventListener("keyup", ptyWrite);
    }
  });

  it("traps Tab focus inside the dialog in both directions", async () => {
    const controller = makeController();
    render(<FilePreviewDialog controller={controller} renderers={{ text: StubTextRenderer }} />);
    await openPreview(controller, SOURCE, REQUEST);

    const dialog = screen.getByRole("dialog");
    const focusables = Array.from(
      dialog.querySelectorAll<HTMLElement>('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'),
    );
    expect(focusables.length).toBeGreaterThan(1);
    const first = focusables[0]!;
    const last = focusables[focusables.length - 1]!;

    last.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(document.activeElement).toBe(first);

    first.focus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);
  });

  it("delegates a ready payload to the injected render slot with the live generation", async () => {
    const controller = makeController();
    render(<FilePreviewDialog controller={controller} renderers={{ text: StubTextRenderer }} />);
    await openPreview(controller, SOURCE, REQUEST);

    expect(screen.getByTestId("stub-text-body").textContent).toBe("첫 줄\n두 번째 줄");
    const state = controller.getState();
    if (state.status !== "ready") throw new Error("expected ready");
    expect(screen.getByTestId("stub-generation").textContent).toBe(String(state.generation));
    expect(screen.getByTestId("stub-markdown").textContent).toBe("none");
  });

  it("passes a Markdown capability to the text slot only for markdown payloads", async () => {
    const controller = makeController({ open: async () => payload({ kind: "markdown", displayName: "readme.md" }) });
    render(<FilePreviewDialog controller={controller} renderers={{ markdown: StubTextRenderer }} />);
    await openPreview(controller, SOURCE, { ...REQUEST, path: "/tmp/readme.md" });

    expect(screen.getByTestId("stub-markdown").textContent).toBe("capability");
  });

  it("states plainly that no renderer is registered for a kind instead of faking one", async () => {
    const controller = makeController({ open: async () => payload({ kind: "video", displayName: "clip.mp4", text: null, mediaUrl: "http://127.0.0.1:1/x", mediaType: "video/mp4" }) });
    render(<FilePreviewDialog controller={controller} />);
    await openPreview(controller, SOURCE, { ...REQUEST, path: "/tmp/clip.mp4" });

    expect(screen.getByTestId("file-preview-no-renderer")).toBeInTheDocument();
    expect(screen.queryByTestId("stub-text")).toBeNull();
  });

  it("opens the external editor from the dialog action using the original request", async () => {
    const openExternalFile = vi.fn(async () => {});
    const controller = makeController({ openExternalFile });
    render(<FilePreviewDialog controller={controller} />);
    await openPreview(controller, SOURCE, REQUEST);

    fireEvent.click(screen.getByTestId("file-preview-external"));

    expect(openExternalFile).toHaveBeenCalledWith({
      path: REQUEST.path,
      backendSessionId: SOURCE.backendSessionId,
      line: REQUEST.line,
      col: REQUEST.col,
    });
  });

  it("shows the machine failure reason and reloads the original file on demand", async () => {
    let attempt = 0;
    const controller = makeController({
      open: async () => {
        attempt += 1;
        if (attempt === 1) {
          throw { code: "IO_ERROR", message: "file changed on disk", details: { reason: "FileChanged" } };
        }
        return payload({ handle: "handle-reloaded" });
      },
    });
    render(<FilePreviewDialog controller={controller} renderers={{ text: StubTextRenderer }} />);
    await openPreview(controller, SOURCE, REQUEST);

    expect(screen.getByTestId("file-preview-failure")).toHaveAttribute("data-reason", "FileChanged");
    fireEvent.click(screen.getByTestId("file-preview-reload"));
    await vi.waitFor(() => expect(screen.getByTestId("stub-text")).toBeInTheDocument());
    expect(attempt).toBe(2);
  });

  it("surfaces a renderer-reported failure for the live generation", async () => {
    const controller = makeController();
    render(<FilePreviewDialog controller={controller} renderers={{ text: StubTextRenderer }} />);
    await openPreview(controller, SOURCE, REQUEST);

    fireEvent.click(screen.getByRole("button", { name: "fail" }));

    expect(screen.getByTestId("file-preview-failure")).toHaveAttribute("data-reason", "UnsupportedEncoding");
  });

  it("reports the unavailable child-document command as a notice inside the dialog", async () => {
    const controller = makeController({ open: async () => payload({ kind: "markdown", displayName: "readme.md" }) });
    render(<FilePreviewDialog controller={controller} renderers={{ markdown: StubTextRenderer }} />);
    await openPreview(controller, SOURCE, { ...REQUEST, path: "/tmp/readme.md" });

    act(() => {
      controller.markdownCapability()!.requestDocument("./nested/guide.md");
    });

    await vi.waitFor(() => expect(screen.getByTestId("file-preview-notice")).toBeInTheDocument());
    expect(screen.getByTestId("file-preview-notice")).toHaveAttribute("data-notice", "child-document-unsupported");
  });

  it("renders an audio payload with a read-only audio element, not the video renderer", async () => {
    const mediaUrl = "http://127.0.0.1:9/clip.mp3";
    const controller = makeController({
      open: async () =>
        payload({
          kind: "audio",
          displayName: "clip.mp3",
          text: null,
          lineCount: null,
          encoding: null,
          mediaType: "audio/mpeg",
          mediaUrl,
        }),
    });
    render(<FilePreviewDialog controller={controller} renderers={MEDIA_RENDERERS} />);
    await openPreview(controller, SOURCE, { ...REQUEST, path: "/tmp/clip.mp3" });

    const audio = expectReadOnlyPreview("file-preview-audio-element");
    expect(audio.tagName).toBe("AUDIO");
    expect(audio).toHaveAttribute("src", mediaUrl);
    expect(audio).toHaveAttribute("controls");
  });

  it("renders a pdf payload with a read-only frame, not the video renderer", async () => {
    const mediaUrl = "http://127.0.0.1:9/spec.pdf";
    const controller = makeController({
      open: async () =>
        payload({
          kind: "pdf",
          displayName: "spec.pdf",
          text: null,
          lineCount: null,
          encoding: null,
          mediaType: "application/pdf",
          mediaUrl,
        }),
    });
    render(<FilePreviewDialog controller={controller} renderers={MEDIA_RENDERERS} />);
    await openPreview(controller, SOURCE, { ...REQUEST, path: "/tmp/spec.pdf" });

    const frame = expectReadOnlyPreview("file-preview-pdf-frame");
    expect(frame.tagName).toBe("IFRAME");
    expect(frame).toHaveAttribute("src", mediaUrl);
  });
});
