import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { FilePreviewPane } from "./FilePreviewPane";
import type { FilePreviewController, FilePreviewState } from "../lib/filePreview";

const reload = vi.fn(async () => undefined);
const failed: FilePreviewState = {
  status: "failed",
  generation: 1,
  source: { leafId: "preview-1", sessionId: "preview-1", backendSessionId: "back-1", workspaceId: null },
  request: { path: "/repo/missing.md", backendSessionId: "back-1", line: null, col: null },
  failure: { reason: "MissingFile", message: "gone", details: null },
};
const controller = {
  getState: () => failed,
  subscribe: () => () => undefined,
  reload,
  openExternal: vi.fn(async () => undefined),
  reportFailure: vi.fn(),
  markdownCapability: () => null,
  open: vi.fn(async () => undefined),
  close: vi.fn(async () => undefined),
} satisfies FilePreviewController;

vi.mock("../lib/filePreviewTabRegistry", () => ({
  getFilePreview: () => controller,
  retainFilePreview: vi.fn(),
  subscribeFilePreviews: () => () => undefined,
}));

describe("FilePreviewPane", () => {
  it("shows a retry for a failed preview instead of an empty pane", () => {
    render(
      <FilePreviewPane
        previewId="preview-1"
        path="/repo/missing.md"
        backendSessionId="back-1"
        line={null}
        col={null}
        workspaceId={null}
      />,
    );
    expect(screen.getByTestId("file-preview-failure")).toHaveAttribute("data-reason", "MissingFile");
    screen.getByTestId("file-preview-reload").click();
    expect(reload).toHaveBeenCalledTimes(1);
  });
});
