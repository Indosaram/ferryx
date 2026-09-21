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
    expect(settings.sessionRestorePolicy).toBe("lazy");
    expect(settings.sessionIdleTimeoutMinutes).toBe(30);
  });

  it("normalizes malformed persisted settings", () => {
    localStorage.setItem(GENERAL_SETTINGS_STORAGE_KEY, JSON.stringify({ confirmCloseTab: "true" }));
    expect(loadGeneralSettings()).toEqual(DEFAULT_GENERAL_SETTINGS);

    localStorage.setItem(GENERAL_SETTINGS_STORAGE_KEY, "null");
    expect(loadGeneralSettings()).toEqual(DEFAULT_GENERAL_SETTINGS);
  });

  it("normalizes restore policy and clamps the idle timeout", () => {
    localStorage.setItem(GENERAL_SETTINGS_STORAGE_KEY, JSON.stringify({
      sessionRestorePolicy: "activeOnly",
      sessionIdleTimeoutMinutes: 0,
    }));
    // 0 is the explicit "off" value and must survive normalization.
    expect(loadGeneralSettings()).toMatchObject({
      sessionRestorePolicy: "activeOnly",
      sessionIdleTimeoutMinutes: 0,
    });

    localStorage.setItem(GENERAL_SETTINGS_STORAGE_KEY, JSON.stringify({
      sessionRestorePolicy: "eager",
      sessionIdleTimeoutMinutes: -5,
    }));
    expect(loadGeneralSettings()).toMatchObject({
      sessionRestorePolicy: "eager",
      sessionIdleTimeoutMinutes: 0,
    });

    localStorage.setItem(GENERAL_SETTINGS_STORAGE_KEY, JSON.stringify({
      sessionRestorePolicy: "eager",
      sessionIdleTimeoutMinutes: 10_000,
    }));
    expect(loadGeneralSettings()).toMatchObject({
      sessionRestorePolicy: "eager",
      sessionIdleTimeoutMinutes: 1440,
    });
  });

  it("saves and persists general settings update", () => {
    let observed: unknown = null;
    const listener = (event: Event) => {
      if (event instanceof CustomEvent) observed = event.detail;
    };
    window.addEventListener(GENERAL_SETTINGS_EVENT, listener);

    const updated = saveGeneralSettings({
      confirmCloseTab: true,
      sessionRestorePolicy: "activeOnly",
      sessionIdleTimeoutMinutes: 45,
    });
    try {
      expect(updated).toEqual({
        confirmCloseTab: true,
        sessionRestorePolicy: "activeOnly",
        sessionIdleTimeoutMinutes: 45,
      });
      expect(JSON.parse(localStorage.getItem(GENERAL_SETTINGS_STORAGE_KEY) ?? "null")).toEqual(updated);
      expect(observed).toEqual(updated);
      expect(loadGeneralSettings()).toEqual(updated);
    } finally {
      window.removeEventListener(GENERAL_SETTINGS_EVENT, listener);
    }
  });

  it("resets settings back to default", () => {
    saveGeneralSettings({ confirmCloseTab: true, sessionRestorePolicy: "eager", sessionIdleTimeoutMinutes: 90 });
    expect(loadGeneralSettings().confirmCloseTab).toBe(true);

    const reset = resetGeneralSettings();
    expect(reset).toEqual(DEFAULT_GENERAL_SETTINGS);
    expect(loadGeneralSettings()).toEqual(DEFAULT_GENERAL_SETTINGS);
  });

  it("migrates legacy settings key upon loading and fills lifecycle defaults", () => {
    localStorage.setItem("rorca.settings.general", JSON.stringify({ confirmCloseTab: true }));
    const settings = loadGeneralSettings();
    expect(settings).toEqual({
      confirmCloseTab: true,
      sessionRestorePolicy: "lazy",
      sessionIdleTimeoutMinutes: 30,
    });
    expect(JSON.parse(localStorage.getItem(GENERAL_SETTINGS_STORAGE_KEY) ?? "null")).toEqual({ confirmCloseTab: true });
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