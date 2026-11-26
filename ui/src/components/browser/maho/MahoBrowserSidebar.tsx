// The Maho browser sidebar: Maho's Arc-style rail, composed from its sections.
//
// One `[data-maho-space]` container carries the Space palette (`spacePaletteStyle`)
// and `maho.css`; everything inside reads only `--maho-*` custom properties, so a
// Space retints the whole rail and nothing leaks into the rest of the app.
//
// Sections that belong to other todos arrive as slots (`agentSection` for Todo 14,
// `nowPlayingSlot` for Todo 13, `libraryContent` for Todo 12), which means those
// todos mount their work here without editing this file. The one dnd-kit
// `DndContext` lives here because a tab must be draggable between Favorites,
// Pinned, a Folder and Today in a single drag session.

import { DndContext, PointerSensor, KeyboardSensor, closestCenter, useSensor, useSensors, type DragEndEvent } from "@dnd-kit/core";
import { sortableKeyboardCoordinates } from "@dnd-kit/sortable";
import { useCallback, useMemo, type CSSProperties, type ReactNode } from "react";

import {
  browserStore,
  selectActiveBrowserId,
  selectSpaceList,
  useBrowserStore,
  type BrowserStore,
} from "../../../state/browserStore";
import { MahoActionRow } from "./MahoActionRow";
import { MahoFavoritesGrid } from "./MahoFavoritesGrid";
import { MahoPinnedSection } from "./MahoPinnedSection";
import { MahoSidebarFooter } from "./MahoSidebarFooter";
import { MahoSidebarTopBar } from "./MahoSidebarTopBar";
import { MahoTodayList } from "./MahoTodayList";
import { MahoUrlPill } from "./MahoUrlPill";
import { applyMahoDrop, parseMahoDragId } from "./mahoBrowserTabActions";
import { spacePaletteStyle, type MahoScheme } from "./mahoSpacePalette";
import "./maho.css";

export type MahoBrowserSidebarProps = {
  store?: BrowserStore;
  /** Space to render; defaults to the store's active Space. */
  spaceId?: string;
  scheme?: MahoScheme;
  collapsed?: boolean;
  onToggleSidebar?: () => void;
  /** Active Space's tab, if the caller already knows it (and keeps it in sync). */
  activeBrowserId?: string | null;
  canGoBack?: boolean;
  canGoForward?: boolean;
  onBack?: () => void;
  onForward?: () => void;
  onReload?: () => void;
  onActivateTab?: (browserId: string) => void;
  onOpenCommandBar?: () => void;
  onNewTab?: () => void;
  onTidy?: () => void;
  onCopyUrl?: (url: string) => void;
  /** Todo 14: agent tasks / control activity chip. */
  controlActivity?: ReactNode;
  agentSection?: ReactNode;
  /** Todo 13: Now Playing card. */
  nowPlayingSlot?: ReactNode;
  /** Todo 12: Library rail + list, shown instead of the tab body when open. */
  libraryContent?: ReactNode;
  libraryOpen?: boolean;
  onToggleLibrary?: () => void;
};

export function MahoBrowserSidebar({
  store = browserStore,
  spaceId,
  scheme = "dark",
  collapsed = false,
  onToggleSidebar,
  activeBrowserId,
  canGoBack = false,
  canGoForward = false,
  onBack,
  onForward,
  onReload,
  onActivateTab,
  onOpenCommandBar,
  onNewTab,
  onTidy,
  onCopyUrl,
  controlActivity,
  agentSection,
  nowPlayingSlot,
  libraryContent,
  libraryOpen = false,
  onToggleLibrary,
}: MahoBrowserSidebarProps) {
  const state = useBrowserStore((snapshot) => snapshot, store);
  const spaces = useMemo(() => selectSpaceList(state), [state]);
  const space = useMemo(
    () => spaces.find((candidate) => candidate.id === (spaceId ?? state.activeSpaceId)) ?? spaces[0],
    [spaceId, spaces, state.activeSpaceId],
  );
  const selectedBrowserId = activeBrowserId ?? selectActiveBrowserId(state, space?.id);
  const activeTab = selectedBrowserId ? state.tabs[selectedBrowserId] ?? null : null;

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      const from = parseMahoDragId(String(active.id));
      const to = parseMahoDragId(String(over.id));
      if (!from || !to) return;
      applyMahoDrop(store, from, to);
    },
    [store],
  );

  if (!space) return null;

  const paletteStyle = spacePaletteStyle(space.color, scheme);

  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
      <aside
        className="maho-sidebar"
        aria-label={`Browser sidebar, ${space.name} Space`}
        data-collapsed={collapsed ? "true" : undefined}
        data-maho-space={space.id}
        data-maho-scheme={scheme}
        style={paletteStyle as unknown as CSSProperties}
      >
        <MahoSidebarTopBar
          collapsed={collapsed}
          onToggleSidebar={onToggleSidebar}
          canGoBack={canGoBack}
          canGoForward={canGoForward}
          onBack={onBack}
          onForward={onForward}
          onReload={onReload}
          controlActivity={controlActivity}
        />
        <div className="maho-sidebar__header">
          <MahoUrlPill tab={activeTab} onOpenCommandBar={onOpenCommandBar} onCopyUrl={onCopyUrl} />
        </div>
        {libraryOpen && libraryContent ? (
          <div className="maho-sidebar__library">{libraryContent}</div>
        ) : (
          <>
            <MahoFavoritesGrid
              store={store}
              activeBrowserId={selectedBrowserId}
              onActivateTab={onActivateTab}
            />
            <div className="maho-sidebar__body">
              <MahoPinnedSection
                store={store}
                spaceId={space.id}
                activeBrowserId={selectedBrowserId}
                onActivateTab={onActivateTab}
              />
              <MahoActionRow store={store} spaceId={space.id} onNewTab={onNewTab} onTidy={onTidy} />
              <MahoTodayList
                store={store}
                spaceId={space.id}
                activeBrowserId={selectedBrowserId}
                onActivateTab={onActivateTab}
              />
            </div>
          </>
        )}
        {agentSection ? <div className="maho-sidebar__agents">{agentSection}</div> : null}
        {nowPlayingSlot ? <div className="maho-sidebar__now-playing">{nowPlayingSlot}</div> : null}
        <MahoSidebarFooter store={store} libraryOpen={libraryOpen} onToggleLibrary={onToggleLibrary} />
      </aside>
    </DndContext>
  );
}
