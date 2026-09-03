import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  DEFAULT_NOTIFICATION_SETTINGS,
  NOTIFICATION_SETTINGS_EVENT,
  loadNotificationSettings,
  resetNotificationSettings,
  saveNotificationSettings,
  useAttentionFrameEnabled,
} from "./notificationSettings";

describe("notificationSettings", () => {
  beforeEach(() => {
    localStorage.clear();
    resetNotificationSettings();
  });

  afterEach(() => {
    localStorage.clear();
    resetNotificationSettings();
  });

  it("loads defaults with attentionFrame as true", () => {
    const settings = loadNotificationSettings();
    expect(settings.attentionFrame).toBe(true);
    expect(settings).toEqual(DEFAULT_NOTIFICATION_SETTINGS);
  });

  it("roundtrips attentionFrame false and preserves other fields", () => {
    const saved = saveNotificationSettings({ attentionFrame: false });
    expect(saved.attentionFrame).toBe(false);

    const loaded = loadNotificationSettings();
    expect(loaded.attentionFrame).toBe(false);
    expect(loaded.enabled).toBe(true);
    expect(loaded.agentTaskComplete).toBe(true);
  });

  it("resets attentionFrame back to true on resetNotificationSettings", () => {
    saveNotificationSettings({ attentionFrame: false });
    expect(loadNotificationSettings().attentionFrame).toBe(false);

    const reset = resetNotificationSettings();
    expect(reset.attentionFrame).toBe(true);
    expect(loadNotificationSettings().attentionFrame).toBe(true);
  });

  it("dispatches NOTIFICATION_SETTINGS_EVENT when saving settings", () => {
    let captured: any = null;
    const listener = (e: Event) => {
      captured = (e as CustomEvent).detail;
    };
    window.addEventListener(NOTIFICATION_SETTINGS_EVENT, listener);

    try {
      saveNotificationSettings({ attentionFrame: false });
      expect(captured).not.toBeNull();
      expect(captured.attentionFrame).toBe(false);
    } finally {
      window.removeEventListener(NOTIFICATION_SETTINGS_EVENT, listener);
    }
  });

  it("useAttentionFrameEnabled reflects stored value and updates on settings change event", () => {
    const { result } = renderHook(() => useAttentionFrameEnabled());
    expect(result.current).toBe(true);

    act(() => {
      saveNotificationSettings({ attentionFrame: false });
    });

    expect(result.current).toBe(false);

    act(() => {
      saveNotificationSettings({ attentionFrame: true });
    });

    expect(result.current).toBe(true);
  });
});
