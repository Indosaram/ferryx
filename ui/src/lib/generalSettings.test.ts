import { beforeEach, describe, expect, it } from "vitest";

import {
  DEFAULT_GENERAL_SETTINGS,
  GENERAL_SETTINGS_EVENT,
  loadGeneralSettings,
  loadSidebarOpenStartup,
  resetGeneralSettings,
  saveGeneralSettings,
  saveSidebarOpenStartup,
} from "./generalSettings";
import { GENERAL_SETTINGS_STORAGE_KEY, SIDEBAR_OPEN_STORAGE_KEY } from "./storageKeys";

describe("generalSettings", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("loads default settings when storage is empty", () => {
    const settings = loadGeneralSettings();
    expect(settings).toEqual(DEFAULT_GENERAL_SETTINGS);
    expect(settings.confirmCloseTab).toBe(false);
  });

  it("normalizes malformed persisted settings", () => {
    localStorage.setItem(GENERAL_SETTINGS_STORAGE_KEY, JSON.stringify({ confirmCloseTab: "true" }));
    expect(loadGeneralSettings()).toEqual(DEFAULT_GENERAL_SETTINGS);

    localStorage.setItem(GENERAL_SETTINGS_STORAGE_KEY, "null");
    expect(loadGeneralSettings()).toEqual(DEFAULT_GENERAL_SETTINGS);
  });

  it("saves and persists general settings update", () => {
    let observed: unknown = null;
    const listener = (event: Event) => {
      if (event instanceof CustomEvent) observed = event.detail;
    };
    window.addEventListener(GENERAL_SETTINGS_EVENT, listener);

    const updated = saveGeneralSettings({ confirmCloseTab: true });
    try {
      expect(updated.confirmCloseTab).toBe(true);
      expect(localStorage.getItem(GENERAL_SETTINGS_STORAGE_KEY)).toBe(JSON.stringify({ confirmCloseTab: true }));
      expect(observed).toEqual({ confirmCloseTab: true });

      const loaded = loadGeneralSettings();
      expect(loaded.confirmCloseTab).toBe(true);
    } finally {
      window.removeEventListener(GENERAL_SETTINGS_EVENT, listener);
    }
  });

  it("resets settings back to default", () => {
    saveGeneralSettings({ confirmCloseTab: true });
    expect(loadGeneralSettings().confirmCloseTab).toBe(true);

    const reset = resetGeneralSettings();
    expect(reset).toEqual(DEFAULT_GENERAL_SETTINGS);
    expect(loadGeneralSettings().confirmCloseTab).toBe(false);
  });

  it("migrates legacy settings key upon loading", () => {
    localStorage.setItem("rorca.settings.general", JSON.stringify({ confirmCloseTab: true }));
    const settings = loadGeneralSettings();
    expect(settings.confirmCloseTab).toBe(true);
    expect(localStorage.getItem(GENERAL_SETTINGS_STORAGE_KEY)).toBe(JSON.stringify({ confirmCloseTab: true }));
  });

  it("loads and persists sidebar open startup preference", () => {
    expect(loadSidebarOpenStartup()).toBe(true);

    saveSidebarOpenStartup(false);
    expect(localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY)).toBe("false");
    expect(loadSidebarOpenStartup()).toBe(false);

    saveSidebarOpenStartup(true);
    expect(localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY)).toBe("true");
    expect(loadSidebarOpenStartup()).toBe(true);
  });

  it("migrates legacy sidebar open key upon loading startup preference", () => {
    localStorage.setItem("orca.sidebar.open", "false");
    expect(loadSidebarOpenStartup()).toBe(false);
    expect(localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY)).toBe("false");
  });
});
