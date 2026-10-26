import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TERMINAL_FILE_LINK_ACTION_EVENT } from "../lib/terminalLinkTarget";
import { TerminalFileLinkActions } from "./TerminalFileLinkActions";

vi.mock("../lib/terminalLinkTarget", () => ({
  TERMINAL_FILE_LINK_ACTION_EVENT: "ferryx:terminal-file-link-actions",
}));

const mockCopyTextToClipboard = vi.fn().mockResolvedValue(true);
vi.mock("../lib/clipboard", () => ({
  copyTextToClipboard: (text: string) => mockCopyTextToClipboard(text),
}));

const mockRevealPath = vi.fn().mockResolvedValue(undefined);
vi.mock("../lib/tauri", () => ({
  revealPath: (path: string) => mockRevealPath(path),
}));

const mockInvoke = vi.fn().mockResolvedValue(true);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: unknown) => mockInvoke(command, args),
}));

const mockToast = {
  custom: vi.fn((renderFn: (id: string) => ReactNode, _options?: any) => {
    render(renderFn("custom-toast-id") as React.ReactElement);
    return "custom-toast-id";
  }),
  dismiss: vi.fn(),
  error: vi.fn(),
  success: vi.fn(),
};

vi.mock("./ui/sonner", () => ({
  toast: {
    custom: (renderFn: (id: string) => ReactNode, options?: any) =>
      mockToast.custom(renderFn, options),
    dismiss: (id?: any) => mockToast.dismiss(id),
    error: (...args: any[]) => mockToast.error(...args),
    success: (...args: any[]) => mockToast.success(...args),
  },
}));

describe("TerminalFileLinkActions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockCopyTextToClipboard.mockResolvedValue(true);
    mockRevealPath.mockResolvedValue(undefined);
    mockInvoke.mockResolvedValue(true);
  });

  afterEach(() => {
    cleanup();
  });

  it("renders file actions and calls open(false) when Open is clicked", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "file", path: "/path/to/src/main.rs", line: 42, col: 10 },
          open,
        },
      }),
    );

    expect(mockToast.custom).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Open terminal file")).toBeInTheDocument();
    expect(screen.getByText("/path/to/src/main.rs:42:10")).toBeInTheDocument();

    const openButton = screen.getByRole("menuitem", { name: "Open" });
    fireEvent.click(openButton);

    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
    expect(open).toHaveBeenCalledWith(false);
  });

  it("opens an unresolved file with the system app override when Open with default app is clicked", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "file", path: "/path/to/src/main.rs" },
          open,
        },
      }),
    );

    const defaultAppButton = screen.getByRole("menuitem", {
      name: "Open with default app",
    });
    fireEvent.click(defaultAppButton);

    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
    expect(open).toHaveBeenCalledWith(true, { editor: "system" });
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("offers only Open and Copy path for a remote file and opens it through detail.open", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "file", path: "/srv/app/out/run.png" },
          open,
          remote: true,
        },
      }),
    );

    expect(screen.getByRole("menuitem", { name: "Open" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Copy path" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Reveal in folder" })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: "Open with default app" })).toBeNull();

    fireEvent.click(screen.getByRole("menuitem", { name: "Open" }));
    expect(open).toHaveBeenCalledWith(false);
    expect(mockRevealPath).not.toHaveBeenCalled();
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("opens a resolved local file with the system app, bypassing the configured editor", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: {
            type: "file",
            path: "assets/foo.png",
            absolutePath: "/workspace/project/assets/foo.png",
          },
          open,
        },
      }),
    );

    fireEvent.click(screen.getByRole("menuitem", { name: "Open with default app" }));

    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("cmd_open_file_path", {
        path: "/workspace/project/assets/foo.png",
        editor: "system",
      });
    });
    expect(open).not.toHaveBeenCalled();
  });

  it("copies path when Copy path is clicked", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "file", path: "/path/to/src/main.rs" },
          open,
        },
      }),
    );

    const copyPathButton = screen.getByRole("menuitem", { name: "Copy path" });
    fireEvent.click(copyPathButton);

    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
    expect(mockCopyTextToClipboard).toHaveBeenCalledWith("/path/to/src/main.rs");
  });

  it("calls revealPath when Reveal in folder is clicked", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "file", path: "/path/to/src/main.rs" },
          open,
        },
      }),
    );

    const revealButton = screen.getByRole("menuitem", { name: "Reveal in folder" });
    fireEvent.click(revealButton);

    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
    expect(mockRevealPath).toHaveBeenCalledWith("/path/to/src/main.rs");
  });

  it("renders url actions with Copy link and Open in browser", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "url", target: "https://example.com/docs" },
          open,
        },
      }),
    );

    expect(mockToast.custom).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Open terminal link")).toBeInTheDocument();
    expect(screen.getByText("https://example.com/docs")).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Copy link" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Copy path" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Reveal in folder" })).not.toBeInTheDocument();

    const copyLinkButton = screen.getByRole("menuitem", { name: "Copy link" });
    fireEvent.click(copyLinkButton);

    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
    expect(mockCopyTextToClipboard).toHaveBeenCalledWith("https://example.com/docs");
  });

  it("calls open(true) when Open in browser is clicked for url token", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "url", target: "https://example.com/docs" },
          open,
        },
      }),
    );

    const browserButton = screen.getByRole("menuitem", { name: "Open in browser" });
    fireEvent.click(browserButton);

    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
    expect(open).toHaveBeenCalledWith(true);
  });

  it("dismisses toast when Escape key is pressed on dialog", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "file", path: "/path/to/file.txt" },
          open,
        },
      }),
    );

    const dialog = screen.getByRole("dialog", { name: "Terminal link actions" });
    fireEvent.keyDown(dialog, { key: "Escape" });

    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
  });

  it("dismisses toast when close button is clicked", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "file", path: "/path/to/file.txt" },
          open,
        },
      }),
    );

    const closeButton = screen.getByLabelText("Close link actions");
    fireEvent.click(closeButton);

    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
  });

  it("ignores events with missing detail or empty path", async () => {
    render(<TerminalFileLinkActions />);

    window.dispatchEvent(new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, { detail: null }));
    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: { token: { type: "file", path: "" }, open: vi.fn() },
      }),
    );
    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: { token: { type: "url", target: "" }, open: vi.fn() },
      }),
    );

    expect(mockToast.custom).not.toHaveBeenCalled();
  });

  it("shows toast.error when revealPath rejects", async () => {
    mockRevealPath.mockRejectedValueOnce(new Error("Path does not exist"));
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "file", path: "/path/to/src/main.rs" },
          open,
        },
      }),
    );

    const revealButton = screen.getByRole("menuitem", { name: "Reveal in folder" });
    fireEvent.click(revealButton);

    await waitFor(() => {
      expect(mockToast.error).toHaveBeenCalledWith("Path does not exist");
    });
    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
  });

  it("copies absolutePath when present for file token", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: {
            type: "file",
            path: "src/main.rs",
            absolutePath: "/workspace/project/src/main.rs",
          },
          open,
        },
      }),
    );

    const copyPathButton = screen.getByRole("menuitem", { name: "Copy path" });
    fireEvent.click(copyPathButton);

    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
    expect(mockCopyTextToClipboard).toHaveBeenCalledWith("/workspace/project/src/main.rs");
    await waitFor(() => {
      expect(mockToast.success).toHaveBeenCalledWith("Path copied");
    });
  });

  it("reveals absolutePath when present for file token", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: {
            type: "file",
            path: "src/main.rs",
            absolutePath: "/workspace/project/src/main.rs",
          },
          open,
        },
      }),
    );

    const revealButton = screen.getByRole("menuitem", { name: "Reveal in folder" });
    fireEvent.click(revealButton);

    expect(mockToast.dismiss).toHaveBeenCalledWith("custom-toast-id");
    expect(mockRevealPath).toHaveBeenCalledWith("/workspace/project/src/main.rs");
  });

  it("shows toast.error when open rejects", async () => {
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockRejectedValue(new Error("Launch error"));

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "file", path: "/path/to/src/main.rs" },
          open,
        },
      }),
    );

    const openButton = screen.getByRole("menuitem", { name: "Open" });
    fireEvent.click(openButton);

    await waitFor(() => {
      expect(mockToast.error).toHaveBeenCalledWith("Launch error");
    });
  });

  it("shows toast.error when copyTextToClipboard returns false", async () => {
    mockCopyTextToClipboard.mockResolvedValueOnce(false);
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "file", path: "/path/to/src/main.rs" },
          open,
        },
      }),
    );

    const copyPathButton = screen.getByRole("menuitem", { name: "Copy path" });
    fireEvent.click(copyPathButton);

    await waitFor(() => {
      expect(mockToast.error).toHaveBeenCalledWith("Could not copy to clipboard.");
    });
  });

  it("shows toast.error when copyTextToClipboard throws", async () => {
    mockCopyTextToClipboard.mockRejectedValueOnce(new Error("Clipboard blocked"));
    render(<TerminalFileLinkActions />);
    const open = vi.fn().mockResolvedValue(undefined);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: {
          token: { type: "url", target: "https://example.com" },
          open,
        },
      }),
    );

    const copyLinkButton = screen.getByRole("menuitem", { name: "Copy link" });
    fireEvent.click(copyLinkButton);

    await waitFor(() => {
      expect(mockToast.error).toHaveBeenCalledWith("Could not copy to clipboard.");
    });
  });

  it("keeps the terminal visible and interactive while the non-modal popover is open", () => {
    render(<TerminalFileLinkActions />);
    window.dispatchEvent(
      new CustomEvent(TERMINAL_FILE_LINK_ACTION_EVENT, {
        detail: { token: { type: "file", path: "/path/to/src/main.rs" }, open: vi.fn() },
      }),
    );

    const dialog = screen.getByRole("dialog", { name: "Terminal link actions" });
    // nativeTerminalVisibility treats an opted-out dialog as non-yielding.
    expect(dialog.closest('[data-native-terminal-yield="off"]')).toBe(dialog);
  });
});
