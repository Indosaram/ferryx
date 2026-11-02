import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Folder, FolderPlus, Globe, Pencil, Trash2, X } from "lucide-react";

import {
  BOOKMARK_BAR_ROOT,
  BOOKMARK_OTHER_ROOT,
  bookmarkChildren,
  clearHostHistory,
  createBookmarkFolder,
  deleteBookmark,
  deleteHostHistoryUrl,
  errorCode,
  listHostHistory,
  moveBookmark,
  onBrowserLibraryChanged,
  updateBookmark,
  useBookmarks,
  type BookmarkTree,
  type HostHistoryEntry,
} from "../lib/browserLibrary";

export type BrowserLibraryView = "bookmarks" | "history";

interface BrowserLibraryPanelProps {
  view: BrowserLibraryView;
  onViewChange: (view: BrowserLibraryView) => void;
  onOpen: (url: string) => void;
  onClose: () => void;
  /** Injectable clock for the clear-range presets. */
  now?: () => number;
}

const HOUR = 3_600_000;
const DAY = 24 * HOUR;
const HISTORY_PAGE = 100;

/** Bookmarks manager and full history page, shown in place of the page inside a browser pane. */
export function BrowserLibraryPanel({ view, onViewChange, onOpen, onClose, now = Date.now }: BrowserLibraryPanelProps) {
  return (
    <div className="absolute inset-0 z-20 flex flex-col bg-background text-xs" data-testid="browser-library">
      <div className="flex items-center gap-1 border-b border-border px-3 py-1.5">
        <div role="tablist" aria-label="Library" className="flex gap-1">
          {(["bookmarks", "history"] as const).map((candidate) => (
            <button
              key={candidate}
              type="button"
              role="tab"
              aria-selected={view === candidate}
              onClick={() => onViewChange(candidate)}
              className={`rounded px-2 py-1 capitalize ${view === candidate ? "bg-accent" : "hover:bg-accent/60"}`}
            >
              {candidate}
            </button>
          ))}
        </div>
        <button type="button" aria-label="Close library" onClick={onClose} className="ml-auto rounded p-1 hover:bg-accent">
          <X className="size-3.5" />
        </button>
      </div>
      {view === "bookmarks" ? <BookmarksManager onOpen={onOpen} /> : <HistoryPage onOpen={onOpen} now={now} />}
    </div>
  );
}

function folderOptions(tree: BookmarkTree): Array<{ id: string; label: string }> {
  const out: Array<{ id: string; label: string }> = [];
  const walk = (parentId: string, depth: number) => {
    for (const folder of tree.folders.filter((f) => f.parentId === parentId).sort((a, b) => a.position - b.position)) {
      out.push({ id: folder.id, label: `${"  ".repeat(depth)}${folder.name}` });
      walk(folder.id, depth + 1);
    }
  };
  out.push({ id: BOOKMARK_BAR_ROOT, label: "Bookmarks bar" });
  walk(BOOKMARK_BAR_ROOT, 1);
  out.push({ id: BOOKMARK_OTHER_ROOT, label: "Other bookmarks" });
  walk(BOOKMARK_OTHER_ROOT, 1);
  return out;
}

function BookmarksManager({ onOpen }: { onOpen: (url: string) => void }) {
  const { tree, error: loadError } = useBookmarks();
  const [folderId, setFolderId] = useState(BOOKMARK_BAR_ROOT);
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<{ id: string; title: string; url: string | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const folders = useMemo(() => folderOptions(tree), [tree]);

  const run = (action: () => Promise<unknown>, label: string) => {
    void action().then(() => setError(null)).catch((err: unknown) => setError(`${label} failed: ${errorCode(err)}`));
  };

  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const rows = terms.length > 0
    ? tree.bookmarks
      .filter((b) => terms.every((term) => `${b.title} ${b.url}`.toLowerCase().includes(term)))
      .map((item) => ({ kind: "bookmark" as const, item }))
    : bookmarkChildren(tree, folderId);

  const breadcrumb: Array<{ id: string; name: string }> = [];
  let cursor: string | undefined = folderId;
  while (cursor) {
    const folder = tree.folders.find((f) => f.id === cursor);
    if (!folder) {
      breadcrumb.unshift({ id: cursor, name: cursor === BOOKMARK_OTHER_ROOT ? "Other bookmarks" : "Bookmarks bar" });
      break;
    }
    breadcrumb.unshift({ id: folder.id, name: folder.name });
    cursor = folder.parentId;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-3 py-1.5">
        <input
          aria-label="Search bookmarks"
          placeholder="Search bookmarks"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          className="h-7 min-w-40 flex-1 rounded border border-border bg-background px-2 outline-none focus:border-ring"
        />
        <select aria-label="Folder" value={folderId} onChange={(event) => setFolderId(event.target.value)} className="h-7 rounded border border-border bg-background px-1">
          {folders.map((folder) => <option key={folder.id} value={folder.id}>{folder.label}</option>)}
        </select>
        <button
          type="button"
          onClick={() => {
            const name = window.prompt("Folder name");
            if (name?.trim()) run(() => createBookmarkFolder(name.trim(), folderId), "New folder");
          }}
          className="flex items-center gap-1 rounded border border-border px-2 py-1 hover:bg-accent"
        >
          <FolderPlus className="size-3.5" /> New folder
        </button>
      </div>
      {!terms.length ? (
        <nav aria-label="Folder path" className="flex gap-1 px-3 py-1 text-[11px] text-muted-foreground">
          {breadcrumb.map((crumb, index) => (
            <button key={crumb.id} type="button" onClick={() => setFolderId(crumb.id)} className="hover:text-foreground">
              {index > 0 ? "/ " : ""}{crumb.name}
            </button>
          ))}
        </nav>
      ) : null}
      {error || loadError ? <div role="alert" className="px-3 py-1 text-[11px] text-destructive">{error ?? `Bookmarks: ${loadError}`}</div> : null}
      <ul aria-label="Bookmarks" className="min-h-0 flex-1 overflow-y-auto px-2 py-1">
        {rows.length === 0 ? <li className="px-2 py-4 text-center text-muted-foreground">No bookmarks here.</li> : null}
        {rows.map((row) => {
          const isFolder = row.kind === "folder";
          const id = row.item.id;
          const title = isFolder ? row.item.name : row.item.title;
          const url = row.kind === "bookmark" ? row.item.url : null;
          if (editing?.id === id) {
            return (
              <li key={id} className="flex items-center gap-1 rounded px-2 py-1">
                <input aria-label="Bookmark name" value={editing.title} onChange={(event) => setEditing({ ...editing, title: event.target.value })} className="h-7 flex-1 rounded border border-border bg-background px-2" />
                {editing.url !== null ? (
                  <input aria-label="Bookmark URL" value={editing.url} onChange={(event) => setEditing({ ...editing, url: event.target.value })} className="h-7 flex-1 rounded border border-border bg-background px-2 font-mono" />
                ) : null}
                <button type="button" onClick={() => { const edit = editing; setEditing(null); run(() => updateBookmark(edit.id, { title: edit.title, url: edit.url ?? undefined }), "Save"); }} className="rounded border border-border px-2 py-1 hover:bg-accent">Save</button>
                <button type="button" onClick={() => setEditing(null)} className="rounded px-2 py-1 hover:bg-accent">Cancel</button>
              </li>
            );
          }
          return (
            <li key={id} className="group flex items-center gap-2 rounded px-2 py-1 hover:bg-accent/60">
              {isFolder ? <Folder className="size-3.5 shrink-0 text-muted-foreground" /> : <Globe className="size-3.5 shrink-0 text-muted-foreground" />}
              <button type="button" onClick={() => (isFolder ? setFolderId(id) : url && onOpen(url))} className="min-w-0 flex-1 truncate text-left">
                {title}
                {url ? <span className="ml-2 font-mono text-[10px] text-muted-foreground">{url}</span> : null}
              </button>
              <select
                aria-label={`Move ${title}`}
                value=""
                onChange={(event) => {
                  const target = event.target.value;
                  if (target) run(() => moveBookmark(id, target), "Move");
                }}
                className="h-6 w-24 rounded border border-border bg-background text-[10px]"
              >
                <option value="">Move to…</option>
                {folders.filter((folder) => folder.id !== id).map((folder) => <option key={folder.id} value={folder.id}>{folder.label}</option>)}
              </select>
              <button type="button" aria-label={`Edit ${title}`} onClick={() => setEditing({ id, title, url })} className="rounded p-1 hover:bg-accent"><Pencil className="size-3" /></button>
              <button type="button" aria-label={`Delete ${title}`} onClick={() => run(() => deleteBookmark(id), "Delete")} className="rounded p-1 hover:bg-accent"><Trash2 className="size-3" /></button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

const CLEAR_RANGES: Array<{ id: string; label: string; span: number | null }> = [
  { id: "hour", label: "Last hour", span: HOUR },
  { id: "day", label: "Last 24 hours", span: DAY },
  { id: "week", label: "Last 7 days", span: 7 * DAY },
  { id: "all", label: "All time", span: null },
];

function HistoryPage({ onOpen, now }: { onOpen: (url: string) => void; now: () => number }) {
  const [query, setQuery] = useState("");
  const [entries, setEntries] = useState<HostHistoryEntry[]>([]);
  // Until the first answer for the current query arrives the list is unknown, not empty.
  const [loaded, setLoaded] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [range, setRange] = useState("hour");
  const [error, setError] = useState<string | null>(null);
  // Loads overlap (mount, a history change, search, paging) and their answers can arrive out of
  // order; only the latest request may set the list, so an older (empty) snapshot never
  // overwrites a newer one.
  const requestSeqRef = useRef(0);

  const load = useCallback((before?: number) => {
    const seq = ++requestSeqRef.current;
    void Promise.resolve()
      .then(() => listHostHistory(query, before, HISTORY_PAGE))
      .then((page) => {
        if (seq !== requestSeqRef.current) return;
        const rows = page ?? [];
        setEntries((current) => (before === undefined ? rows : [...current, ...rows]));
        setHasMore(rows.length === HISTORY_PAGE);
        setLoaded(true);
        setError(null);
      })
      .catch((err: unknown) => {
        if (seq !== requestSeqRef.current) return;
        setLoaded(true);
        setError(`History failed: ${errorCode(err)}`);
      });
  }, [query]);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void Promise.resolve()
      .then(() => onBrowserLibraryChanged((kind) => { if (kind === "history") load(); }))
      .then((cleanup) => {
        if (disposed) {
          cleanup?.();
          return;
        }
        unlisten = cleanup;
        // A visit recorded between the first query and this subscription sent its change
        // event to nobody; query again now that changes are heard.
        load();
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [load]);

  const clearRange = () => {
    const preset = CLEAR_RANGES.find((candidate) => candidate.id === range);
    if (!preset) return;
    const end = now();
    const task = preset.span === null ? clearHostHistory() : clearHostHistory(end - preset.span, end + 1);
    void task.then(() => load()).catch((err: unknown) => setError(`Clear failed: ${errorCode(err)}`));
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-3 py-1.5">
        <input
          aria-label="Search history"
          placeholder="Search history"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          className="h-7 min-w-40 flex-1 rounded border border-border bg-background px-2 outline-none focus:border-ring"
        />
        <select aria-label="Clear range" value={range} onChange={(event) => setRange(event.target.value)} className="h-7 rounded border border-border bg-background px-1">
          {CLEAR_RANGES.map((candidate) => <option key={candidate.id} value={candidate.id}>{candidate.label}</option>)}
        </select>
        <button type="button" onClick={clearRange} className="rounded border border-border px-2 py-1 hover:bg-accent">Clear browsing data</button>
      </div>
      {error ? <div role="alert" className="px-3 py-1 text-[11px] text-destructive">{error}</div> : null}
      <ul aria-label="History" className="min-h-0 flex-1 overflow-y-auto px-2 py-1">
        {loaded && entries.length === 0 ? <li className="px-2 py-4 text-center text-muted-foreground">No browsing history.</li> : null}
        {entries.map((entry) => (
          <li key={entry.url} className="flex items-center gap-2 rounded px-2 py-1 hover:bg-accent/60">
            <span className="w-28 shrink-0 font-mono text-[10px] text-muted-foreground">{new Date(entry.lastVisitedAt).toLocaleString()}</span>
            <button type="button" onClick={() => onOpen(entry.url)} className="min-w-0 flex-1 truncate text-left">
              {entry.title || entry.url}
              <span className="ml-2 font-mono text-[10px] text-muted-foreground">{entry.url}</span>
            </button>
            <button
              type="button"
              aria-label={`Remove ${entry.title || entry.url} from history`}
              onClick={() => void deleteHostHistoryUrl(entry.url).then(() => load()).catch((err: unknown) => setError(`Remove failed: ${errorCode(err)}`))}
              className="rounded p-1 hover:bg-accent"
            >
              <Trash2 className="size-3" />
            </button>
          </li>
        ))}
        {hasMore ? (
          <li className="py-2 text-center">
            <button type="button" onClick={() => load(entries[entries.length - 1]?.lastVisitedAt)} className="rounded border border-border px-2 py-1 hover:bg-accent">Load more</button>
          </li>
        ) : null}
      </ul>
    </div>
  );
}
