// Maho Today: the vertical, per-Space tab list.
//
// Today is the only section that is not shared across Spaces, so the list is
// keyed on the active Space and its rows carry the worktree tag (a browser tab
// opened from a worktree split) plus hover close and a pin action.

import { useDroppable } from "@dnd-kit/core";
import { SortableContext, verticalListSortingStrategy } from "@dnd-kit/sortable";

import {
  browserStore,
  selectActiveBrowserId,
  selectTodayTabs,
  useBrowserStore,
  type BrowserStore,
} from "../../../state/browserStore";
import { MahoBrowserTabRow } from "./MahoBrowserTabRow";
import { mahoDragId } from "./mahoBrowserTabActions";

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
        null
      ) : (
        <SortableContext
          items={tabs.map((tab) => mahoDragId({ kind: "tab", tabId: tab.browserId }))}
          strategy={verticalListSortingStrategy}
        >
          {tabs.map((tab) => (
            <MahoBrowserTabRow
              key={tab.browserId}
              store={store}
              tab={tab}
              sectionLabel="Today"
              active={tab.browserId === selectedBrowserId}
              onActivate={activate}
              onClose={(browserId) => store.closeTab(browserId)}

            />
          ))}
        </SortableContext>
      )}
    </section>
  );
}
