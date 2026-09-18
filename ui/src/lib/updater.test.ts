import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

const rustRegistry = readFileSync(resolve(__dirname, "../../../src-tauri/src/lib.rs"), "utf8");
const registeredUpdaterCommands = new Set(
  [...rustRegistry.matchAll(/tauri::generate_handler!\[([\s\S]*?)\]/g)].flatMap((handler) =>
    [...(handler[1] ?? "").matchAll(/crate::ipc::updater::(\w+)\s*,/g)].map((entry) => entry[1]),
  ),
);

const check = vi.fn();
const relaunch = vi.fn();
const getVersion = vi.fn();
const isTauri = vi.fn(() => true);
const invoke = vi.fn();

vi.mock("@tauri-apps/plugin-updater", () => ({ check: (...args: unknown[]) => check(...args) }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: (...args: unknown[]) => relaunch(...args) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: (...args: unknown[]) => getVersion(...args) }));
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => isTauri(),
  invoke: async (command: string) => {
    if (command !== "cmd_updater_managed_externally" || !registeredUpdaterCommands.has(command)) {
      throw new Error(`Unregistered updater command: ${command}`);
    }
    return invoke(command);
  },
}));

async function freshModule() {
  vi.resetModules();
  return await import("./updater");
}

type DownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

function updateHandle(events: DownloadEvent[], options: { failWith?: Error } = {}) {
  return {
    version: "2026.08.26.1",
    body: "Adds in-app updates",
    downloadAndInstall: vi.fn(async (onEvent: (event: DownloadEvent) => void) => {
      for (const event of events) onEvent(event);
      if (options.failWith) throw options.failWith;
    }),
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
  });

  it.each(["installer", "native"])("offers plugin updates when the channel is %s", async () => {
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

  it("reports monotonic download progress and relaunches after installation", async () => {
    check.mockResolvedValue(
      updateHandle([
        { event: "Started", data: { contentLength: 100 } },
        { event: "Progress", data: { chunkLength: 40 } },
        { event: "Progress", data: { chunkLength: 60 } },
        { event: "Finished" },
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
    expect(relaunch).toHaveBeenCalledTimes(1);
  });

  it("surfaces a download failure as an error state", async () => {
    check.mockResolvedValue(
      updateHandle([{ event: "Started", data: { contentLength: 10 } }], {
        failWith: new Error("disk full"),
      }),
    );
    const updater = await freshModule();

    await updater.checkForUpdate();
    await updater.downloadAndInstallUpdate();

    expect(updater.getUpdateStatus()).toMatchObject({ state: "error", error: "disk full" });
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
          { event: "Started", data: { contentLength: 10 } },
          { event: "Progress", data: { chunkLength: 10 } },
          { event: "Finished" },
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
  });

  it("never touches the updater plugin outside the desktop shell", async () => {
    const updater = await freshModule();

    await updater.checkForUpdate();
    await updater.downloadAndInstallUpdate();
    await updater.relaunchApp();

    expect(check).not.toHaveBeenCalled();
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
