import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { UPDATE_CHANNEL_STORAGE_KEY } from "./storageKeys";

const updaterRs = readFileSync(resolve(__dirname, "../../../src-tauri/src/ipc/updater.rs"), "utf8");
const tauriConf = JSON.parse(readFileSync(resolve(__dirname, "../../../src-tauri/tauri.conf.json"), "utf8"));

function rustConst(name: string): string {
  const match = updaterRs.match(new RegExp(`pub const ${name}: &str =\\s*"([^"]+)";`));
  if (!match) throw new Error(`${name} not found in ipc/updater.rs`);
  return match[1];
}

const invoke = vi.fn();
const fetchMock = vi.fn();

vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(async () => "2026.1024.1") }));
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  Channel: class {},
  invoke: (command: string, args?: unknown) => invoke(command, args),
}));

async function loadUpdater() {
  vi.resetModules();
  return import("./updater");
}

describe("update channel", () => {
  beforeEach(() => {
    localStorage.clear();
    fetchMock.mockReset();
    fetchMock.mockResolvedValue({ status: 204 });
    vi.stubGlobal("fetch", fetchMock);
    invoke.mockReset();
    // Fake backend with the endpoint choice of ipc/updater.rs: the first endpoint of the channel
    // the UI sends is fetched, and 204 means no update.
    invoke.mockImplementation(async (command: string, args?: { updateChannel?: unknown }) => {
      if (command === "cmd_updater_managed_externally") return false;
      if (command === "cmd_updater_check") {
        const { UPDATE_ENDPOINTS } = await import("./updater");
        const channel = args?.updateChannel === "beta" ? "beta" : "stable";
        const response = await fetch(UPDATE_ENDPOINTS[channel][0]);
        expect(response.status).toBe(204);
        return null;
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("defaults to stable and checks latest.json", async () => {
    const updater = await loadUpdater();
    expect(updater.getUpdateChannel()).toBe("stable");

    await updater.checkForUpdate();

    expect(invoke).toHaveBeenCalledWith("cmd_updater_check", { updateChannel: "stable" });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0][0]).toMatch(/\/latest\.json$/);
  });

  it("treats an unknown stored value as stable", async () => {
    localStorage.setItem(UPDATE_CHANNEL_STORAGE_KEY, "nightly");
    const updater = await loadUpdater();
    expect(updater.getUpdateChannel()).toBe("stable");
  });

  it("with the beta channel selected, the check hits latest-beta.json", async () => {
    const updater = await loadUpdater();
    await updater.setUpdateChannel("beta");

    expect(localStorage.getItem(UPDATE_CHANNEL_STORAGE_KEY)).toBe("beta");
    expect(updater.getUpdateChannel()).toBe("beta");
    expect(invoke).toHaveBeenCalledWith("cmd_updater_check", { updateChannel: "beta" });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0][0]).toMatch(/\/latest-beta\.json$/);
  });

  it("endpoints match the Rust channel table and the configured stable endpoint", async () => {
    const { UPDATE_ENDPOINTS } = await loadUpdater();
    const stable = rustConst("STABLE_UPDATE_ENDPOINT");
    const beta = rustConst("BETA_UPDATE_ENDPOINT");
    expect(tauriConf.plugins.updater.endpoints).toEqual([stable]);
    expect(UPDATE_ENDPOINTS.stable).toEqual([stable]);
    expect(UPDATE_ENDPOINTS.beta).toEqual([beta, stable]);
    expect(beta).toMatch(/\/latest-beta\.json$/);
  });
});
