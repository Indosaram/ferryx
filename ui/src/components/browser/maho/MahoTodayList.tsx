// Maho Today: the vertical, per-Space tab list.
//
// Today is the only section that is not shared across Spaces, so the list is
// keyed on the active Space and its rows carry the worktree tag (a browser tab
// opened from a worktree split) plus hover close and a pin action.

import { useDroppable } from "@dnd-kit/core";
import { SortableContext, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { Pin, Star } from "lucide-react";

import {
  browserStore,
  selectActiveBrowserId,
  selectTodayTabs,
  useBrowserStore,
  type BrowserStore,
} from "../../../state/browserStore";
import { MahoBrowserTabRow } from "./MahoBrowserTabRow";
import { mahoDragId, addTabToFavorites, moveTabToPinned } from "./mahoBrowserTabActions";

export type MahoTodayListProps = {
  store?: BrowserStore;
  spaceId?: string;
  activeBrowserId?: string | null;
  onActivateTab?: (browserId: string) => void;
};

export function MahoTodayList({ store = browserStore, spaceId, activeBrowserId, onActivateTab }: MahoTodayListProps) {
  const activeSpaceId = useBrowserStore((state) => state.activeSpaceId, store);
  const targetSpaceId = spaceId ?? activeSpaceId;
  const tabs = useBrowserStore((state) => selectTodayTabs(state, targetSpaceId), store);
  const storeActiveBrowserId = useBrowserStore(
    (state) => selectActiveBrowserId(state, targetSpaceId),
    store,
  );
  const selectedBrowserId = activeBrowserId ?? storeActiveBrowserId;
  const { setNodeRef, isOver } = useDroppable({ id: mahoDragId({ kind: "section", section: "today" }) });

  const activate = (browserId: string) => {
    if (onActivateTab) {
      onActivateTab(browserId);
      return;
    }
    store.touch(browserId);
  };

  return (
    <section
      ref={setNodeRef}
      className="maho-today"
      aria-label="Today"
      data-over={isOver ? "true" : undefined}
      data-empty={tabs.length === 0 ? "true" : undefined}
    >
      {tabs.length === 0 ? (
        <p className="maho-today__empty">No tabs in Today</p>
      ) : (
        <SortableContext
          items={tabs.map((tab) => mahoDragId({ kind: "tab", tabId: tab.browserId }))}
          strategy={verticalListSortingStrategy}
        >
          {tabs.map((tab) => (
            <MahoBrowserTabRow
              key={tab.browserId}
              tab={tab}
              sectionLabel="Today"
              active={tab.browserId === selectedBrowserId}
              onActivate={activate}
              onClose={(browserId) => store.closeTab(browserId)}
              actions={
                <>
                  <button
                    type="button"
                    className="maho-row__action maho-tab-row__pin"
                    aria-label={`Pin ${tab.title.trim() || tab.url}`}
                    title="Pin to Pinned"
                    onClick={(event) => {
                      event.stopPropagation();
                      moveTabToPinned(store, tab.browserId);
                    }}
                  >
                    <Pin aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    className="maho-row__action maho-tab-row__favorite"
                    aria-label={`Add ${tab.title.trim() || tab.url} to Favorites`}
                    title="Add to Favorites"
                    onClick={(event) => {
                      event.stopPropagation();
                      addTabToFavorites(store, tab.browserId);
                    }}
                  >
                    <Star aria-hidden="true" />
                  </button>
                </>
              }
            />
          ))}
        </SortableContext>
      )}
    </section>
  );
}
