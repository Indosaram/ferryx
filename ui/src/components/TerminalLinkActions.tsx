import { useEffect, useState } from "react";
import { ExternalLink, Globe, X } from "lucide-react";

import {
  routeHttpLink,
  TERMINAL_LINK_ACTION_EVENT,
  type TerminalLinkActionRequest,
} from "../lib/linkRouting";

export function TerminalLinkActions() {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<TerminalLinkActionRequest>).detail;
      if (detail?.url) setUrl(detail.url);
    };
    window.addEventListener(TERMINAL_LINK_ACTION_EVENT, handler);
    return () => window.removeEventListener(TERMINAL_LINK_ACTION_EVENT, handler);
  }, []);

  if (!url) return null;

  const open = async (destination: "builtin" | "external") => {
    const target = url;
    setUrl(null);
    await routeHttpLink(target, { source: "terminal", destination }).catch(() => undefined);
  };

  return (
    <div
      role="dialog"
      aria-label="Terminal link actions"
      className="fixed bottom-4 left-1/2 z-[120] w-[min(460px,calc(100vw-2rem))] -translate-x-1/2 rounded-xl border border-border bg-popover p-3 text-popover-foreground shadow-2xl"
    >
      <div className="flex items-start gap-3">
        <Globe className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        <div className="min-w-0 flex-1">
          <div className="text-xs font-semibold">Open terminal link</div>
          <div className="mt-1 truncate font-mono text-[10px] text-muted-foreground" title={url}>{url}</div>
        </div>
        <button type="button" aria-label="Close link actions" onClick={() => setUrl(null)} className="rounded p-1 hover:bg-accent">
          <X className="size-3.5" />
        </button>
      </div>
      <div className="mt-3 flex justify-end gap-2">
        <button
          type="button"
          onClick={() => void open("external")}
          className="flex h-8 items-center gap-1.5 rounded-md border border-border px-2.5 text-[11px] hover:bg-accent"
        >
          <ExternalLink className="size-3.5" />
          Web Browser
        </button>
        <button
          type="button"
          onClick={() => void open("builtin")}
          className="flex h-8 items-center gap-1.5 rounded-md bg-foreground px-2.5 text-[11px] text-background hover:opacity-90"
        >
          <Globe className="size-3.5" />
          Built-in Browser
        </button>
      </div>
    </div>
  );
}
