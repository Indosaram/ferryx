// Maho Ctrl+Tab MRU switcher: hold Control, tap Tab, release to land.
//
// The card overlay is a *view* of `selectMruTabs`: the store already keeps the
// most-recently-used order (`mru`), so the switcher never re-sorts by time. It is
// controlled by `open` and reports the chosen `browserId`; committing touches the
// tab, which is what promotes it to the head of the MRU list.
//
// NATIVE OCCLUSION. `lib/nativeTerminalVisibility.tsx` has no imperative push/pop
// API: a surface occludes native terminal/browser views by *being* an input-owning
// overlay in the DOM (`[role="dialog"]`, not `aria-hidden`, not opted out). Rendering
// the card as `role="dialog"` while `open` is therefore what hides the native page
// underneath; returning `null` on close (or on unmount) removes the surface and the
// observed mutation releases occlusion immediately, with no state to leak.

import { useCallback, useEffect, useRef, useState } from "react";

import {
  browserStore,
  type BrowserStore,
  type BrowserStoreTab,
} from "../../../state/browserStore";

/** `forward` is Ctrl+Tab (next MRU), `backward` is Ctrl+Shift+Tab. */
export type MahoTabSwitcherDirection = "forward" | "backward";

export type MahoTabSwitcherProps = {
  open: boolean;
  direction?: MahoTabSwitcherDirection;
  /** MRU order, as `selectMruTabs` returns it. */
  tabs: readonly BrowserStoreTab[];
  /** Highlight to start on; the switcher owns cycling from there. */
  selectedIndex: number;
  onSelect: (browserId: string) => void;
  onClose: () => void;
  onShowAllTabs?: () => void;
  thumbnails?: Readonly<Record<string, string>>;
  store?: BrowserStore;
};

function clampIndex(index: number, length: number): number {
  if (length <= 0) return 0;
  if (!Number.isFinite(index)) return 0;
  return Math.min(Math.max(Math.trunc(index), 0), length - 1);
}

// maho_ctrl_tab_layout.h: fit cards before reducing the visible count.
export function fitMahoSwitcher(availableWidth: number, total: number) {
  if (!Number.isFinite(availableWidth) || availableWidth <= 0) {
    return { count: Math.min(total, 7), width: 220 };
  }
  for (let count = Math.min(total, 7); count > 0; count -= 1) {
    const more = total > count;
    const fixed = 50 + (more ? 120 : 0) + (count + Number(more) - 1) * 12;
    const width = Math.floor((availableWidth - fixed) / count);
    if (width >= 140) return { count, width: Math.min(width, 220) };
  }
  return { count: Math.min(total, 1), width: 140 };
}

export function MahoTabSwitcher({
  open,
  direction = "forward",
  tabs,
  selectedIndex,
  onSelect,
  onClose,
  onShowAllTabs,
  thumbnails,
  store = browserStore,
}: MahoTabSwitcherProps) {
  const [windowWidth, setWindowWidth] = useState(() => window.innerWidth);
  const layout = fitMahoSwitcher(windowWidth - 32, tabs.length);
  // Until the shell supplies its palette callback, keep every tab reachable.
  const visibleCount = onShowAllTabs ? layout.count : tabs.length;
  const hasMore = tabs.length > visibleCount;
  const slotCount = visibleCount + Number(hasMore);
  const [highlight, setHighlight] = useState(0);
  // Mirrored in a ref so a key handler always reads the live highlight without
  // resubscribing the window listeners in the middle of a Control+Tab hold.
  const highlightRef = useRef(0);
  // One opening commits at most once: Enter followed by the Control keyup that ends
  // the same hold must not select (and touch) the tab a second time.
  const committedRef = useRef(false);

  const applyHighlight = useCallback((next: number) => {
    highlightRef.current = next;
    setHighlight(next);
  }, []);

  // Each opening starts from the caller's index; cycling inside is local state, so a
  // parent that re-renders with the same props does not fight the highlight.
  useEffect(() => {
    if (!open) return;
    applyHighlight(clampIndex(selectedIndex, slotCount));
  }, [applyHighlight, open, selectedIndex, slotCount]);

  useEffect(() => {
    if (!open) return;
    const resize = () => setWindowWidth(window.innerWidth);
    resize();
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    document.querySelector(`[data-maho-tab-switcher] [data-slot="${highlight}"]`)
      ?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [highlight, open]);

  const commit = useCallback((index: number) => {
    if (committedRef.current) return;
    committedRef.current = true;
    if (hasMore && index === visibleCount) {
      onClose();
      onShowAllTabs?.();
      return;
    }
    const tab = tabs[clampIndex(index, visibleCount)];
    if (tab) {
      onSelect(tab.browserId);
      // Committing is a use: it moves the tab to the head of the MRU list.
      store.touch(tab.browserId);
    }
    onClose();
  }, [hasMore, onClose, onSelect, onShowAllTabs, store, tabs, visibleCount]);

  // The handlers read through this ref, so a highlight change never tears them down.
  const handlers = useRef({ commit, length: slotCount });
  handlers.current = { commit, length: slotCount };

  // A fresh opening may commit again; resetting on `open` alone keeps a mid-hold
  // re-render (a new `onClose` identity, a tab count change) from re-arming it.
  useEffect(() => {
    if (open) committedRef.current = false;
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const cycle = (step: number) => {
      const length = handlers.current.length;
      if (length <= 0) return;
      applyHighlight((clampIndex(highlightRef.current, length) + step + length) % length);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        committedRef.current = true;
        onClose();
        return;
      }
      if (event.key === "Enter") {
        event.preventDefault();
        handlers.current.commit(highlightRef.current);
        return;
      }
      if (event.key === "ArrowRight" || event.key === "ArrowDown") {
        event.preventDefault();
        cycle(1);
        return;
      }
      if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
        event.preventDefault();
        cycle(-1);
        return;
      }
      if (event.key !== "Tab") return;
      event.preventDefault();
      cycle(event.shiftKey ? -1 : 1);
    };
    const handleKeyUp = (event: KeyboardEvent) => {
      // Releasing Control is the commit gesture: the key that opened the switcher
      // is the key that chooses.
      if (event.key !== "Control") return;
      event.preventDefault();
      handlers.current.commit(highlightRef.current);
    };
    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
    };
  }, [applyHighlight, onClose, open]);

  if (!open) return null;

  const bounded = clampIndex(highlight, slotCount);

  return (
    <div
      className="maho-switcher-backdrop"
      data-testid="maho-tab-switcher-backdrop"
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Tab switcher"
        data-maho-tab-switcher="true"
        data-direction={direction}
        className="maho-switcher-panel"
        onMouseDown={(event) => event.stopPropagation()}
      >
        {tabs.length === 0 ? (
          <p className="maho-command-empty">No open tabs</p>
        ) : (
          <div role="listbox" aria-label="Tabs by recent use" className="maho-switcher-cards">
            {tabs.slice(0, visibleCount).map((tab, index) => {
              const selected = index === bounded;
              const label = tab.title.trim() || tab.url;
              return (
                <div
                  key={tab.browserId}
                  role="option"
                  aria-selected={selected}
                  aria-label={label}
                  data-testid={`maho-tab-switcher-card-${index}`}
                  data-browser-id={tab.browserId}
                  data-slot={index}
                  data-selected={selected ? "true" : undefined}
                  onMouseEnter={() => applyHighlight(index)}
                  onClick={() => commit(index)}
                  className="maho-switcher-card"
                  style={{ width: layout.width }}
                >
                  <span className="maho-switcher-preview" style={{ height: Math.max(1, Math.floor((layout.width - 12) * 120 / 208)) }}>
                    {thumbnails?.[tab.browserId] ? (
                      <img src={thumbnails[tab.browserId]} alt="" className="maho-switcher-thumbnail" />
                    ) : tab.faviconUrl ? (
                      <img src={tab.faviconUrl} alt="" className="maho-switcher-fallback" />
                    ) : null}
                  </span>
                  <span className="maho-switcher-footer">
                    <span className="maho-switcher-favicon">
                      {tab.faviconUrl ? <img src={tab.faviconUrl} alt="" /> : null}
                    </span>
                    <span className="maho-switcher-title">{label}</span>
                  </span>
                </div>
              );
            })}
            {hasMore ? (
              <div
                role="option"
                aria-selected={bounded === visibleCount}
                data-slot={visibleCount}
                className="maho-switcher-more"
                style={{ height: 52 + Math.max(1, Math.floor((layout.width - 12) * 120 / 208)) }}
                onMouseEnter={() => applyHighlight(visibleCount)}
                onClick={() => commit(visibleCount)}
              >
                <span>+{tabs.length - visibleCount} more</span>
                <span>Show all tabs</span>
              </div>
            ) : null}
          </div>
        )}
      </div>
    </div>
  );
}
