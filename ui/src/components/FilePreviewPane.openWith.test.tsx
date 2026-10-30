import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { FilePreviewPane } from "./FilePreviewPane";
import type { FilePreviewController, FilePreviewState } from "../lib/filePreview";
import type { FilePreviewPayload } from "../lib/filePreviewTypes";

const mocks = vi.hoisted(() => ({
  controller: null as unknown,
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
}));
vi.mock("./FilePreviewText", () => ({ FilePreviewText: () => null }));
vi.mock("../lib/filePreviewCommands", () => ({
  checkFilePreviewChanged: vi.fn(),
}));
vi.mock("../lib/filePreviewTabRegistry", () => ({
  getFilePreview: () => mocks.controller as FilePreviewController | null,
  retainFilePreview: vi.fn(),
  subscribeFilePreviews: () => () => undefined,
}));

describe("FilePreviewPane openWith and print", () => {
  const readyTextPayload: FilePreviewPayload & { resolvedPath?: string | null } = {
    handle: "h1",
    displayName: "a.txt",
    kind: "text",
    byteLength: 1,
    encoding: "utf-8",
    mediaType: null,
    mediaUrl: null,
    text: "hello text",
    lineCount: 1,
    target: null,
    resolvedPath: "/repo/a.txt",
  };

  const readyMarkdownPayload: FilePreviewPayload & { resolvedPath?: string | null } = {
    handle: "h2",
    displayName: "readme.md",
    kind: "markdown",
    byteLength: 10,
    encoding: "utf-8",
    mediaType: null,
    mediaUrl: null,
    text: "# Hello markdown",
    lineCount: 1,
    target: null,
    resolvedPath: "/repo/readme.md",
  };

  const reload = vi.fn(async () => undefined);
  const openExternal = vi.fn(async () => undefined);

  function createReadyState(
    payload: FilePreviewPayload & { resolvedPath?: string | null },
  ): FilePreviewState {
    return {
      status: "ready",
      generation: 1,
      source: {
        leafId: "preview-1",
        sessionId: "preview-1",
        backendSessionId: "back-1",
        workspaceId: null,
      },
      request: {
        path: payload.displayName,
        backendSessionId: "back-1",
        line: null,
        col: null,
      },
      payload,
      remainingChildHandles: 32,
      notice: null,
    };
  }

  let listeners: Array<() => void> = [];
  let currentState: FilePreviewState = createReadyState(readyTextPayload);

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
    mocks.invoke.mockReset();
    openExternal.mockClear();
    reload.mockClear();
    currentState = createReadyState(readyTextPayload);
  });

  afterEach(() => {
    cleanup();
  });

  it("opens menu and calls cmd_file_open_with_apps with path", async () => {
    mocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_file_open_with_apps") {
        return {
          apps: [{ id: "editor-1", name: "Custom Editor" }],
          supportsChooser: false,
        };
      }
      return undefined;
    });

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

    const openWithButton = screen.getByTestId("file-preview-open-with");
    expect(openWithButton).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("menu")).toBeNull();

    fireEvent.click(openWithButton);

    expect(openWithButton).toHaveAttribute("aria-expanded", "true");
    expect(mocks.invoke).toHaveBeenCalledWith("cmd_file_open_with_apps", {
      path: "/repo/a.txt",
    });

    const appItem = await screen.findByRole("menuitem", { name: "Custom Editor" });
    expect(appItem).toBeInTheDocument();
  });

  it("clicking an app calls cmd_file_open_with with its id and closes the menu", async () => {
    mocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_file_open_with_apps") {
        return {
          apps: [{ id: "editor-1", name: "Custom Editor" }],
          supportsChooser: false,
        };
      }
      if (cmd === "cmd_file_open_with") {
        return undefined;
      }
      return undefined;
    });

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

    fireEvent.click(screen.getByTestId("file-preview-open-with"));
    const appItem = await screen.findByRole("menuitem", { name: "Custom Editor" });

    fireEvent.click(appItem);

    expect(mocks.invoke).toHaveBeenCalledWith("cmd_file_open_with", {
      path: "/repo/a.txt",
      appId: "editor-1",
    });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("supportsChooser shows 'Choose application...' and clicking calls cmd_file_open_with with chooser", async () => {
    mocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_file_open_with_apps") {
        return {
          apps: [{ id: "editor-1", name: "Custom Editor" }],
          supportsChooser: true,
        };
      }
      if (cmd === "cmd_file_open_with") {
        return undefined;
      }
      return undefined;
    });

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

    fireEvent.click(screen.getByTestId("file-preview-open-with"));
    const chooserItem = await screen.findByRole("menuitem", {
      name: "Choose application...",
    });
    expect(chooserItem).toBeInTheDocument();

    fireEvent.click(chooserItem);

    expect(mocks.invoke).toHaveBeenCalledWith("cmd_file_open_with", {
      path: "/repo/a.txt",
      appId: "chooser",
    });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("clicking Default app calls controller.openExternal and closes the menu", async () => {
    mocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_file_open_with_apps") {
        return {
          apps: [],
          supportsChooser: false,
        };
      }
      return undefined;
    });

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

    fireEvent.click(screen.getByTestId("file-preview-open-with"));
    const defaultAppItem = screen.getByRole("menuitem", { name: "Default app" });

    fireEvent.click(defaultAppItem);

    expect(openExternal).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("markdown payload shows Export PDF and clicking calls cmd_webview_print", async () => {
    currentState = createReadyState(readyMarkdownPayload);
    mocks.invoke.mockResolvedValue(undefined);

    render(
      <FilePreviewPane
        previewId="preview-1"
        path="readme.md"
        backendSessionId="back-1"
        line={null}
        col={null}
        workspaceId={null}
      />,
    );

    const exportPdfButton = screen.getByTestId("file-preview-export-pdf");
    expect(exportPdfButton).toBeInTheDocument();
    expect(exportPdfButton).toHaveTextContent("Export PDF");

    fireEvent.click(exportPdfButton);

    expect(mocks.invoke).toHaveBeenCalledWith("cmd_webview_print");
  });

  it("text payload has no Export PDF button", () => {
    currentState = createReadyState(readyTextPayload);

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

    expect(screen.queryByTestId("file-preview-export-pdf")).toBeNull();
  });

  it("status rows are not menu items and the menu supports keyboard navigation and Escape", async () => {
    mocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_file_open_with_apps") {
        return { apps: [{ id: "editor-1", name: "Custom Editor" }], supportsChooser: true };
      }
      return undefined;
    });
    const outerKeyDown = vi.fn();
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
    const trigger = screen.getByTestId("file-preview-open-with");
    fireEvent.click(trigger);
    const defaultApp = screen.getByRole("menuitem", { name: "Default app" });
    expect(defaultApp).toHaveFocus();
    expect(screen.getByText("Loading…")).toHaveAttribute("role", "presentation");
    expect(screen.getAllByRole("menuitem")).toHaveLength(1);

    const editor = await screen.findByRole("menuitem", { name: "Custom Editor" });
    const chooser = screen.getByRole("menuitem", { name: "Choose application..." });
    const menu = screen.getByRole("menu");

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(editor).toHaveFocus();
    fireEvent.keyDown(menu, { key: "End" });
    expect(chooser).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(defaultApp).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowUp" });
    expect(chooser).toHaveFocus();
    fireEvent.keyDown(menu, { key: "Home" });
    expect(defaultApp).toHaveFocus();

    // Escape must stay inside the menu: a document-level handler (the pane or
    // dialog closing on Escape) must never see it.
    document.addEventListener("keydown", outerKeyDown);
    fireEvent.keyDown(defaultApp, { key: "Escape" });
    document.removeEventListener("keydown", outerKeyDown);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
    expect(outerKeyDown).not.toHaveBeenCalled();
  });

  it("renders 'No other apps found' as a presentation row", async () => {
    mocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_file_open_with_apps") return { apps: [], supportsChooser: false };
      return undefined;
    });
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
    fireEvent.click(screen.getByTestId("file-preview-open-with"));
    expect(await screen.findByText("No other apps found")).toHaveAttribute("role", "presentation");
    expect(screen.getAllByRole("menuitem")).toHaveLength(1);
  });
});
