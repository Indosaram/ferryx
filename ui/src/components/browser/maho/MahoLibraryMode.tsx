// Maho Library mode: the 92dp category rail plus the list pane beside it.
//
// Maho's `maho_sidebar_library_rail_view` replaces the sidebar body with a rail of
// four categories (Archive, Downloads, Bookmarks, History) and one list pane. The
// geometry comes from `MAHO_LAYOUT_TOKENS.library` (92dp rail, 80dp item, 24dp icon)
// and every color is a Ferryx semantic token, so the pane matches the app and the
// Space palette tints only the rail through `maho.css`.
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
  Bookmark,
  Download,
  FolderPlus,
  Globe,
  History as HistoryIcon,
  Pencil,
  RotateCcw,
  Search,
  Trash2,
  X,
  type LucideIcon,
} from "lucide-react";

import { BrowserDownloadRow } from "../../BrowserDownloadRow";
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
import { browserStore, useBrowserStore, type BrowserStore } from "../../../state/browserStore";
import { MAHO_LAYOUT_TOKENS } from "./mahoTokens";

const LIBRARY = MAHO_LAYOUT_TOKENS.library;
const HISTORY_LIMIT = 100;

export type MahoLibraryCategory = "archive" | "downloads" | "bookmarks" | "history";

/** The rail's four categories, in Maho's order. */
export const MAHO_LIBRARY_CATEGORIES: ReadonlyArray<{
  id: MahoLibraryCategory;
  label: string;
  icon: LucideIcon;
}> = [
  { id: "archive", label: "Archive", icon: Archive },
  { id: "downloads", label: "Downloads", icon: Download },
  { id: "bookmarks", label: "Bookmarks", icon: Bookmark },
  { id: "history", label: "History", icon: HistoryIcon },
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
}: MahoLibraryModeProps) {
  const [internalCategory, setInternalCategory] = useState<MahoLibraryCategory>(defaultCategory);
  const active = category ?? internalCategory;
  const activeLabel = MAHO_LIBRARY_CATEGORIES.find((candidate) => candidate.id === active)?.label ?? "Archive";

  const select = useCallback((next: MahoLibraryCategory) => {
    setInternalCategory(next);
    onCategoryChange?.(next);
  }, [onCategoryChange]);

  return (
    <LibraryFrame overlay={overlay}>
      <div className="flex h-full min-h-0 w-full">
        <nav
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
          {MAHO_LIBRARY_CATEGORIES.map(({ id, label, icon: Icon }) => (
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
              <span className="max-w-full truncate text-[10px] leading-none">{label}</span>
            </button>
          ))}
        </nav>
        <section
          role="tabpanel"
          aria-label={activeLabel}
          className="flex min-h-0 min-w-0 flex-1 flex-col bg-background text-foreground"
        >
          <div className="flex items-center gap-1 border-b border-border px-2.5 py-1.5">
            <h2 className="min-w-0 flex-1 truncate text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
              {activeLabel}
            </h2>
            {onClose ? (
              <button
                type="button"
                aria-label="Close library"
                onClick={onClose}
                className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
              >
                <X className="size-3.5" aria-hidden="true" />
              </button>
            ) : null}
          </div>
          {active === "archive" ? (
            <ArchivePanel store={store} onOpenTab={onOpenTab} />
          ) : active === "downloads" ? (
            <DownloadsPanel />
          ) : active === "bookmarks" ? (
            <BookmarksPanel onOpenUrl={onOpenUrl} />
          ) : (
            <HistoryPanel onOpenUrl={onOpenUrl} />
          )}
        </section>
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
      className="absolute inset-y-0 start-0 z-30 flex min-h-0 flex-col overflow-hidden rounded-e-xl border-e border-border bg-background text-foreground shadow-2xl"
      style={{ width: LIBRARY.railWidth + LIBRARY.overlayContentWidth }}
    >
      {children}
    </div>
  );
}

function SearchField({
  label,
  placeholder,
  value,
  onChange,
}: {
  label: string;
  placeholder: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="flex items-center gap-1.5 border-b border-border px-2 py-1.5">
      <Search className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
      <input
        aria-label={label}
        placeholder={placeholder}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="h-7 min-w-0 flex-1 rounded border border-border bg-background px-2 text-xs outline-none focus:border-ring"
      />
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
  const terms = useMemo(() => searchTerms(query), [query]);
  const rows = useMemo(
    () => archive.filter((entry) => matchesTerms(terms, `${entry.tab.title} ${entry.tab.url}`)),
    [archive, terms],
  );

  const restore = (browserId: string) => {
    const result = store.restoreFromArchive(browserId);
    if (result.ok) onOpenTab?.(browserId);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="maho-library-archive">
      <SearchField label="Search archive" placeholder="Search archive" value={query} onChange={setQuery} />
      <ul aria-label="Archived tabs" className="min-h-0 flex-1 overflow-y-auto px-1.5 py-1">
        {rows.length === 0 ? (
          <li className="px-2 py-6 text-center text-[11px] text-muted-foreground">
            {archive.length === 0 ? "Nothing archived yet." : "No archived tab matches that search."}
          </li>
        ) : null}
        {rows.map((entry) => {
          const label = entry.tab.title.trim() || entry.tab.url;
          return (
            <li key={entry.tab.browserId} className="flex items-center gap-2 rounded px-1.5 py-1 hover:bg-accent/60">
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[11px]">{label}</span>
                <span className="block truncate font-mono text-[10px] text-muted-foreground">
                  {entry.tab.url} · {entry.reason === "cleared" ? "Cleared" : "Closed"}
                </span>
              </span>
              <button
                type="button"
                aria-label={`Restore ${label} to Today`}
                title="Restore to Today"
                onClick={() => restore(entry.tab.browserId)}
                className="flex shrink-0 items-center gap-1 rounded border border-border px-1.5 py-0.5 text-[10px] hover:bg-accent"
              >
                <RotateCcw className="size-3" aria-hidden="true" />
                Restore
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function DownloadsPanel() {
  const { downloads, loadErrorCode, loading } = useBrowserDownloads();
  const { missingFileIds, actionErrors, handleCancel, handlePause, handleOpen, handleReveal } =
    useBrowserDownloadActions();
  const sorted = useMemo(() => [...downloads].sort((a, b) => b.createdAtMs - a.createdAtMs), [downloads]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto p-2" data-testid="maho-library-downloads">
      {loadErrorCode ? (
        <div className="rounded border border-destructive/40 bg-destructive/10 px-2 py-1 text-[11px] text-destructive">
          Could not read the download list <span className="font-mono text-[10px]">{loadErrorCode}</span>
        </div>
      ) : null}
      {sorted.length === 0 && !loading && !loadErrorCode ? (
        <div className="py-6 text-center text-[11px] text-muted-foreground">No downloads yet.</div>
      ) : null}
      <div role="list" aria-label="Downloads" className="flex flex-col gap-1.5">
        {sorted.map((record) => (
          <div role="listitem" key={record.id}>
            <BrowserDownloadRow
              record={record}
              missing={missingFileIds.has(record.id)}
              actionError={actionErrors[record.id]}
              onCancel={(id) => void handleCancel(id)}
              onPause={(id, pause) => void handlePause(id, pause)}
              onOpen={(item) => void handleOpen(item)}
              onReveal={(item) => void handleReveal(item)}
            />
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
      <ul aria-label="Bookmarks" className="min-h-0 flex-1 overflow-y-auto px-1.5 py-1">
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
      <ul aria-label="History" className="min-h-0 flex-1 overflow-y-auto px-1.5 py-1">
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
