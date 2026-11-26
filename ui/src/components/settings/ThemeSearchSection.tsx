import { useState } from "react";
import { Download, Loader2, Search } from "lucide-react";

import {
  loadOpenVsxExtensionThemes,
  searchOpenVsxThemes,
  type OpenVsxThemeExtension,
} from "../../lib/openVsxThemes";
import type { ThemeDocumentV1 } from "../../lib/themeDocument";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

export type ThemeSearchSectionProps = {
  onInstallThemes: (docs: ThemeDocumentV1[]) => void;
};

export function ThemeSearchSection({ onInstallThemes }: ThemeSearchSectionProps) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<OpenVsxThemeExtension[]>([]);
  const [searching, setSearching] = useState(false);
  const [installingId, setInstallingId] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleSearch() {
    setSearching(true);
    setError(null);
    setMessage(null);
    try {
      const items = await searchOpenVsxThemes(query);
      setResults(items);
      if (items.length === 0) setMessage("No matching VS Code themes found on OpenVSX.");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSearching(false);
    }
  }

  async function handleInstall(ext: OpenVsxThemeExtension) {
    setInstallingId(ext.id);
    setError(null);
    setMessage(null);
    try {
      const docs = await loadOpenVsxExtensionThemes(ext);
      if (docs.length === 0) {
        setError(`No importable color themes were found inside ${ext.name}.`);
        return;
      }
      onInstallThemes(docs);
      setMessage(`Installed ${docs.length} theme${docs.length > 1 ? "s" : ""} from ${ext.name}.`);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setInstallingId(null);
    }
  }

  return (
    <div className="space-y-2.5 rounded-lg border border-border bg-background/60 p-3">
      <div>
        <p className="text-[12px] font-medium text-foreground">Search VS Code themes (OpenVSX)</p>
        <p className="text-[11px] text-muted-foreground">
          Find Catppuccin, Tokyo Night, Dracula, Solarized, Rose Pine, and more.
        </p>
      </div>
      <div className="flex gap-2">
        <Input
          aria-label="Search OpenVSX themes"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void handleSearch();
            }
          }}
          placeholder="Search Catppuccin, Tokyo Night, Dracula..."
          className="h-8 text-[12px]"
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={searching}
          onClick={() => void handleSearch()}
          className="h-8 shrink-0 gap-1.5 px-3 text-[11px]"
        >
          {searching ? <Loader2 className="size-3 animate-spin" aria-hidden="true" /> : <Search className="size-3" aria-hidden="true" />}
          Search
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

      {results.length > 0 ? (
        <ul className="divide-y divide-border rounded-md border border-border bg-panel">
          {results.map((ext) => (
            <li key={ext.id} className="flex items-center justify-between gap-3 px-3 py-2">
              <div className="min-w-0 flex-1">
                <p className="truncate text-[12px] font-medium text-foreground">{ext.name}</p>
                <p className="truncate text-[11px] text-muted-foreground">
                  {ext.publisher} · {ext.downloadCount.toLocaleString()} downloads
                </p>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={installingId !== null}
                onClick={() => void handleInstall(ext)}
                className="h-7 shrink-0 gap-1 px-2.5 text-[11px]"
              >
                {installingId === ext.id ? (
                  <Loader2 className="size-3 animate-spin" aria-hidden="true" />
                ) : (
                  <Download className="size-3" aria-hidden="true" />
                )}
                Install
              </Button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
