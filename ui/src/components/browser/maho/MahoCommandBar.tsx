// Maho Command Bar (`Cmd+L` / `Cmd+T` / URL Pill click).
//
// One centered palette that searches and ranks every place a page can come from:
// the open tabs of the global `browserStore` (highest boost - they are already
// live, so switching costs nothing), the Favorites grid (a favorite is a store tab
// in the `favorite` section, so it carries the same boost plus a bonus), host
// bookmarks, local browsing history, and finally a synthesized address / web-search
// row driven by `normalizeBrowserAddress` + `searchUrlFor`.
//
// NATIVE OCCLUSION. `lib/nativeTerminalVisibility.tsx` has no imperative
// push/pop API: a surface occludes native terminal/browser views by *being* an
// input-owning overlay in the DOM (`[role="dialog"]`, `[role="alertdialog"]` or
// `[role="search"]`, not `aria-hidden`, not opted out). `getOverlayState()` walks
// that selector, so rendering this palette as `role="dialog"` while `open` is what
// hides the native page underneath on macOS, and returning `null` when it closes
// (or on unmount) removes the surface again - the observed mutation is what
// releases occlusion, immediately, with no state to leak.
//
// Styling is scoped: Ferryx semantic Tailwind tokens (`bg-popover`, `border-border`,
// ...) plus the Maho geometry numbers from `mahoTokens.ts` applied inline. No new
// global CSS, and the Space swatch is the only per-Space color, painted from the
// store value.

import { Globe, History, Search, Star, X, type LucideIcon } from "lucide-react";
import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";

import { fuzzyMatchScore, loadBrowserHistory, type BrowserHistoryEntry } from "../../../lib/browserHistory";
import { normalizeSuggestionUrl, useBookmarks, type Bookmark } from "../../../lib/browserLibrary";
import {
  normalizeBrowserAddress,
  searchUrlFor,
  useBrowserSettings,
  type BrowserSearchEngine,
  type BrowserSettingsState,
} from "../../../lib/browserSettings";
import { isMacShortcutPlatform } from "../../../lib/shortcuts";
import {
  DEFAULT_SPACE_COLOR,
  DEFAULT_SPACE_NAME,
  useBrowserStore,
  type BrowserSpace,
  type BrowserStoreTab,
} from "../../../state/browserStore";
import { MAHO_LAYOUT_TOKENS } from "./mahoTokens";

/** Where a chosen result lands. `mini` is the transient Mini Pane slot. */
export type MahoCommandBarTarget = "current" | "new-tab" | "mini";

/** `navigate` is the `Cmd+L` face; `new-tab` is the `Cmd+T` face. */
export type MahoCommandBarMode = "navigate" | "new-tab";

export type MahoCommandBarSource = "tab" | "bookmark" | "history" | "action";

export type MahoCommandBarResult = {
  id: string;
  source: MahoCommandBarSource;
  title: string;
  url: string;
  /** Secondary line: the compact address, or the search engine for a web search. */
  subtitle: string;
  /** Present for open tabs: choosing the row switches instead of navigating. */
  browserId?: string;
  /** Present for open tabs: the Space badge. */
  spaceName?: string;
  spaceColor?: string;
  favorite: boolean;
  /** The action row would run a web search rather than navigate to an address. */
  isSearch: boolean;
  score: number;
  /** Tie-break only: last visit/creation time. */
  recency: number;
};

export type MahoCommandBarInput = {
  query: string;
  tabs: readonly BrowserStoreTab[];
  spaces: Readonly<Record<string, BrowserSpace>>;
  bookmarks: readonly Bookmark[];
  history: readonly BrowserHistoryEntry[];
  settings: BrowserSettingsState;
};

export type MahoCommandBarProps = {
  open: boolean;
  /** Prefills the field (`Cmd+L` seeds the current address). */
  initialQuery?: string;
  mode?: MahoCommandBarMode;
  onClose: () => void;
  /** An open tab was chosen: switch to it rather than navigate. */
  onSelectOpenTab: (browserId: string) => void;
  onNavigate: (url: string, target: MahoCommandBarTarget) => void;
};

/** Open tabs outrank every other source; the boost is what keeps search below them. */
const SOURCE_WEIGHT: Readonly<Record<MahoCommandBarSource, number>> = {
  tab: 400,
  bookmark: 300,
  history: 200,
  action: 0,
};

/** A favorite is a pinned open tab: same tier, one step ahead of a plain tab. */
const FAVORITE_BONUS = 25;
const MAX_RESULTS = 8;

const ENGINE_LABELS: Readonly<Record<BrowserSearchEngine, string>> = {
  duckduckgo: "DuckDuckGo",
  google: "Google",
  bing: "Bing",
  brave: "Brave",
};

/** Address as a reader thinks of it: no scheme, no `www.`, no trailing slash. */
function compactUrl(url: string): string {
  return url
    .replace(/^[a-z][a-z0-9+.-]*:\/\//i, "")
    .replace(/^www\./i, "")
    .replace(/\/$/, "");
}

/**
 * Best fuzzy score over the page title, the full URL and the compact URL - the same
 * three ways `rankOmniboxSuggestions` reads a page, so both launchers agree.
 */
function candidateScore(query: string, url: string, title: string): number {
  return Math.max(
    fuzzyMatchScore(query, url),
    fuzzyMatchScore(query, compactUrl(url)),
    title ? fuzzyMatchScore(query, title) : 0,
  );
}

function actionRow(query: string, settings: BrowserSettingsState): MahoCommandBarResult | null {
  if (!query) return null;
  const url = normalizeBrowserAddress(query, settings);
  if (!url || url === "about:blank") return null;
  // `normalizeBrowserAddress` returns exactly this string for anything it does not
  // recognise as an address, so an equality check is the URL-vs-search decision.
  const isSearch = url === searchUrlFor(settings.searchEngine, query);
  return {
    id: isSearch ? "action:search" : "action:navigate",
    source: "action",
    title: isSearch ? `Search for "${query}"` : `Open ${compactUrl(url)}`,
    url,
    subtitle: isSearch ? ENGINE_LABELS[settings.searchEngine] : "Address",
    favorite: false,
    isSearch,
    score: SOURCE_WEIGHT.action,
    recency: 0,
  };
}

/**
 * Pure ranking. One row per page (normalized URL), best source wins the row, then the
 * source tier decides the order and the fuzzy score orders inside a tier. With no
 * query the palette behaves like a tab switcher: open tabs by recency, nothing else.
 */
export function buildMahoCommandBarResults(input: MahoCommandBarInput): MahoCommandBarResult[] {
  const query = input.query.trim();
  const byUrl = new Map<string, MahoCommandBarResult>();
  const consider = (candidate: MahoCommandBarResult) => {
    const key = normalizeSuggestionUrl(candidate.url);
    const existing = byUrl.get(key);
    if (!existing || candidate.score > existing.score) byUrl.set(key, candidate);
  };

  for (const tab of input.tabs) {
    const title = tab.title.trim() || compactUrl(tab.url);
    const match = query ? candidateScore(query, tab.url, title) : 1;
    if (match <= 0) continue;
    const space = input.spaces[tab.spaceId];
    const favorite = tab.section === "favorite";
    consider({
      id: `tab:${tab.browserId}`,
      source: "tab",
      title,
      url: tab.url,
      subtitle: compactUrl(tab.url),
      browserId: tab.browserId,
      spaceName: space?.name ?? DEFAULT_SPACE_NAME,
      spaceColor: space?.color ?? DEFAULT_SPACE_COLOR,
      favorite,
      isSearch: false,
      score: SOURCE_WEIGHT.tab + match + (favorite ? FAVORITE_BONUS : 0),
      recency: tab.lastActiveAt,
    });
  }

  if (query) {
    for (const bookmark of input.bookmarks) {
      const title = bookmark.title.trim() || compactUrl(bookmark.url);
      const match = candidateScore(query, bookmark.url, title);
      if (match <= 0) continue;
      consider({
        id: `bookmark:${bookmark.id}`,
        source: "bookmark",
        title,
        url: bookmark.url,
        subtitle: compactUrl(bookmark.url),
        favorite: false,
        isSearch: false,
        score: SOURCE_WEIGHT.bookmark + match,
        recency: bookmark.createdAt,
      });
    }
    for (const entry of input.history) {
      const title = entry.title?.trim() || compactUrl(entry.url);
      const match = candidateScore(query, entry.url, title);
      if (match <= 0) continue;
      consider({
        id: `history:${entry.id}`,
        source: "history",
        title,
        url: entry.url,
        subtitle: compactUrl(entry.url),
        favorite: false,
        isSearch: false,
        score: SOURCE_WEIGHT.history + match,
        recency: entry.visitedAt,
      });
    }
  }

  const ranked = [...byUrl.values()]
    .sort((left, right) => right.score - left.score
      || right.recency - left.recency
      || left.title.localeCompare(right.title))
    .slice(0, MAX_RESULTS);

  const action = actionRow(query, input.settings);
  if (!action) return ranked;
  // The same page is already offered by a tab/bookmark/history row, which can open it in a
  // new tab on its own: a second identical row would only split the choice.
  const actionKey = normalizeSuggestionUrl(action.url);
  if (ranked.some((row) => normalizeSuggestionUrl(row.url) === actionKey)) return ranked;
  // A typed address is what the user most likely means, so it leads; a web search is
  // the fallback and stays below anything that already exists.
  return action.isSearch ? [...ranked, action] : [action, ...ranked];
}

function rowIcon(row: MahoCommandBarResult): LucideIcon {
  if (row.source === "history") return History;
  if (row.source === "bookmark" || row.favorite) return Star;
  if (row.source === "action" && row.isSearch) return Search;
  return Globe;
}

export function MahoCommandBar({
  open,
  initialQuery,
  mode = "navigate",
  onClose,
  onSelectOpenTab,
  onNavigate,
}: MahoCommandBarProps) {
  const [query, setQuery] = useState(initialQuery ?? "");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [history, setHistory] = useState<BrowserHistoryEntry[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const listboxId = useId();
  const { settings } = useBrowserSettings();
  const { tree } = useBookmarks();
  const tabs = useBrowserStore((state) => state.tabs);
  const spaces = useBrowserStore((state) => state.spaces);
  const isMac = isMacShortcutPlatform();
  const newTabModifier = isMac ? "Cmd" : "Ctrl";

  const results = useMemo(
    () => buildMahoCommandBarResults({
      query,
      tabs: Object.values(tabs),
      spaces,
      bookmarks: tree.bookmarks,
      history: settings.rememberBrowsingHistory ? history : [],
      settings,
    }),
    [history, query, settings, spaces, tabs, tree.bookmarks],
  );

  // Opening remembers who had focus and takes it. Kept on `open` alone so a second face
  // (`Cmd+T` while already open) does not capture the palette's own input as the opener;
  // the cleanup runs on close and on unmount, which is exactly when focus goes back.
  useEffect(() => {
    if (!open) return;
    openerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const input = inputRef.current;
    input?.focus();
    input?.select();
    return () => {
      const opener = openerRef.current;
      openerRef.current = null;
      if (opener?.isConnected) opener.focus();
    };
  }, [open]);

  // Each face seeds its own starting text; re-seeding never steals focus.
  useEffect(() => {
    if (!open) return;
    setQuery(initialQuery ?? "");
    setSelectedIndex(0);
  }, [initialQuery, mode, open]);

  useEffect(() => {
    if (!open) return;
    setHistory(loadBrowserHistory());
  }, [open]);

  // Escape is handled on the window so it works wherever focus sits inside the
  // palette; the input's own handler leaves it to this listener.
  useEffect(() => {
    if (!open) return;
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleEscape);
    return () => window.removeEventListener("keydown", handleEscape);
  }, [onClose, open]);

  // Keep the highlight inside the list as the query narrows it.
  useEffect(() => {
    setSelectedIndex((current) => Math.max(0, Math.min(current, results.length - 1)));
  }, [results.length]);

  const boundedIndex = results.length === 0 ? 0 : Math.min(selectedIndex, results.length - 1);

  const commit = useCallback((row: MahoCommandBarResult | undefined, target: MahoCommandBarTarget) => {
    if (!row) return;
    // `Cmd+T` opened this palette to land somewhere new: a typed address must not
    // replace the page the user is looking at.
    const resolved = target === "current" && row.source === "action" && mode === "new-tab"
      ? "new-tab"
      : target;
    if (row.source === "tab" && row.browserId && resolved === "current") {
      onSelectOpenTab(row.browserId);
      onClose();
      return;
    }
    onNavigate(row.url, resolved);
    onClose();
  }, [mode, onClose, onNavigate, onSelectOpenTab]);

  const handleKeyDown = useCallback((event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (results.length === 0) return;
      setSelectedIndex((current) => (Math.min(current, results.length - 1) + 1) % results.length);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      if (results.length === 0) return;
      setSelectedIndex((current) => (Math.min(current, results.length - 1) - 1 + results.length) % results.length);
      return;
    }
    if (event.key !== "Enter") return;
    event.preventDefault();
    // Shift wins over Cmd: "Mini Pane" is the more specific request.
    const target: MahoCommandBarTarget = event.shiftKey
      ? "mini"
      : event.metaKey || event.ctrlKey
        ? "new-tab"
        : "current";
    commit(results[boundedIndex], target);
  }, [boundedIndex, commit, results]);

  if (!open) return null;

  const activeOptionId = results.length > 0 ? `${listboxId}-option-${boundedIndex}` : undefined;
  const placeholder = mode === "new-tab"
    ? "Search or enter an address to open in a new tab"
    : "Search tabs, favorites, bookmarks, history or enter an address";

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/60 px-4 pt-[12vh] backdrop-blur-sm"
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Maho command bar"
        data-maho-command-bar="true"
        className="w-full max-w-2xl overflow-hidden rounded-xl border border-border bg-popover text-foreground shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div
          className="flex items-center gap-2 border-b border-border px-3"
          style={{ height: MAHO_LAYOUT_TOKENS.searchPill.height }}
        >
          <Search className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          <input
            ref={inputRef}
            role="combobox"
            aria-label="Maho command bar search"
            aria-expanded={results.length > 0}
            aria-controls={listboxId}
            aria-autocomplete="list"
            aria-activedescendant={activeOptionId}
            data-testid="maho-command-bar-input"
            value={query}
            placeholder={placeholder}
            onChange={(event) => {
              setQuery(event.target.value);
              setSelectedIndex(0);
            }}
            onKeyDown={handleKeyDown}
            className="h-full min-w-0 flex-1 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground"
          />
          <button
            type="button"
            aria-label="Close command bar"
            onClick={onClose}
            className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
          >
            <X className="size-4" aria-hidden="true" />
          </button>
        </div>

        <div
          id={listboxId}
          role="listbox"
          aria-label="Command bar results"
          data-testid="maho-command-bar-results"
          className="max-h-[55vh] overflow-y-auto p-1 scrollbar-sleek"
        >
          {results.map((row, index) => {
            const Icon = rowIcon(row);
            const selected = index === boundedIndex;
            return (
              <div
                key={row.id}
                id={`${listboxId}-option-${index}`}
                role="option"
                aria-selected={selected}
                data-testid={`maho-command-bar-option-${index}`}
                data-source={row.source}
                data-browser-id={row.browserId}
                onMouseEnter={() => setSelectedIndex(index)}
                onClick={() => commit(row, "current")}
                className={`flex cursor-pointer items-center gap-2 px-2 ${
                  selected ? "bg-accent text-accent-foreground" : "text-foreground"
                }`}
                style={{
                  height: MAHO_LAYOUT_TOKENS.rows.height,
                  borderRadius: MAHO_LAYOUT_TOKENS.rows.cornerRadius,
                }}
              >
                <Icon
                  className="shrink-0 text-muted-foreground"
                  style={{
                    width: MAHO_LAYOUT_TOKENS.rows.iconSize,
                    height: MAHO_LAYOUT_TOKENS.rows.iconSize,
                  }}
                  aria-hidden="true"
                />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm">{row.title}</span>
                  <span className="block truncate font-mono text-[10px] text-muted-foreground">{row.subtitle}</span>
                </span>
                {row.spaceName ? (
                  <span
                    data-testid="maho-command-bar-space-badge"
                    className="inline-flex shrink-0 items-center gap-1 rounded-full bg-muted px-1.5 py-px text-[10px] text-muted-foreground"
                  >
                    <span
                      className="size-1.5 rounded-full"
                      style={{ backgroundColor: row.spaceColor }}
                      aria-hidden="true"
                    />
                    {row.spaceName}
                  </span>
                ) : null}
                {row.favorite ? (
                  <Star
                    className="shrink-0 text-muted-foreground"
                    style={{ width: MAHO_LAYOUT_TOKENS.rows.iconSize, height: MAHO_LAYOUT_TOKENS.rows.iconSize }}
                    aria-label="Favorite"
                  />
                ) : null}
              </div>
            );
          })}
        </div>

        {results.length === 0 ? (
          <div className="px-3 py-8 text-center text-xs text-muted-foreground">
            No matching tabs, bookmarks or history.
          </div>
        ) : null}

        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-border px-3 py-2 text-[10px] text-muted-foreground">
          <Hint keys={["Up", "Down"]} label="Navigate" />
          <Hint keys={["Enter"]} label={mode === "new-tab" ? "Open in new tab" : "Open"} />
          <Hint keys={[`${newTabModifier}+Enter`]} label="New tab" />
          <Hint keys={["Shift+Enter"]} label="Mini pane" />
          <Hint keys={["Esc"]} label="Close" />
        </div>
      </div>
    </div>
  );
}

function Hint({ keys, label }: { keys: readonly string[]; label: string }) {
  return (
    <span className="inline-flex items-center gap-1">
      {keys.map((key) => (
        <kbd key={key} className="rounded border border-border bg-muted px-1 py-px text-foreground">
          {key}
        </kbd>
      ))}
      <span>{label}</span>
    </span>
  );
}
