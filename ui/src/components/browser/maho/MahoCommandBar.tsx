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
// Geometry maps to maho_command_overlay_view.cc and maho_command_result_row_view.cc.

import { ChevronRight, Globe } from "lucide-react";
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
import {
  browserStore,
  type BrowserStore,
  DEFAULT_SPACE_COLOR,
  DEFAULT_SPACE_NAME,
  useBrowserStore,
  type BrowserSpace,
  type BrowserStoreTab,
} from "../../../state/browserStore";

/** Where a chosen result lands. `mini` is the transient Mini Pane slot. */
export type MahoCommandBarTarget = "current" | "new-tab" | "mini";

/** `navigate` is the `Cmd+L` face; `new-tab` is the `Cmd+T` face. */
export type MahoCommandBarMode = "navigate" | "new-tab" | "all-tabs" | "commands";

export type MahoBrowserCommand = { id: string; title: string; run: () => void; disabled?: boolean };
export type MahoSiteSearch = { keyword: string; name: string; urlTemplate: string };

// MahoCommandModel::ExpandSiteSearchTemplate accepts all three template tokens.
export const MAHO_SITE_SEARCHES: readonly MahoSiteSearch[] = [
  { keyword: "google.com", name: "Google", urlTemplate: "https://www.google.com/search?q={query}" },
  { keyword: "duckduckgo.com", name: "DuckDuckGo", urlTemplate: "https://duckduckgo.com/?q={query}" },
  { keyword: "bing.com", name: "Bing", urlTemplate: "https://www.bing.com/search?q={query}" },
  { keyword: "search.brave.com", name: "Brave", urlTemplate: "https://search.brave.com/search?q={query}" },
  { keyword: "youtube.com", name: "YouTube", urlTemplate: "https://www.youtube.com/results?search_query={query}" },
  { keyword: "github.com", name: "GitHub", urlTemplate: "https://github.com/search?q={query}" },
  { keyword: "wikipedia.org", name: "Wikipedia", urlTemplate: "https://en.wikipedia.org/w/index.php?search={query}" },
];

export function expandMahoSiteSearch(template: string, query: string): string | null {
  if (!query.trim() || !/\{query\}|\{searchTerms\}|%s/.test(template)) return null;
  const expanded = template.replace(/\{query\}|\{searchTerms\}|%s/g, encodeURIComponent(query.trim()).replace(/%20/g, "+"));
  try {
    const url = new URL(expanded);
    return url.protocol === "https:" || url.protocol === "http:" ? url.href : null;
  } catch { return null; }
}

export function matchMahoSiteSearch(query: string, engines: readonly MahoSiteSearch[]) {
  const match = /^@?(\S+)\s+(.+)$/.exec(query.trim());
  if (!match) return null;
  const keyword = match[1].toLowerCase().replace(/^www\./, "");
  const engine = engines.find((entry) => entry.keyword === keyword || entry.name.toLowerCase() === keyword);
  if (!engine) return null;
  const url = expandMahoSiteSearch(engine.urlTemplate, match[2]);
  return url ? { engine, url, query: match[2] } : null;
}

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
  faviconUrl?: string;
  commandId?: string;
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
  mode?: MahoCommandBarMode;
};

export type MahoCommandBarProps = {
  open: boolean;
  /** Prefills the field (`Cmd+L` seeds the current address). */
  initialQuery?: string;
  mode?: MahoCommandBarMode;
  commands?: readonly MahoBrowserCommand[];
  siteSearches?: readonly MahoSiteSearch[];
  store?: BrowserStore;
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
const MAX_RESULTS = 5;

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
  const allTabs = input.mode === "all-tabs";
  const byUrl = new Map<string, MahoCommandBarResult>();
  const consider = (candidate: MahoCommandBarResult) => {
    const key = allTabs ? candidate.id : normalizeSuggestionUrl(candidate.url);
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
      faviconUrl: tab.faviconUrl,
      isSearch: false,
      score: SOURCE_WEIGHT.tab + match + (favorite ? FAVORITE_BONUS : 0),
      recency: tab.lastActiveAt,
    });
  }

  if (query && !allTabs) {
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

  const ranked = [...byUrl.values()].sort((left, right) => right.score - left.score
    || right.recency - left.recency
    || left.title.localeCompare(right.title));

  if (allTabs) return ranked;
  const action = actionRow(query, input.settings);
  if (!action) return ranked.slice(0, MAX_RESULTS);
  // The same page is already offered by a tab/bookmark/history row, which can open it in a
  // new tab on its own: a second identical row would only split the choice.
  const actionKey = normalizeSuggestionUrl(action.url);
  if (ranked.some((row) => normalizeSuggestionUrl(row.url) === actionKey)) {
    return ranked.slice(0, MAX_RESULTS);
  }
  // The action row is part of the list, not a bonus entry: the whole palette still fits in
  // `MAX_RESULTS`, so the list never grows past what the panel can show.
  const room = MAX_RESULTS - 1;
  // A typed address is what the user most likely means, so it leads; a web search is
  // the fallback and stays below anything that already exists.
  return action.isSearch ? [...ranked.slice(0, room), action] : [action, ...ranked.slice(0, room)];
}

export function MahoCommandBar({
  open,
  initialQuery,
  mode = "navigate",
  commands = [],
  siteSearches = MAHO_SITE_SEARCHES,
  store = browserStore,
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
  const tabs = useBrowserStore((state) => state.tabs, store);
  const spaces = useBrowserStore((state) => state.spaces, store);
  const [siteSearch, setSiteSearch] = useState<MahoSiteSearch | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const commandsMode = mode === "commands" || query.startsWith(">");
  const matchedSite = siteSearch || mode === "all-tabs" ? null : matchMahoSiteSearch(query, siteSearches);
  const siteUrl = siteSearch ? expandMahoSiteSearch(siteSearch.urlTemplate, query) : matchedSite?.url;
  const activeSite = siteSearch ?? matchedSite?.engine;
  const enterSiteSearch = useCallback((engine: MahoSiteSearch) => {
    setSiteSearch(engine);
    setQuery("");
    setSelectedIndex(0);
    setPickerOpen(false);
    inputRef.current?.focus();
  }, []);

  const results = useMemo(
    (): MahoCommandBarResult[] => {
      if (commandsMode) {
        const filter = query.replace(/^>\s*/, "").trim();
        return commands.filter((command) => !command.disabled && (!filter || candidateScore(filter, command.id, command.title) > 0))
          .map((command) => ({ id: `command:${command.id}`, commandId: command.id, source: "action", title: command.title,
            url: "", subtitle: "", favorite: false, isSearch: false, score: 0, recency: 0 }));
      }
      if (activeSite) return siteUrl ? [{ id: "action:site-search", source: "action", title: siteSearch ? query : matchedSite?.query ?? query,
        url: siteUrl, subtitle: activeSite.name, favorite: false, isSearch: true, score: 0, recency: 0 }] : [];
      return buildMahoCommandBarResults({
        query,
        tabs: Object.values(tabs),
        spaces,
        bookmarks: tree.bookmarks,
        history: settings.rememberBrowsingHistory ? history : [],
        settings,
        mode,
      });
    },
    [activeSite, commands, commandsMode, history, matchedSite?.query, mode, query, settings, siteSearch, siteUrl, spaces, tabs, tree.bookmarks],
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
    setSiteSearch(null);
    setPickerOpen(false);
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
      if (pickerOpen) { setPickerOpen(false); inputRef.current?.focus(); return; }
      if (siteSearch || query.startsWith(">")) {
        setSiteSearch(null);
        setQuery("");
        return;
      }
      onClose();
    };
    window.addEventListener("keydown", handleEscape);
    return () => window.removeEventListener("keydown", handleEscape);
  }, [onClose, open, pickerOpen, query, siteSearch]);

  // Keep the highlight inside the list as the query narrows it.
  useEffect(() => {
    setSelectedIndex((current) => Math.max(0, Math.min(current, results.length - 1)));
  }, [results.length]);

  const boundedIndex = results.length === 0 ? 0 : Math.min(selectedIndex, results.length - 1);

  // `Cmd+T` opened this palette to land somewhere new. An existing tab/bookmark/history row
  // for the typed address is the same page as the typed address, so it must not silently
  // replace the page the user is looking at either.
  const typedAddressKey = useMemo(() => {
    if (mode !== "new-tab") return null;
    if (!query.trim()) return null;
    const url = normalizeBrowserAddress(query, settings);
    if (!url || url === "about:blank") return null;
    return normalizeSuggestionUrl(url);
  }, [mode, query, settings]);

  const commit = useCallback((row: MahoCommandBarResult | undefined, target: MahoCommandBarTarget) => {
    if (!row) return;
    if (row.commandId) {
      onClose();
      commands.find((command) => command.id === row.commandId)?.run();
      return;
    }
    const matchesTypedAddress = typedAddressKey !== null
      && normalizeSuggestionUrl(row.url) === typedAddressKey;
    const resolved = target === "current"
      && (matchesTypedAddress || (row.source === "action" && mode === "new-tab"))
      ? "new-tab"
      : target;
    if (row.source === "tab" && row.browserId && resolved === "current") {
      onSelectOpenTab(row.browserId);
      onClose();
      return;
    }
    onNavigate(row.url, resolved);
    onClose();
  }, [commands, mode, onClose, onNavigate, onSelectOpenTab, typedAddressKey]);

  const handleKeyDown = useCallback((event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Tab" && !event.shiftKey && !commandsMode && mode !== "all-tabs") {
      const keyword = query.trim().replace(/^@/, "").toLowerCase();
      const engine = siteSearches.find((entry) => entry.keyword === keyword || entry.name.toLowerCase() === keyword);
      if (engine) { event.preventDefault(); enterSiteSearch(engine); return; }
    }
    if (event.key === "Backspace" && !query && siteSearch) { setSiteSearch(null); return; }
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
  }, [boundedIndex, commandsMode, commit, enterSiteSearch, mode, query, results, siteSearch, siteSearches]);

  if (!open) return null;

  const activeOptionId = results.length > 0 ? `${listboxId}-option-${boundedIndex}` : undefined;
  const contextFavicon = results.find((row) => row.url === query)?.faviconUrl;

  return (
    <div
      className="maho-command-backdrop"
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Maho command bar"
        data-maho-command-bar="true"
        className="maho-command-panel"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="maho-command-search-row">
          <span className="maho-command-glyph" aria-hidden="true">
            {query ? contextFavicon ? <img src={contextFavicon} alt="" /> : <Globe /> : null}
          </span>
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
            placeholder={commandsMode ? "Search commands…" : activeSite ? "Search…" : mode === "all-tabs" ? "Search tabs…" : "Search or Enter URL…"}
            onChange={(event) => {
              setQuery(event.target.value);
              setSelectedIndex(0);
            }}
            onKeyDown={handleKeyDown}
            className="maho-command-input"
          />
          {commandsMode || activeSite || mode === "all-tabs" ? (
            <span className="maho-command-mode-chip"><i />{commandsMode ? "Commands" : activeSite?.name ?? "Tabs"}</span>
          ) : null}
          <button type="button" className="maho-command-picker-button" aria-label="Choose search mode or engine"
            aria-expanded={pickerOpen} onClick={() => setPickerOpen((current) => !current)}>Search</button>
        </div>
        {pickerOpen ? (
          <div className="maho-command-picker" role="group" aria-label="Search modes and engines">
            <button type="button" onClick={() => { setSiteSearch(null); setQuery(">"); setPickerOpen(false); inputRef.current?.focus(); }}>Commands</button>
            {siteSearches.map((engine) => <button type="button" key={engine.keyword}
              onClick={() => enterSiteSearch(engine)}>{engine.name}</button>)}
          </div>
        ) : null}

        <div
          id={listboxId}
          role="listbox"
          aria-label="Command bar results"
          data-testid="maho-command-bar-results"
          className="maho-command-results scrollbar-sleek"
        >
          {results.map((row, index) => {
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
                className="maho-command-result"
              >
                <span className="maho-command-result-content">
                  <span className="maho-command-glyph" aria-hidden="true">
                    {row.faviconUrl ? <img src={row.faviconUrl} alt="" /> : <Globe />}
                  </span>
                  <span className="maho-command-result-text">
                    <span className="maho-command-result-title">{row.title}</span>
                    {query.trim() && row.subtitle ? (
                      <span className="maho-command-result-metadata">— {row.subtitle}</span>
                    ) : null}
                  </span>
                  {row.source === "tab" ? (
                    <span className="maho-command-switch-tab">
                      <span>Switch to Tab</span>
                      <span className="maho-command-chevron"><ChevronRight aria-hidden="true" /></span>
                    </span>
                  ) : null}
                </span>
              </div>
            );
          })}
        </div>

        {results.length === 0 ? (
          <div className="maho-command-empty" data-compact={Boolean(query.trim())}>
            {query.trim() ? `No results for “${query.trim()}”` : "Type to search tabs, history, and URLs"}
          </div>
        ) : null}

      </div>
    </div>
  );
}
