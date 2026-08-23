import { beforeEach, describe, expect, it } from "vitest";

import {
  DEFAULT_GENERAL_SETTINGS,
  GENERAL_SETTINGS_EVENT,
  loadGeneralSettings,
  resetGeneralSettings,
  saveGeneralSettings,
} from "./generalSettings";
import { GENERAL_SETTINGS_STORAGE_KEY } from "./storageKeys";

describe("generalSettings", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("loads default settings when storage is empty", () => {
    const settings = loadGeneralSettings();
    expect(settings).toEqual(DEFAULT_GENERAL_SETTINGS);
    expect(settings.confirmCloseTab).toBe(false);
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
});
