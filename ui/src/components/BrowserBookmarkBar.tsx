import { useState } from "react";
import { ChevronDown, Folder, Globe } from "lucide-react";

import { BOOKMARK_BAR_ROOT, bookmarkChildren, type BookmarkTree } from "../lib/browserLibrary";
import { isMacHost } from "../lib/platform";

interface BrowserBookmarkBarProps {
  tree: BookmarkTree;
  onOpen: (url: string) => void;
}

/** Bookmarks bar under the toolbar: bar-root bookmarks as buttons, folders as drop-down menus. */
export function BrowserBookmarkBar({ tree, onOpen }: BrowserBookmarkBarProps) {
  const [openFolderId, setOpenFolderId] = useState<string | null>(null);
  const items = bookmarkChildren(tree, BOOKMARK_BAR_ROOT);
  return (
    <div
      role="toolbar"
      aria-label="Bookmarks bar"
      className="flex min-h-6 items-center gap-0.5 overflow-x-auto border-b border-border bg-card px-2 py-0.5 text-[11px]"
    >
      {items.length === 0 ? (
        <span className="px-1 text-muted-foreground">
          Bookmark pages with the star or {isMacHost() ? "Cmd+D" : "Ctrl+D"} to see them here.
        </span>
      ) : null}
      {items.map((entry) =>
        entry.kind === "bookmark" ? (
          <button
            key={entry.item.id}
            type="button"
            title={entry.item.url}
            onClick={() => onOpen(entry.item.url)}
            className="flex max-w-40 shrink-0 items-center gap-1 rounded px-1.5 py-0.5 hover:bg-muted"
          >
            <Globe className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
            <span className="truncate">{entry.item.title}</span>
          </button>
        ) : (
          <div key={entry.item.id} className="relative shrink-0">
            <button
              type="button"
              aria-haspopup="menu"
              aria-expanded={openFolderId === entry.item.id}
              onClick={() => setOpenFolderId((current) => (current === entry.item.id ? null : entry.item.id))}
              className="flex max-w-40 items-center gap-1 rounded px-1.5 py-0.5 hover:bg-muted"
            >
              <Folder className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
              <span className="truncate">{entry.item.name}</span>
              <ChevronDown className="size-3 shrink-0" aria-hidden="true" />
            </button>
          </div>
        ),
      )}
      {openFolderId ? (
        // Rendered in flow (not as a floating popover) so the pane reserves its height and the
        // native page view, which DOM cannot cover, moves below it.
        <BookmarkFolderMenu
          tree={tree}
          folderId={openFolderId}
          onOpen={(url) => {
            setOpenFolderId(null);
            onOpen(url);
          }}
          onClose={() => setOpenFolderId(null)}
        />
      ) : null}
    </div>
  );
}

function BookmarkFolderMenu({
  tree,
  folderId,
  onOpen,
  onClose,
}: {
  tree: BookmarkTree;
  folderId: string;
  onOpen: (url: string) => void;
  onClose: () => void;
}) {
  const [path, setPath] = useState<string[]>([folderId]);
  const current = path[path.length - 1];
  const items = bookmarkChildren(tree, current);
  const name = tree.folders.find((folder) => folder.id === current)?.name ?? "";
  return (
    <div role="menu" aria-label={`Folder ${name}`} className="basis-full flex flex-wrap items-center gap-0.5 border-t border-border/70 pt-0.5">
      {path.length > 1 ? (
        <button type="button" role="menuitem" onClick={() => setPath((p) => p.slice(0, -1))} className="rounded px-1.5 py-0.5 hover:bg-muted">..</button>
      ) : null}
      {items.length === 0 ? <span className="px-1 text-muted-foreground">Empty folder</span> : null}
      {items.map((entry) =>
        entry.kind === "bookmark" ? (
          <button key={entry.item.id} type="button" role="menuitem" title={entry.item.url} onClick={() => onOpen(entry.item.url)} className="flex max-w-48 items-center gap-1 rounded px-1.5 py-0.5 hover:bg-muted">
            <Globe className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
            <span className="truncate">{entry.item.title}</span>
          </button>
        ) : (
          <button key={entry.item.id} type="button" role="menuitem" onClick={() => setPath((p) => [...p, entry.item.id])} className="flex max-w-48 items-center gap-1 rounded px-1.5 py-0.5 hover:bg-muted">
            <Folder className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
            <span className="truncate">{entry.item.name}</span>
          </button>
        ),
      )}
      <button type="button" aria-label="Close folder" onClick={onClose} className="ml-auto rounded px-1.5 py-0.5 text-muted-foreground hover:bg-muted">Close</button>
    </div>
  );
}
