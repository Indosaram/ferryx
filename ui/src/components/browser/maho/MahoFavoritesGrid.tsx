// Count-dependent essentials layout from MahoSidebarFavoritesGridView::GetLayoutSpec.

import { useDroppable } from "@dnd-kit/core";
import { SortableContext, rectSortingStrategy, useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Globe, GripVertical, StarOff, WandSparkles, X } from "lucide-react";
import { useState, type CSSProperties } from "react";

import {
  browserStore,
  selectFavoriteTabs,
  useBrowserStore,
  type BrowserStore,
  type BrowserStoreTab,
} from "../../../state/browserStore";
import { activateMahoTab, displayUrl, mahoDragId, moveTabToToday } from "./mahoBrowserTabActions";
import { toast } from "sonner";
import { MAHO_LAYOUT_TOKENS } from "./mahoTokens";

export type MahoFavoritesGridProps = {
  store?: BrowserStore;
  activeBrowserId?: string | null;
  onActivateTab?: (browserId: string) => void;
};

const FAVORITES = MAHO_LAYOUT_TOKENS.favorites;
const FAVORITE_COLUMNS = [1, 1, 2, 3, 2, 4, 3, 4, 4, 3, 4, 4, 4] as const;

export function favoriteColumns(count: number): number {
  return FAVORITE_COLUMNS[Math.min(count, FAVORITES.capacity)];
}

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
        title={label}
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
  const [hintDismissed, setHintDismissed] = useState(false);
  const columns = favoriteColumns(favorites.length);
  const { setNodeRef, isOver } = useDroppable({ id: mahoDragId({ kind: "section", section: "favorite" }) });

  const activate = (browserId: string) => {
    void activateMahoTab(store, browserId, onActivateTab).catch(() => toast.error("Could not restore tab"));
  };

  return (
    <section
      ref={setNodeRef}
      className="maho-favorites"
      aria-label="Favorites"
      data-over={isOver ? "true" : undefined}
      data-columns={columns}
      data-capacity={FAVORITES.capacity}
      data-empty={favorites.length === 0 ? "true" : undefined}
      data-hint-dismissed={hintDismissed && !isOver ? "true" : undefined}
      style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}
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
      {favorites.length === 0 && (!hintDismissed || isOver) ? (
        <div className="maho-favorites__empty">
          <span className="maho-favorites__empty-badge" aria-hidden="true"><WandSparkles /></span>
          <button type="button" className="maho-favorites__dismiss" aria-label="Dismiss favorites hint" onClick={() => setHintDismissed(true)}><X aria-hidden="true" /></button>
          <strong>Drag to add Favorites</strong>
          <p>Favorites keep your most used sites<br />and apps close</p>
        </div>
      ) : null}
    </section>
  );
}
