import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { FilePreviewPane } from "./FilePreviewPane";
import type { FilePreviewController, FilePreviewState } from "../lib/filePreview";
import type { FilePreviewPayload } from "../lib/filePreviewTypes";

const mocks = vi.hoisted(() => ({ controller: null as unknown }));

vi.mock("./FilePreviewText", () => ({ FilePreviewText: () => null }));
vi.mock("../lib/filePreviewCommands", () => ({
  checkFilePreviewChanged: vi.fn(),
}));
vi.mock("../lib/filePreviewTabRegistry", () => ({
  getFilePreview: () => mocks.controller as FilePreviewController | null,
  retainFilePreview: vi.fn(),
  subscribeFilePreviews: () => () => undefined,
}));

import { checkFilePreviewChanged } from "../lib/filePreviewCommands";

const mockCheckChanged = vi.mocked(checkFilePreviewChanged);

describe("FilePreviewPane chrome", () => {
  const readyPayload: FilePreviewPayload & { resolvedPath?: string | null } = {
    handle: "h1",
    displayName: "a.txt",
    kind: "text",
    byteLength: 1,
    encoding: "utf-8",
    mediaType: null,
    mediaUrl: null,
    text: "a",
    lineCount: 1,
    target: null,
    resolvedPath: "/repo/a.txt",
  };

  const reload = vi.fn(async () => undefined);
  const openExternal = vi.fn(async () => undefined);

  const readyState: FilePreviewState = {
    status: "ready",
    generation: 1,
    source: {
      leafId: "preview-1",
      sessionId: "preview-1",
      backendSessionId: "back-1",
      workspaceId: null,
    },
    request: {
      path: "a.txt",
      backendSessionId: "back-1",
      line: null,
      col: null,
    },
    payload: readyPayload,
    remainingChildHandles: 32,
    notice: null,
  };

  let listeners: Array<() => void> = [];
  let currentState: FilePreviewState = readyState;

  const controller = {
    getState: () => currentState,
    subscribe: (listener: () => void) => {
      listeners.push(listener);
      return () => {
        listeners = listeners.filter((l) => l !== listener);
      };
    },
    reload,
    openExternal,
    reportFailure: vi.fn(),
    markdownCapability: () => null,
    open: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
  } satisfies FilePreviewController;

  beforeEach(() => {
    mocks.controller = controller;
  });

  afterEach(() => {
    cleanup();
  });

  it("shows resolvedPath in header path and has title attribute", () => {
    currentState = readyState;
    render(
      <FilePreviewPane
        previewId="preview-1"
        path="a.txt"
        backendSessionId="back-1"
        line={null}
        col={null}
        workspaceId={null}
      />,
    );

    const pathSpan = screen.getByTestId("file-preview-path");
    expect(pathSpan).toHaveTextContent("/repo/a.txt");
    expect(pathSpan).toHaveAttribute("title", "/repo/a.txt");
  });

  it("calls controller.reload when header reload button is clicked", () => {
    reload.mockClear();
    currentState = readyState;
    render(
      <FilePreviewPane
        previewId="preview-1"
        path="a.txt"
        backendSessionId="back-1"
        line={null}
        col={null}
        workspaceId={null}
      />,
    );

    const reloadButton = screen.getByTestId("file-preview-header-reload");
    reloadButton.click();
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("displays changed banner on window focus when checkFilePreviewChanged resolves true, and banner reload calls controller.reload", async () => {
    reload.mockClear();
    currentState = readyState;
    mockCheckChanged.mockReset();
    mockCheckChanged.mockResolvedValue(true);

    render(
      <FilePreviewPane
        previewId="preview-1"
        path="a.txt"
        backendSessionId="back-1"
        line={null}
        col={null}
        workspaceId={null}
      />,
    );

    expect(screen.queryByTestId("file-preview-changed-banner")).toBeNull();

    window.dispatchEvent(new Event("focus"));

    const banner = await screen.findByTestId("file-preview-changed-banner");
    expect(banner).toBeInTheDocument();
    expect(banner).toHaveTextContent("This file changed on disk.");

    const bannerReload = screen.getByTestId("file-preview-changed-reload");
    bannerReload.click();
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("reopens the preview when the change poll reports an expired handle", async () => {
    reload.mockClear();
    currentState = readyState;
    mockCheckChanged.mockReset();
    const expired = Promise.reject({ code: "INVALID_ARGUMENT", message: "gone", details: { reason: "ExpiredHandle" } });
    expired.catch(() => undefined);
    mockCheckChanged.mockReturnValue(expired);

    render(
      <FilePreviewPane
        previewId="preview-1"
        path="a.txt"
        backendSessionId="back-1"
        line={null}
        col={null}
        workspaceId={null}
      />,
    );

    window.dispatchEvent(new Event("focus"));
    await waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
  });

  it("does not show banner when checkFilePreviewChanged resolves false", async () => {
    currentState = readyState;
    mockCheckChanged.mockReset();
    let resolveChangePromise: (value: boolean) => void = () => {};
    const changePromise = new Promise<boolean>((resolve) => {
      resolveChangePromise = resolve;
    });
    mockCheckChanged.mockReturnValue(changePromise);

    render(
      <FilePreviewPane
        previewId="preview-1"
        path="a.txt"
        backendSessionId="back-1"
        line={null}
        col={null}
        workspaceId={null}
      />,
    );

    window.dispatchEvent(new Event("focus"));
    resolveChangePromise(false);
    await changePromise;

    expect(screen.queryByTestId("file-preview-changed-banner")).toBeNull();
  });

  it("keeps a stable hook order when the controller goes from closed to ready", () => {
    currentState = { status: "closed" };
    render(
      <FilePreviewPane
        previewId="preview-1"
        path="a.txt"
        backendSessionId="back-1"
        line={null}
        col={null}
        workspaceId={null}
      />,
    );
    expect(screen.getByTestId("file-preview-pane-pending")).toBeInTheDocument();

    act(() => {
      currentState = readyState;
      for (const listener of [...listeners]) listener();
    });

    expect(screen.getByTestId("file-preview-path")).toHaveTextContent("/repo/a.txt");
  });
});
