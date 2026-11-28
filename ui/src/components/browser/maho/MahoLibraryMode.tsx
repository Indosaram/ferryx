// Maho Library mode: the 92dp category rail plus the list pane beside it.
//
// Source: maho_sidebar_library_rail_view.cc and maho_library_overlay_view.cc.
// Ferryx retains its working bookmarks/history managers alongside the ported panels.
//
// The pane reuses the shipped data layer instead of re-implementing it:
// `useBookmarks` / `addBookmark` / `updateBookmark` / `deleteBookmark` /
// `createBookmarkFolder` for host bookmarks, `listHostHistory` for the host visit
// store, `useBrowserDownloads` + `useBrowserDownloadActions` + `BrowserDownloadRow`
// for the download registry, and the global `browserStore` archive for closed tabs.
//
// OCCLUSION. `lib/nativeTerminalVisibility.tsx` has no imperative show/hide API: a
// surface owns input by *being* an input-owning overlay in the DOM. Overlaying a
// native page renders as `role="dialog"`, which is exactly what hides the native
// surface underneath and releases it again when this node unmounts. Inside the
// sidebar drawer the pane is in flow, so it must not claim a dialog role.

import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  AlertTriangle,
  Archive,
  Home,
  Images,
  ArrowDownToLine,
  FolderPlus,
  Globe,
  Pencil,
  Search,
  Trash2,
  X,
  type LucideIcon,
} from "lucide-react";

import { extractDownloadName, formatBytes } from "../../BrowserDownloadRow";
import { downloadFailureLabel } from "../../../lib/browserSystem";
import { useBrowserDownloadActions, useBrowserDownloads } from "../../BrowserDownloadsShelf";
import {
  addBookmark,
  createBookmarkFolder,
  deleteBookmark,
  errorCode,
  listHostHistory,
  updateBookmark,
  useBookmarks,
  type HostHistoryEntry,
} from "../../../lib/browserLibrary";
import { browserStore, selectSpaceList, useBrowserStore, type BrowserStore } from "../../../state/browserStore";
import { MAHO_LAYOUT_TOKENS } from "./mahoTokens";

const LIBRARY = MAHO_LAYOUT_TOKENS.library;
const HISTORY_LIMIT = 100;

export type MahoLibraryCategory = "archive" | "downloads" | "spaces" | "media" | "bookmarks" | "history";

/** The rail's four categories, in Maho's order. */
export const MAHO_LIBRARY_CATEGORIES: ReadonlyArray<{
  id: MahoLibraryCategory;
  label: string;
  icon: LucideIcon;
}> = [
  { id: "downloads", label: "Downloads", icon: ArrowDownToLine },
  { id: "archive", label: "Archived Tabs", icon: Archive },
  { id: "spaces", label: "Spaces", icon: Home },
  { id: "media", label: "Media", icon: Images },
];

const RECOVERY_NOTICE: Readonly<Record<"recoveredFromBackup" | "resetCorrupt", string>> = {
  recoveredFromBackup: "Bookmarks were restored from a backup copy after the saved file could not be read.",
  resetCorrupt: "The bookmarks file was damaged and has been reset. Saved bookmarks start empty.",
};

export type MahoLibraryModeProps = {
  store?: BrowserStore;
  /** Controlled category; omit to let the rail own the selection. */
  category?: MahoLibraryCategory;
  defaultCategory?: MahoLibraryCategory;
  onCategoryChange?: (category: MahoLibraryCategory) => void;
  /** True when this surface covers a native page instead of sitting inside the drawer. */
  overlay?: boolean;
  onOpenTab?: (browserId: string) => void;
  onOpenUrl?: (url: string) => void;
  onClose?: () => void;
  showMedia?: boolean;
  railOnly?: boolean;
  contentOnly?: boolean;
};

export function MahoLibraryMode({
  store = browserStore,
  category,
  defaultCategory = "archive",
  onCategoryChange,
  overlay = false,
  onOpenTab,
  onOpenUrl,
  onClose,
  showMedia = false,
  railOnly = false,
  contentOnly = false,
}: MahoLibraryModeProps) {
  const [internalCategory, setInternalCategory] = useState<MahoLibraryCategory>(defaultCategory);
  const active = category ?? internalCategory;
  const activeLabel = MAHO_LIBRARY_CATEGORIES.find((candidate) => candidate.id === active)?.label ?? "Archive";

  useEffect(() => {
    if (!overlay || !onClose) return;
    const dismiss = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", dismiss);
    return () => window.removeEventListener("keydown", dismiss);
  }, [overlay, onClose]);

  const select = useCallback((next: MahoLibraryCategory) => {
    setInternalCategory(next);
    onCategoryChange?.(next);
  }, [onCategoryChange]);

  return (
    <LibraryFrame overlay={overlay}>
      <div className="flex h-full min-h-0 w-full">
        {!contentOnly ? <nav
          role="tablist"
          aria-label="Browser library"
          aria-orientation="vertical"
          className="maho-library-rail shrink-0"
          style={{
            width: LIBRARY.railWidth,
            flex: `0 0 ${LIBRARY.railWidth}px`,
            paddingTop: LIBRARY.railInsets.top,
            paddingBottom: LIBRARY.railInsets.bottom,
          }}
        >
          {MAHO_LIBRARY_CATEGORIES.filter((item) => item.id !== "media" || showMedia).map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={active === id}
              aria-label={label}
              title={label}
              data-category={id}
              className="maho-library-rail__item"
              style={{ paddingBlock: LIBRARY.itemVerticalInset }}
              onClick={() => select(id)}
            >
              <Icon aria-hidden="true" />
              <span className="maho-library-rail__label">{id === "archive" ? <>Archived<br />Tabs</> : label}</span>
            </button>
          ))}
        </nav> : null}
        {!railOnly ? <section
          role="tabpanel"
          aria-label={activeLabel}
          className="maho-library-content flex min-h-0 min-w-0 flex-1 flex-col"
        >
          {onClose ? (
            <div className="maho-library-close">
              <button
                type="button"
                aria-label="Close library"
                onClick={onClose}
                className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
              >
                <X className="size-3.5" aria-hidden="true" />
              </button>
            </div>
          ) : null}
          {active === "archive" ? (
            <ArchivePanel store={store} onOpenTab={onOpenTab} />
          ) : active === "downloads" ? (
            <DownloadsPanel />
          ) : active === "spaces" ? (
            <LibrarySpaces store={store} onOpenTab={onOpenTab} />
          ) : active === "media" ? (
            <LibraryMedia store={store} onOpenTab={onOpenTab} />
          ) : active === "bookmarks" ? (
            <BookmarksPanel onOpenUrl={onOpenUrl} />
          ) : (
            <HistoryPanel onOpenUrl={onOpenUrl} />
          )}
        </section> : null}
      </div>
    </LibraryFrame>
  );
}

/**
 * The one place the overlay decision lives: a covering surface is a dialog (and so
 * hides the native page), an in-flow one is a plain panel.
 */
function LibraryFrame({ overlay, children }: { overlay: boolean; children: ReactNode }) {
  if (!overlay) {
    return (
      <div className="flex h-full min-h-0 w-full flex-col overflow-hidden" data-maho-library="true">
        {children}
      </div>
    );
  }
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Browser library"
      data-maho-library="true"
      data-maho-library-overlay="true"
      className="maho-library-overlay absolute inset-y-0 start-0 z-30 flex min-h-0 flex-col overflow-hidden"
      style={{ width: LIBRARY.railWidth + LIBRARY.overlayContentWidth }}
    >
      {children}
    </div>
  );
}

function LibrarySpaces({ store, onOpenTab }: { store: BrowserStore; onOpenTab?: (id: string) => void }) {
  const state = useBrowserStore((snapshot) => snapshot, store);
  return <div className="maho-library-spaces">{selectSpaceList(state).map((space) => (
    <section key={space.id}>
      <button type="button" className="maho-library-space-title" onClick={() => store.setActiveSpace(space.id)}><Home aria-hidden="true" style={{ color: space.color }} />{space.name}</button>
      {Object.values(state.tabs).filter((tab) => tab.spaceId === space.id && tab.browserId !== state.miniBrowserId).map((tab) => <button type="button" key={tab.browserId} className="maho-library-row__open" onClick={() => onOpenTab?.(tab.browserId)}>{tab.title || compactLocation(tab.url)}</button>)}
    </section>
  ))}</div>;
}

function LibraryMedia({ store, onOpenTab }: { store: BrowserStore; onOpenTab?: (id: string) => void }) {
  const tabs = useBrowserStore((state) => Object.values(state.tabs), store);
  const media = tabs.filter((tab) => /\.(?:mp4|webm|mp3|ogg|wav|png|jpe?g|gif|webp)(?:[?#]|$)/i.test(tab.url));
  return <div className="maho-library-media">{media.map((tab) => <button type="button" key={tab.browserId} onClick={() => onOpenTab?.(tab.browserId)}>{/\.(?:png|jpe?g|gif|webp)(?:[?#]|$)/i.test(tab.url) ? <img src={tab.url} alt="" /> : <Images aria-hidden="true" />}<span>{tab.title || compactLocation(tab.url)}</span></button>)}</div>;
}

function SearchField({
  label,
  placeholder,
  value,
  onChange,
  autoFocus = false,
}: {
  label: string;
  placeholder: string;
  value: string;
  onChange: (value: string) => void;
  autoFocus?: boolean;
}) {
  return (
    <div className="maho-library-search-wrap">
      <label className="maho-library-search">
      <Search className="size-3.5 shrink-0" aria-hidden="true" />
      <input
        aria-label={label}
        autoFocus={autoFocus}
        placeholder={placeholder}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="min-w-0 flex-1 bg-transparent outline-none"
      />
      </label>
    </div>
  );
}

function searchTerms(query: string): string[] {
  return query.trim().toLowerCase().split(/\s+/).filter(Boolean);
}

function matchesTerms(terms: readonly string[], haystack: string): boolean {
  if (terms.length === 0) return true;
  const lower = haystack.toLowerCase();
  return terms.every((term) => lower.includes(term));
}

/** Closed/cleared tabs from the global store, restorable to Today in one click. */
function ArchivePanel({ store, onOpenTab }: { store: BrowserStore; onOpenTab?: (browserId: string) => void }) {
  const archive = useBrowserStore((state) => state.archive, store);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("Any time");
  const [menu, setMenu] = useState<{ browserId: string; x: number; y: number } | null>(null);
  const deleteEntry = (id: string) => store.setState((state) => ({ ...state, archive: state.archive.filter((entry) => entry.tab.browserId !== id) }));
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    window.addEventListener("click", close);
    window.addEventListener("keydown", key);
    return () => { window.removeEventListener("click", close); window.removeEventListener("keydown", key); };
  }, [menu]);
  const terms = useMemo(() => searchTerms(query), [query]);
  const rows = useMemo(() => {
    const today = new Date().setHours(0, 0, 0, 0);
    const week = today - 7 * 86_400_000;
    return archive.filter((entry) => matchesTerms(terms, `${entry.tab.title} ${entry.tab.url}`)
      && (filter === "Any time" || (filter === "Today" ? entry.archivedAt >= today
        : filter === "Past week" ? entry.archivedAt >= week : entry.archivedAt < week)));
  }, [archive, terms, filter]);

  const restore = (browserId: string) => {
    const result = store.restoreFromArchive(browserId);
    if (result.ok) onOpenTab?.(browserId);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="maho-library-archive">
      <SearchField label="Search Archive" placeholder="Search archived tabs..." value={query} onChange={setQuery} autoFocus />
      <div className="maho-library-filter">
        <span>{rows.length === 0 ? "No tabs" : `${rows.length} ${rows.length === 1 ? "tab" : "tabs"}`}</span>
        <select aria-label="Filter archived tabs" value={filter} onChange={(event) => setFilter(event.target.value)}>
          {["Any time", "Today", "Past week", "Older"].map((value) => <option key={value}>{value}</option>)}
        </select>
      </div>
      <ul aria-label="Archived tabs" className="maho-library-list">
        {rows.length === 0 ? (
          <li className="maho-library-empty">
            <svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="3" width="18" height="4" rx="1" /><path d="M5 7v13h14V7M10 11h4" /></svg>
            <strong>{archive.length === 0 ? "Nothing archived yet" : "No matching tabs"}</strong>
            <span>{archive.length === 0 ? "Tabs you archive will stay here until you bring them back." : "Clear the search or adjust the filter to see archived tabs."}</span>
          </li>
        ) : null}
        {rows.map((entry, index) => {
          const label = entry.tab.title.trim() || compactLocation(entry.tab.url);
          const group = archiveDateGroup(entry.archivedAt);
          return (
            <li key={entry.tab.browserId}>
              {index === 0 || archiveDateGroup(rows[index - 1].archivedAt) !== group ? <div className="maho-library-date">{group}<span>{rows.filter((row) => archiveDateGroup(row.archivedAt) === group).length} tabs</span></div> : null}
              <div className="maho-library-row" onContextMenu={(event) => { event.preventDefault(); setMenu({ browserId: entry.tab.browserId, x: event.clientX, y: event.clientY }); }}>
                <button type="button" aria-label={`Restore archived tab ${label}`} onClick={() => restore(entry.tab.browserId)} className="maho-library-row__open">
                  <span className="maho-library-tile" aria-hidden="true">{entry.tab.faviconUrl ? <img src={entry.tab.faviconUrl} alt="" /> : label.charAt(0).toUpperCase()}</span>
                  <span className="min-w-0 flex-1">
                    <span className="maho-library-row__title">{label}</span>
                    <span className="maho-library-row__subtitle">{compactLocation(entry.tab.url)}</span>
                  </span>
                </button>
                <button type="button" className="maho-library-row__delete" aria-label={`Delete archived tab ${label}`} onClick={() => deleteEntry(entry.tab.browserId)}>
                  <Trash2 aria-hidden="true" />
                </button>
              </div>
            </li>
          );
        })}
      </ul>
      {menu ? <div role="dialog" aria-label="Archived tab actions" className="maho-library-context" style={{ left: menu.x, top: menu.y }}><div role="menu"><button type="button" role="menuitem" onClick={() => { restore(menu.browserId); setMenu(null); }}>Restore</button><button type="button" role="menuitem" onClick={() => { deleteEntry(menu.browserId); setMenu(null); }}>Delete</button></div></div> : null}
    </div>
  );
}

export function archiveDateGroup(timestamp: number, now = Date.now()): string {
  if (!timestamp) return "Earlier";
  const date = new Date(timestamp);
  const today = new Date(now);
  const days = Math.max(0, Math.round((Date.UTC(today.getFullYear(), today.getMonth(), today.getDate()) - Date.UTC(date.getFullYear(), date.getMonth(), date.getDate())) / 86_400_000));
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days <= 7) return `${days} days ago`;
  if (days <= 14) return "Last 2 weeks";
  if (days <= 21) return "3 weeks ago";
  if (days <= 28) return "4 weeks ago";
  return "Earlier";
}

function compactLocation(url: string): string {
  try {
    const parsed = new URL(url);
    const path = parsed.pathname.replace(/^\/+|\/+$/g, "");
    return parsed.host + (path ? ` / ${path.length > 28 ? `${path.slice(0, 25)}...` : path}` : "");
  } catch {
    return url;
  }
}

function dateGroup(timestamp: number): string {
  const today = new Date().setHours(0, 0, 0, 0);
  if (timestamp >= today) return "Today";
  if (timestamp >= today - 86_400_000) return "Yesterday";
  if (timestamp >= today - 7 * 86_400_000) return "Last Week";
  return "Older";
}

function DownloadsPanel() {
  const [query, setQuery] = useState("");
  const { downloads, loadErrorCode, loading } = useBrowserDownloads();
  const { missingFileIds, actionErrors, handleCancel, handlePause, handleOpen, handleReveal } =
    useBrowserDownloadActions();
  const sorted = useMemo(() => [...downloads]
    .filter((record) => matchesTerms(searchTerms(query), `${extractDownloadName(record.filePath, record.url)} ${record.url}`))
    .sort((a, b) => b.createdAtMs - a.createdAtMs), [downloads, query]);

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="maho-library-downloads">
      <SearchField label="Search downloads" placeholder="Search downloads..." value={query} onChange={setQuery} />
      {loadErrorCode ? (
        <div className="rounded border border-destructive/40 bg-destructive/10 px-2 py-1 text-[11px] text-destructive">
          Could not read the download list <span className="font-mono text-[10px]">{loadErrorCode}</span>
        </div>
      ) : null}
      {sorted.length === 0 && !loading && !loadErrorCode ? (
        <div className="maho-library-empty">
          <ArrowDownToLine aria-hidden="true" />
          <strong>{downloads.length ? "No matching downloads" : "No downloads yet"}</strong>
          <span>{downloads.length ? "Clear the search to see all downloads." : "Files you download will show up here."}</span>
        </div>
      ) : null}
      <div role="list" aria-label="Downloads" className="maho-library-list maho-library-download-list">
        {sorted.map((record, index) => (
          <div role="listitem" key={record.id}>
            {index === 0 || dateGroup(sorted[index - 1].createdAtMs) !== dateGroup(record.createdAtMs) ? <div className="maho-library-date">{dateGroup(record.createdAtMs)}</div> : null}
            <div className="maho-library-download-row" data-testid={`download-row-${record.id}`}>
              <span className="maho-library-tile" aria-hidden="true"><ArrowDownToLine /></span>
              <div className="min-w-0 flex-1">
                <span className="maho-library-row__title">{extractDownloadName(record.filePath, record.url)}</span>
                <span className="maho-library-row__subtitle">
                  {record.status === "failed" ? downloadFailureLabel(record.failure, record.error) : missingFileIds.has(record.id) ? "Deleted on disk" : record.status === "cancelled" ? "Cancelled" : `${record.status === "paused" ? "Paused - " : ""}${formatBytes(record.receivedBytes)}${record.totalBytes && record.status !== "completed" ? ` of ${formatBytes(record.totalBytes)}` : ""} from ${compactLocation(record.url).split(" / ")[0]}`}
                </span>
                {actionErrors[record.id] ? <span role="alert" className="text-destructive text-xs">{actionErrors[record.id]}</span> : null}
              </div>
              <div className="maho-library-download-actions">
                {record.status === "inProgress" || record.status === "pending" || record.status === "paused" ? (
                  <>
                    <progress aria-label="Download progress" max={record.totalBytes || undefined} value={record.totalBytes ? record.receivedBytes : undefined} />
                    {record.canPause ? <button type="button" aria-label={record.status === "paused" ? "Resume download" : "Pause download"} onClick={() => void handlePause(record.id, record.status !== "paused")}>{record.status === "paused" ? "Resume" : "Pause"}</button> : null}
                    {record.canCancel !== false ? <button type="button" aria-label="Cancel download" onClick={() => void handleCancel(record.id)}>Cancel</button> : null}
                  </>
                ) : record.status === "completed" && !missingFileIds.has(record.id) ? (
                  <>
                    <button type="button" aria-label="Open download" onClick={() => void handleOpen(record)}>Open</button>
                    <button type="button" aria-label="Reveal in folder" onClick={() => void handleReveal(record)}>Show in folder</button>
                  </>
                ) : null}
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function BookmarksPanel({ onOpenUrl }: { onOpenUrl?: (url: string) => void }) {
  const { tree, error: bookmarkError, refresh } = useBookmarks();
  const [query, setQuery] = useState("");
  const [title, setTitle] = useState("");
  const [url, setUrl] = useState("");
  const [editing, setEditing] = useState<{ id: string; title: string; url: string } | null>(null);
  const [creatingFolder, setCreatingFolder] = useState(false);
  const [folderName, setFolderName] = useState("");
  const [error, setError] = useState<string | null>(null);

  // `useBookmarks` reports the host's recovery marker through the same channel as a
  // load failure; the two are different states and must never share a message.
  const recovery = bookmarkError === "recoveredFromBackup" || bookmarkError === "resetCorrupt"
    ? bookmarkError
    : null;
  const loadError = recovery ? null : bookmarkError;

  const run = (action: () => Promise<unknown>, label: string) => {
    void action()
      .then(() => {
        setError(null);
        refresh();
      })
      .catch((err: unknown) => setError(`${label} failed: ${errorCode(err)}`));
  };

  const submitBookmark = () => {
    const nextTitle = title.trim();
    const nextUrl = url.trim();
    if (!nextTitle || !nextUrl) return;
    setTitle("");
    setUrl("");
    run(() => addBookmark(nextTitle, nextUrl), "Add bookmark");
  };

  const submitFolder = () => {
    const name = folderName.trim();
    if (!name) return;
    setCreatingFolder(false);
    setFolderName("");
    run(() => createBookmarkFolder(name), "New folder");
  };

  const terms = useMemo(() => searchTerms(query), [query]);
  const rows = useMemo(
    () => tree.bookmarks.filter((mark) => matchesTerms(terms, `${mark.title} ${mark.url}`)),
    [terms, tree.bookmarks],
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="maho-library-bookmarks">
      <SearchField label="Search bookmarks" placeholder="Search bookmarks" value={query} onChange={setQuery} />
      <form
        className="flex items-center gap-1 border-b border-border px-2 py-1.5"
        onSubmit={(event) => {
          event.preventDefault();
          submitBookmark();
        }}
      >
        <input
          aria-label="New bookmark title"
          placeholder="Title"
          value={title}
          onChange={(event) => setTitle(event.target.value)}
          className="h-7 min-w-0 flex-1 rounded border border-border bg-background px-2 text-xs outline-none focus:border-ring"
        />
        <input
          aria-label="New bookmark URL"
          placeholder="https://"
          value={url}
          onChange={(event) => setUrl(event.target.value)}
          className="h-7 min-w-0 flex-1 rounded border border-border bg-background px-2 font-mono text-xs outline-none focus:border-ring"
        />
        <button
          type="submit"
          aria-label="Add bookmark"
          disabled={!title.trim() || !url.trim()}
          className="shrink-0 rounded border border-border px-1.5 py-1 text-[10px] hover:bg-accent disabled:opacity-40"
        >
          Add
        </button>
        <button
          type="button"
          aria-label="New folder"
          aria-expanded={creatingFolder}
          onClick={() => setCreatingFolder((current) => !current)}
          className="shrink-0 rounded border border-border p-1 hover:bg-accent"
        >
          <FolderPlus className="size-3" aria-hidden="true" />
        </button>
      </form>
      {creatingFolder ? (
        <form
          className="flex items-center gap-1 border-b border-border px-2 py-1.5"
          onSubmit={(event) => {
            event.preventDefault();
            submitFolder();
          }}
        >
          <input
            aria-label="New folder name"
            placeholder="Folder name"
            value={folderName}
            onChange={(event) => setFolderName(event.target.value)}
            className="h-7 min-w-0 flex-1 rounded border border-border bg-background px-2 text-xs outline-none focus:border-ring"
          />
          <button
            type="submit"
            disabled={!folderName.trim()}
            className="rounded border border-border px-1.5 py-1 text-[10px] hover:bg-accent disabled:opacity-40"
          >
            Create folder
          </button>
          <button
            type="button"
            onClick={() => {
              setCreatingFolder(false);
              setFolderName("");
            }}
            className="rounded px-1.5 py-1 text-[10px] hover:bg-accent"
          >
            Cancel
          </button>
        </form>
      ) : null}
      {recovery ? (
        <div
          role="status"
          data-testid="maho-library-bookmark-recovery"
          className="flex items-start gap-1.5 border-b border-amber-500/40 bg-amber-500/10 px-2 py-1.5 text-[11px] text-amber-600 dark:text-amber-400"
        >
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
          <span className="min-w-0 flex-1">{RECOVERY_NOTICE[recovery]}</span>
        </div>
      ) : null}
      {loadError ? (
        <div role="alert" className="px-2 py-1 text-[11px] text-destructive">
          Bookmarks: {loadError}
        </div>
      ) : null}
      {error ? (
        <div role="alert" className="px-2 py-1 text-[11px] text-destructive">
          {error}
        </div>
      ) : null}
      <ul aria-label="Bookmarks" className="maho-library-manager-list min-h-0 flex-1 overflow-y-auto">
        {rows.length === 0 ? (
          <li className="px-2 py-6 text-center text-[11px] text-muted-foreground">No bookmarks here.</li>
        ) : null}
        {rows.map((mark) => {
          if (editing?.id === mark.id) {
            return (
              <li key={mark.id} className="flex items-center gap-1 rounded px-1.5 py-1">
                <input
                  aria-label="Bookmark name"
                  value={editing.title}
                  onChange={(event) => setEditing({ ...editing, title: event.target.value })}
                  className="h-7 min-w-0 flex-1 rounded border border-border bg-background px-2 text-xs"
                />
                <input
                  aria-label="Bookmark URL"
                  value={editing.url}
                  onChange={(event) => setEditing({ ...editing, url: event.target.value })}
                  className="h-7 min-w-0 flex-1 rounded border border-border bg-background px-2 font-mono text-xs"
                />
                <button
                  type="button"
                  onClick={() => {
                    const edit = editing;
                    setEditing(null);
                    run(() => updateBookmark(edit.id, { title: edit.title, url: edit.url }), "Save bookmark");
                  }}
                  className="rounded border border-border px-1.5 py-1 text-[10px] hover:bg-accent"
                >
                  Save
                </button>
                <button
                  type="button"
                  onClick={() => setEditing(null)}
                  className="rounded px-1.5 py-1 text-[10px] hover:bg-accent"
                >
                  Cancel
                </button>
              </li>
            );
          }
          return (
            <li key={mark.id} className="group flex items-center gap-1.5 rounded px-1.5 py-1 hover:bg-accent/60">
              <Globe className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
              <button
                type="button"
                onClick={() => onOpenUrl?.(mark.url)}
                className="min-w-0 flex-1 truncate text-left text-[11px]"
              >
                {mark.title}
                <span className="ml-1.5 font-mono text-[10px] text-muted-foreground">{mark.url}</span>
              </button>
              <button
                type="button"
                aria-label={`Edit ${mark.title}`}
                onClick={() => setEditing({ id: mark.id, title: mark.title, url: mark.url })}
                className="rounded p-1 hover:bg-accent"
              >
                <Pencil className="size-3" aria-hidden="true" />
              </button>
              <button
                type="button"
                aria-label={`Delete ${mark.title}`}
                onClick={() => run(() => deleteBookmark(mark.id), "Delete bookmark")}
                className="rounded p-1 hover:bg-accent"
              >
                <Trash2 className="size-3" aria-hidden="true" />
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function HistoryPanel({ onOpenUrl }: { onOpenUrl?: (url: string) => void }) {
  const [query, setQuery] = useState("");
  const [entries, setEntries] = useState<HostHistoryEntry[]>([]);
  // Unknown until the first answer arrives; "empty" and "not read yet" are different states.
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Overlapping reads resolve out of order; only the newest query may set the list.
  const requestSeqRef = useRef(0);

  useEffect(() => {
    const seq = ++requestSeqRef.current;
    void Promise.resolve()
      .then(() => listHostHistory(query, undefined, HISTORY_LIMIT))
      .then((page) => {
        if (seq !== requestSeqRef.current) return;
        setEntries(page ?? []);
        setLoaded(true);
        setError(null);
      })
      .catch((err: unknown) => {
        if (seq !== requestSeqRef.current) return;
        setLoaded(true);
        setError(`History failed: ${errorCode(err)}`);
      });
  }, [query]);

  const terms = useMemo(() => searchTerms(query), [query]);
  // The host already searches, but filtering here keeps one contract for every backend:
  // the list only ever shows rows that match what is typed.
  const rows = useMemo(
    () => entries.filter((entry) => matchesTerms(terms, `${entry.title ?? ""} ${entry.url}`)),
    [entries, terms],
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="maho-library-history">
      <SearchField label="Search history" placeholder="Search history" value={query} onChange={setQuery} />
      {error ? (
        <div role="alert" className="px-2 py-1 text-[11px] text-destructive">
          {error}
        </div>
      ) : null}
      <ul aria-label="History" className="maho-library-manager-list min-h-0 flex-1 overflow-y-auto">
        {loaded && rows.length === 0 ? (
          <li className="px-2 py-6 text-center text-[11px] text-muted-foreground">No browsing history.</li>
        ) : null}
        {rows.map((entry) => {
          const label = entry.title || entry.url;
          return (
            <li key={entry.url} className="flex items-center gap-2 rounded px-1.5 py-1 hover:bg-accent/60">
              <span className="w-[64px] shrink-0 font-mono text-[10px] text-muted-foreground">
                {new Date(entry.lastVisitedAt).toLocaleTimeString()}
              </span>
              <button
                type="button"
                aria-label={`History: ${label}`}
                onClick={() => onOpenUrl?.(entry.url)}
                className="min-w-0 flex-1 truncate text-left text-[11px]"
              >
                {label}
                <span className="ml-1.5 font-mono text-[10px] text-muted-foreground">{entry.url}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
