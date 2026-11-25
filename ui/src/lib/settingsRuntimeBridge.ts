import { isDesktopShell as selectDesktopShell } from "./clientRuntime";
import {
  APPEARANCE_SETTINGS_EVENT,
  applyAppearanceSettings,
  applyThemeBackgroundUrl,
  getPublishedEnvironmentThemes,
  loadAppearanceSettings,
  prefersLightColorScheme,
  resolveActiveTerminalPalette,
  resolveActiveThemeChoice,
  resolveTerminalAppearance,
  saveAppearanceSettings,
  setPublishedEnvironmentThemes,
  subscribeSystemColorScheme,
  type AppearanceSettingsState,
  type AppearanceTheme,
  type PublishedEnvironmentTheme,
} from "./appearanceSettings";
import {
  listEnvironmentThemes,
  readEnvironmentThemeBackground,
  setTerminalAppearance,
  setTerminalThemePalette,
} from "./tauri";
import { syncTerminalBackground } from "./terminalSettings";
import { parseAnyThemeImport } from "./vscodeThemeImport";

let installed = false;
let pushRequest = 0;
let lastPushedAppearance: "light" | "dark" | null = null;
let lastPushedPaletteKey = "null";
let lastAppliedEnvRevision: number | null = null;
let lastBackgroundKey: string | null = null;
let uninstallListeners: (() => void) | null = null;

export function _resetSettingsRuntimeBridgeForTest() {
  uninstallListeners?.();
  uninstallListeners = null;
  installed = false;
  pushRequest = 0;
  lastPushedAppearance = null;
  lastPushedPaletteKey = "null";
  lastAppliedEnvRevision = null;
  lastBackgroundKey = null;
}

function isDesktopShell() {
  return selectDesktopShell(Boolean((window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__));
}

function pushTerminalAppearance(settings: AppearanceSettingsState) {
  if (!isDesktopShell()) return;
  const theme = document.documentElement.dataset.theme as Exclude<AppearanceTheme, "system">;
  const appearance = resolveTerminalAppearance(theme, prefersLightColorScheme());
  if (appearance !== lastPushedAppearance) {
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

  const palette = resolveActiveTerminalPalette(settings);
  const paletteKey = palette ? JSON.stringify(palette) : "null";
  if (paletteKey !== lastPushedPaletteKey && typeof setTerminalThemePalette === "function") {
    lastPushedPaletteKey = paletteKey;
    void Promise.resolve(setTerminalThemePalette(palette)).then(
      (preferences) => {
        const background = preferences?.theme?.background ?? palette?.background;
        if (background) syncTerminalBackground(background);
      },
      () => {
        lastPushedPaletteKey = "null";
      },
    );
  }
}

function syncThemeBackground(settings: AppearanceSettingsState) {
  const { doc } = resolveActiveThemeChoice(settings);
  const isSavedOverride = Boolean(doc && settings.customThemes.some((t) => t.id === doc.id));
  const published =
    doc && !isSavedOverride
      ? getPublishedEnvironmentThemes().find((item) => item.doc.id === doc.id && item.hasBackground)
      : undefined;
  const key = published ? `${published.doc.id}:${published.backgroundRevision ?? ""}` : null;
  if (key === lastBackgroundKey) return;
  lastBackgroundKey = key;
  if (!published || !isDesktopShell() || typeof readEnvironmentThemeBackground !== "function") {
    applyThemeBackgroundUrl(null);
    return;
  }
  void Promise.resolve(readEnvironmentThemeBackground(published.doc.id)).then(
    (bg) => {
      if (lastBackgroundKey !== key) return;
      applyThemeBackgroundUrl(bg?.dataUrl ?? null);
    },
    () => {
      if (lastBackgroundKey === key) applyThemeBackgroundUrl(null);
    },
  );
}

function refreshEnvironmentThemes() {
  if (!isDesktopShell() || typeof listEnvironmentThemes !== "function") return;
  void Promise.resolve(listEnvironmentThemes()).then(
    (snapshot) => {
      if (!snapshot || !Array.isArray(snapshot.themes)) return;
      const entries: PublishedEnvironmentTheme[] = [];
      for (const row of snapshot.themes) {
        const parsed = parseAnyThemeImport(JSON.stringify(row.document));
        if (!parsed.ok) continue;
        entries.push({
          doc: { ...parsed.doc, id: row.id },
          hasBackground: Boolean(row.hasBackground),
          backgroundRevision: row.backgroundRevision,
        });
      }
      setPublishedEnvironmentThemes(entries);

      if (snapshot.revision > 0 && snapshot.revision !== lastAppliedEnvRevision) {
        lastAppliedEnvRevision = snapshot.revision;
        const patch: Partial<AppearanceSettingsState> = {};
        if (
          snapshot.defaultAppearance === "light" ||
          snapshot.defaultAppearance === "dark" ||
          snapshot.defaultAppearance === "system"
        ) {
          patch.theme = snapshot.defaultAppearance;
        }
        if (snapshot.defaultThemeId) {
          if (
            snapshot.defaultThemeId === "charcoal" ||
            snapshot.defaultThemeId === "dark" ||
            snapshot.defaultThemeId === "light" ||
            snapshot.defaultThemeId === "system"
          ) {
            patch.theme = snapshot.defaultThemeId;
            patch.activeCustomThemeId = null;
          } else {
            patch.activeCustomThemeId = snapshot.defaultThemeId;
          }
        }
        if (Object.keys(patch).length > 0) {
          saveAppearanceSettings(patch);
        }
      }
      syncThemeBackground(loadAppearanceSettings());
    },
    () => {},
  );
}

export function installSettingsRuntimeBridge() {
  if (installed || typeof window === "undefined" || typeof document === "undefined") return;
  installed = true;

  const initial = loadAppearanceSettings();
  applyAppearanceSettings(initial);
  pushTerminalAppearance(initial);
  syncThemeBackground(initial);
  refreshEnvironmentThemes();

  const handleSystemThemeChange = () => {
    const settings = loadAppearanceSettings();
    if (settings.theme !== "system") return;
    applyAppearanceSettings(settings);
    pushTerminalAppearance(settings);
  };
  const unsubscribeSystemTheme = subscribeSystemColorScheme(handleSystemThemeChange);

  const handleAppearanceSettingChange = (event: Event) => {
    const detail = (event as CustomEvent<AppearanceSettingsState>).detail;
    const current = detail ?? loadAppearanceSettings();
    applyAppearanceSettings(current);
    pushTerminalAppearance(current);
    syncThemeBackground(current);
  };
  window.addEventListener(APPEARANCE_SETTINGS_EVENT, handleAppearanceSettingChange);
  uninstallListeners = () => {
    unsubscribeSystemTheme();
    window.removeEventListener(APPEARANCE_SETTINGS_EVENT, handleAppearanceSettingChange);
  };
}
