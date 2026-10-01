import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NativeTerminalPane, resetNativeTerminalPaneForTest } from "./NativeTerminalPane";
import { resetNativeTerminalLifecycleForTest } from "../lib/nativeTerminalLifecycle";
import type { TerminalSession } from "../lib/types";

const tauriCoreMocks = vi.hoisted(() => {
  class MockChannel<T = unknown> {
    id = 1;
    onmessage?: (response: T) => void;
    constructor(onmessage?: (response: T) => void) {
      this.onmessage = onmessage;
    }
  }
  return {
    Channel: MockChannel,
    invoke: vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined),
    isTauri: vi.fn(() => true),
  };
});

const toastMocks = vi.hoisted(() => ({
  error: vi.fn(),
  info: vi.fn(),
  loading: vi.fn(),
  success: vi.fn(),
  dismiss: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: (...args: any[]) => toastMocks.error(...args),
    info: (...args: any[]) => toastMocks.info(...args),
    loading: (...args: any[]) => toastMocks.loading(...args),
    success: (...args: any[]) => toastMocks.success(...args),
    dismiss: (...args: any[]) => toastMocks.dismiss(...args),
  },
}));

const tauriWindowMocks = vi.hoisted(() => {
  let dragDropListeners: Array<(event: { payload: any }) => void> = [];
  const unlisten = vi.fn();
  const onDragDropEvent = vi.fn(async (handler: (event: { payload: any }) => void) => {
    dragDropListeners.push(handler);
    return unlisten;
  });
  return {
    onDragDropEvent,
    unlisten,
    getDragDropListener: () => dragDropListeners.at(-1) ?? null,
    getDragDropListeners: () => [...dragDropListeners],
    reset: () => {
      dragDropListeners = [];
      unlisten.mockClear();
      onDragDropEvent.mockClear();
    },
  };
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCoreMocks.invoke,
  isTauri: tauriCoreMocks.isTauri,
  Channel: tauriCoreMocks.Channel,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onDragDropEvent: tauriWindowMocks.onDragDropEvent,
  }),
}));

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  onNativeTerminalFocus: async () => () => undefined,
  onNativeTerminalPaste: async () => () => undefined,
  onNativeTerminalCopyOrInterrupt: async () => () => undefined,
  onNativeTerminalScrollbar: async () => () => undefined,
  onNativeTerminalInputReceipt: async () => () => undefined,
  setNativeTerminalScrollbarOverlay: async () => undefined,
  setNativeTerminalAttentionFrame: async () => undefined,
}));

const PANE_RECT = {
  x: 10,
  y: 20,
  width: 800,
  height: 600,
  top: 20,
  bottom: 620,
  left: 10,
  right: 810,
  toJSON: () => ({}),
} as DOMRect;

function stubPaneRect(): () => void {
  const original = HTMLElement.prototype.getBoundingClientRect;
  HTMLElement.prototype.getBoundingClientRect = function () {
    return PANE_RECT;
  };
  return () => {
    HTMLElement.prototype.getBoundingClientRect = original;
  };
}

function createSession(
  sessionId = "term-session-1",
  workspaceId = "ssh:9f2c",
): TerminalSession {
  return {
    id: sessionId,
    cwd: "/remote/workspace",
    workspaceId,
    worktree: { wsId: workspaceId, slug: "main" },
    backendSessionId: sessionId,
    lifecycle: "working",
  };
}

describe("NativeTerminalPane remote file drop integration", () => {
  let restorePaneRect: () => void;

  beforeEach(() => {
    restorePaneRect = stubPaneRect();
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);
    tauriCoreMocks.isTauri.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
    tauriWindowMocks.reset();
    resetNativeTerminalLifecycleForTest();
    resetNativeTerminalPaneForTest();
    toastMocks.error.mockReset();
    toastMocks.info.mockReset();
    toastMocks.loading.mockReset();
    toastMocks.dismiss.mockReset();
  });

  afterEach(async () => {
    restorePaneRect();
    await act(async () => {
      cleanup();
    });
    resetNativeTerminalLifecycleForTest();
    resetNativeTerminalPaneForTest();
    vi.restoreAllMocks();
  });

  it("uploads dropped files to remote host and pastes remote paths on ssh session", async () => {
    const session = createSession("term-drop-ssh", "ssh:host-a");

    tauriCoreMocks.invoke.mockImplementation(async (cmd: string, args: any) => {
      if (cmd === "cmd_remote_upload_dropped_files") {
        return {
          platform: "posix",
          files: [
            {
              localPath: "/Users/dev/my file.txt",
              remotePath: "/tmp/ferryx-paste/u1/my file.txt",
              byteLength: 100,
            },
            {
              localPath: "/Users/dev/second.bin",
              remotePath: "/tmp/ferryx-paste/u1/second.bin",
              byteLength: 200,
            },
          ],
        };
      }
      return undefined;
    });

    render(<NativeTerminalPane sessionId="term-drop-ssh" session={session} />);

    await waitFor(() => {
      expect(tauriWindowMocks.onDragDropEvent).toHaveBeenCalled();
    });

    const listener = tauriWindowMocks.getDragDropListener();
    expect(listener).not.toBeNull();

    act(() => {
      listener?.({
        payload: {
          type: "drop",
          paths: ["/Users/dev/my file.txt", "/Users/dev/second.bin"],
          position: { x: 50, y: 100 },
        },
      });
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_remote_upload_dropped_files",
        expect.objectContaining({
          workspaceId: "ssh:host-a",
          paths: ["/Users/dev/my file.txt", "/Users/dev/second.bin"],
        }),
      );
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
        sessionId: "term-drop-ssh",
        text: "'/tmp/ferryx-paste/u1/my file.txt' /tmp/ferryx-paste/u1/second.bin ",
      });
    });

    expect(toastMocks.loading).toHaveBeenCalled();
    expect(toastMocks.dismiss).toHaveBeenCalled();
  });

  it("quotes Windows paths correctly with double quotes when path contains spaces", async () => {
    const session = createSession("term-drop-win", "ssh:host-win");

    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_remote_upload_dropped_files") {
        return {
          platform: "windows",
          files: [
            {
              localPath: "C:\\dev\\report.pdf",
              remotePath: "C:\\Temp\\ferryx-paste\\u1\\my report.pdf",
              byteLength: 500,
            },
          ],
        };
      }
      return undefined;
    });

    render(<NativeTerminalPane sessionId="term-drop-win" session={session} />);

    await waitFor(() => {
      expect(tauriWindowMocks.onDragDropEvent).toHaveBeenCalled();
    });

    const listener = tauriWindowMocks.getDragDropListener();
    act(() => {
      listener?.({
        payload: {
          type: "drop",
          paths: ["C:\\dev\\report.pdf"],
          position: { x: 50, y: 100 },
        },
      });
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
        sessionId: "term-drop-win",
        text: '"C:\\Temp\\ferryx-paste\\u1\\my report.pdf" ',
      });
    });
  });

  it("handles upload failure by showing toast error without pasting", async () => {

    const session = createSession("term-drop-err", "ssh:host-err");

    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_remote_upload_dropped_files") {
        throw { code: "PAYLOAD_TOO_LARGE", message: "File exceeds 30 MiB" };
      }
      return undefined;
    });

    render(<NativeTerminalPane sessionId="term-drop-err" session={session} />);

    await waitFor(() => {
      expect(tauriWindowMocks.onDragDropEvent).toHaveBeenCalled();
    });

    const listener = tauriWindowMocks.getDragDropListener();
    act(() => {
      listener?.({
        payload: {
          type: "drop",
          paths: ["/Users/dev/large.iso"],
          position: { x: 50, y: 100 },
        },
      });
    });

    await waitFor(() => {
      expect(toastMocks.error).toHaveBeenCalledWith(
        expect.stringContaining("File exceeds 30 MiB"),
      );
    });

    const pastes = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd]) => cmd === "cmd_native_terminal_paste",
    );
    expect(pastes).toHaveLength(0);
  });

  it("cancels an in-flight upload through the progress toast and stays silent on UPLOAD_CANCELLED", async () => {
    const session = createSession("term-drop-cancel", "ssh:host-cancel");

    let rejectUpload: ((reason: unknown) => void) | undefined;
    tauriCoreMocks.invoke.mockImplementation((cmd: string) => {
      if (cmd === "cmd_remote_upload_dropped_files") {
        return new Promise((_resolve, reject) => {
          rejectUpload = reject;
        });
      }
      return Promise.resolve(undefined);
    });

    render(<NativeTerminalPane sessionId="term-drop-cancel" session={session} />);

    await waitFor(() => {
      expect(tauriWindowMocks.onDragDropEvent).toHaveBeenCalled();
    });

    const listener = tauriWindowMocks.getDragDropListener();
    act(() => {
      listener?.({
        payload: {
          type: "drop",
          paths: ["/Users/dev/huge.iso"],
          position: { x: 50, y: 100 },
        },
      });
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_remote_upload_dropped_files",
        expect.objectContaining({ workspaceId: "ssh:host-cancel" }),
      );
    });

    const uploadArgs = tauriCoreMocks.invoke.mock.calls.find(
      ([cmd]) => cmd === "cmd_remote_upload_dropped_files",
    )?.[1] as { uploadId: string };
    expect(uploadArgs.uploadId).toBeTruthy();

    // The toast action is the only cancel affordance; it must reach the backend command.
    const action = (toastMocks.loading.mock.calls[0]?.[1] as { action?: { onClick: () => void } })
      ?.action;
    expect(action).toBeDefined();
    act(() => {
      action?.onClick();
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_remote_upload_cancel", {
        uploadId: uploadArgs.uploadId,
      });
    });
    expect(toastMocks.dismiss).toHaveBeenCalledWith(`remote-drop-${uploadArgs.uploadId}`);

    // A cancelled upload must not surface an error toast nor paste a stale path.
    await act(async () => {
      rejectUpload?.({ code: "UPLOAD_CANCELLED", message: "Upload was cancelled" });
    });

    expect(toastMocks.error).not.toHaveBeenCalled();
    expect(
      tauriCoreMocks.invoke.mock.calls.filter(([cmd]) => cmd === "cmd_native_terminal_paste"),
    ).toHaveLength(0);
  });
});
