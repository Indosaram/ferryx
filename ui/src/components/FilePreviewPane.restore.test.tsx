import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { FilePreviewPane } from "./FilePreviewPane";
import { releaseFilePreview } from "../lib/filePreviewTabRegistry";

const open = vi.fn(async () => undefined);
const loadingState = { status: "loading" as const };

vi.mock("../lib/filePreview", () => ({
  createFilePreviewController: () => ({
    getState: () => loadingState,
    subscribe: () => () => undefined,
    open,
    reload: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
    openExternal: vi.fn(async () => undefined),
    markdownCapability: () => null,
    reportFailure: vi.fn(),
  }),
}));

describe("FilePreviewPane restore", () => {
  afterEach(async () => {
    await releaseFilePreview("preview-restored");
    open.mockClear();
  });

  it("creates a controller for a restored pane and leaves the empty state", async () => {
    render(
      <FilePreviewPane
        previewId="preview-restored"
        path="/repo/readme.md"
        backendSessionId="back-1"
        line={4}
        col={2}
        workspaceId="ws-1"
      />,
    );
    await waitFor(() => {
      expect(screen.queryByTestId("file-preview-pane-missing")).toBeNull();
      expect(screen.getByTestId("file-preview-loading")).toBeInTheDocument();
    });
    expect(open).toHaveBeenCalledWith(
      expect.objectContaining({ backendSessionId: "back-1", workspaceId: "ws-1" }),
      expect.objectContaining({ path: "/repo/readme.md", line: 4, col: 2 }),
    );
  });
});
