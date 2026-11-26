import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { isDesktopShell } from "./clientRuntime";
import { APPEARANCE_SETTINGS_STORAGE_KEY } from "./storageKeys";
import { parseThemeColor, parseThemeDocument, type ThemeDocumentV1, type ThemeValidationError } from "./themeDocument";
import { scopeCustomCss, type CustomCssError } from "./customThemeCss";
import { applyAppThemeCss, clearAppThemeCss } from "./themeCss";
import {
  getCuratedTheme,
  resolveDocumentRoles,
  themeColorsToTerminalPalette,
  themeDefinitionToDocument,
  type TerminalThemePalette,
  type ThemeAppearance,
  type ThemeHalves,
} from "./themePalettes";

export type AppearanceTheme = "charcoal" | "dark" | "light" | "system";
export type AppearanceAccent = "default" | "blue" | "emerald" | "purple" | "amber" | "rose";
export type AppearanceDensity = "compact" | "comfortable";

export type AppearanceSettingsState = {
  theme: AppearanceTheme;
  accentColor: AppearanceAccent;
  density: AppearanceDensity;
  customThemes: ThemeDocumentV1[];
  activeCustomThemeId: string | null;
  customCss: string;
  renderMath: boolean;
  themeHalves?: ThemeHalves;
};

export type PublishedEnvironmentTheme = {
  doc: ThemeDocumentV1;
  hasBackground: boolean;
  backgroundRevision?: string;
};

export type InvalidAppearanceTheme = { raw: unknown; errors: ThemeValidationError[] };
export type AppearanceSettingsLoadResult = {
  settings: AppearanceSettingsState;
  invalidThemes: InvalidAppearanceTheme[];
};

export const DEFAULT_APPEARANCE_SETTINGS: AppearanceSettingsState = {
  theme: "charcoal",
  accentColor: "default",
  density: "compact",
  customThemes: [],
  activeCustomThemeId: null,
  customCss: "",
  renderMath: true,
};

export const APPEARANCE_SETTINGS_EVENT = "ferryx:appearance-settings";

const THEMES = new Set<AppearanceTheme>(["charcoal", "dark", "light", "system"]);
const ACCENTS = new Set<AppearanceAccent>(["default", "blue", "emerald", "purple", "amber", "rose"]);
const DENSITIES = new Set<AppearanceDensity>(["compact", "comfortable"]);
const ALPHA_COMPANION_TOKENS = new Set([
  "--border",
  "--input",
  "--sidebar-border",
  "--worktree-sidebar-border",
]);

let publishedEnvironmentThemes: PublishedEnvironmentTheme[] = [];
const environmentThemeListeners = new Set<() => void>();

export function getPublishedEnvironmentThemes(): ReadonlyArray<PublishedEnvironmentTheme> {
  return publishedEnvironmentThemes;
}

export function setPublishedEnvironmentThemes(themes: ReadonlyArray<PublishedEnvironmentTheme>): void {
  publishedEnvironmentThemes = [...themes];
  for (const listener of environmentThemeListeners) listener();
}

export function subscribePublishedEnvironmentThemes(listener: () => void): () => void {
  environmentThemeListeners.add(listener);
  return () => environmentThemeListeners.delete(listener);
}

export function usePublishedEnvironmentThemes(): ReadonlyArray<PublishedEnvironmentTheme> {
  return useSyncExternalStore(
    subscribePublishedEnvironmentThemes,
    getPublishedEnvironmentThemes,
    getPublishedEnvironmentThemes,
  );
}

let draftPreviewDoc: ThemeDocumentV1 | null = null;
let draftPreviewAppearance: ThemeAppearance | null = null;
const draftPreviewListeners = new Set<() => void>();

export function getThemeDraftPreview(): { doc: ThemeDocumentV1; appearance: ThemeAppearance } | null {
  return draftPreviewDoc ? { doc: draftPreviewDoc, appearance: draftPreviewAppearance ?? draftPreviewDoc.base } : null;
}

export function setThemeDraftPreview(doc: ThemeDocumentV1 | null, appearance?: ThemeAppearance): void {
  draftPreviewDoc = doc;
  draftPreviewAppearance = doc ? (appearance ?? doc.base) : null;
  for (const listener of draftPreviewListeners) listener();
  if (typeof document !== "undefined") {
    applyAppearanceSettings(loadAppearanceSettings());
  }
}

export function clearThemeDraftPreview(): void {
  if (!draftPreviewDoc) return;
  setThemeDraftPreview(null);
}

export function subscribeThemeDraftPreview(listener: () => void): () => void {
  draftPreviewListeners.add(listener);
  return () => draftPreviewListeners.delete(listener);
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function normalizeThemeHalves(raw: unknown): ThemeHalves | undefined {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return undefined;
  const source = raw as Record<string, unknown>;
  const light = typeof source.light === "string" && source.light.trim() ? source.light.trim() : undefined;
  const dark = typeof source.dark === "string" && source.dark.trim() ? source.dark.trim() : undefined;
  if (!light && !dark) return undefined;
  return { ...(light ? { light } : {}), ...(dark ? { dark } : {}) };
}

function isResolvableThemeId(id: string, customIds: ReadonlySet<string>): boolean {
  return (
    customIds.has(id) ||
    getCuratedTheme(id) !== null ||
    publishedEnvironmentThemes.some((item) => item.doc.id === id)
  );
}

function normalizeAppearanceSettings(value: unknown): AppearanceSettingsLoadResult {
  const source = value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown> : {};
  const customThemes: ThemeDocumentV1[] = [];
  const invalidThemes: InvalidAppearanceTheme[] = [];
  const ids = new Set<string>();
  if (source.customThemes !== undefined && !Array.isArray(source.customThemes)) {
    invalidThemes.push({ raw: source.customThemes, errors: [{ path: "customThemes", message: "Expected a theme document array." }] });
  }
  for (const raw of Array.isArray(source.customThemes) ? source.customThemes : []) {
    const result = parseThemeDocument(raw);
    if (!result.ok) invalidThemes.push({ raw, errors: result.errors });
    else if (ids.has(result.doc.id)) invalidThemes.push({ raw, errors: [{ path: "id", message: "Theme ids must be unique." }] });
    else { ids.add(result.doc.id); customThemes.push(result.doc); }
  }
  for (const row of Array.isArray(source.invalidThemes) ? source.invalidThemes : []) {
    if (row && typeof row === "object" && Array.isArray(row.errors) && row.errors.length > 0
      && row.errors.every((error: unknown) => error && typeof error === "object"
        && typeof (error as ThemeValidationError).path === "string" && typeof (error as ThemeValidationError).message === "string")) {
      invalidThemes.push({ raw: row.raw, errors: row.errors });
    } else invalidThemes.push({ raw: row, errors: [{ path: "invalidThemes", message: "Malformed saved theme diagnostic." }] });
  }
  const themeHalves = normalizeThemeHalves(source.themeHalves);
  const activeId =
    typeof source.activeCustomThemeId === "string" && isResolvableThemeId(source.activeCustomThemeId, ids)
      ? source.activeCustomThemeId
      : null;
  return { settings: {
    theme: typeof source.theme === "string" && THEMES.has(source.theme as AppearanceTheme)
      ? (source.theme as AppearanceTheme)
      : DEFAULT_APPEARANCE_SETTINGS.theme,
    accentColor: typeof source.accentColor === "string" && ACCENTS.has(source.accentColor as AppearanceAccent)
      ? (source.accentColor as AppearanceAccent)
      : DEFAULT_APPEARANCE_SETTINGS.accentColor,
    density: typeof source.density === "string" && DENSITIES.has(source.density as AppearanceDensity)
      ? (source.density as AppearanceDensity)
      : DEFAULT_APPEARANCE_SETTINGS.density,
    customThemes,
    activeCustomThemeId: activeId,
    customCss: typeof source.customCss === "string" ? source.customCss : "",
    renderMath: typeof source.renderMath === "boolean" ? source.renderMath : true,
    ...(themeHalves ? { themeHalves } : {}),
  }, invalidThemes };
}

export function loadAppearanceSettings(storage: Storage | null = browserStorage()): AppearanceSettingsState {
  return loadAppearanceSettingsState(storage).settings;
}

export function loadAppearanceSettingsState(storage: Storage | null = browserStorage()): AppearanceSettingsLoadResult {
  const defaults = () => normalizeAppearanceSettings({});
  if (!storage) return defaults();
  let raw: string | null;
  try {
    raw = storage.getItem(APPEARANCE_SETTINGS_STORAGE_KEY);
  } catch (error) {
    return { ...defaults(), invalidThemes: [{ raw: null, errors: [{ path: "$", message: `Could not read appearance storage: ${String(error)}` }] }] };
  }
  if (raw === null) return defaults();
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return { ...defaults(), invalidThemes: [{ raw, errors: [{ path: "$", message: "Stored appearance settings are not valid JSON." }] }] };
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { ...defaults(), invalidThemes: [{ raw: parsed, errors: [{ path: "$", message: "Expected a stored appearance settings object." }] }] };
  }
  return normalizeAppearanceSettings(parsed);
}

export function saveAppearanceSettings(
  patch: Partial<AppearanceSettingsState>,
  storage: Storage | null = browserStorage(),
): AppearanceSettingsState {
  const loaded = loadAppearanceSettingsState(storage);
  const merged = { ...loaded.settings, ...patch, invalidThemes: loaded.invalidThemes };
  if ("themeHalves" in patch && patch.themeHalves === undefined) {
    delete merged.themeHalves;
  }
  const normalized = normalizeAppearanceSettings(merged);
  const next = normalized.settings;
  try {
    storage?.setItem(APPEARANCE_SETTINGS_STORAGE_KEY, JSON.stringify({ ...next, invalidThemes: normalized.invalidThemes }));
  } catch (error) {
    console.warn("Ferryx could not persist appearance settings", error);
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<AppearanceSettingsState>(APPEARANCE_SETTINGS_EVENT, { detail: next }));
  }
  return next;
}

export function resetAppearanceSettings(storage: Storage | null = browserStorage()): AppearanceSettingsState {
  try {
    storage?.removeItem(APPEARANCE_SETTINGS_STORAGE_KEY);
  } catch (error) {
    console.warn("Ferryx could not reset appearance settings", error);
  }
  const next = normalizeAppearanceSettings({}).settings;
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<AppearanceSettingsState>(APPEARANCE_SETTINGS_EVENT, { detail: next }));
  }
  return next;
}

const LIGHT_QUERY = "(prefers-color-scheme: light)";

let nativeOsTheme: "light" | "dark" | null = null;

export function _resetNativeOsThemeForTest() {
  nativeOsTheme = null;
  draftPreviewDoc = null;
  draftPreviewAppearance = null;
  publishedEnvironmentThemes = [];
}

export function prefersLightColorScheme(): boolean {
  if (nativeOsTheme) return nativeOsTheme === "light";
  return typeof window !== "undefined"
    && typeof window.matchMedia === "function"
    && window.matchMedia(LIGHT_QUERY).matches;
}

function hasDesktopShell(): boolean {
  return typeof window !== "undefined"
    && isDesktopShell(Boolean((window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__));
}

export function subscribeSystemColorScheme(onChange: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  const media = typeof window.matchMedia === "function" ? window.matchMedia(LIGHT_QUERY) : null;
  const handleMedia = () => {
    nativeOsTheme = null;
    onChange();
  };
  media?.addEventListener?.("change", handleMedia);

  let active = true;
  let unlistenNative: (() => void) | null = null;
  if (hasDesktopShell()) {
    try {
      const win = getCurrentWindow();
      if (typeof win?.onThemeChanged === "function") void win
        .onThemeChanged((event) => {
          if (event.payload !== "light" && event.payload !== "dark") return;
          nativeOsTheme = event.payload;
          onChange();
        })
        .then(
          (unlisten) => {
            if (active) unlistenNative = unlisten;
            else unlisten();
          },
          (error: unknown) => console.warn("Ferryx could not follow the OS theme event", error),
        );
    } catch (error) {
      console.warn("Ferryx could not follow the OS theme event", error);
    }
  }

  return () => {
    active = false;
    media?.removeEventListener?.("change", handleMedia);
    unlistenNative?.();
    unlistenNative = null;
  };
}

function resolvedTheme(theme: AppearanceTheme): Exclude<AppearanceTheme, "system"> {
  if (theme !== "system") return theme;
  return prefersLightColorScheme() ? "light" : "dark";
}

export function resolveTerminalAppearance(theme: AppearanceTheme, prefersLight: boolean): "light" | "dark" {
  if (theme === "light") return "light";
  if (theme === "system") return prefersLight ? "light" : "dark";
  return "dark";
}

export function resolveThemeDocumentById(
  id: string | null | undefined,
  customThemes: ReadonlyArray<ThemeDocumentV1>,
): ThemeDocumentV1 | null {
  if (!id) return null;
  const saved = customThemes.find((doc) => doc.id === id);
  if (saved) return saved;
  const published = publishedEnvironmentThemes.find((item) => item.doc.id === id);
  if (published) return published.doc;
  const curated = getCuratedTheme(id);
  if (curated) return themeDefinitionToDocument(curated);
  return null;
}

export function resolveActiveThemeChoice(settings: AppearanceSettingsState): {
  doc: ThemeDocumentV1 | null;
  appearance: ThemeAppearance;
  effectiveTheme: Exclude<AppearanceTheme, "system">;
} {
  if (draftPreviewDoc) {
    const appearance = draftPreviewAppearance ?? draftPreviewDoc.base;
    return { doc: draftPreviewDoc, appearance, effectiveTheme: appearance };
  }
  const baseResolved = resolvedTheme(settings.theme);
  const targetAppearance: ThemeAppearance = baseResolved === "light" ? "light" : "dark";

  if (settings.themeHalves) {
    const halfId = settings.themeHalves[targetAppearance];
    if (halfId) {
      if (halfId === "charcoal" || halfId === "dark" || halfId === "light") {
        return { doc: null, appearance: halfId === "light" ? "light" : "dark", effectiveTheme: halfId };
      }
      const halfDoc = resolveThemeDocumentById(halfId, settings.customThemes);
      if (halfDoc) {
        const hasTarget = halfDoc.base === targetAppearance || Boolean(halfDoc.variants?.[targetAppearance]);
        const finalAppearance = hasTarget ? targetAppearance : halfDoc.base;
        return { doc: halfDoc, appearance: finalAppearance, effectiveTheme: finalAppearance };
      }
    }
  }

  const activeDoc = resolveThemeDocumentById(settings.activeCustomThemeId, settings.customThemes);
  if (activeDoc) {
    const hasTargetVariant =
      activeDoc.base !== targetAppearance && Boolean(activeDoc.variants?.[targetAppearance]);
    const effectiveAppearance =
      settings.theme === "system" && hasTargetVariant
        ? targetAppearance
        : activeDoc.base;
    return { doc: activeDoc, appearance: effectiveAppearance, effectiveTheme: effectiveAppearance };
  }

  return { doc: null, appearance: targetAppearance, effectiveTheme: baseResolved };
}

export function resolveActiveTerminalPalette(settings: AppearanceSettingsState): TerminalThemePalette | null {
  const { doc, appearance } = resolveActiveThemeChoice(settings);
  if (!doc) return null;
  const activeTokens =
    appearance !== doc.base && doc.variants?.[appearance]
      ? doc.variants[appearance]!
      : doc.tokens;
  const activeRoles =
    appearance !== doc.base && doc.variantRoles?.[appearance]
      ? doc.variantRoles[appearance]
      : doc.roles;
  if (!activeRoles?.terminalBackground && !activeTokens["--terminal"]) {
    return null;
  }
  const roles = resolveDocumentRoles(doc, appearance);
  return themeColorsToTerminalPalette(roles);
}

type AppliedCustomAppearance = {
  originalProperties: Map<string, { value: string; priority: string }>;
  styles: HTMLStyleElement[];
  observer?: MutationObserver;
  markedScopes: Set<HTMLElement>;
};
const appliedCustomAppearance = new WeakMap<HTMLElement, AppliedCustomAppearance>();

function clearCustomAppearance(root: HTMLElement) {
  clearAppThemeCss(root.ownerDocument);
  const previous = appliedCustomAppearance.get(root);
  if (!previous) return;
  previous.observer?.disconnect();
  for (const [token, original] of previous.originalProperties) {
    if (original.value) root.style.setProperty(token, original.value, original.priority);
    else root.style.removeProperty(token);
  }
  for (const style of previous.styles) style.remove();
  for (const scope of previous.markedScopes) scope.removeAttribute("data-chat-scope");
  delete root.dataset.customTheme;
  appliedCustomAppearance.delete(root);
}

function markChatScopes(root: HTMLElement, applied: AppliedCustomAppearance) {
  const mark = () => {
    for (const node of root.querySelectorAll<HTMLElement>('[data-testid="mobile-chat-workspace"]')) {
      if (!node.hasAttribute("data-chat-scope")) {
        node.setAttribute("data-chat-scope", "");
        applied.markedScopes.add(node);
      }
    }
  };
  mark();
  const Observer = root.ownerDocument.defaultView?.MutationObserver;
  if (Observer) {
    applied.observer = new Observer(mark);
    applied.observer.observe(root, { childList: true, subtree: true });
  }
}

export function applyThemeBackgroundUrl(
  url: string | null,
  root: HTMLElement | null = typeof document !== "undefined" ? document.documentElement : null,
): void {
  if (!root) return;
  if (!url) {
    root.style.removeProperty("--theme-background-image");
  } else {
    root.style.setProperty("--theme-background-image", `url("${url.replace(/"/g, '\\"')}")`);
  }
}

export function applyAppearanceSettings(
  settings: AppearanceSettingsState,
  root: HTMLElement | null = typeof document !== "undefined" ? document.documentElement : null,
) {
  if (!root) return;
  const choice = resolveActiveThemeChoice(settings);
  const candidate = choice.doc;
  const parsed = candidate ? parseThemeDocument(candidate) : null;
  let activeTheme = parsed?.ok ? parsed.doc : null;
  const errors: CustomCssError[] = parsed && !parsed.ok ? parsed.errors : [];
  const scopeId = activeTheme?.id ?? "custom-css";

  const isAppScopedCss = activeTheme?.cssScope === "app";
  const themeCss = !isAppScopedCss
    ? scopeCustomCss(activeTheme?.css ?? "", scopeId)
    : ({ ok: true as const, css: "" });
  if (!themeCss.ok) {
    errors.push(...themeCss.errors.map((error) => ({ ...error, path: `theme.css.${error.path}` })));
    activeTheme = null;
  }
  const customCss = scopeCustomCss(settings.customCss, scopeId);
  if (!customCss.ok) errors.push(...customCss.errors.map((error) => ({ ...error, path: `customCss.${error.path}` })));

  clearCustomAppearance(root);
  const fallbackTheme = choice.doc ? resolvedTheme(settings.theme) : choice.effectiveTheme;
  const activeAppearance: ThemeAppearance | null = activeTheme
    ? (choice.appearance !== activeTheme.base && activeTheme.variants?.[choice.appearance]
        ? choice.appearance
        : activeTheme.base)
    : null;
  const theme = activeAppearance ?? fallbackTheme;
  root.dataset.theme = theme;
  root.dataset.themePreference = settings.theme;
  root.dataset.accent = settings.accentColor;
  root.dataset.density = settings.density;
  root.style.colorScheme = theme === "light" ? "light" : "dark";
  root.classList.toggle("dark", theme !== "light");

  const activeTokens: Record<string, string> =
    activeTheme && activeAppearance
      ? (activeAppearance !== activeTheme.base && activeTheme.variants?.[activeAppearance]
          ? activeTheme.variants[activeAppearance]!
          : activeTheme.tokens)
      : {};

  const cssSources = [
    activeTheme && !isAppScopedCss && themeCss.ok ? themeCss.css : "",
    customCss.ok ? customCss.css : "",
  ];
  const hasAppCss = Boolean(activeTheme && isAppScopedCss && activeTheme.css);

  if (activeTheme || cssSources.some(Boolean)) {
    const applied: AppliedCustomAppearance = { originalProperties: new Map(), styles: [], markedScopes: new Set() };
    appliedCustomAppearance.set(root, applied);
    root.dataset.customTheme = scopeId;
    const remoteRootDecls: string[] = [];

    for (const [token, value] of Object.entries(activeTokens)) {
      const color = parseThemeColor(value);
      if (!color) continue;
      const pairs: Array<[string, string]> = [
        [token, value],
        [`${token}-rgb`, color.slice(0, 3).join(" ")],
      ];
      if (ALPHA_COMPANION_TOKENS.has(token)) {
        pairs.push([`${token}-alpha`, String(Number(color[3].toFixed(4)))]);
      }
      for (const [name, resolved] of pairs) {
        applied.originalProperties.set(name, {
          value: root.style.getPropertyValue(name),
          priority: root.style.getPropertyPriority(name),
        });
        root.style.setProperty(name, resolved);
        remoteRootDecls.push(`${name}: ${resolved};`);
      }
    }

    if (remoteRootDecls.length > 0) {
      const mirrorStyle = root.ownerDocument.createElement("style");
      mirrorStyle.dataset.customThemeRemoteMirror = scopeId;
      mirrorStyle.textContent = `:root[data-custom-theme="${scopeId}"] .remote-app-root { ${remoteRootDecls.join(" ")} }`;
      (root.ownerDocument.head ?? root).appendChild(mirrorStyle);
      applied.styles.push(mirrorStyle);
    }

    if (hasAppCss && activeTheme?.css) {
      applyAppThemeCss(activeTheme.css, root.ownerDocument);
    }

    if (cssSources.some(Boolean)) {
      markChatScopes(root, applied);
      for (const css of cssSources) {
        if (!css) continue;
        const style = root.ownerDocument.createElement("style");
        style.dataset.customThemeCss = scopeId;
        style.textContent = css;
        (root.ownerDocument.head ?? root).appendChild(style);
        applied.styles.push(style);
      }
    }
  }
  return { errors };
}

export function useAppearanceSettings() {
  const [loaded, setLoaded] = useState<AppearanceSettingsLoadResult>(loadAppearanceSettingsState);
  const settings = loaded.settings;

  useEffect(() => {
    const sync = () => setLoaded(loadAppearanceSettingsState());
    const storage = (event: StorageEvent) => {
      if (event.key === APPEARANCE_SETTINGS_STORAGE_KEY || event.key === null) setLoaded(loadAppearanceSettingsState());
    };
    window.addEventListener(APPEARANCE_SETTINGS_EVENT, sync);
    window.addEventListener("storage", storage);
    const unsubEnv = subscribePublishedEnvironmentThemes(sync);
    return () => {
      window.removeEventListener(APPEARANCE_SETTINGS_EVENT, sync);
      window.removeEventListener("storage", storage);
      unsubEnv();
    };
  }, []);

  const updateSettings = useCallback((patch: Partial<AppearanceSettingsState>) => {
    const next = saveAppearanceSettings(patch);
    setLoaded({ ...loadAppearanceSettingsState(), settings: next });
    return next;
  }, []);

  const resetSettings = useCallback(() => {
    clearThemeDraftPreview();
    const next = resetAppearanceSettings();
    setLoaded({ ...loadAppearanceSettingsState(), settings: next });
    return next;
  }, []);

  return { settings, invalidThemes: loaded.invalidThemes, updateSettings, resetSettings };
}

export function useApplyAppearanceSettings() {
  const { settings } = useAppearanceSettings();
  const envThemes = usePublishedEnvironmentThemes();

  useEffect(() => {
    applyAppearanceSettings(settings);
    if (settings.theme !== "system") return;
    return subscribeSystemColorScheme(() => applyAppearanceSettings(settings));
  }, [settings, envThemes]);

  return settings;
}

export function useResolvedTheme(theme: AppearanceTheme): Exclude<AppearanceTheme, "system"> {
  const [resolved, setResolved] = useState(() => resolvedTheme(theme));

  useEffect(() => {
    setResolved(resolvedTheme(theme));
    if (theme !== "system") return;
    return subscribeSystemColorScheme(() => setResolved(resolvedTheme(theme)));
  }, [theme]);

  return resolved;
}
