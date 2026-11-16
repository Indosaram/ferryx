import type { AppearanceTheme } from "./appearanceSettings";

/** Built-ins only; user themes are selected explicitly by their palette/library row. */
export const APPEARANCE_THEME_CYCLE = ["charcoal", "dark", "light", "system"] as const satisfies readonly AppearanceTheme[];

/**
 * Advance from the stored built-in preference, even when a user theme is active.
 * Clear the user theme in the same settings write; its palette row can re-select it.
 * Pass the current settings and the store's updateSettings/saveAppearanceSettings
 * function so desktop, remote and settings all use the installed appearance store.
 */
export function cycleAppearanceTheme(
  settings: { theme: AppearanceTheme },
  updateSettings: (patch: { theme: AppearanceTheme; activeCustomThemeId: null }) => unknown,
): AppearanceTheme {
  const index = APPEARANCE_THEME_CYCLE.indexOf(settings.theme);
  const theme = APPEARANCE_THEME_CYCLE[(index + 1) % APPEARANCE_THEME_CYCLE.length];
  updateSettings({ theme, activeCustomThemeId: null });
  return theme;
}

/** The shell supplies the callback that opens its appearance dialog or remote sheet. */
export function openAppearanceSurface(openSurface: () => void): void {
  openSurface();
}
