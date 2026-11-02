import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  BrowserDownloadsShelf,
  extractDownloadName,
  formatBytes,
  sanitizeDownloadFilename,
  type DownloadRecord,
} from "./BrowserDownloadsShelf";

let updateListener: ((record: DownloadRecord) => void) | null = null;

const tauriEventMocks = vi.hoisted(() => ({
  listen: vi.fn(async () => () => undefined),
}));

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

const browserTauriMocks = vi.hoisted(() => ({
  extractBrowserErrorCode: vi.fn((err: unknown) => {
    if (!err) return "UNKNOWN_ERROR";
    if (typeof err === "object" && "code" in err && typeof (err as { code: unknown }).code === "string") {
      return (err as { code: string }).code;
    }
    return "UNKNOWN_ERROR";
  }),
  onBrowserDownloadUpdated: vi.fn(async (listener: (record: DownloadRecord) => void) => {
    updateListener = listener;
    return () => {
      if (updateListener === listener) {
        updateListener = null;
      }
    };
  }),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauriCoreMocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauriEventMocks.listen }));
vi.mock("../lib/browserTauri", () => browserTauriMocks);

afterEach(() => {
  cleanup();
  updateListener = null;
  vi.clearAllMocks();
});

describe("BrowserDownloadsShelf helpers", () => {
  it("sanitizes filenames", () => {
    expect(sanitizeDownloadFilename("file.zip?query=1#hash")).toBe("file.zip");
    expect(sanitizeDownloadFilename("../../etc/passwd")).toBe("passwd");
    expect(sanitizeDownloadFilename("")).toBe("download");
  });

  it("extracts download name preferring filepath", () => {
    expect(extractDownloadName("/downloads/report.pdf", "https://example.com/dl")).toBe("report.pdf");
    expect(extractDownloadName("", "https://example.com/image.png")).toBe("image.png");
  });

  it("formats bytes accurately", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024 * 50)).toBe("50.0 KB");
    expect(formatBytes(1024 * 1024 * 5)).toBe("5.0 MB");
  });
});

describe("BrowserDownloadsShelf lifecycle and events", () => {
  beforeEach(() => {
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_browser_download_list") {
        return [];
      }
      return undefined;
    });
  });

  it("renders an in-progress row with a working Cancel button from an updated event", async () => {
    render(<BrowserDownloadsShelf />);

    await waitFor(() => {
      expect(browserTauriMocks.onBrowserDownloadUpdated).toHaveBeenCalled();
      expect(updateListener).not.toBeNull();
    });

    const inProgressRecord: DownloadRecord = {
      id: "dl-100",
      url: "https://example.com/archive.tar.gz",
      filePath: "/Users/test/Downloads/archive.tar.gz",
      status: "inProgress",
      totalBytes: 1000,
      receivedBytes: 450,
      createdAtMs: Date.now(),
      updatedAtMs: Date.now(),
    };

    await act(async () => {
      updateListener?.(inProgressRecord);
    });

    const row = await screen.findByTestId("download-row-dl-100");
    expect(row).toBeDefined();
    expect(screen.getByText("45%")).toBeDefined();
    const cancelButton = screen.getByText("Cancel");
    expect(cancelButton).toBeDefined();

    tauriCoreMocks.invoke.mockImplementation(async (cmd: string, args: unknown) => {
      if (cmd === "cmd_browser_download_cancel") {
        expect(args).toEqual({ downloadId: "dl-100" });
        return true;
      }
      return undefined;
    });

    await act(async () => {
      fireEvent.click(cancelButton);
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_browser_download_cancel", {
      downloadId: "dl-100",
    });
  });

  it("surfaces structured error code when cancel rejects without message fallback", async () => {
    render(<BrowserDownloadsShelf />);

    await waitFor(() => {
      expect(updateListener).not.toBeNull();
    });

    const inProgressRecord: DownloadRecord = {
      id: "dl-200",
      url: "https://example.com/large.iso",
      filePath: "/Users/test/Downloads/large.iso",
      status: "inProgress",
      receivedBytes: 1024,
      createdAtMs: Date.now(),
      updatedAtMs: Date.now(),
    };

    await act(async () => {
      updateListener?.(inProgressRecord);
    });

    const cancelButton = await screen.findByText("Cancel");

    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_browser_download_cancel") {
        throw { code: "DOWNLOAD_ALREADY_FINISHED", message: "Download has finished already" };
      }
      return undefined;
    });

    await act(async () => {
      fireEvent.click(cancelButton);
    });

    await waitFor(() => {
      expect(screen.getByText("DOWNLOAD_ALREADY_FINISHED")).toBeDefined();
      expect(screen.queryByText("Download has finished already")).toBeNull();
      expect(screen.queryByText("Cancel failed")).toBeNull();
    });
  });

  it("surfaces structured error code when listener registration fails", async () => {
    browserTauriMocks.onBrowserDownloadUpdated.mockRejectedValueOnce({
      code: "EVENT_LISTEN_FAILED",
      message: "Tauri event listen rejected",
    });

    render(<BrowserDownloadsShelf />);

    await waitFor(() => {
      expect(screen.getByTestId("downloads-load-error")).toBeDefined();
      expect(screen.getByText("EVENT_LISTEN_FAILED")).toBeDefined();
    });
  });

  const engineRecord = (overrides: Partial<DownloadRecord>): DownloadRecord => ({
    id: "wv-1",
    url: "https://example.com/setup.exe",
    filePath: "/Users/test/Downloads/setup.exe",
    status: "inProgress",
    totalBytes: 100,
    receivedBytes: 40,
    createdAtMs: 1,
    updatedAtMs: 1,
    ...overrides,
  });

  const showRecord = async (record: DownloadRecord) => {
    render(<BrowserDownloadsShelf />);
    await waitFor(() => expect(updateListener).not.toBeNull());
    await act(async () => updateListener?.(record));
    return screen.findByTestId(`download-row-${record.id}`);
  };

  it.each([
    { label: "pausable running", record: { status: "inProgress", canPause: true, canCancel: true }, pause: "Pause download", cancel: true },
    { label: "paused", record: { status: "paused", canPause: true, canCancel: true }, pause: "Resume download", cancel: true },
    { label: "cancel-only", record: { status: "inProgress", canPause: false, canCancel: true }, pause: null, cancel: true },
    { label: "uncontrolled", record: { status: "inProgress", canPause: false, canCancel: false }, pause: null, cancel: false },
    { label: "legacy without flags", record: { status: "inProgress" }, pause: null, cancel: true },
    { label: "completed", record: { status: "completed", canPause: true, canCancel: true }, pause: null, cancel: false },
  ] as const)("renders only the controls a $label download supports", async ({ record, pause, cancel }) => {
    const row = await showRecord(engineRecord(record));
    const pauseButtons = within(row).queryAllByRole("button", { name: /^(Pause|Resume) download$/ });
    expect(pauseButtons.map((button) => button.getAttribute("aria-label"))).toEqual(pause ? [pause] : []);
    expect(within(row).queryByRole("button", { name: "Cancel" }) !== null).toBe(cancel);
  });

  it("pauses, resumes and cancels through the download commands", async () => {
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => (cmd === "cmd_browser_download_list" ? [] : true));
    const row = await showRecord(engineRecord({ canPause: true, canCancel: true }));

    fireEvent.click(within(row).getByRole("button", { name: "Pause download" }));
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_browser_download_pause", { downloadId: "wv-1", pause: true });

    await act(async () => updateListener?.(engineRecord({ status: "paused", canPause: true, canCancel: true, updatedAtMs: 2 })));
    fireEvent.click(within(row).getByRole("button", { name: "Resume download" }));
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_browser_download_pause", { downloadId: "wv-1", pause: false });

    fireEvent.click(within(row).getByRole("button", { name: "Cancel" }));
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_browser_download_cancel", { downloadId: "wv-1" });
  });

  it("keeps a newer download event over an older list response and drops out-of-order list reads", async () => {
    const record = (receivedBytes: number, updatedAtMs: number, id = "dl-300"): DownloadRecord => ({
      id,
      url: "https://example.com/big.bin",
      filePath: "/Users/test/Downloads/big.bin",
      status: "inProgress",
      totalBytes: 100,
      receivedBytes,
      createdAtMs: 1,
      updatedAtMs,
    });
    const pending: Array<(records: DownloadRecord[]) => void> = [];
    tauriCoreMocks.invoke.mockImplementation((cmd: string) => {
      if (cmd === "cmd_browser_download_list") {
        return new Promise<DownloadRecord[]>((resolve) => pending.push(resolve));
      }
      return Promise.resolve(undefined);
    });

    const { rerender } = render(<BrowserDownloadsShelf refreshToken={0} />);
    await waitFor(() => expect(updateListener).not.toBeNull());
    await act(async () => pending[0]([record(10, 10)]));
    expect(await screen.findByText("10%")).toBeDefined();

    // A live event reports newer progress than the registry list that is about to arrive.
    await act(async () => updateListener?.(record(90, 90)));
    expect(screen.getByText("90%")).toBeDefined();

    rerender(<BrowserDownloadsShelf refreshToken={1} />);
    rerender(<BrowserDownloadsShelf refreshToken={2} />);
    await waitFor(() => expect(pending).toHaveLength(3));

    // The superseded read settles first; it is no longer current, so nothing in it may apply.
    // The newest read settles after it with a stale copy of dl-300 that must not beat the event.
    await act(async () => {
      pending[1]([record(95, 95), record(0, 95, "dl-302")]);
      pending[2]([record(50, 50), record(0, 50, "dl-301")]);
    });
    expect(await screen.findByTestId("download-row-dl-301")).toBeDefined();
    expect(screen.queryByTestId("download-row-dl-302")).toBeNull();
    expect(screen.getByText("90%")).toBeDefined();
    expect(screen.queryByText("95%")).toBeNull();
    expect(screen.queryByText("50%")).toBeNull();
  });
});
