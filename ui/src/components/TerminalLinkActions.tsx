import { useEffect } from "react";
import { ExternalLink, Globe, X } from "lucide-react";

import {
  routeHttpLink,
  TERMINAL_LINK_ACTION_EVENT,
  type TerminalLinkActionRequest,
} from "../lib/linkRouting";
import { toast } from "./ui/sonner";

export function TerminalLinkActions() {
  useEffect(() => {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<TerminalLinkActionRequest>).detail;
      const url = detail?.url;
      if (!url) return;

      toast.custom(
        (t) => {
          const open = async (destination: "builtin" | "external") => {
            toast.dismiss(t);
            try {
              await routeHttpLink(url, { source: "terminal", destination });
            } catch {
              // ignore routing error
            }
          };

          return (
            <div
              role="dialog"
              aria-label="Terminal link actions"
              className="w-[min(26rem,calc(100vw-2rem))] rounded-xl border border-border bg-popover p-3 text-popover-foreground shadow-2xl"
            >
              <div className="flex items-start gap-3">
                <Globe className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
                <div className="min-w-0 flex-1">
                  <div className="text-xs font-semibold">Open terminal link</div>
                  <div className="mt-1 truncate font-mono text-[10px] text-muted-foreground" title={url}>
                    {url}
                  </div>
                </div>
                <button
                  type="button"
                  aria-label="Close link actions"
                  onClick={() => toast.dismiss(t)}
                  className="rounded p-1 hover:bg-accent"
                >
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
        },
        { duration: Infinity },
      );
    };

    window.addEventListener(TERMINAL_LINK_ACTION_EVENT, handler);
    return () => window.removeEventListener(TERMINAL_LINK_ACTION_EVENT, handler);
  }, []);

  return null;
}
