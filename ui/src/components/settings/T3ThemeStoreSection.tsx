import { useState } from "react";
import { Download, Loader2, Search } from "lucide-react";

import {
  filterT3StoreThemes,
  loadT3StoreCatalog,
  type T3StoreTheme,
} from "../../lib/t3ThemeStore";
import type { ThemeDocumentV1 } from "../../lib/themeDocument";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

export type T3ThemeStoreSectionProps = {
  onInstallThemes: (docs: ThemeDocumentV1[]) => void;
};

export function T3ThemeStoreSection({ onInstallThemes }: T3ThemeStoreSectionProps) {
  const [query, setQuery] = useState("");
  const [themes, setThemes] = useState<T3StoreTheme[]>([]);
  const [loading, setLoading] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleLoad() {
    setLoading(true);
    setError(null);
    setMessage(null);
    try {
      const result = await loadT3StoreCatalog();
      if (!result.ok) {
        setError(result.error.message);
        return;
      }
      setThemes(result.themes);
      setLoaded(true);
      if (result.skipped > 0) {
        setMessage(`Loaded ${result.themes.length} themes; ${result.skipped} could not be imported.`);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  function handleInstall(theme: T3StoreTheme) {
    onInstallThemes([theme.doc]);
    setMessage(`Installed ${theme.name}.`);
  }

  const visible = filterT3StoreThemes(themes, query);

  return (
    <div className="space-y-2.5 rounded-lg border border-border bg-background/60 p-3">
      <div>
        <p className="text-[12px] font-medium text-foreground">t3code theme store</p>
        <p className="text-[11px] text-muted-foreground">
          Community themes from t3themes.com, imported as Ferryx themes.
        </p>
      </div>
      <div className="flex gap-2">
        <Input
          aria-label="Filter t3code themes"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          disabled={!loaded}
          placeholder={loaded ? "Filter by name, author or description..." : "Load the store first"}
          className="h-8 text-[12px]"
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={loading}
          onClick={() => void handleLoad()}
          className="h-8 shrink-0 gap-1.5 px-3 text-[11px]"
        >
          {loading ? <Loader2 className="size-3 animate-spin" aria-hidden="true" /> : <Search className="size-3" aria-hidden="true" />}
          {loaded ? "Reload" : "Browse"}
        </Button>
      </div>

      {error ? (
        <p role="alert" className="text-[11px] text-destructive">
          {error}
        </p>
      ) : null}
      {message ? (
        <p role="status" className="text-[11px] text-muted-foreground">
          {message}
        </p>
      ) : null}

      {loaded && visible.length === 0 ? (
        <p className="text-[11px] text-muted-foreground">No store theme matches that filter.</p>
      ) : null}

      {visible.length > 0 ? (
        <ul className="max-h-80 divide-y divide-border overflow-y-auto rounded-md border border-border bg-panel">
          {visible.map((theme) => (
            <li key={theme.id} className="flex items-center justify-between gap-3 px-3 py-2">
              <div className="flex min-w-0 flex-1 items-center gap-2.5">
                <span className="flex shrink-0 gap-1" aria-hidden="true">
                  {theme.swatches.map((swatch, index) => (
                    <span
                      key={index}
                      className="size-4 rounded-sm border border-border"
                      style={{ backgroundColor: swatch }}
                    />
                  ))}
                </span>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[12px] font-medium text-foreground">{theme.name}</p>
                  <p className="truncate text-[11px] text-muted-foreground">
                    {theme.author ?? "Unknown author"} · {theme.base}
                    {theme.description ? ` · ${theme.description}` : ""}
                  </p>
                </div>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => handleInstall(theme)}
                className="h-7 shrink-0 gap-1 px-2.5 text-[11px]"
              >
                <Download className="size-3" aria-hidden="true" />
                Install
              </Button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
