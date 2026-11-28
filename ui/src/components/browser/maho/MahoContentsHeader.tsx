// Port of Maho's per-pane contents header
// (`maho-chromium/browser/ui/views/frame/maho_contents_header_view.{h,cc}`): a 24dp strip
// on the Space surface above each page, with back / forward / reload, the host (Maho's
// only address entry, which opens the Command Bar) and copy URL. Geometry is in `maho.css`.

import { ChevronLeft, ChevronRight, Link, RotateCw } from "lucide-react";
import type { ReactNode } from "react";

import type { BrowserStoreTab } from "../../../state/browserStore";

export const MAHO_SEARCH_PLACEHOLDER = "Search or Enter URL...";

export function mahoHeaderHostText(url: string | null | undefined): string {
  if (!url || url === "about:blank") return MAHO_SEARCH_PLACEHOLDER;
  try {
    const parsed = new URL(url);
    if (!parsed.hostname) return MAHO_SEARCH_PLACEHOLDER;
    return parsed.hostname.replace(/^www\./, "");
  } catch {
    return MAHO_SEARCH_PLACEHOLDER;
  }
}

export type MahoContentsHeaderProps = {
  tab: BrowserStoreTab | null;
  active: boolean;
  onBack?: () => void;
  onForward?: () => void;
  onReload?: () => void;
  onOpenAddress?: () => void;
  onCopyUrl?: (url: string) => void;
  onActivate?: () => void;
  trailing?: ReactNode;
};

export function MahoContentsHeader({
  tab,
  active,
  onBack,
  onForward,
  onReload,
  onOpenAddress,
  onCopyUrl,
  onActivate,
  trailing,
}: MahoContentsHeaderProps) {
  const host = mahoHeaderHostText(tab?.url);
  const hasPage = Boolean(tab && tab.url && tab.url !== "about:blank");

  return (
    <div
      className="maho-contents-header"
      role="toolbar"
      aria-label="Page"
      data-testid="maho-contents-header"
      data-active={active ? "true" : "false"}
      onPointerDown={onActivate}
    >
      <button
        type="button"
        className="maho-contents-header__button"
        aria-label="Back"
        title="Back"
        disabled={!tab?.canGoBack}
        onClick={onBack}
      >
        <ChevronLeft aria-hidden="true" />
      </button>
      <button
        type="button"
        className="maho-contents-header__button"
        aria-label="Forward"
        title="Forward"
        disabled={!tab?.canGoForward}
        onClick={onForward}
      >
        <ChevronRight aria-hidden="true" />
      </button>
      <button
        type="button"
        className="maho-contents-header__button"
        aria-label="Reload"
        title="Reload"
        disabled={!tab}
        onClick={onReload}
      >
        <RotateCw aria-hidden="true" className={tab?.loading ? "animate-spin" : undefined} />
      </button>
      <button
        type="button"
        className="maho-contents-header__host"
        aria-label={`Address: ${host}. Press Enter to edit`}
        title={hasPage && tab ? tab.url : host}
        onClick={onOpenAddress}
      >
        {host}
      </button>
      <span className="maho-contents-header__spacer" />
      <button
        type="button"
        className="maho-contents-header__button"
        aria-label="Copy URL"
        title="Copy URL"
        disabled={!hasPage}
        onClick={() => {
          if (tab && hasPage) onCopyUrl?.(tab.url);
        }}
      >
        <Link aria-hidden="true" />
      </button>
      {trailing}
    </div>
  );
}
