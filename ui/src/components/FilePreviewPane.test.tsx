import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { FilePreviewPane } from "./FilePreviewPane";
import type { FilePreviewController, FilePreviewState } from "../lib/filePreview";

const reload = vi.fn(async () => undefined);
let failed: FilePreviewState = {
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
vi.mock("../lib/tauri", () => ({ openPermissionsSystemSettings: vi.fn(async () => undefined) }));

describe("FilePreviewPane", () => {
  afterEach(cleanup);

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
    expect(screen.getByText("This file no longer exists.")).toBeInTheDocument();
    screen.getByTestId("file-preview-reload").click();
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("offers the Full Disk Access path when macOS blocks the read", () => {
    failed = {
      ...failed,
      failure: { reason: "PermissionDenied", message: "EPERM", details: { reason: "PermissionDenied", fullDiskAccessRequired: true } },
    } as FilePreviewState;
    render(
      <FilePreviewPane
        previewId="preview-1"
        path="/Users/u/Desktop/a.md"
        backendSessionId="back-1"
        line={null}
        col={null}
        workspaceId={null}
      />,
    );
    expect(screen.getByTestId("file-preview-fda-explainer")).toBeInTheDocument();
    expect(screen.getByTestId("file-preview-open-fda-settings")).toBeInTheDocument();
  });
});
