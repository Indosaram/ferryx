import { useEffect, useMemo, useRef, useState } from "react";
import { Copy, FileUp, Palette, Pencil, Plus, RotateCcw, Sparkles } from "lucide-react";

import {
  useAppearanceSettings,
  usePublishedEnvironmentThemes,
  useResolvedTheme,
  type AppearanceSettingsState,
  type InvalidAppearanceTheme,
} from "../../lib/appearanceSettings";
import {
  parseThemeColor,
  parseThemeDocument,
  THEME_TOKEN_ALLOWLIST,
  type ThemeDocumentV1,
  type ThemeValidationError,
} from "../../lib/themeDocument";
import { scopeCustomCss } from "../../lib/customThemeCss";
import {
  CURATED_THEMES,
  getCuratedTheme,
  resolveDocumentRoles,
  serializeToT3ThemeFile,
  T3_CODE_DARK_THEME_COLORS,
  T3_CODE_LIGHT_THEME_COLORS,
  themeDefinitionToDocument,
  type ThemeAppearance,
} from "../../lib/themePalettes";
import { isT3ThemeFormat, isVsCodeThemeFile, parseAnyThemeImport } from "../../lib/vscodeThemeImport";
import { isTauriRuntime, saveSupervisorTranscriptFile } from "../../lib/tauri";
import { Button } from "../ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import { SettingRow, SettingsGroup, SettingsHeading } from "./primitives";
import { AppearanceChatPreview } from "./AppearanceChatPreview";
import { ThemeEditorPanel } from "./ThemeEditorPanel";
import { ThemeImportDialog } from "./ThemeImportDialog";
import { ThemePreviewCircles } from "./ThemePreviewCircles";
import { ThemeSearchSection } from "./ThemeSearchSection";
import { T3ThemeStoreSection } from "./T3ThemeStoreSection";

export function AppearanceSection() {
  const { settings, invalidThemes, updateSettings, resetSettings } = useAppearanceSettings();
  const resolvedTheme = useResolvedTheme(settings.theme);

  return (
    <section aria-labelledby="settings-appearance-heading">
      <SettingsHeading
        icon={<Palette />}
        title="Appearance"
        description="Customize the theme palette, light/dark theme halves, accent colors, custom CSS, and interface density."
      />
      <h2 id="settings-appearance-heading" className="sr-only">
        Appearance
      </h2>
      <SettingsGroup
        title="Display & Styling"
        action={
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={resetSettings}
            className="no-drag h-7 shrink-0 gap-1.5 px-2 text-[11px] text-muted-foreground hover:text-foreground"
          >
            <RotateCcw className="size-3" />
            Reset to defaults
          </Button>
        }
      >
        <div className="border-b border-border">
          <SettingRow
            label="Theme mode"
            description={
              settings.theme === "system"
                ? `Follows your OS appearance, terminal included. Currently ${resolvedTheme === "light" ? "Light" : "Dark"}.`
                : "Select the base color theme for the desktop interface."
            }
          >
            <Select
              value={settings.theme}
              onValueChange={(val) =>
                updateSettings({
                  theme: val as AppearanceSettingsState["theme"],
                  activeCustomThemeId: null,
                  themeHalves: undefined,
                })
              }
            >
              <SelectTrigger
                id="appearance-theme-mode"
                aria-label="Theme mode"
                className="h-8 w-[180px] text-[11px]"
              >
                <SelectValue placeholder="Theme mode" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="charcoal">Charcoal (Default)</SelectItem>
                <SelectItem value="dark">Dark</SelectItem>
                <SelectItem value="light">Light</SelectItem>
                <SelectItem value="system">System (follow OS)</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>

          <SettingRow
            label="Accent color"
            description="Accent highlight color used for active badges, selection rings, and indicators."
          >
            <Select
              value={settings.accentColor}
              onValueChange={(val) =>
                updateSettings({
                  accentColor: val as AppearanceSettingsState["accentColor"],
                })
              }
            >
              <SelectTrigger
                id="appearance-accent-color"
                aria-label="Accent color"
                className="h-8 w-[180px] text-[11px]"
              >
                <SelectValue placeholder="Accent color" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="default">Slate (Default)</SelectItem>
                <SelectItem value="blue">Ocean Blue</SelectItem>
                <SelectItem value="emerald">Emerald</SelectItem>
                <SelectItem value="purple">Violet</SelectItem>
                <SelectItem value="amber">Amber</SelectItem>
                <SelectItem value="rose">Rose</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>

          <SettingRow
            label="Interface density"
            description="Adjust spacing and padding across tabs, sidebars, and dialog chrome."
          >
            <Select
              value={settings.density}
              onValueChange={(val) =>
                updateSettings({
                  density: val as AppearanceSettingsState["density"],
                })
              }
            >
              <SelectTrigger
                id="appearance-density"
                aria-label="Interface density"
                className="h-8 w-[180px] text-[11px]"
              >
                <SelectValue placeholder="Interface density" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="compact">Compact (Default)</SelectItem>
                <SelectItem value="comfortable">Comfortable</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>
        </div>
      </SettingsGroup>
      <AppearanceChatPreview />
      <ThemeLibrary
        settings={settings}
        invalidThemes={invalidThemes}
        updateSettings={updateSettings}
        resolvedTheme={resolvedTheme}
      />
    </section>
  );
}

const controlClass = "min-h-11 rounded-md border border-border bg-background px-3 text-sm";
const builtInThemes = ["charcoal", "dark", "light", "system"] as const;

export type ThemeLibraryProps = {
  settings: AppearanceSettingsState;
  invalidThemes: InvalidAppearanceTheme[];
  updateSettings: (patch: Partial<AppearanceSettingsState>) => unknown;
  resolvedTheme: "charcoal" | "dark" | "light";
};

function FieldErrors({ errors }: { errors: ThemeValidationError[] }) {
  return errors.length ? (
    <ul role="alert" className="space-y-1 text-sm text-destructive">
      {errors.map((error, index) => (
        <li key={`${error.path}-${index}`}>
          {error.path}: {error.message}
        </li>
      ))}
    </ul>
  ) : null;
}

async function downloadText(fileName: string, text: string, type: string) {
  if (isTauriRuntime()) return saveSupervisorTranscriptFile(fileName, text);
  const url = URL.createObjectURL(new Blob([text], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  try {
    link.click();
  } finally {
    link.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  }
}

export function ThemeLibrary({
  settings,
  invalidThemes,
  updateSettings,
  resolvedTheme,
}: ThemeLibraryProps) {
  const publishedThemes = usePublishedEnvironmentThemes();
  const [json, setJson] = useState("");
  const [errors, setErrors] = useState<ThemeValidationError[]>([]);
  const [status, setStatus] = useState("");
  const [cssOpen, setCssOpen] = useState(false);
  const [css, setCss] = useState(settings.customCss);
  const [cssErrors, setCssErrors] = useState<ThemeValidationError[]>([]);
  const [renameId, setRenameId] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [editorTarget, setEditorTarget] = useState<ThemeDocumentV1 | null | undefined>(undefined);
  const [importDialogOpen, setImportDialogOpen] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);

  const active = useMemo(() => {
    const saved = settings.customThemes.find((theme) => theme.id === settings.activeCustomThemeId);
    if (saved) return saved;
    const published = publishedThemes.find((item) => item.doc.id === settings.activeCustomThemeId);
    if (published) return published.doc;
    const curated = settings.activeCustomThemeId ? getCuratedTheme(settings.activeCustomThemeId) : null;
    return curated ? themeDefinitionToDocument(curated) : undefined;
  }, [publishedThemes, settings.activeCustomThemeId, settings.customThemes]);

  const existingIds = useMemo(
    () => new Set(settings.customThemes.map((t) => t.id)),
    [settings.customThemes],
  );

  useEffect(() => {
    setCss(settings.customCss);
  }, [settings.customCss]);

  function saveAndActivateDocument(doc: ThemeDocumentV1) {
    updateSettings({
      customThemes: [...settings.customThemes.filter((theme) => theme.id !== doc.id), doc],
      activeCustomThemeId: doc.id,
      themeHalves: undefined,
    });
    setErrors([]);
    setStatus(`Activated ${doc.name}.`);
  }

  function installDocuments(docs: ThemeDocumentV1[]) {
    const byId = new Map(settings.customThemes.map((theme) => [theme.id, theme]));
    for (const doc of docs) byId.set(doc.id, doc);
    const first = docs[0];
    updateSettings({
      customThemes: Array.from(byId.values()),
      ...(first ? { activeCustomThemeId: first.id, themeHalves: undefined } : {}),
    });
    setErrors([]);
    if (first) setStatus(`Installed and activated ${first.name}.`);
  }

  function acceptDocument(raw: unknown) {
    if (isVsCodeThemeFile(raw) || isT3ThemeFormat(raw)) {
      const converted = parseAnyThemeImport(JSON.stringify(raw));
      if (!converted.ok) {
        setErrors(converted.errors);
        return;
      }
      saveAndActivateDocument(converted.doc);
      return;
    }
    const parsed = parseThemeDocument(raw);
    if (!parsed.ok) {
      setErrors(parsed.errors);
      return;
    }
    if (parsed.doc.cssScope !== "app") {
      const scoped = scopeCustomCss(parsed.doc.css ?? "", parsed.doc.id);
      if (!scoped.ok) {
        setErrors(scoped.errors.map((error) => ({ ...error, path: `theme.css.${error.path}` })));
        return;
      }
    }
    saveAndActivateDocument(parsed.doc);
  }

  function importJson(text: string) {
    let raw: unknown;
    try {
      raw = JSON.parse(text);
    } catch {
      const fallback = parseAnyThemeImport(text);
      if (fallback.ok) {
        saveAndActivateDocument(fallback.doc);
        return;
      }
      setErrors([{ path: "$", message: "Expected valid JSON." }]);
      return;
    }
    acceptDocument(raw);
  }

  async function importFile(file: File) {
    try {
      importJson(await file.text());
    } catch (error) {
      setErrors([{ path: "$", message: `Could not read theme file: ${String(error)}` }]);
    }
  }

  function selectThemeHalf(themeId: string, appearance: ThemeAppearance) {
    const nextHalves = {
      ...(settings.themeHalves ?? {}),
      [appearance]: themeId,
    };
    updateSettings({
      theme: "system",
      activeCustomThemeId: themeId,
      themeHalves: nextHalves,
    });
    setStatus(`Assigned ${themeId} to ${appearance} mode.`);
  }

  function currentDocument(): ThemeDocumentV1 {
    if (active) return active;
    const tokens: Record<string, string> = {};
    const computed = getComputedStyle(document.documentElement);
    for (const token of THEME_TOKEN_ALLOWLIST) {
      const value = computed.getPropertyValue(token).trim();
      if (parseThemeColor(value)) tokens[token] = value;
    }
    return {
      version: 1,
      id: `builtin-${settings.theme}-${settings.accentColor}`,
      name: `${settings.theme} / ${settings.accentColor}`,
      base: resolvedTheme === "light" ? "light" : "dark",
      tokens,
    };
  }

  function authorTheme() {
    const doc = currentDocument();
    setJson(JSON.stringify({ ...doc, id: `theme-${crypto.randomUUID()}`, name: "My theme" }, null, 2));
    setErrors([]);
    setStatus("New theme draft ready in Theme JSON. Edit it, then import it to save.");
  }

  async function exportText(fileName: string, text: string, type: string) {
    try {
      await downloadText(fileName, text, type);
      setStatus(`Exported ${fileName}.`);
    } catch (error) {
      setErrors([{ path: "export", message: `Could not export: ${String(error)}` }]);
    }
  }

  function exportTheme() {
    const parsed = parseThemeDocument(currentDocument());
    if (!parsed.ok) {
      setErrors(parsed.errors);
      return;
    }
    void exportText(
      `${parsed.doc.id}.ferryx-theme.json`,
      JSON.stringify(parsed.doc, null, 2),
      "application/json",
    );
  }

  function exportT3Theme() {
    const doc = currentDocument();
    void exportText(`${doc.id}.t3-theme.json`, serializeToT3ThemeFile(doc), "application/json");
  }

  function renameTheme(theme: ThemeDocumentV1) {
    const parsed = parseThemeDocument({ ...theme, name });
    if (!parsed.ok) {
      setErrors(parsed.errors);
      return;
    }
    updateSettings({
      customThemes: settings.customThemes.map((row) => (row.id === theme.id ? parsed.doc : row)),
    });
    setRenameId(null);
    setErrors([]);
  }

  function duplicateTheme(source: ThemeDocumentV1) {
    const copy: ThemeDocumentV1 = {
      ...source,
      id: `${source.id.slice(0, 50)}-copy-${crypto.randomUUID().slice(0, 6)}`,
      name: `${source.name} Copy`.slice(0, 64),
    };
    setEditorTarget(copy);
  }

  return (
    <SettingsGroup
      title="Theme library"
      description="Create custom OKLCH palettes, pick colors from the app, import T3 Code or VS Code themes, and write custom CSS."
      action={
        <div className="flex flex-wrap items-center gap-1.5">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setImportDialogOpen(true)}
            className="h-7 gap-1 px-2.5 text-[11px]"
          >
            <FileUp className="size-3" aria-hidden="true" />
            Import T3 / VS Code
          </Button>
          <Button
            type="button"
            size="sm"
            onClick={() => setEditorTarget(null)}
            className="h-7 gap-1 px-2.5 text-[11px]"
          >
            <Plus className="size-3" aria-hidden="true" />
            Create theme
          </Button>
        </div>
      }
    >
      <div className="space-y-4">
        {editorTarget !== undefined ? (
          <ThemeEditorPanel
            initialTheme={editorTarget}
            defaultAppearance={resolvedTheme === "light" ? "light" : "dark"}
            existingIds={existingIds}
            onSave={(doc) => {
              saveAndActivateDocument(doc);
              setEditorTarget(undefined);
            }}
            onCancel={() => setEditorTarget(undefined)}
          />
        ) : null}

        <div className="space-y-2">
          <p className="text-[12px] font-medium text-foreground">Curated T3 Code themes</p>
          <p className="text-[11px] text-muted-foreground">
            Click a theme to activate it, or click its light/dark circle to assign independent light and dark themes.
          </p>
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
            {CURATED_THEMES.map((curated) => {
              const isSelected = settings.activeCustomThemeId === curated.id;
              const selectedLight = settings.themeHalves?.light === curated.id || (isSelected && resolvedTheme === "light");
              const selectedDark = settings.themeHalves?.dark === curated.id || (isSelected && resolvedTheme !== "light");
              return (
                <div
                  key={curated.id}
                  className={`flex items-center justify-between gap-2 rounded-lg border px-3 py-2.5 transition-colors ${
                    isSelected ? "border-primary bg-accent/30" : "border-border bg-background/70"
                  }`}
                >
                  <button
                    type="button"
                    aria-pressed={isSelected}
                    onClick={() =>
                      updateSettings({
                        activeCustomThemeId: curated.id,
                        themeHalves: undefined,
                      })
                    }
                    className="min-w-0 flex-1 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <span className="block truncate text-[12px] font-medium text-foreground">
                      {curated.label}
                    </span>
                    <span className="block text-[11px] text-muted-foreground">
                      Light &amp; Dark
                    </span>
                  </button>
                  <ThemePreviewCircles
                    label={curated.label}
                    lightColors={curated.appearance === "light" ? curated.colors : curated.variants?.light}
                    darkColors={curated.appearance === "dark" ? curated.colors : curated.variants?.dark}
                    selectedLight={selectedLight}
                    selectedDark={selectedDark}
                    onSelectAppearance={(mode) => selectThemeHalf(curated.id, mode)}
                    size="sm"
                  />
                </div>
              );
            })}
          </div>
        </div>

        {publishedThemes.length > 0 ? (
          <div className="space-y-2">
            <p className="flex items-center gap-1.5 text-[12px] font-medium text-foreground">
              <Sparkles className="size-3.5 text-primary" aria-hidden="true" />
              Environment themes
            </p>
            <ul className="space-y-2">
              {publishedThemes.map(({ doc, hasBackground }) => {
                const isSelected = settings.activeCustomThemeId === doc.id;
                return (
                  <li
                    key={doc.id}
                    className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border bg-background/70 px-3 py-2"
                  >
                    <div className="flex items-center gap-2.5">
                      <ThemePreviewCircles
                        label={doc.name}
                        lightColors={
                          doc.base === "light" || doc.variants?.light
                            ? resolveDocumentRoles(doc, "light")
                            : undefined
                        }
                        darkColors={
                          doc.base === "dark" || doc.variants?.dark
                            ? resolveDocumentRoles(doc, "dark")
                            : undefined
                        }
                        size="sm"
                      />
                      <div>
                        <p className="text-[12px] font-medium text-foreground">{doc.name}</p>
                        <p className="text-[11px] text-muted-foreground">
                          Published · {doc.base}
                          {doc.css ? " · CSS" : ""}
                          {hasBackground ? " · Wallpaper" : ""}
                        </p>
                      </div>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <Button
                        type="button"
                        size="sm"
                        variant={isSelected ? "default" : "outline"}
                        aria-pressed={isSelected}
                        onClick={() => updateSettings({ activeCustomThemeId: doc.id, themeHalves: undefined })}
                        className="h-7 px-2.5 text-[11px]"
                      >
                        Use {doc.name}
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        aria-label={`Duplicate ${doc.name}`}
                        onClick={() => duplicateTheme(doc)}
                        className="h-7 gap-1 px-2 text-[11px]"
                      >
                        <Copy className="size-3" aria-hidden="true" />
                        Duplicate
                      </Button>
                    </div>
                  </li>
                );
              })}
            </ul>
          </div>
        ) : null}

        <ul aria-label="Themes" className="space-y-2">
          <li className="flex flex-wrap items-center gap-2">
            {builtInThemes.map((theme) => (
              <button
                key={theme}
                type="button"
                className={`${controlClass} inline-flex items-center gap-2`}
                aria-pressed={!active && settings.theme === theme}
                onClick={() =>
                  updateSettings({ theme, activeCustomThemeId: null, themeHalves: undefined })
                }
              >
                <ThemePreviewCircles
                  label={theme}
                  lightColors={theme === "light" || theme === "system" ? T3_CODE_LIGHT_THEME_COLORS : undefined}
                  darkColors={theme !== "light" ? T3_CODE_DARK_THEME_COLORS : undefined}
                  size="sm"
                />
                <span>Use {theme}</span>
              </button>
            ))}
          </li>

          {settings.customThemes.map((theme) => (
            <li
              key={theme.id}
              className="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-background/60 p-2"
            >
              <ThemePreviewCircles
                label={theme.name}
                lightColors={
                  theme.base === "light" || theme.variants?.light
                    ? resolveDocumentRoles(theme, "light")
                    : undefined
                }
                darkColors={
                  theme.base === "dark" || theme.variants?.dark
                    ? resolveDocumentRoles(theme, "dark")
                    : undefined
                }
                selectedLight={settings.themeHalves?.light === theme.id}
                selectedDark={settings.themeHalves?.dark === theme.id}
                onSelectAppearance={(mode) => selectThemeHalf(theme.id, mode)}
                size="sm"
              />
              <button
                type="button"
                className={controlClass}
                aria-pressed={active?.id === theme.id}
                onClick={() => updateSettings({ activeCustomThemeId: theme.id, themeHalves: undefined })}
              >
                Use {theme.name}
              </button>
              <button
                type="button"
                className={`${controlClass} inline-flex items-center gap-1.5`}
                aria-label={`Edit ${theme.name}`}
                onClick={() => setEditorTarget(theme)}
              >
                <Pencil className="size-3.5" aria-hidden="true" />
                Edit
              </button>
              <button
                type="button"
                className={controlClass}
                aria-label={`Duplicate ${theme.name}`}
                onClick={() => duplicateTheme(theme)}
              >
                Duplicate
              </button>
              {renameId === theme.id ? (
                <>
                  <input
                    className={controlClass}
                    aria-label={`Name for ${theme.name}`}
                    value={name}
                    onChange={(event) => setName(event.target.value)}
                  />
                  <button type="button" className={controlClass} onClick={() => renameTheme(theme)}>
                    Save name
                  </button>
                  <button type="button" className={controlClass} onClick={() => setRenameId(null)}>
                    Cancel rename
                  </button>
                </>
              ) : (
                <button
                  type="button"
                  className={controlClass}
                  aria-label={`Rename ${theme.name}`}
                  onClick={() => {
                    setRenameId(theme.id);
                    setName(theme.name);
                  }}
                >
                  Rename
                </button>
              )}
              <button
                type="button"
                className={controlClass}
                aria-label={`Delete ${theme.name}`}
                onClick={() => {
                  updateSettings({
                    customThemes: settings.customThemes.filter((row) => row.id !== theme.id),
                    ...(active?.id === theme.id ? { activeCustomThemeId: null } : {}),
                  });
                  if (renameId === theme.id) setRenameId(null);
                }}
              >
                Delete
              </button>
            </li>
          ))}

          {invalidThemes.map((row, index) => (
            <li key={index} className="rounded-md border border-destructive p-3">
              <p>Stored theme {index + 1} could not be loaded. Original data retained.</p>
              <FieldErrors errors={row.errors} />
              <details>
                <summary>Original theme data</summary>
                <pre className="selectable whitespace-pre-wrap break-all">
                  {JSON.stringify(row.raw, null, 2)}
                </pre>
              </details>
            </li>
          ))}
        </ul>

        <ThemeSearchSection onInstallThemes={installDocuments} />

        <T3ThemeStoreSection onInstallThemes={installDocuments} />

        <div className="my-3 flex flex-wrap gap-2">
          <button type="button" className={controlClass} onClick={() => fileInput.current?.click()}>
            Import theme...
          </button>
          <input
            ref={fileInput}
            type="file"
            accept=".json,.jsonc,.ferryx-theme.json,application/json"
            aria-label="Import theme file"
            className="sr-only"
            onChange={(event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (file) void importFile(file);
            }}
          />
          <button type="button" className={controlClass} onClick={exportTheme}>
            Export active theme
          </button>
          <button type="button" className={controlClass} onClick={exportT3Theme}>
            Export as T3 Code JSON
          </button>
          <button type="button" className={controlClass} onClick={authorTheme}>
            New theme from current appearance
          </button>
        </div>

        <label className="block text-sm" htmlFor="theme-library-json">
          Theme JSON
        </label>
        <textarea
          id="theme-library-json"
          className={`${controlClass} my-2 min-h-40 w-full font-mono`}
          value={json}
          onChange={(event) => setJson(event.target.value)}
        />
        <button type="button" className={controlClass} onClick={() => importJson(json)}>
          Import pasted JSON
        </button>
        <FieldErrors errors={errors} />
        <p role="status" className="my-2 text-sm">
          {status}
        </p>

        <SettingRow label="Render math in chat" description="Typeset formulas in assistant messages.">
          <input
            type="checkbox"
            aria-label="Render math in chat"
            className="size-5 accent-primary"
            checked={settings.renderMath}
            onChange={(event) => updateSettings({ renderMath: event.target.checked })}
          />
        </SettingRow>

        <button
          type="button"
          className={controlClass}
          aria-expanded={cssOpen}
          onClick={() => setCssOpen(!cssOpen)}
        >
          Custom CSS...
        </button>
        {cssOpen ? (
          <div className="mt-3 space-y-2">
            <p className="text-sm">
              Theme CSS source: {active?.name ?? "Built-in (no document CSS)"}. Exported with the theme; applied before standalone CSS.
            </p>
            {active?.css ? (
              <pre aria-label="Theme CSS source" className="selectable whitespace-pre-wrap break-all">
                {active.css}
              </pre>
            ) : null}
            <label htmlFor="appearance-custom-css" className="block text-sm">
              Standalone custom CSS (applied last)
            </label>
            <textarea
              id="appearance-custom-css"
              className={`${controlClass} min-h-40 w-full font-mono`}
              value={css}
              onChange={(event) => setCss(event.target.value)}
            />
            <button
              type="button"
              className={controlClass}
              onClick={() => {
                const result = scopeCustomCss(css, active?.id ?? "custom-css");
                if (!result.ok) {
                  setCssErrors(
                    result.errors.map((error) => ({ ...error, path: `customCss.${error.path}` })),
                  );
                  return;
                }
                updateSettings({ customCss: css });
                setCssErrors([]);
              }}
            >
              Apply custom CSS
            </button>
            <button
              type="button"
              className={controlClass}
              onClick={() => void exportText("custom.ferryx.css", settings.customCss, "text/css")}
            >
              Export custom CSS
            </button>
            <FieldErrors errors={cssErrors} />
          </div>
        ) : null}
        <button
          type="button"
          className={`${controlClass} ml-2`}
          onClick={() => {
            updateSettings({ customCss: "" });
            setCss("");
            setCssErrors([]);
          }}
        >
          Remove custom CSS
        </button>
      </div>

      <ThemeImportDialog
        open={importDialogOpen}
        onOpenChange={setImportDialogOpen}
        existingIds={existingIds}
        onImport={(doc) => saveAndActivateDocument(doc)}
      />
    </SettingsGroup>
  );
}

export { AppearanceSection as AppearanceSettings };
export default AppearanceSection;
