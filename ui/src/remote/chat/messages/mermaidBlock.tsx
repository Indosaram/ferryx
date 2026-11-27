import React, { useEffect, useId, useRef, useState } from "react";
import { Code, Eye } from "lucide-react";
import { cn } from "../../../lib/cn";

/**
 * Mermaid code blocks render as diagrams (t3code MarkdownMermaidCodeBlock /
 * MermaidDiagram). mermaid loads lazily, only for a `mermaid` fence; the theme
 * comes from the chat's CSS tokens so light and dark follow the app; a render
 * error falls back to the plain code block.
 */

/** The slice of mermaid's API this file uses, so no global mermaid type leaks in. */
interface MermaidApi {
  initialize: (config: Record<string, unknown>) => void;
  render: (id: string, text: string) => Promise<{ svg: string }>;
}

let mermaidLoad: Promise<MermaidApi | undefined> | undefined;
let mermaidConfigured = false;

function readToken(name: string, fallback: string): string {
  if (typeof window === "undefined") return fallback;
  const value = getComputedStyle(document.body).getPropertyValue(name).trim();
  return value.length > 0 ? value : fallback;
}

function loadMermaid(): Promise<MermaidApi | undefined> {
  mermaidLoad ??= import("mermaid").then(
    (loaded) => {
      // mermaid ships its API as the default export; the named module object has it too.
      const api = (loaded as unknown as { default?: MermaidApi }).default ??
        (loaded as unknown as MermaidApi);
      return api;
    },
    () => undefined,
  );
  return mermaidLoad;
}

function configureMermaid(api: MermaidApi): void {
  if (mermaidConfigured) return;
  api.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    theme: "base",
    themeVariables: {
      // CSS tokens, so the diagram follows the app's theme including dark mode.
      background: readToken("--chat-screen", "#ffffff"),
      primaryColor: readToken("--chat-surface", "#f5f5f5"),
      primaryTextColor: readToken("--chat-foreground", "#111111"),
      lineColor: readToken("--chat-border", "#d4d4d4"),
      fontFamily: readToken("--font-sans", "ui-sans-serif, system-ui, sans-serif"),
    },
  });
  mermaidConfigured = true;
}

export const MermaidCodeBlock: React.FC<{
  code: string;
  className?: string;
  /**
   * The chat's own code block, drawn when the reader asks for the source or the diagram
   * fails: a diagram must never cost the copy and wrap controls every code block has.
   */
  fallback?: React.ReactNode;
}> = ({ code, className, fallback }) => {
  const trimmed = code.trim();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [state, setState] = useState<"loading" | "rendered" | "failed">("loading");
  const [showCode, setShowCode] = useState(false);
  const renderId = useId().replace(/[^a-zA-Z0-9]/g, "");
  // mermaid refuses an id it has already drawn, so every attempt gets its own.
  const attemptRef = useRef(0);

  useEffect(() => {
    // Showing the source detaches the diagram; coming back re-renders into the fresh node.
    if (showCode) return;
    if (!trimmed) {
      setState("failed");
      return;
    }
    let active = true;
    setState("loading");
    // A new source must not show the previous diagram while it renders.
    if (containerRef.current) containerRef.current.innerHTML = "";
    void loadMermaid().then(async (module) => {
      if (!active) return;
      if (!module) {
        setState("failed");
        return;
      }
      configureMermaid(module);
      const attemptId = `${renderId}-${++attemptRef.current}`;
      try {
        // A synchronous throw from a bad diagram is as much a failed render as a rejection.
        const { svg } = await module.render(attemptId, trimmed);
        if (!active || !containerRef.current) return;
        containerRef.current.innerHTML = svg;
        setState("rendered");
      } catch {
        if (active) setState("failed");
      }
    });
    return () => {
      active = false;
    };
  }, [trimmed, renderId, showCode]);

  // A diagram that cannot draw, or a reader who asked for the source, gets the chat's own code
  // block: copy and wrap must survive a broken diagram (t3code passes the code block as the
  // error fallback for the same reason).
  if (state === "failed" || showCode) {
    return (
      <div className={cn(!fallback && "my-2.5", className)}>
        {showCode && (
          <div className="flex items-center justify-end">
            <button
              type="button"
              data-testid="mermaid-show-diagram"
              aria-pressed={false}
              onClick={() => setShowCode(false)}
              className="flex min-h-9 items-center gap-1 px-2 text-xs text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <Eye aria-hidden="true" className="size-3" />
              Show diagram
            </button>
          </div>
        )}
        {fallback ?? (
          <div
            data-testid="mermaid-fallback"
            className="rounded-xl border border-chat-border bg-chat-screen p-3 font-mono text-[0.8125rem] text-chat-foreground whitespace-pre-wrap break-words"
          >
            {code}
          </div>
        )}
      </div>
    );
  }

  return (
    <div
      data-testid="mermaid-diagram"
      data-state={state}
      className={cn(
        "my-2.5 overflow-x-auto rounded-xl border border-chat-border bg-chat-screen p-3 text-chat-foreground",
        state === "loading" && "min-h-16 animate-pulse",
        className,
      )}
    >
      {state === "rendered" && (
        <div className="flex items-center justify-end">
          <button
            type="button"
            data-testid="mermaid-show-code"
            aria-pressed={false}
            onClick={() => setShowCode(true)}
            className="flex min-h-9 items-center gap-1 px-2 text-xs text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <Code aria-hidden="true" className="size-3" />
            Show code
          </button>
        </div>
      )}
      <div ref={containerRef} className="mermaid-diagram-svg [&_svg]:mx-auto [&_svg]:max-w-full" />
    </div>
  );
};
