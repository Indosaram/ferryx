import { useEffect, useRef, useState } from "react";
import { Palette, RotateCcw } from "lucide-react";

import {
  useAppearanceSettings,
  useResolvedTheme,
  type AppearanceSettingsState,
  type InvalidAppearanceTheme,
} from "../../lib/appearanceSettings";
import { parseThemeColor, parseThemeDocument, THEME_TOKEN_ALLOWLIST, type ThemeDocumentV1, type ThemeValidationError } from "../../lib/themeDocument";
import { scopeCustomCss } from "../../lib/customThemeCss";
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

export function AppearanceSection() {
  const { settings, invalidThemes, updateSettings, resetSettings } = useAppearanceSettings();
  const resolvedTheme = useResolvedTheme(settings.theme);

  return (
    <section aria-labelledby="settings-appearance-heading">
      <SettingsHeading
        icon={<Palette />}
        title="Appearance"
        description="Customize the desktop theme palette, accent colors, and interface scale density."
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
      <ThemeLibrary settings={settings} invalidThemes={invalidThemes} updateSettings={updateSettings} resolvedTheme={resolvedTheme} />
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
  return errors.length ? <ul role="alert" className="space-y-1 text-sm text-destructive">
    {errors.map((error, index) => <li key={`${error.path}-${index}`}>{error.path}: {error.message}</li>)}
  </ul> : null;
}

// The desktop webview has no browser download handler. Reuse its existing text-file saver.
async function downloadText(fileName: string, text: string, type: string) {
  if (isTauriRuntime()) return saveSupervisorTranscriptFile(fileName, text);
  const url = URL.createObjectURL(new Blob([text], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  try { link.click(); } finally {
    link.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  }
}

export function ThemeLibrary({ settings, invalidThemes, updateSettings, resolvedTheme }: ThemeLibraryProps) {
  const [json, setJson] = useState("");
  const [errors, setErrors] = useState<ThemeValidationError[]>([]);
  const [status, setStatus] = useState("");
  const [cssOpen, setCssOpen] = useState(false);
  const [css, setCss] = useState(settings.customCss);
  const [cssErrors, setCssErrors] = useState<ThemeValidationError[]>([]);
  const [renameId, setRenameId] = useState<string | null>(null);
  const [name, setName] = useState("");
  const fileInput = useRef<HTMLInputElement>(null);
  const active = settings.customThemes.find((theme) => theme.id === settings.activeCustomThemeId);
  useEffect(() => { setCss(settings.customCss); }, [settings.customCss]);

  function acceptDocument(raw: unknown) {
    const parsed = parseThemeDocument(raw);
    if (!parsed.ok) { setErrors(parsed.errors); return; }
    const scoped = scopeCustomCss(parsed.doc.css ?? "", parsed.doc.id);
    if (!scoped.ok) {
      setErrors(scoped.errors.map((error) => ({ ...error, path: `theme.css.${error.path}` })));
      return;
    }
    // Reimporting an exported id replaces that document, never creates duplicate ids.
    updateSettings({
      customThemes: [...settings.customThemes.filter((theme) => theme.id !== parsed.doc.id), parsed.doc],
      activeCustomThemeId: parsed.doc.id,
    });
    setErrors([]);
    setStatus(`Activated ${parsed.doc.name}.`);
  }

  function importJson(text: string) {
    let raw: unknown;
    try { raw = JSON.parse(text); } catch {
      setErrors([{ path: "$", message: "Expected valid JSON." }]);
      return;
    }
    acceptDocument(raw);
  }

  async function importFile(file: File) {
    try { importJson(await file.text()); } catch (error) {
      setErrors([{ path: "$", message: `Could not read theme file: ${String(error)}` }]);
    }
  }

  function currentDocument(): ThemeDocumentV1 {
    if (active) return active;
    const tokens: Record<string, string> = {};
    const computed = getComputedStyle(document.documentElement);
    for (const token of THEME_TOKEN_ALLOWLIST) {
      const value = computed.getPropertyValue(token).trim();
      if (parseThemeColor(value)) tokens[token] = value;
    }
    return { version: 1, id: `builtin-${settings.theme}-${settings.accentColor}`,
      name: `${settings.theme} / ${settings.accentColor}`, base: resolvedTheme === "light" ? "light" : "dark", tokens };
  }

  function authorTheme() {
    const doc = currentDocument();
    // Authoring stays JSON-based: seed an editable draft, and save only after validation.
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
    if (!parsed.ok) { setErrors(parsed.errors); return; }
    void exportText(`${parsed.doc.id}.ferryx-theme.json`, JSON.stringify(parsed.doc, null, 2), "application/json");
  }

  function renameTheme(theme: ThemeDocumentV1) {
    const parsed = parseThemeDocument({ ...theme, name });
    if (!parsed.ok) { setErrors(parsed.errors); return; }
    updateSettings({ customThemes: settings.customThemes.map((row) => row.id === theme.id ? parsed.doc : row) });
    setRenameId(null);
    setErrors([]);
  }

  return <SettingsGroup title="Theme library" description="Import or author a JSON theme. Theme CSS runs first; your standalone custom CSS runs last, inside chat only.">
    <ul aria-label="Themes" className="space-y-2">
      {builtInThemes.map((theme) => <li key={theme}>
        <button type="button" className={controlClass} aria-pressed={!active && settings.theme === theme}
          onClick={() => updateSettings({ theme, activeCustomThemeId: null })}>Use {theme}</button>
      </li>)}
      {settings.customThemes.map((theme) => <li key={theme.id} className="flex flex-wrap items-center gap-2">
        <button type="button" className={controlClass} aria-pressed={active?.id === theme.id}
          onClick={() => updateSettings({ activeCustomThemeId: theme.id })}>Use {theme.name}</button>
        {renameId === theme.id ? <>
          <input className={controlClass} aria-label={`Name for ${theme.name}`} value={name} onChange={(event) => setName(event.target.value)} />
          <button type="button" className={controlClass} onClick={() => renameTheme(theme)}>Save name</button>
          <button type="button" className={controlClass} onClick={() => setRenameId(null)}>Cancel rename</button>
        </> : <button type="button" className={controlClass} aria-label={`Rename ${theme.name}`}
          onClick={() => { setRenameId(theme.id); setName(theme.name); }}>Rename</button>}
        <button type="button" className={controlClass} aria-label={`Delete ${theme.name}`} onClick={() => {
          updateSettings({ customThemes: settings.customThemes.filter((row) => row.id !== theme.id),
            ...(active?.id === theme.id ? { activeCustomThemeId: null } : {}) });
          if (renameId === theme.id) setRenameId(null);
        }}>Delete</button>
      </li>)}
      {invalidThemes.map((row, index) => <li key={index} className="rounded-md border border-destructive p-3">
        <p>Stored theme {index + 1} could not be loaded. Original data retained.</p>
        <FieldErrors errors={row.errors} />
        <details><summary>Original theme data</summary><pre className="selectable whitespace-pre-wrap break-all">{JSON.stringify(row.raw, null, 2)}</pre></details>
      </li>)}
    </ul>
    <div className="my-3 flex flex-wrap gap-2">
      <button type="button" className={controlClass} onClick={() => fileInput.current?.click()}>Import theme...</button>
      <input ref={fileInput} type="file" accept=".json,.ferryx-theme.json,application/json" aria-label="Import theme file" className="sr-only"
        onChange={(event) => { const file = event.target.files?.[0]; event.target.value = ""; if (file) void importFile(file); }} />
      <button type="button" className={controlClass} onClick={exportTheme}>Export active theme</button>
      <button type="button" className={controlClass} onClick={authorTheme}>New theme from current appearance</button>
    </div>
    <label className="block text-sm" htmlFor="theme-library-json">Theme JSON</label>
    <textarea id="theme-library-json" className={`${controlClass} my-2 min-h-40 w-full font-mono`} value={json} onChange={(event) => setJson(event.target.value)} />
    <button type="button" className={controlClass} onClick={() => importJson(json)}>Import pasted JSON</button>
    <FieldErrors errors={errors} />
    <p role="status" className="my-2 text-sm">{status}</p>
    <SettingRow label="Render math in chat" description="Typeset formulas in assistant messages.">
      <input type="checkbox" aria-label="Render math in chat" className="size-11" checked={settings.renderMath}
        onChange={(event) => updateSettings({ renderMath: event.target.checked })} />
    </SettingRow>
    <button type="button" className={controlClass} aria-expanded={cssOpen} onClick={() => setCssOpen(!cssOpen)}>Custom CSS...</button>
    {cssOpen ? <div className="mt-3 space-y-2">
      <p className="text-sm">Theme CSS source: {active?.name ?? "Built-in (no document CSS)"}. Exported with the theme; applied before standalone CSS.</p>
      {active?.css ? <pre aria-label="Theme CSS source" className="selectable whitespace-pre-wrap break-all">{active.css}</pre> : null}
      <label htmlFor="appearance-custom-css" className="block text-sm">Standalone custom CSS (applied last)</label>
      <textarea id="appearance-custom-css" className={`${controlClass} min-h-40 w-full font-mono`} value={css} onChange={(event) => setCss(event.target.value)} />
      <button type="button" className={controlClass} onClick={() => {
        const result = scopeCustomCss(css, active?.id ?? "custom-css");
        if (!result.ok) { setCssErrors(result.errors.map((error) => ({ ...error, path: `customCss.${error.path}` }))); return; }
        updateSettings({ customCss: css }); setCssErrors([]);
      }}>Apply custom CSS</button>
      <button type="button" className={controlClass} onClick={() => void exportText("custom.ferryx.css", settings.customCss, "text/css")}>Export custom CSS</button>
      <FieldErrors errors={cssErrors} />
    </div> : null}
    <button type="button" className={`${controlClass} ml-2`} onClick={() => {
      updateSettings({ customCss: "" }); setCss(""); setCssErrors([]);
    }}>Remove custom CSS</button>
  </SettingsGroup>;
}

export { AppearanceSection as AppearanceSettings };
export default AppearanceSection;
