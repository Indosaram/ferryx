import {
  applyAppearanceSettings,
  loadAppearanceSettings,
} from "./appearanceSettings";

let installed = false;

export function _resetSettingsRuntimeBridgeForTest() {
  installed = false;
}

export function installSettingsRuntimeBridge() {
  if (installed || typeof window === "undefined" || typeof document === "undefined") return;
  installed = true;

  applyAppearanceSettings(loadAppearanceSettings());

  const media = typeof window.matchMedia === "function" ? window.matchMedia("(prefers-color-scheme: light)") : null;
  const handleSystemThemeChange = () => {
    const settings = loadAppearanceSettings();
    if (settings.theme === "system") applyAppearanceSettings(settings);
  };
  media?.addEventListener?.("change", handleSystemThemeChange);
}
