import { useEffect, useState } from "react";
import { SHORTCUTS, shortcutLabel, isMacShortcutPlatform } from "../lib/shortcuts";
import { useShortcutOverrides, useShortcutPreset } from "../lib/shortcutOverrides";
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "./ui/dialog";

type DocHeading = { path: string; title: string; text: string };

/** Help is bundled and usable offline; shortcut labels use the same registry as dispatch. */
export function ProductHelp() {
  const isMac = isMacShortcutPlatform();
  const { overrides } = useShortcutOverrides();
  const { preset } = useShortcutPreset(isMac);
  const [page, setPage] = useState<"shortcuts" | "docs" | null>(null);
  const [docs, setDocs] = useState<DocHeading[]>([]);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  useEffect(() => {
    if (page !== "docs") return;
    const controller = new AbortController();
    void fetch("/search.json", { signal: controller.signal }).then(async (response) => {
      if (!response.ok) throw new Error(`Documentation unavailable (${response.status})`);
      setDocs(await response.json() as DocHeading[]);
    }).catch((reason: unknown) => {
      if (!controller.signal.aborted) setError(String(reason));
    });
    return () => controller.abort();
  }, [page]);
  return <>
    <details className="relative text-xs">
      <summary className="cursor-pointer p-1" aria-label="Help">Help</summary>
      <nav aria-label="Help" className="absolute bottom-full right-0 z-40 grid w-48 gap-2 rounded border border-border bg-card p-3 text-foreground shadow-lg">
        <button type="button" onClick={() => setPage("shortcuts")}>Keyboard shortcuts</button>
        <button type="button" onClick={() => setPage("docs")}>Documentation</button>
        <a href="/llms.txt" target="_blank" rel="noreferrer">Agent documentation (llms.txt)</a>
      </nav>
    </details>
    <Dialog open={page !== null} onOpenChange={(open) => { if (!open) setPage(null); }}>
      <DialogContent className="max-h-[80vh] max-w-2xl overflow-y-auto">
        <DialogTitle>{page === "shortcuts" ? "Keyboard shortcuts" : "Documentation"}</DialogTitle>
        <DialogDescription>{page === "shortcuts" ? "Current keyboard bindings" : "Search bundled documentation"}</DialogDescription>
        {page === "shortcuts" ? <dl className="grid grid-cols-2 gap-2 text-sm">
          {SHORTCUTS.map((shortcut) => <div key={shortcut.id} className="contents">
            <dt>{shortcut.title}</dt><dd>{shortcutLabel(shortcut.id, isMac, overrides, preset) || "Unbound"}</dd>
          </div>)}
        </dl> : <>
          <input aria-label="Search documentation" value={query} onChange={(event) => setQuery(event.target.value)} className="rounded border border-border bg-background p-2" />
          {error ? <p role="alert">{error}</p> : null}
          {docs.filter((doc) => `${doc.title} ${doc.text}`.toLowerCase().includes(query.toLowerCase())).map((doc, index) => <details key={`${doc.path}:${index}`}>
            <summary>{doc.title} <span className="text-xs text-muted-foreground">{doc.path}</span></summary>
            <pre className="whitespace-pre-wrap break-words text-xs">{doc.text}</pre>
          </details>)}
        </>}
      </DialogContent>
    </Dialog>
  </>;
}
