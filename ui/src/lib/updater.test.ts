import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

const rustRegistry = readFileSync(resolve(__dirname, "../../../src-tauri/src/lib.rs"), "utf8");
const registeredUpdaterCommands = new Set(
  [...rustRegistry.matchAll(/tauri::generate_handler!\[([\s\S]*?)\]/g)].flatMap((handler) =>
    [...(handler[1] ?? "").matchAll(/crate::ipc::updater::(\w+)\s*,/g)].map((entry) => entry[1]),
  ),
);

// Fake backend for `cmd_updater_check`: resolves to an offer or null.
const check = vi.fn();
const relaunch = vi.fn();
const getVersion = vi.fn();
const isTauri = vi.fn(() => true);
const invoke = vi.fn();

type DownloadEvent =
  | { event: "started"; data: { contentLength: number | null } }
  | { event: "progress"; data: { chunkLength: number } }
  | { event: "finished" }
  | { event: "cancelled" }
  | { event: "stalled" }
  | { event: "error"; data: { message: string } };

type FakeChannel = { onmessage: (event: DownloadEvent) => void };
type DownloadArgs = { expectedVersion: string; onEvent: FakeChannel };

// Fake backend for the todo-14 commands; every call is also logged in order.
const backendLog: string[] = [];
let downloadEvents: DownloadEvent[] = [];
const downloadCommand = vi.fn(async (args: DownloadArgs) => {
  for (const event of downloadEvents) args.onEvent.onmessage(event);
});
const cancelCommand = vi.fn(async () => {});
const installCommand = vi.fn(async () => {});

const UPDATER_COMMANDS: Record<string, (args: unknown) => Promise<unknown>> = {
  cmd_updater_managed_externally: () => invoke("cmd_updater_managed_externally"),
  cmd_updater_check: () => check(),
  cmd_updater_download: (args) => downloadCommand(args as DownloadArgs),
  cmd_updater_cancel: () => cancelCommand(),
  cmd_updater_install: () => installCommand(),
};

vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: (...args: unknown[]) => relaunch(...args) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: (...args: unknown[]) => getVersion(...args) }));
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => isTauri(),
  Channel: class {
    onmessage: (event: DownloadEvent) => void;
    constructor(onmessage: (event: DownloadEvent) => void) {
      this.onmessage = onmessage;
    }
  },
  invoke: async (command: string, args?: unknown) => {
    const handler = UPDATER_COMMANDS[command];
    if (!handler || !registeredUpdaterCommands.has(command)) {
      throw new Error(`Unregistered updater command: ${command}`);
    }
    if (command !== "cmd_updater_managed_externally" && command !== "cmd_updater_check") {
      backendLog.push(command);
    }
    return handler(args);
  },
}));

function resetBackend() {
  backendLog.length = 0;
  downloadEvents = [];
  downloadCommand.mockReset();
  downloadCommand.mockImplementation(async (args: DownloadArgs) => {
    for (const event of downloadEvents) args.onEvent.onmessage(event);
  });
  cancelCommand.mockReset();
  cancelCommand.mockResolvedValue(undefined);
  installCommand.mockReset();
  installCommand.mockResolvedValue(undefined);
}

/**
 * A download the test drives step by step, with the cancel semantics of ipc/updater.rs: the
 * command first checks for the update, and a cancel during that check is a no-op; once the
 * download runs, a cancel ends it with a `cancelled` event before the command returns.
 */
function controlledDownload() {
  let phase: "checking" | "downloading" | "done" = "checking";
  let channel: FakeChannel | null = null;
  let finish: () => void = () => {};
  const finished = new Promise<void>((resolve) => {
    finish = resolve;
  });
  let entered: () => void = () => {};
  const commandEntered = new Promise<void>((resolve) => {
    entered = resolve;
  });
  const end = (event: DownloadEvent) => {
    phase = "done";
    channel?.onmessage(event);
    finish();
  };
  downloadCommand.mockImplementationOnce(async (args: DownloadArgs) => {
    channel = args.onEvent;
    entered();
    await finished;
  });
  cancelCommand.mockImplementation(async () => {
    if (phase === "downloading") end({ event: "cancelled" });
  });
  return {
    commandEntered,
    startDownload(contentLength: number) {
      phase = "downloading";
      channel?.onmessage({ event: "started", data: { contentLength } });
      channel?.onmessage({ event: "progress", data: { chunkLength: contentLength / 4 } });
    },
    beginWithoutEvents() {
      phase = "downloading";
    },
    stall: () => end({ event: "stalled" }),
    complete: () => end({ event: "finished" }),
    get phase() {
      return phase;
    },
  };
}

async function freshModule() {
  vi.resetModules();
  return await import("./updater");
}

function updateHandle(events: DownloadEvent[], options: { failWith?: Error } = {}) {
  downloadEvents = options.failWith
    ? [...events, { event: "error", data: { message: options.failWith.message } }]
    : events;
  return {
    version: "2026.08.26.1",
    body: "Adds in-app updates",
  };
}

describe("updater status machine", () => {
  beforeEach(() => {
    check.mockReset();
    relaunch.mockReset();
    getVersion.mockReset();
    invoke.mockReset();
    invoke.mockResolvedValue(false);
    isTauri.mockReturnValue(true);
    resetBackend();
  });

  it.each(["installer", "native"])("offers backend-checked updates when the channel is %s", async () => {
    // Given: both non-Store channels report updater-owned in the Rust command.
    invoke.mockResolvedValue(false);
    check.mockResolvedValue(updateHandle([]));
    const updater = await freshModule();

    // When
    await updater.checkForUpdate();

    // Then
    expect(invoke).toHaveBeenCalledExactlyOnceWith("cmd_updater_managed_externally");
    expect(check).toHaveBeenCalledTimes(1);
    expect(updater.getUpdateStatus()).toMatchObject({ state: "available", version: "2026.08.26.1" });
  });

  it("starts idle", async () => {
    const updater = await freshModule();
    expect(updater.getUpdateStatus()).toEqual({ state: "idle" });
  });

  it("moves through checking to available when an update exists", async () => {
    check.mockResolvedValue(updateHandle([]));
    const updater = await freshModule();
    const seen: string[] = [];
    updater.subscribeUpdateStatus((status) => seen.push(status.state));

    await updater.checkForUpdate();

    expect(seen).toEqual(["checking", "available"]);
    expect(updater.getUpdateStatus()).toMatchObject({
      state: "available",
      version: "2026.08.26.1",
      releaseNotes: "Adds in-app updates",
    });
  });

  it("returns to idle when the endpoint reports no update", async () => {
    check.mockResolvedValue(null);
    const updater = await freshModule();

    await updater.checkForUpdate();

    expect(updater.getUpdateStatus()).toEqual({ state: "idle" });
  });

  it("surfaces a check failure as an error state instead of throwing", async () => {
    check.mockRejectedValue(new Error("network unreachable"));
    const updater = await freshModule();

    await expect(updater.checkForUpdate()).resolves.toBeUndefined();

    expect(updater.getUpdateStatus()).toEqual({
      state: "error",
      error: "network unreachable",
    });
  });

  it("surfaces a structured backend check error by its message", async () => {
    check.mockRejectedValue({ code: "INTERNAL", message: "endpoint returned 500", details: null });
    const updater = await freshModule();

    await updater.checkForUpdate();

    expect(updater.getUpdateStatus()).toEqual({ state: "error", error: "endpoint returned 500" });
  });

  it("reports monotonic download progress and stages installation without automatic relaunch", async () => {
    check.mockResolvedValue(
      updateHandle([
        { event: "started", data: { contentLength: 100 } },
        { event: "progress", data: { chunkLength: 40 } },
        { event: "progress", data: { chunkLength: 60 } },
        { event: "finished" },
      ]),
    );
    const updater = await freshModule();
    const progress: number[] = [];
    updater.subscribeUpdateStatus((status) => {
      if (status.state === "downloading") progress.push(status.downloadProgress ?? -1);
    });

    await updater.checkForUpdate();
    await updater.downloadAndInstallUpdate();

    expect(progress.length).toBeGreaterThan(0);
    expect(progress).toEqual([...progress].sort((a, b) => a - b));
    expect(progress.at(-1)).toBe(1);
    expect(updater.getUpdateStatus().state).toBe("downloaded");
    expect(relaunch).not.toHaveBeenCalled();
  });

  it("does not relaunch on its own after installation until explicit confirmation entry point is called", async () => {
    check.mockResolvedValue(
      updateHandle([
        { event: "started", data: { contentLength: 100 } },
        { event: "finished" },
      ]),
    );
    const updater = await freshModule();

    await updater.checkForUpdate();
    await updater.downloadAndInstallUpdate();

    expect(updater.getUpdateStatus().state).toBe("downloaded");
    expect(relaunch).not.toHaveBeenCalled();

    await updater.relaunchApp();
    expect(relaunch).toHaveBeenCalledTimes(1);
  });

  it("surfaces a download failure as an error state", async () => {
    check.mockResolvedValue(
      updateHandle([{ event: "started", data: { contentLength: 10 } }], {
        failWith: new Error("disk full"),
      }),
    );
    const updater = await freshModule();

    await updater.checkForUpdate();
    await updater.downloadAndInstallUpdate();

    expect(updater.getUpdateStatus()).toMatchObject({ state: "error", error: "disk full" });
  });

  it("cancel returns to available and allows retry", async () => {
    check.mockResolvedValue(updateHandle([]));
    const updater = await freshModule();
    await updater.checkForUpdate();

    const download = controlledDownload();
    const running = updater.downloadAndInstallUpdate();
    await download.commandEntered;
    download.startDownload(100);
    expect(updater.getUpdateStatus()).toMatchObject({ state: "downloading", downloadProgress: 0.25 });

    await updater.cancelUpdateDownload();
    await running;

    expect(downloadCommand).toHaveBeenCalledWith(
      expect.objectContaining({ expectedVersion: "2026.08.26.1" }),
    );
    expect(backendLog).toEqual(["cmd_updater_download", "cmd_updater_cancel"]);
    expect(updater.getUpdateStatus()).toEqual({
      state: "available",
      version: "2026.08.26.1",
      releaseNotes: "Adds in-app updates",
      lastFailure: "cancelled",
    });

    // The cancelled offer does not block the periodic check, and a retry installs it.
    await updater.checkForUpdate();
    expect(check).toHaveBeenCalledTimes(2);
    downloadEvents = [
      { event: "started", data: { contentLength: 10 } },
      { event: "progress", data: { chunkLength: 10 } },
      { event: "finished" },
    ];
    await updater.downloadAndInstallUpdate();
    expect(updater.getUpdateStatus()).toMatchObject({ state: "downloaded", version: "2026.08.26.1" });
    expect(installCommand).toHaveBeenCalledTimes(1);
  });

  it("stall surfaces a retry", async () => {
    check.mockResolvedValue(updateHandle([]));
    const updater = await freshModule();
    await updater.checkForUpdate();

    const download = controlledDownload();
    const running = updater.downloadAndInstallUpdate();
    await download.commandEntered;
    download.startDownload(100);
    download.stall();
    await running;

    expect(updater.getUpdateStatus()).toEqual({
      state: "available",
      version: "2026.08.26.1",
      releaseNotes: "Adds in-app updates",
      lastFailure: "stalled",
    });
    expect(installCommand).not.toHaveBeenCalled();

    downloadEvents = [
      { event: "started", data: { contentLength: 10 } },
      { event: "progress", data: { chunkLength: 10 } },
      { event: "finished" },
    ];
    await updater.downloadAndInstallUpdate();
    expect(updater.getUpdateStatus().state).toBe("downloaded");
    expect(backendLog).toEqual(["cmd_updater_download", "cmd_updater_download", "cmd_updater_install"]);
  });

  it("install never runs after cancel", async () => {
    check.mockResolvedValue(updateHandle([]));
    const updater = await freshModule();
    await updater.checkForUpdate();

    const download = controlledDownload();
    // The cancel loses the race: the backend has already received the whole body.
    cancelCommand.mockImplementation(async () => {});
    const running = updater.downloadAndInstallUpdate();
    await download.commandEntered;
    download.startDownload(100);
    await updater.cancelUpdateDownload();
    download.complete();
    await running;

    expect(installCommand).not.toHaveBeenCalled();
    // The second cancel discards the downloaded bytes the backend kept for install.
    expect(backendLog).toEqual(["cmd_updater_download", "cmd_updater_cancel", "cmd_updater_cancel"]);
    expect(updater.getUpdateStatus()).toMatchObject({ state: "available", lastFailure: "cancelled" });
  });

  it("re-sends a cancel issued while the backend is still checking for the update", async () => {
    vi.useFakeTimers();
    try {
      check.mockResolvedValue(updateHandle([]));
      const updater = await freshModule();
      await updater.checkForUpdate();

      const download = controlledDownload();
      const running = updater.downloadAndInstallUpdate();
      await download.commandEntered;

      // ipc/updater.rs ignores a cancel while its check() is in flight.
      await updater.cancelUpdateDownload();
      expect(cancelCommand).toHaveBeenCalledTimes(1);
      expect(download.phase).toBe("checking");
      expect(updater.getUpdateStatus()).toMatchObject({ state: "downloading", cancelRequested: true });

      // The download starts but never delivers a byte, so no event will arrive to react to.
      download.beginWithoutEvents();
      await vi.advanceTimersByTimeAsync(updater.CANCEL_RETRY_MS - 1);
      expect(cancelCommand).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(1);
      await running;

      expect(cancelCommand).toHaveBeenCalledTimes(2);
      expect(installCommand).not.toHaveBeenCalled();
      expect(updater.getUpdateStatus()).toMatchObject({ state: "available", lastFailure: "cancelled" });

      // The retry stops once the backend has acknowledged the cancel.
      await vi.advanceTimersByTimeAsync(updater.CANCEL_RETRY_MS * 5);
      expect(cancelCommand).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not cancel once the downloaded update is installing", async () => {
    check.mockResolvedValue(
      updateHandle([
        { event: "started", data: { contentLength: 10 } },
        { event: "progress", data: { chunkLength: 10 } },
        { event: "finished" },
      ]),
    );
    let finishInstall: () => void = () => {};
    let installEntered: () => void = () => {};
    const installStarted = new Promise<void>((resolve) => {
      installEntered = resolve;
    });
    installCommand.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finishInstall = resolve;
          installEntered();
        }),
    );
    const updater = await freshModule();
    await updater.checkForUpdate();

    const running = updater.downloadAndInstallUpdate();
    await installStarted;
    expect(updater.getUpdateStatus()).toMatchObject({ state: "downloading", installing: true });

    await updater.cancelUpdateDownload();
    expect(cancelCommand).not.toHaveBeenCalled();

    finishInstall();
    await running;
    expect(updater.getUpdateStatus().state).toBe("downloaded");
  });

  it("surfaces a refused download request as an error state", async () => {
    check.mockResolvedValue(updateHandle([]));
    downloadCommand.mockRejectedValueOnce({
      code: "UPDATER_VERSION_MISMATCH",
      message: "update check now offers 2026.09.01.1, not 2026.08.26.1",
      details: { expected: "2026.08.26.1", offered: "2026.09.01.1" },
    });
    const updater = await freshModule();
    await updater.checkForUpdate();

    await updater.downloadAndInstallUpdate();

    expect(updater.getUpdateStatus()).toMatchObject({
      state: "error",
      error: "update check now offers 2026.09.01.1, not 2026.08.26.1",
    });
    expect(installCommand).not.toHaveBeenCalled();
  });

  it("refuses to download before an update has been found", async () => {
    const updater = await freshModule();

    await updater.downloadAndInstallUpdate();

    expect(updater.getUpdateStatus().state).toBe("idle");
  });

  it("stops notifying an unsubscribed listener", async () => {
    check.mockResolvedValue(null);
    const updater = await freshModule();
    const listener = vi.fn();
    const unsubscribe = updater.subscribeUpdateStatus(listener);

    unsubscribe();
    await updater.checkForUpdate();

    expect(listener).not.toHaveBeenCalled();
  });

  it("relaunches through the process plugin", async () => {
    const updater = await freshModule();

    await updater.relaunchApp();

    expect(relaunch).toHaveBeenCalledTimes(1);
  });

  it("flushes workspace persistence guards before restarting for an installed update", async () => {
    const updater = await freshModule();
    const callOrder: string[] = [];
    updater.registerWindowCloseGuard(() => {
      callOrder.push("persist-workspace");
    });
    relaunch.mockImplementation(async () => {
      callOrder.push("relaunch");
    });

    await updater.relaunchApp();

    expect(callOrder).toEqual(["persist-workspace", "relaunch"]);
  });

  it("reads the running app version", async () => {
    getVersion.mockResolvedValue("2026.08.25");
    const updater = await freshModule();

    await expect(updater.getCurrentVersion()).resolves.toBe("2026.08.25");
  });

  it("skips the update check when the install is Store-managed", async () => {
    invoke.mockResolvedValue(true);
    const updater = await freshModule();
    const seen: string[] = [];
    updater.subscribeUpdateStatus((status) => seen.push(status.state));

    await updater.checkForUpdate();
    await updater.downloadAndInstallUpdate();

    expect(check).not.toHaveBeenCalled();
    expect(seen).toEqual([]);
    expect(updater.getUpdateStatus()).toEqual({ state: "idle" });
  });

  it("caches the Store-managed probe across checks", async () => {
    invoke.mockResolvedValue(true);
    const updater = await freshModule();

    await updater.checkForUpdate();
    await updater.checkForUpdate();

    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("treats a failed Store-managed probe as updater-owned", async () => {
    invoke.mockRejectedValue(new Error("command not found"));
    check.mockResolvedValue(null);
    const updater = await freshModule();

    await updater.checkForUpdate();

    expect(check).toHaveBeenCalledTimes(1);
    expect(updater.getUpdateStatus()).toEqual({ state: "idle" });
  });
});

describe("periodic update polling", () => {
  beforeEach(() => {
    check.mockReset();
    relaunch.mockReset();
    getVersion.mockReset();
    invoke.mockReset();
    invoke.mockResolvedValue(false);
    isTauri.mockReturnValue(true);
    resetBackend();
  });

  it("checks immediately, then once per interval until stopped", async () => {
    vi.useFakeTimers();
    try {
      check.mockResolvedValue(null);
      const updater = await freshModule();

      const stop = updater.startUpdatePolling(1000);
      await vi.advanceTimersByTimeAsync(0);
      expect(check).toHaveBeenCalledTimes(1);

      await vi.advanceTimersByTimeAsync(1000);
      expect(check).toHaveBeenCalledTimes(2);

      await vi.advanceTimersByTimeAsync(1000);
      expect(check).toHaveBeenCalledTimes(3);

      stop();
      await vi.advanceTimersByTimeAsync(5000);
      expect(check).toHaveBeenCalledTimes(3);
      expect(updater.getUpdateStatus()).toEqual({ state: "idle" });
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps re-checking while an update is offered so a newer release replaces it", async () => {
    vi.useFakeTimers();
    try {
      check.mockResolvedValue(updateHandle([]));
      const updater = await freshModule();

      const stop = updater.startUpdatePolling(1000);
      await vi.advanceTimersByTimeAsync(0);
      expect(updater.getUpdateStatus()).toMatchObject({
        state: "available",
        version: "2026.08.26.1",
      });

      check.mockResolvedValue({ ...updateHandle([]), version: "2026.09.18.1" });
      await vi.advanceTimersByTimeAsync(1000);

      expect(check).toHaveBeenCalledTimes(2);
      expect(updater.getUpdateStatus()).toMatchObject({
        state: "available",
        version: "2026.09.18.1",
      });
      stop();
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not re-check once an update is staged for install", async () => {
    vi.useFakeTimers();
    try {
      check.mockResolvedValue(
        updateHandle([
          { event: "started", data: { contentLength: 10 } },
          { event: "progress", data: { chunkLength: 10 } },
          { event: "finished" },
        ]),
      );
      const updater = await freshModule();

      await updater.checkForUpdate();
      await updater.downloadAndInstallUpdate();
      expect(updater.getUpdateStatus().state).toBe("downloaded");

      const stop = updater.startUpdatePolling(1000);
      await vi.advanceTimersByTimeAsync(5000);

      expect(check).toHaveBeenCalledTimes(1);
      expect(updater.getUpdateStatus().state).toBe("downloaded");
      stop();
    } finally {
      vi.useRealTimers();
    }
  });

  it("never polls outside the desktop shell", async () => {
    vi.useFakeTimers();
    try {
      isTauri.mockReturnValue(false);
      const updater = await freshModule();

      updater.startUpdatePolling(1000);
      await vi.advanceTimersByTimeAsync(5000);

      expect(check).not.toHaveBeenCalled();
      updater.stopUpdatePolling();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("non-Tauri runtime", () => {
  beforeEach(() => {
    check.mockReset();
    relaunch.mockReset();
    getVersion.mockReset();
    isTauri.mockReturnValue(false);
    resetBackend();
  });

  it("never touches the updater plugin outside the desktop shell", async () => {
    const updater = await freshModule();

    await updater.checkForUpdate();
    await updater.downloadAndInstallUpdate();
    await updater.cancelUpdateDownload();
    await updater.relaunchApp();

    expect(check).not.toHaveBeenCalled();
    expect(backendLog).toEqual([]);
    expect(relaunch).not.toHaveBeenCalled();
    expect(updater.getUpdateStatus()).toEqual({ state: "idle" });
  });

  it("reports an unknown version instead of calling into Tauri", async () => {
    const updater = await freshModule();

    await expect(updater.getCurrentVersion()).resolves.toBe(null);
    expect(getVersion).not.toHaveBeenCalled();
  });
});

describe("window close guards", () => {
  it("keeps running every registered guard", async () => {
    const updater = await freshModule();
    const first = vi.fn();
    const second = vi.fn();

    updater.registerWindowCloseGuard(first);
    const remove = updater.registerWindowCloseGuard(second);
    remove();
    await updater.flushCloseGuards();

    expect(first).toHaveBeenCalledTimes(1);
    expect(second).not.toHaveBeenCalled();
  });
});
