import {
  applyAppearanceSettings,
  loadAppearanceSettings,
  saveAppearanceSettings,
  type AppearanceSettingsState,
} from "./appearanceSettings";
import { listBrowsers, setBrowserZoom } from "./browserTauri";
import { loadBrowserSettings, saveBrowserSettings } from "./browserSettings";
import { loadTerminalSettings } from "./terminalSettings";

let installed = false;

export function _resetSettingsRuntimeBridgeForTest() {
  installed = false;
}

function findSettingValue(label: string): HTMLElement | null {
  const labels = Array.from(document.querySelectorAll<HTMLElement>("div"));
  const heading = labels.find((element) => element.textContent?.trim() === label);
  const row = heading?.parentElement?.parentElement;
  if (!row) return null;
  const valueContainer = row.lastElementChild;
  return valueContainer?.querySelector<HTMLElement>("span") ?? null;
}

function syncGeneralAppearanceLabels(settings: AppearanceSettingsState) {
  const themeLabel = settings.theme === "system"
    ? "System"
    : settings.theme === "charcoal"
      ? "Charcoal"
      : settings.theme[0].toUpperCase() + settings.theme.slice(1);
  const densityLabel = settings.density === "comfortable" ? "Comfortable" : "Compact";
  const colorValue = findSettingValue("Color scheme");
  const densityValue = findSettingValue("Density");
  if (colorValue && colorValue.textContent !== themeLabel) colorValue.textContent = themeLabel;
  if (densityValue && densityValue.textContent !== densityLabel) densityValue.textContent = densityLabel;
}

function syncTerminalSourceLabel() {
  const settings = loadTerminalSettings();
  const hasLocalOverride = settings.fontFamily !== null || settings.macosOptionAsAlt !== null || settings.fontSize !== null;
  if (!hasLocalOverride) return;

  const headings = Array.from(document.querySelectorAll<HTMLElement>("h2"));
  const heading = headings.find((element) => element.textContent?.trim() === "Effective preferences");
  const source = heading?.nextElementSibling as HTMLElement | null;
  if (source && source.textContent !== "Local override") source.textContent = "Local override";
}

function syncAppearance() {
  const settings = saveAppearanceSettings(loadAppearanceSettings());
  applyAppearanceSettings(settings);
  syncGeneralAppearanceLabels(settings);
}

function syncBrowser() {
  saveBrowserSettings(loadBrowserSettings());
}

function afterReactEvent(callback: () => void) {
  window.setTimeout(callback, 0);
}

function isInsideSettingsSection(target: EventTarget | null, controlSelector: string) {
  if (!(target instanceof Element)) return false;
  const section = target.closest("section");
  return Boolean(section?.querySelector(controlSelector));
}

async function applyBrowserDefaultZoom() {
  const settings = loadBrowserSettings();
  try {
    const browsers = await listBrowsers();
    await Promise.all(browsers.map((browser) => setBrowserZoom(browser.browserId, settings.defaultZoom / 100)));
  } catch {
    // Browser runtime may not be available in web-preview mode.
  }
}

export function installSettingsRuntimeBridge() {
  if (installed || typeof window === "undefined" || typeof document === "undefined") return;
  installed = true;

  syncAppearance();

  const media = typeof window.matchMedia === "function" ? window.matchMedia("(prefers-color-scheme: light)") : null;
  const handleSystemThemeChange = () => {
    if (loadAppearanceSettings().theme === "system") syncAppearance();
  };
  media?.addEventListener?.("change", handleSystemThemeChange);

  window.addEventListener("change", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLInputElement || target instanceof HTMLSelectElement)) return;

    if (["appearance-theme-mode", "appearance-accent-color", "appearance-density"].includes(target.id)) {
      afterReactEvent(syncAppearance);
      return;
    }
    if (["browser-search-engine", "browser-default-zoom", "browser-restore-tabs"].includes(target.id)) {
      afterReactEvent(() => {
        syncBrowser();
        if (target.id === "browser-default-zoom") void applyBrowserDefaultZoom();
      });
      return;
    }
    if (["terminal-font-family", "terminal-font-size", "terminal-scrollback"].includes(target.id)) {
      afterReactEvent(syncTerminalSourceLabel);
    }
  }, true);

  window.addEventListener("orca:terminal-settings", () => afterReactEvent(syncTerminalSourceLabel));

  window.addEventListener("click", (event) => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const button = target.closest("button");
    if (!button) return;
    const text = button.textContent?.trim() ?? "";

    if (text === "Reset to defaults") {
      if (isInsideSettingsSection(button, "#appearance-theme-mode")) {
        afterReactEvent(syncAppearance);
      } else if (isInsideSettingsSection(button, "#browser-search-engine")) {
        afterReactEvent(() => {
          syncBrowser();
          void applyBrowserDefaultZoom();
        });
      }
    }
  }, true);
}
