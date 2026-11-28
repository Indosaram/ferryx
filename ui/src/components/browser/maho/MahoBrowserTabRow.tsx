// One Maho tab row, shared by Pinned, Folders and Today.
//
// dnd-kit's sortable id lives on the row so a whole row is the drop target, while
// the drag *activator* is the small grip handle: a row that is itself a button
// cannot legally contain the close/pin buttons. The grip is a real button, so a
// keyboard user can Tab to it, then Space to lift and arrow keys to move.

import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Globe, GripVertical, Minus, X } from "lucide-react";
import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { openNativePopupMenu } from "../../../lib/nativeMenu";

import { browserStore, type BrowserStore, type BrowserStoreTab } from "../../../state/browserStore";
import { activateMahoTab, addTabToFavorites, displayUrl, mahoDragId, moveTabToPinned, moveTabToToday, suspendMahoTab, worktreeTagLabel } from "./mahoBrowserTabActions";

export type MahoBrowserTabRowProps = {
  tab: BrowserStoreTab;
  store?: BrowserStore;
  active?: boolean;
  onActivate: (browserId: string) => void;
  onClose?: (browserId: string) => void;
  /** Prefixes the row's accessible name, e.g. "Pinned". */
  sectionLabel?: string;
};

export function MahoBrowserTabRow({
  tab,
  store = browserStore,
  active = false,
  onActivate,
  onClose,
  sectionLabel,
}: MahoBrowserTabRowProps) {
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, transform, transition, isDragging } = useSortable({
    id: mahoDragId({ kind: "tab", tabId: tab.browserId }),
  });
  const menuAbort = useRef<AbortController | null>(null);
  useEffect(() => () => menuAbort.current?.abort(), []);
  const unload = tab.section === "pinned" && !tab.suspended;
  const label = tab.title.trim() || displayUrl(tab.url);
  const worktree = tab.worktreeTag ? worktreeTagLabel(tab.worktreeTag) : null;

  return (
    <div
      ref={setNodeRef}
      className="maho-row maho-tab-row"
      data-selected={active ? "true" : undefined}
      data-dragging={isDragging ? "true" : undefined}
      data-suspended={tab.suspended ? "true" : undefined}
      onKeyDown={(event) => {
        if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
          event.preventDefault();
          const rect = event.currentTarget.getBoundingClientRect();
          event.currentTarget.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: rect.left, clientY: rect.bottom }));
        }
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        menuAbort.current?.abort();
        const controller = new AbortController();
        menuAbort.current = controller;
        void openNativePopupMenu("cmd_native_sidebar_context_menu", [
          { kind: "item", id: "pin", label: tab.section === "pinned" ? "Unpin to Today" : "Pin tab" },
          { kind: "item", id: "favorite", label: "Add to Favorites" },
          { kind: "separator" },
          { kind: "item", id: "close", label: "Close tab" },
        ], { x: event.clientX, y: event.clientY }, (id) => {
          if (id === "pin") {
            if (tab.section === "pinned") moveTabToToday(store, tab.browserId);
            else moveTabToPinned(store, tab.browserId);
          } else if (id === "favorite") addTabToFavorites(store, tab.browserId);
          else if (id === "close") onClose?.(tab.browserId);
        }, controller.signal).catch(() => toast.error("Could not open tab menu"));
      }}
      style={{ transform: CSS.Transform.toString(transform), transition }}
    >
      <button
        ref={setActivatorNodeRef}
        type="button"
        className="maho-tab-row__grip"
        aria-label={`Reorder ${label}`}
        {...attributes}
        {...listeners}
      >
        <GripVertical aria-hidden="true" />
      </button>
      <button
        type="button"
        className="maho-tab-row__main"
        aria-label={`${sectionLabel ? `${sectionLabel}: ` : ""}${label}`}
        aria-current={active ? "page" : undefined}
        title={displayUrl(tab.url)}
        data-browser-id={tab.browserId}
        onClick={() => { void activateMahoTab(store, tab.browserId, onActivate).catch(() => toast.error("Could not restore tab")); }}
      >
        <span className="maho-row__icon maho-tab-row__icon">
          {tab.faviconUrl ? (
            <img src={tab.faviconUrl} alt="" width={17} height={17} />
          ) : (
            <Globe aria-hidden="true" />
          )}
        </span>
        <span className="maho-tab-row__title">{label}</span>
        {worktree ? (
          <span className="maho-worktree-tag" title={`Worktree ${tab.worktreeTag?.worktreePath ?? ""}`}>
            {worktree}
          </span>
        ) : null}
        {tab.agentControlled ? <span className="maho-agent-badge" title="Agent controlled" aria-hidden="true" /> : null}
      </button>
      <span className="maho-tab-row__actions">
        {onClose ? (
          <button
            type="button"
            className="maho-row__action"
            aria-label={`${unload ? "Unload" : "Close"} ${label}`}
            title={unload ? "Unload tab" : "Close tab"}
            onClick={(event) => {
              event.stopPropagation();
              if (unload) void suspendMahoTab(store, tab.browserId).catch(() => toast.error("Could not unload tab"));
              else onClose(tab.browserId);
            }}
          >
            {unload ? <Minus aria-hidden="true" /> : <X aria-hidden="true" />}
          </button>
        ) : null}
      </span>
    </div>
  );
}
