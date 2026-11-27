// One Maho tab row, shared by Pinned, Folders and Today.
//
// dnd-kit's sortable id lives on the row so a whole row is the drop target, while
// the drag *activator* is the small grip handle: a row that is itself a button
// cannot legally contain the close/pin buttons. The grip is a real button, so a
// keyboard user can Tab to it, then Space to lift and arrow keys to move.

import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Globe, GripVertical, X } from "lucide-react";
import type { ReactNode } from "react";

import type { BrowserStoreTab } from "../../../state/browserStore";
import { displayUrl, mahoDragId, worktreeTagLabel } from "./mahoBrowserTabActions";

export type MahoBrowserTabRowProps = {
  tab: BrowserStoreTab;
  active?: boolean;
  onActivate: (browserId: string) => void;
  onClose?: (browserId: string) => void;
  /** Trailing buttons (pin, unpin, favorite, ...). */
  actions?: ReactNode;
  /** Prefixes the row's accessible name, e.g. "Pinned". */
  sectionLabel?: string;
};

export function MahoBrowserTabRow({
  tab,
  active = false,
  onActivate,
  onClose,
  actions,
  sectionLabel,
}: MahoBrowserTabRowProps) {
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, transform, transition, isDragging } = useSortable({
    id: mahoDragId({ kind: "tab", tabId: tab.browserId }),
  });
  const label = tab.title.trim() || displayUrl(tab.url);
  const worktree = tab.worktreeTag ? worktreeTagLabel(tab.worktreeTag) : null;

  return (
    <div
      ref={setNodeRef}
      className="maho-row maho-tab-row"
      data-selected={active ? "true" : undefined}
      data-dragging={isDragging ? "true" : undefined}
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
        onClick={() => onActivate(tab.browserId)}
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
        {actions}
        {onClose ? (
          <button
            type="button"
            className="maho-row__action"
            aria-label={`Close ${label}`}
            title="Close tab"
            onClick={(event) => {
              event.stopPropagation();
              onClose(tab.browserId);
            }}
          >
            <X aria-hidden="true" />
          </button>
        ) : null}
      </span>
    </div>
  );
}
