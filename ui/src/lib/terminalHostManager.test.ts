import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  createDeferred,
  createSession,
  mocks,
  setupTerminalHostTestEnv,
} from "./terminalHostManagerTestHelper";
import { terminalHostManager } from "./terminalHostManager";
import * as terminalSettings from "./terminalSettings";

describe("terminalHostManager core lifecycle & race conditions", () => {
  beforeEach(() => {
    setupTerminalHostTestEnv();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders Ferryx branding in non-Tauri preview banner", async () => {
    const session = createSession("sess-banner");
    await terminalHostManager.getOrCreate(session, true);

    const bannerText = mocks.terminalWrites.join("");
    expect(bannerText).toContain("Ferryx");
    expect(bannerText).not.toContain("rorca");
    terminalHostManager.destroy("sess-banner");
  });

  it("fires onBell and onTitleChange exactly once and does not fire stale initial closures when cleared", async () => {
    const session = createSession("sess-events");
    const onBell1 = vi.fn();
    const onTitle1 = vi.fn();

    await terminalHostManager.getOrCreate(session, true, onBell1, onTitle1);

    mocks.triggerBell();
    expect(onBell1).toHaveBeenCalledTimes(1);

    mocks.triggerTitleChange("new title 1");
    expect(onTitle1).toHaveBeenCalledTimes(1);
    expect(onTitle1).toHaveBeenCalledWith("new title 1");

    // Re-register with new callbacks (e.g. component re-render)
    const onBell2 = vi.fn();
    const onTitle2 = vi.fn();
    await terminalHostManager.getOrCreate(session, true, onBell2, onTitle2);

    mocks.triggerBell();
    expect(onBell1).toHaveBeenCalledTimes(1); // Should not fire stale callback
    expect(onBell2).toHaveBeenCalledTimes(1);

    mocks.triggerTitleChange("new title 2");
    expect(onTitle1).toHaveBeenCalledTimes(1); // Should not fire stale callback
    expect(onTitle2).toHaveBeenCalledTimes(1);
    expect(onTitle2).toHaveBeenCalledWith("new title 2");

    // When cleared, neither stale callback should fire
    await terminalHostManager.getOrCreate(session, true, undefined, undefined);
    mocks.triggerBell();
    expect(onBell1).toHaveBeenCalledTimes(1);
    expect(onBell2).toHaveBeenCalledTimes(1);

    mocks.triggerTitleChange("new title 3");
    expect(onTitle1).toHaveBeenCalledTimes(1);
    expect(onTitle2).toHaveBeenCalledTimes(1);

    terminalHostManager.destroy("sess-events");
  });

  it("integrates search addon into terminal instance and disposes it on destroy", async () => {
    const session = createSession("sess-search");
    const unreg = terminalHostManager.registerVisible("sess-search");
    const instance = await terminalHostManager.getOrCreate(session, true);

    expect(instance.searchAddon).toBeDefined();
    expect(instance.terminal.loadAddon).toHaveBeenCalledWith(instance.searchAddon);

    unreg();
    terminalHostManager.destroy("sess-search");
    expect(mocks.searchAddonDisposed).toHaveBeenCalledTimes(1);
  });

  it("disposes stale renderer and does not resurrect instances or LRU when destroy occurs during pending creation", async () => {
    const deferredPrefs = createDeferred<typeof terminalSettings.FALLBACK_PREFERENCES>();
    const prefsSpy = vi.spyOn(terminalSettings, "fetchCachedNativePreferences").mockImplementation(() => deferredPrefs.promise);

    const session = createSession("sess-race-destroy", "backend-race-destroy");

    try {
      // 1. Initiate getOrCreate while preference resolution (renderer creation) is pending
      const pendingCreation = terminalHostManager.getOrCreate(session, true);

      // 2. Destroy session while creation is awaiting
      terminalHostManager.destroy("sess-race-destroy");

      // Verify not yet in instances
      expect(terminalHostManager.getInstance("sess-race-destroy")).toBeUndefined();

      // 3. Resolve the delayed creation promise
      deferredPrefs.resolve(terminalSettings.FALLBACK_PREFERENCES);
      const staleInstance = await pendingCreation;

      // 4. Stale instance must be disposed and not populate instances or cache
      expect(terminalHostManager.getInstance("sess-race-destroy")).toBeUndefined();
      expect(mocks.terminalDisposed).toHaveBeenCalledTimes(1);
      expect(mocks.searchAddonDisposed).toHaveBeenCalledTimes(1);

      // 5. Subsequent legitimate getOrCreate for the same session ID succeeds with a fresh instance
      prefsSpy.mockResolvedValue(terminalSettings.FALLBACK_PREFERENCES);
      const freshInstance = await terminalHostManager.getOrCreate(session, true);
      expect(freshInstance).toBeDefined();
      expect(freshInstance).not.toBe(staleInstance);
      expect(terminalHostManager.getInstance("sess-race-destroy")).toBe(freshInstance);
    } finally {
      prefsSpy.mockRestore();
      terminalHostManager.destroy("sess-race-destroy");
    }
  });

  it("handles overlapping destroy and new getOrCreate before stale creation resolves without erasing newer pending creation", async () => {
    const deferredPrefs1 = createDeferred<typeof terminalSettings.FALLBACK_PREFERENCES>();
    const deferredPrefs2 = createDeferred<typeof terminalSettings.FALLBACK_PREFERENCES>();

    const prefsSpy = vi.spyOn(terminalSettings, "fetchCachedNativePreferences")
      .mockImplementationOnce(() => deferredPrefs1.promise)
      .mockImplementationOnce(() => deferredPrefs2.promise);

    const session = createSession("sess-overlap-destroy", "backend-overlap-destroy");

    try {
      // 1. Initiate first getOrCreate
      const firstCreation = terminalHostManager.getOrCreate(session, true);

      // 2. Destroy session while first creation is pending
      terminalHostManager.destroy("sess-overlap-destroy");

      // 3. Initiate second getOrCreate before first creation resolves
      const secondCreation = terminalHostManager.getOrCreate(session, true);

      // 4. Resolve first (stale) creation
      deferredPrefs1.resolve(terminalSettings.FALLBACK_PREFERENCES);
      const staleInstance = await firstCreation;

      // Stale instance must be disposed, not in instances
      expect(terminalHostManager.getInstance("sess-overlap-destroy")).toBeUndefined();
      expect(mocks.terminalDisposed).toHaveBeenCalled();

      // 5. Resolve second (legitimate) creation
      deferredPrefs2.resolve(terminalSettings.FALLBACK_PREFERENCES);
      const newInstance = await secondCreation;

      expect(newInstance).toBeDefined();
      expect(newInstance).not.toBe(staleInstance);
      expect(terminalHostManager.getInstance("sess-overlap-destroy")).toBe(newInstance);
    } finally {
      prefsSpy.mockRestore();
      terminalHostManager.destroy("sess-overlap-destroy");
    }
  });
});
