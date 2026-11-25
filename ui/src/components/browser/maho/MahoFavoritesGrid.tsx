// Maho Favorites: one shared 4x3 grid for every Space.
//
// Capacity and columns come from `MAHO_LAYOUT_TOKENS`, so the DOM grid matches the
// stylesheet instead of trusting a hand-written `repeat(4, ...)`. Empty slots are
// rendered (decoratively, never focusable) so the grid keeps its twelve cells and
// the "full" state is visible rather than implied.
//
// Tiles are sortable through dnd-kit and the drop decision itself lives in
// `applyMahoDrop`, so a keyboard drag and a pointer drag take the same path as the
// click handlers do.

import { useDroppable } from "@dnd-kit/core";
import { SortableContext, rectSortingStrategy, useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Globe, GripVertical, StarOff } from "lucide-react";
import type { CSSProperties } from "react";

import {
  browserStore,
  selectFavoriteTabs,
  useBrowserStore,
  type BrowserStore,
  type BrowserStoreTab,
} from "../../../state/browserStore";
import { displayUrl, mahoDragId, moveTabToToday } from "./mahoBrowserTabActions";
import { MAHO_LAYOUT_TOKENS } from "./mahoTokens";

export type MahoFavoritesGridProps = {
  store?: BrowserStore;
  activeBrowserId?: string | null;
  onActivateTab?: (browserId: string) => void;
};

const FAVORITES = MAHO_LAYOUT_TOKENS.favorites;
const EMPTY_SLOTS = Array.from({ length: FAVORITES.capacity }, (_, index) => index);

function favoriteTileStyle(): CSSProperties {
  return {
    height: `${FAVORITES.tileHeight}px`,
    borderRadius: `${FAVORITES.tileCornerRadius}px`,
  };
}

function FavoriteTile({
  tab,
  active,
  onActivate,
  onUnpin,
}: {
  tab: BrowserStoreTab;
  active: boolean;
  onActivate: (browserId: string) => void;
  onUnpin: (browserId: string) => void;
}) {
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, transform, transition, isDragging } = useSortable({
    id: mahoDragId({ kind: "tab", tabId: tab.browserId }),
  });
  const label = tab.title.trim() || displayUrl(tab.url);
  return (
    <div
      ref={setNodeRef}
      className="maho-favorite-tile"
      data-selected={active ? "true" : undefined}
      data-dragging={isDragging ? "true" : undefined}
      style={{
        ...favoriteTileStyle(),
        transform: CSS.Transform.toString(transform),
        transition,
      }}
    >
      <button
        ref={setActivatorNodeRef}
        type="button"
        className="maho-favorite-tile__grip"
        aria-label={`Reorder ${label}`}
        {...attributes}
        {...listeners}
      >
        <GripVertical aria-hidden="true" />
      </button>
      <button
        type="button"
        className="maho-favorite-tile__action"
        aria-label={`Open ${label}`}
        title={displayUrl(tab.url)}
        data-browser-id={tab.browserId}
        onClick={() => onActivate(tab.browserId)}
      >
        <span className="maho-favorite-tile__glyph">
          {tab.faviconUrl ? (
            <img src={tab.faviconUrl} alt="" width={FAVORITES.glyphSize} height={FAVORITES.glyphSize} />
          ) : (
            <Globe aria-hidden="true" />
          )}
        </span>
        <span className="maho-favorite-tile__label">{label}</span>
      </button>
      <button
        type="button"
        className="maho-favorite-tile__unpin"
        aria-label={`Unpin ${label} to Today`}
        title="Unpin to Today"
        onClick={() => onUnpin(tab.browserId)}
      >
        <StarOff aria-hidden="true" />
      </button>
    </div>
  );
}

export function MahoFavoritesGrid({ store = browserStore, activeBrowserId = null, onActivateTab }: MahoFavoritesGridProps) {
  const favorites = useBrowserStore(selectFavoriteTabs, store);
  const { setNodeRef, isOver } = useDroppable({ id: mahoDragId({ kind: "section", section: "favorite" }) });
  const emptySlots = EMPTY_SLOTS.slice(favorites.length);

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
      className="maho-favorites"
      aria-label="Favorites"
      data-over={isOver ? "true" : undefined}
      data-columns={FAVORITES.columns}
      data-capacity={FAVORITES.capacity}
      style={{ gridTemplateColumns: `repeat(${FAVORITES.columns}, minmax(0, 1fr))` }}
    >
      <SortableContext
        items={favorites.map((tab) => mahoDragId({ kind: "tab", tabId: tab.browserId }))}
        strategy={rectSortingStrategy}
      >
        {favorites.map((tab) => (
          <FavoriteTile
            key={tab.browserId}
            tab={tab}
            active={tab.browserId === activeBrowserId}
            onActivate={activate}
            onUnpin={(browserId) => moveTabToToday(store, browserId)}
          />
        ))}
      </SortableContext>
      {emptySlots.map((slot) => (
        <div key={`empty-${slot}`} className="maho-favorite-tile" data-empty="true" aria-hidden="true" style={favoriteTileStyle()} />
      ))}
    </section>
  );
}
