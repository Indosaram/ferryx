import { useState, useRef, useEffect } from "react";
import { TerminalSquare, Globe, ArrowRight } from "lucide-react";
import { isMacShortcutPlatform, shortcutLabel } from "../lib/shortcuts";

interface NewTabPopoverProps {
  open: boolean;
  onClose: () => void;
  onNewTerminal: () => void;
  onNewBrowser: (url?: string) => void;
}

export function NewTabPopover({
  open,
  onClose,
  onNewTerminal,
  onNewBrowser,
}: NewTabPopoverProps) {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const isMac = isMacShortcutPlatform();

  useEffect(() => {
    if (!open) {
      setQuery("");
      return;
    }
    inputRef.current?.focus();

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };

    const handleClickOutside = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        onClose();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    document.addEventListener("mousedown", handleClickOutside);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [open, onClose]);

  if (!open) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = query.trim();
    if (!trimmed) {
      onNewTerminal();
      onClose();
      return;
    }

    if (
      trimmed.startsWith("http://") ||
      trimmed.startsWith("https://") ||
      trimmed.startsWith("localhost") ||
      trimmed.startsWith("127.0.0.1") ||
      trimmed.includes(".")
    ) {
      let target = trimmed;
      if (!target.startsWith("http://") && !target.startsWith("https://")) {
        target = target.startsWith("localhost") || target.startsWith("127.0.0.1")
          ? `http://${target}`
          : `https://${target}`;
      }
      onNewBrowser(target);
    } else {
      onNewBrowser(`https://www.google.com/search?q=${encodeURIComponent(trimmed)}`);
    }
    onClose();
  };

  return (
    <div
      ref={containerRef}
      role="dialog"
      aria-label="New tab menu"
      className="absolute top-full left-0 mt-1 w-80 rounded-xl border border-border bg-popover text-popover-foreground shadow-2xl z-50 p-2 text-xs select-none backdrop-blur-md"
    >
      <form onSubmit={handleSubmit} className="mb-2">
        <div className="relative flex items-center">
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search open tabs, files, URLs, agents..."
            aria-label="New tab query input"
            className="w-full bg-background/80 border border-border/80 rounded-lg px-2.5 py-1.5 text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:border-primary font-sans"
          />
          {query.trim() ? (
            <button
              type="submit"
              className="absolute right-1.5 p-1 text-muted-foreground hover:text-foreground"
              aria-label="Open query"
            >
              <ArrowRight className="size-3.5" />
            </button>
          ) : null}
        </div>
      </form>

      <div className="flex flex-col gap-0.5 border-t border-border/60 pt-1.5">
        <button
          type="button"
          onClick={() => {
            onNewTerminal();
            onClose();
          }}
          className="flex items-center justify-between px-2.5 py-1.5 rounded-lg text-left text-foreground hover:bg-accent/50 transition-colors group"
        >
          <div className="flex items-center gap-2.5">
            <TerminalSquare className="size-4 text-muted-foreground group-hover:text-foreground" />
            <span className="font-medium">New Terminal</span>
          </div>
          <span className="text-[11px] font-mono text-muted-foreground">
            {shortcutLabel("tab.newTerminal", isMac)}
          </span>
        </button>

        <button
          type="button"
          onClick={() => {
            onNewBrowser();
            onClose();
          }}
          className="flex items-center justify-between px-2.5 py-1.5 rounded-lg text-left text-foreground hover:bg-accent/50 transition-colors group"
        >
          <div className="flex items-center gap-2.5">
            <Globe className="size-4 text-muted-foreground group-hover:text-foreground" />
            <span className="font-medium">New Browser Tab</span>
          </div>
          <span className="text-[11px] font-mono text-muted-foreground">
            {shortcutLabel("tab.newBrowser", isMac)}
          </span>
        </button>
      </div>
    </div>
  );
}
