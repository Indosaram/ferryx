import { act, cleanup, render } from "@testing-library/react";
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
import { retainFilePreview } from "../lib/filePreviewTabRegistry";

const mockCheckChanged = vi.mocked(checkFilePreviewChanged);
const mockRetain = vi.mocked(retainFilePreview);

const readyPayload: FilePreviewPayload = {
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
  resolvedPath: null,
};

const readyState: FilePreviewState = {
  status: "ready",
  generation: 1,
  source: { leafId: "preview-1", sessionId: "preview-1", backendSessionId: "back-1", workspaceId: null },
  request: { path: "a.txt", backendSessionId: "back-1", line: null, col: null },
  payload: readyPayload,
  remainingChildHandles: 32,
  notice: null,
};

const readyController = {
  getState: () => readyState,
  subscribe: () => () => undefined,
  reload: vi.fn(async () => undefined),
  openExternal: vi.fn(async () => undefined),
  reportFailure: vi.fn(),
  markdownCapability: () => null,
  open: vi.fn(async () => undefined),
  close: vi.fn(async () => undefined),
} satisfies FilePreviewController;

function pane(visible: boolean) {
  return (
    <FilePreviewPane
      previewId="preview-1"
      path="a.txt"
      backendSessionId="back-1"
      line={null}
      col={null}
      workspaceId={null}
      visible={visible}
    />
  );
}

describe("FilePreviewPane visibility", () => {
  beforeEach(() => {
    mocks.controller = null;
    mockRetain.mockClear();
    mockCheckChanged.mockReset();
    mockCheckChanged.mockResolvedValue(false);
  });

  afterEach(cleanup);

  it("does not open a hidden restored tab until it is shown for the first time", () => {
    const { rerender } = render(pane(false));
    expect(mockRetain).not.toHaveBeenCalled();

    rerender(pane(true));
    expect(mockRetain).toHaveBeenCalledTimes(1);
    expect(mockRetain).toHaveBeenCalledWith(
      "preview-1",
      expect.objectContaining({ backendSessionId: "back-1" }),
      expect.objectContaining({ path: "a.txt" }),
    );
  });

  it("ignores window focus while hidden and checks for changes once when shown again", async () => {
    mocks.controller = readyController;
    const { rerender } = render(pane(false));

    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(mockCheckChanged).not.toHaveBeenCalled();

    await act(async () => {
      rerender(pane(true));
    });
    expect(mockCheckChanged).toHaveBeenCalledTimes(1);
    expect(mockCheckChanged).toHaveBeenCalledWith("h1");

    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(mockCheckChanged).toHaveBeenCalledTimes(2);
  });

  it("does not check for changes on first mount of a visible ready tab", async () => {
    mocks.controller = readyController;
    await act(async () => {
      render(pane(true));
    });
    expect(mockCheckChanged).not.toHaveBeenCalled();
  });
});
