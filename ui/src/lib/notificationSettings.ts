import { useCallback, useEffect, useState } from "react";

import { getMigratedItem, NOTIFICATION_SETTINGS_STORAGE_KEY } from "./storageKeys";

export interface NotificationSettings {
  enabled: boolean;
  agentTaskComplete: boolean;
  terminalBell: boolean;
  attentionFrame: boolean;
  customSoundId: "system" | "none" | "custom" | string;
  customSoundPath: string | null;
  customSoundVolume: number;
}

export { NOTIFICATION_SETTINGS_STORAGE_KEY };
export const NOTIFICATION_SETTINGS_EVENT = "ferryx:notifications:settings-changed";

export const DEFAULT_NOTIFICATION_SETTINGS: NotificationSettings = {
  enabled: true,
  agentTaskComplete: true,
  terminalBell: false,
  attentionFrame: true,
  customSoundId: "system",
  customSoundPath: null,
  customSoundVolume: 0.8,
};

export function loadNotificationSettings(): NotificationSettings {
  if (typeof window === "undefined" || !window.localStorage) {
    return { ...DEFAULT_NOTIFICATION_SETTINGS };
  }
  try {
    const raw = getMigratedItem(NOTIFICATION_SETTINGS_STORAGE_KEY);
    if (!raw) return { ...DEFAULT_NOTIFICATION_SETTINGS };
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return { ...DEFAULT_NOTIFICATION_SETTINGS };
    return {
      enabled: typeof parsed.enabled === "boolean" ? parsed.enabled : DEFAULT_NOTIFICATION_SETTINGS.enabled,
      agentTaskComplete: typeof parsed.agentTaskComplete === "boolean" ? parsed.agentTaskComplete : DEFAULT_NOTIFICATION_SETTINGS.agentTaskComplete,
      terminalBell: typeof parsed.terminalBell === "boolean" ? parsed.terminalBell : DEFAULT_NOTIFICATION_SETTINGS.terminalBell,
      attentionFrame: typeof parsed.attentionFrame === "boolean" ? parsed.attentionFrame : DEFAULT_NOTIFICATION_SETTINGS.attentionFrame,
      customSoundId: typeof parsed.customSoundId === "string" ? parsed.customSoundId : DEFAULT_NOTIFICATION_SETTINGS.customSoundId,
      customSoundPath: typeof parsed.customSoundPath === "string" ? parsed.customSoundPath : null,
      customSoundVolume: typeof parsed.customSoundVolume === "number" ? Math.max(0, Math.min(1, parsed.customSoundVolume)) : DEFAULT_NOTIFICATION_SETTINGS.customSoundVolume,
    };
  } catch {
    return { ...DEFAULT_NOTIFICATION_SETTINGS };
  }
}

export function saveNotificationSettings(settings: Partial<NotificationSettings>): NotificationSettings {
  const current = loadNotificationSettings();
  const next: NotificationSettings = {
    ...current,
    ...settings,
    customSoundVolume: typeof settings.customSoundVolume === "number"
      ? Math.max(0, Math.min(1, settings.customSoundVolume))
      : current.customSoundVolume,
  };
  if (typeof window !== "undefined" && window.localStorage) {
    try {
      localStorage.setItem(NOTIFICATION_SETTINGS_STORAGE_KEY, JSON.stringify(next));
      window.dispatchEvent(new CustomEvent(NOTIFICATION_SETTINGS_EVENT, { detail: next }));
    } catch {
      // ignore
    }
  }
  return next;
}

export function resetNotificationSettings(): NotificationSettings {
  if (typeof window !== "undefined" && window.localStorage) {
    try {
      localStorage.removeItem(NOTIFICATION_SETTINGS_STORAGE_KEY);
      window.dispatchEvent(new CustomEvent(NOTIFICATION_SETTINGS_EVENT, { detail: DEFAULT_NOTIFICATION_SETTINGS }));
    } catch {
      // ignore
    }
  }
  return { ...DEFAULT_NOTIFICATION_SETTINGS };
}

export function useNotificationSettings() {
  const [settings, setSettings] = useState<NotificationSettings>(loadNotificationSettings);

  useEffect(() => {
    const handleUpdate = (e: Event) => {
      const customEvent = e as CustomEvent<NotificationSettings>;
      if (customEvent.detail) {
        setSettings(customEvent.detail);
      } else {
        setSettings(loadNotificationSettings());
      }
    };
    window.addEventListener(NOTIFICATION_SETTINGS_EVENT, handleUpdate);
    return () => window.removeEventListener(NOTIFICATION_SETTINGS_EVENT, handleUpdate);
  }, []);

  const updateSettings = useCallback((partial: Partial<NotificationSettings>) => {
    const next = saveNotificationSettings(partial);
    setSettings(next);
  }, []);

  const reset = useCallback(() => {
    const next = resetNotificationSettings();
    setSettings(next);
  }, []);

  return {
    settings,
    updateSettings,
    resetSettings: reset,
  };
}

export function useAttentionFrameEnabled(): boolean {
  const [attentionFrame, setAttentionFrame] = useState<boolean>(() => loadNotificationSettings().attentionFrame);

  useEffect(() => {
    const handleUpdate = (e: Event) => {
      const customEvent = e as CustomEvent<NotificationSettings>;
      if (customEvent.detail && typeof customEvent.detail.attentionFrame === "boolean") {
        setAttentionFrame(customEvent.detail.attentionFrame);
      } else {
        setAttentionFrame(loadNotificationSettings().attentionFrame);
      }
    };
    const handleStorage = (e: StorageEvent) => {
      if (e.key === NOTIFICATION_SETTINGS_STORAGE_KEY) {
        setAttentionFrame(loadNotificationSettings().attentionFrame);
      }
    };
    window.addEventListener(NOTIFICATION_SETTINGS_EVENT, handleUpdate);
    window.addEventListener("storage", handleStorage);
    return () => {
      window.removeEventListener(NOTIFICATION_SETTINGS_EVENT, handleUpdate);
      window.removeEventListener("storage", handleStorage);
    };
  }, []);

  return attentionFrame;
}
