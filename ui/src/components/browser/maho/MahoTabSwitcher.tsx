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
  useBrowserStore,
  type BrowserStore,
  type BrowserStoreTab,
} from "../../../state/browserStore";
import { displayHost } from "./mahoBrowserTabActions";
import { MAHO_LAYOUT_TOKENS } from "./mahoTokens";

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
  store?: BrowserStore;
};

function clampIndex(index: number, length: number): number {
  if (length <= 0) return 0;
  if (!Number.isFinite(index)) return 0;
  return Math.min(Math.max(Math.trunc(index), 0), length - 1);
}

export function MahoTabSwitcher({
  open,
  direction = "forward",
  tabs,
  selectedIndex,
  onSelect,
  onClose,
  store = browserStore,
}: MahoTabSwitcherProps) {
  const spaces = useBrowserStore((state) => state.spaces, store);
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
    applyHighlight(clampIndex(selectedIndex, tabs.length));
  }, [applyHighlight, open, selectedIndex, tabs.length]);

  const commit = useCallback((index: number) => {
    if (committedRef.current) return;
    committedRef.current = true;
    const tab = tabs[clampIndex(index, tabs.length)];
    if (tab) {
      onSelect(tab.browserId);
      // Committing is a use: it moves the tab to the head of the MRU list.
      store.touch(tab.browserId);
    }
    onClose();
  }, [onClose, onSelect, store, tabs]);

  // The handlers read through this ref, so a highlight change never tears them down.
  const handlers = useRef({ commit, length: tabs.length, direction });
  handlers.current = { commit, length: tabs.length, direction };

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
      const base = handlers.current.direction === "backward" ? -1 : 1;
      cycle(event.shiftKey ? -base : base);
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

  const bounded = clampIndex(highlight, tabs.length);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-6 backdrop-blur-sm"
      data-testid="maho-tab-switcher-backdrop"
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Tab switcher"
        data-maho-tab-switcher="true"
        data-direction={direction}
        className="w-full max-w-3xl rounded-xl border border-border bg-popover p-3 text-foreground shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        {tabs.length === 0 ? (
          <p className="px-2 py-6 text-center text-xs text-muted-foreground">No open tabs</p>
        ) : (
          <div role="listbox" aria-label="Tabs by recent use" className="flex flex-wrap gap-2">
            {tabs.map((tab, index) => {
              const space = spaces[tab.spaceId];
              const selected = index === bounded;
              const label = tab.title.trim() || displayHost(tab.url);
              return (
                <div
                  key={tab.browserId}
                  role="option"
                  aria-selected={selected}
                  aria-label={label}
                  data-testid={`maho-tab-switcher-card-${index}`}
                  data-browser-id={tab.browserId}
                  data-selected={selected ? "true" : undefined}
                  onMouseEnter={() => applyHighlight(index)}
                  onClick={() => commit(index)}
                  className={`flex cursor-pointer flex-col justify-between gap-1 border px-2 py-2 ${
                    selected
                      ? "border-border bg-accent text-accent-foreground"
                      : "border-transparent bg-muted/40 text-foreground"
                  }`}
                  style={{
                    width: 188,
                    minHeight: MAHO_LAYOUT_TOKENS.rows.height * 1.9,
                    borderRadius: MAHO_LAYOUT_TOKENS.rows.cornerRadius,
                  }}
                >
                  <span className="flex items-center gap-1.5">
                    {tab.faviconUrl ? (
                      <img src={tab.faviconUrl} alt="" className="size-3.5 shrink-0 rounded-sm" />
                    ) : null}
                    <span className="min-w-0 flex-1 truncate text-xs">{label}</span>
                  </span>
                  <span className="flex items-center gap-1.5">
                    <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-muted-foreground">
                      {displayHost(tab.url)}
                    </span>
                    {space ? (
                      <span
                        data-testid="maho-tab-switcher-space-badge"
                        className="inline-flex shrink-0 items-center gap-1 rounded-full bg-muted px-1.5 py-px text-[10px] text-muted-foreground"
                      >
                        <span
                          className="size-1.5 rounded-full"
                          style={{ backgroundColor: space.color }}
                          aria-hidden="true"
                        />
                        {space.name}
                      </span>
                    ) : null}
                  </span>
                </div>
              );
            })}
          </div>
        )}
        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-border px-1 pt-2 text-[10px] text-muted-foreground">
          {["Tab", "Enter", "Esc"].map((key) => (
            <kbd key={key} className="rounded border border-border bg-muted px-1 py-px text-foreground">
              {key}
            </kbd>
          ))}
          <span>Release Control or Enter to switch, Esc to cancel</span>
        </div>
      </div>
    </div>
  );
}
