import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "../../i18n";
import { useTerminalSettings, type TerminalSettings } from "../../lib/terminalSettings";
import { Button } from "../ui/button";

export type ImportPreview = {
  settings: Pick<TerminalSettings, "fontFamily" | "fontSize" | "cursorStyle" | "theme"> & { scrollback: number | null };
  skipped: string[];
};

export function ImportSection() {
  const { t } = useTranslation();
  const { updateSettings } = useTerminalSettings();
  const [format, setFormat] = useState("ghostty");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [applied, setApplied] = useState(false);
  const request = useRef(0);
  return <section className="space-y-4">
    <h2>{t("settings.import.title")}</h2>
    <p>{t("settings.import.description")}</p>
    <label>{t("settings.import.format")} <select value={format} onChange={(e) => {
      request.current += 1; setFormat(e.target.value); setPreview(null); setError(null); setBusy(false); setApplied(false);
    }}>
      <option value="ghostty">Ghostty</option><option value="iterm2">iTerm2</option><option value="wezterm">WezTerm</option>
    </select></label>
    <label>{t("settings.import.file")} <input type="file" onChange={async (event) => {
      const file = event.target.files?.[0];
      const id = ++request.current;
      setPreview(null); setError(null); setApplied(false);
      if (!file) return;
      setBusy(true);
      try {
        if (file.size > 1024 * 1024) throw new Error("IMPORT_PARSE_ERROR");
        const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
        const result = await invoke<ImportPreview>("cmd_config_import", { format, bytes });
        if (request.current === id) setPreview(result);
      } catch {
        if (request.current === id) setError("IMPORT_PARSE_ERROR");
      } finally { if (request.current === id) setBusy(false); }
    }} /></label>
    {error ? <p role="alert">{error}</p> : null}
    {preview ? <>
      <h3>{t("settings.import.preview")}</h3>
      <pre className="whitespace-pre-wrap">{JSON.stringify(preview.settings, null, 2)}</pre>
      <p>{t("settings.import.skipped")}: {preview.skipped.join(", ")}</p>
      <Button disabled={busy || applied} onClick={() => {
        const patch = Object.fromEntries(Object.entries(preview.settings).filter(([, value]) => value !== null));
        updateSettings(patch); setApplied(true);
      }}>{t(applied ? "settings.import.applied" : "settings.import.apply")}</Button>
    </> : null}
  </section>;
}
