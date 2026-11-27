// Maho URL pill: the active tab's host, a click target for the Command Bar, and a
// copy-URL quick action. It never edits the URL itself - Maho routes every edit
// through the Command Bar so the omnibox has exactly one owner.

import { Copy, Search } from "lucide-react";
import { useCallback } from "react";
import { toast } from "sonner";

import type { BrowserStoreTab } from "../../../state/browserStore";
import { displayHost, displayUrl } from "./mahoBrowserTabActions";

export type MahoUrlPillProps = {
  tab?: BrowserStoreTab | null;
  onOpenCommandBar?: () => void;
  /** Supplied by callers that own the clipboard (desktop bridge, tests). */
  onCopyUrl?: (url: string) => void;
};

export function MahoUrlPill({ tab, onOpenCommandBar, onCopyUrl }: MahoUrlPillProps) {
  const url = tab?.url ?? "about:blank";
  const host = displayHost(url);

  const handleCopy = useCallback(() => {
    if (url === "about:blank") return;
    if (onCopyUrl) {
      onCopyUrl(url);
      return;
    }
    const clipboard = typeof navigator === "undefined" ? undefined : navigator.clipboard;
    if (!clipboard) {
      toast.error("Clipboard is not available");
      return;
    }
    void clipboard.writeText(url).then(
      () => toast.success("URL copied"),
      () => toast.error("Could not copy the URL"),
    );
  }, [onCopyUrl, url]);

  return (
    <div className="maho-url-pill" role="group" aria-label="Address bar">
      <button
        type="button"
        className="maho-search-pill maho-url-pill__main"
        aria-label={`Address: ${host}`}
        title={displayUrl(url)}
        onClick={onOpenCommandBar}
      >
        <Search aria-hidden="true" />
        <span className="maho-url-pill__label">{host}</span>
      </button>
      <button
        type="button"
        className="maho-top-bar__button"
        aria-label="Copy URL"
        title="Copy URL"
        onClick={handleCopy}
      >
        <Copy aria-hidden="true" />
      </button>
    </div>
  );
}
