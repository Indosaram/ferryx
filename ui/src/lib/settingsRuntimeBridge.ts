import { isDesktopShell as selectDesktopShell } from "./clientRuntime";
import {
  APPEARANCE_SETTINGS_EVENT,
  applyAppearanceSettings,
  loadAppearanceSettings,
  prefersLightColorScheme,
  resolveTerminalAppearance,
  subscribeSystemColorScheme,
  type AppearanceSettingsState,
} from "./appearanceSettings";
import { setTerminalAppearance } from "./tauri";
import { syncTerminalBackground } from "./terminalSettings";

let installed = false;
let pushRequest = 0;
let lastPushedAppearance: "light" | "dark" | null = null;
let uninstallListeners: (() => void) | null = null;

export function _resetSettingsRuntimeBridgeForTest() {
  uninstallListeners?.();
  uninstallListeners = null;
  installed = false;
  pushRequest = 0;
  lastPushedAppearance = null;
}

function isDesktopShell() {
  return selectDesktopShell(Boolean((window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__));
}

/** Desktop-only: tells the native terminal which side of the app appearance to follow. */
function pushTerminalAppearance(settings: AppearanceSettingsState) {
  if (!isDesktopShell()) return;
  const appearance = resolveTerminalAppearance(settings.theme, prefersLightColorScheme());
  if (appearance === lastPushedAppearance) return;
  lastPushedAppearance = appearance;
  const request = ++pushRequest;
  void setTerminalAppearance(appearance).then(
    (preferences) => {
      if (request !== pushRequest) return;
      const background = preferences?.theme?.background;
      if (background) syncTerminalBackground(background);
    },
    (error: unknown) => {
      if (request === pushRequest) lastPushedAppearance = null;
      console.warn("Ferryx could not push the app appearance to the terminal", error);
    },
  );
}

export function installSettingsRuntimeBridge() {
  if (installed || typeof window === "undefined" || typeof document === "undefined") return;
  installed = true;

  const initial = loadAppearanceSettings();
  applyAppearanceSettings(initial);
  pushTerminalAppearance(initial);

  const handleSystemThemeChange = () => {
    const settings = loadAppearanceSettings();
    if (settings.theme !== "system") return;
    applyAppearanceSettings(settings);
    pushTerminalAppearance(settings);
  };
  const unsubscribeSystemTheme = subscribeSystemColorScheme(handleSystemThemeChange);

  const handleAppearanceSettingChange = (event: Event) => {
    const detail = (event as CustomEvent<AppearanceSettingsState>).detail;
    pushTerminalAppearance(detail ?? loadAppearanceSettings());
  };
  window.addEventListener(APPEARANCE_SETTINGS_EVENT, handleAppearanceSettingChange);
  uninstallListeners = () => {
    unsubscribeSystemTheme();
    window.removeEventListener(APPEARANCE_SETTINGS_EVENT, handleAppearanceSettingChange);
  };
}
