import { useState, useEffect } from "react";
import { ArrowLeft, ArrowRight, RotateCw, ExternalLink } from "lucide-react";
import { openExternalUrl } from "../lib/browserTauri";
import type { BrowserTab } from "../lib/types";

interface BrowserToolbarProps {
  tab: BrowserTab;
  onNavigate: (url: string) => void;
  onReload: () => void;
  onGoBack?: () => void;
  onGoForward?: () => void;
}

export function BrowserToolbar({
  tab,
  onNavigate,
  onReload,
  onGoBack,
  onGoForward,
}: BrowserToolbarProps) {
  const [inputUrl, setInputUrl] = useState(tab.url);

  useEffect(() => {
    setInputUrl(tab.url);
  }, [tab.url]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    let url = inputUrl.trim();
    if (!url) return;
    if (!url.startsWith("http://") && !url.startsWith("https://") && url !== "about:blank") {
      if (url.startsWith("localhost") || url.startsWith("127.0.0.1")) {
        url = `http://${url}`;
      } else {
        url = `https://${url}`;
      }
    }
    onNavigate(url);
  };

  const handleExternalOpen = () => {
    if (tab.url && tab.url !== "about:blank") {
      void openExternalUrl(tab.url);
    }
  };

  return (
    <div className="flex items-center gap-2 px-3 py-1.5 bg-sidebar border-b border-border text-xs text-foreground select-none">
      <div className="flex items-center gap-1">
        <button
          type="button"
          disabled={!tab.canGoBack}
          onClick={onGoBack}
          aria-label="Back"
          className="p-1 rounded hover:bg-muted disabled:opacity-30 transition-colors"
        >
          <ArrowLeft className="w-3.5 h-3.5" />
        </button>
        <button
          type="button"
          disabled={!tab.canGoForward}
          onClick={onGoForward}
          aria-label="Forward"
          className="p-1 rounded hover:bg-muted disabled:opacity-30 transition-colors"
        >
          <ArrowRight className="w-3.5 h-3.5" />
        </button>
        <button
          type="button"
          onClick={onReload}
          aria-label="Reload"
          className="p-1 rounded hover:bg-muted transition-colors"
        >
          <RotateCw className={`w-3.5 h-3.5 ${tab.loading ? "animate-spin" : ""}`} />
        </button>
      </div>

      <form onSubmit={handleSubmit} className="flex-1 flex items-center">
        <input
          type="text"
          value={inputUrl}
          onChange={(e) => setInputUrl(e.target.value)}
          placeholder="Search or enter URL"
          aria-label="URL address bar"
          className="w-full bg-background border border-border rounded px-2.5 py-1 text-xs text-foreground focus:outline-none focus:border-primary font-mono"
        />
      </form>

      <button
        type="button"
        onClick={handleExternalOpen}
        title="Open in default browser"
        aria-label="Open in external browser"
        className="p-1 rounded hover:bg-muted transition-colors"
      >
        <ExternalLink className="w-3.5 h-3.5 text-muted-foreground" />
      </button>
    </div>
  );
}
