import React, { useState } from "react";
import { Globe, Maximize2, Minimize2 } from "lucide-react";
import { cn } from "../../../lib/cn";

/**
 * Visual reply frame (t3code HtmlRenderFrame). An `htmlRender` part renders a
 * self-contained HTML page in a sandboxed iframe: scripts run, but the page has
 * no same-origin access to the app (`sandbox="allow-scripts"` only), and links
 * cannot navigate the frame itself.
 */

export const HTML_RENDER_SANDBOX = "allow-scripts";

export const HtmlRenderFrame: React.FC<{
  html: string;
  title?: string;
  className?: string;
}> = ({ html, title, className }) => {
  const [expanded, setExpanded] = useState(false);
  return (
    <figure
      data-testid="html-render-frame"
      className={cn("my-2 overflow-hidden rounded-xl border border-chat-border bg-chat-surface", className)}
    >
      <figcaption className="flex items-center gap-2 px-3 py-2 text-xs text-chat-foreground-secondary">
        <Globe aria-hidden="true" className="size-3 shrink-0" />
        <span className="min-w-0 flex-1 truncate">{title ?? "Visual reply"}</span>
        <button
          type="button"
          data-testid="html-render-expand"
          aria-expanded={expanded}
          aria-label={expanded ? "Collapse visual reply" : "Expand visual reply"}
          onClick={() => setExpanded((prev) => !prev)}
          className="flex min-h-9 items-center justify-center px-2 transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {expanded ? (
            <Minimize2 aria-hidden="true" className="size-3.5" />
          ) : (
            <Maximize2 aria-hidden="true" className="size-3.5" />
          )}
        </button>
      </figcaption>
      <iframe
        data-testid="html-render-iframe"
        // Scripts run inside the page, but it is sandboxed away from the app:
        // no allow-same-origin, so the frame never reaches session data.
        sandbox={HTML_RENDER_SANDBOX}
        srcDoc={html}
        title={title ?? "Visual reply"}
        className={cn(
          "w-full border-0 bg-white",
          expanded ? "h-[32rem]" : "h-64",
        )}
      />
    </figure>
  );
};
