// Maho Pinned: the Active Space header plus the Pinned tabs and Folders tree.
//
// The header owns the Space identity (name, color dot, rename/color popover); the
// tree is a flat two-level list because `browserStore` folders nest at most one
// level deep (`parentId` on a folder, `folderId` on a pinned tab). Both folder rows
// and the Pinned body are drop targets, so dragging a Today tab here pins it.

import { useDndContext, useDroppable } from "@dnd-kit/core";
import { SortableContext, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { FolderOpen, FolderPlus, Folder as FolderIcon, Palette, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";

import {
  browserStore,
  SPACE_IDENTITY_ICONS,
  selectActiveBrowserId,
  selectFolders,
  selectPinnedTabs,
  selectSpaceList,
  selectTabsInSection,
  useBrowserStore,
  type BrowserFolder,
  type BrowserStore,
  type BrowserSpace,
} from "../../../state/browserStore";
import { MahoBrowserTabRow } from "./MahoBrowserTabRow";
import { MahoSpaceIdentity } from "./MahoSpaceIdentity";
import { mahoDragId } from "./mahoBrowserTabActions";
import { SPACE_PRESETS } from "./mahoSpacePalette";

export type MahoPinnedSectionProps = {
  store?: BrowserStore;
  spaceId?: string;
  activeBrowserId?: string | null;
  onActivateTab?: (browserId: string) => void;
  showSpaceHeader?: boolean;
};

function SpaceColorSwatches({ space, store }: { space: BrowserSpace; store: BrowserStore }) {
  return (
    <div className="maho-space-colors" role="group" aria-label="Space color">
      {SPACE_PRESETS.map((preset) => (
        <button
          key={preset.id}
          type="button"
          className="maho-space-colors__swatch"
          style={{ backgroundColor: preset.hex }}
          aria-label={preset.label}
          aria-pressed={space.color.toLowerCase() === preset.hex.toLowerCase()}
          data-color={preset.hex}
          onClick={() => store.recolorSpace(space.id, preset.hex)}
        />
      ))}
    </div>
  );
}

/** Active Space header: name, color indicator, and the rename/color popover. */
export function MahoSpaceHeader({ store = browserStore, space }: { store?: BrowserStore; space: BrowserSpace }) {
  const [editing, setEditing] = useState(false);
  const [draftName, setDraftName] = useState(space.name);

  const commitName = () => {
    const result = store.renameSpace(space.id, draftName);
    if (!result.ok) {
      setDraftName(space.name);
      return;
    }
    setEditing(false);
  };

  return (
    <div className="maho-space-header">
      <MahoSpaceIdentity icon={space.icon} />
      {editing ? (
        <form
          className="maho-space-header__rename"
          onSubmit={(event) => {
            event.preventDefault();
            commitName();
          }}
        >
          <input
            className="maho-space-header__input"
            aria-label="Space name"
            value={draftName}
            autoFocus
            onChange={(event) => setDraftName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                setDraftName(space.name);
                setEditing(false);
              }
            }}
          />
          <button type="submit" className="maho-row__action" aria-label="Save Space name">
            Save
          </button>
        </form>
      ) : (
        <h2 className="maho-space-header__name">{space.name}</h2>
      )}
      <button
        type="button"
        className="maho-row__action"
        aria-label={editing ? "Close Space editor" : "Edit Space"}
        aria-expanded={editing}
        title="Rename or recolor Space"
        onClick={() => {
          setDraftName(space.name);
          setEditing((current) => !current);
        }}
      >
        <Palette aria-hidden="true" />
      </button>
      {editing ? (
        <div className="maho-space-popover" role="dialog" aria-label={`Edit Space ${space.name}`}>
          <span className="maho-space-popover__label">Color</span>
          <SpaceColorSwatches space={space} store={store} />
          <div className="maho-create-space-pane__icons" role="group" aria-label="Space identity icon">
            <button type="button" aria-label="No icon" onClick={() => store.setSpaceIcon(space.id, undefined)}>None</button>
            {SPACE_IDENTITY_ICONS.map((icon) => <button type="button" key={icon} aria-label={icon} aria-pressed={space.icon === icon} onClick={() => store.setSpaceIcon(space.id, icon)}><MahoSpaceIdentity icon={icon} /></button>)}
          </div>
        </div>
      ) : null}
    </div>
  );
}

function FolderRow({
  folder,
  store,
  collapsed,
  onToggleCollapsed,
  tabs,
  selectedBrowserId,
  onActivateTab,
}: {
  folder: BrowserFolder;
  store: BrowserStore;
  collapsed: boolean;
  onToggleCollapsed: (folderId: string) => void;
  tabs: ReturnType<typeof selectTabsInSection>;
  selectedBrowserId: string | null;
  onActivateTab: (browserId: string) => void;
}) {
  const [renaming, setRenaming] = useState(false);
  const [draftName, setDraftName] = useState(folder.name);
  const { setNodeRef, isOver } = useDroppable({ id: mahoDragId({ kind: "section", section: "pinned", folderId: folder.id }) });

  const commitName = () => {
    if (!store.renameFolder(folder.id, draftName).ok) setDraftName(folder.name);
    setRenaming(false);
  };

  return (
    <div ref={setNodeRef} className="maho-folder" data-over={isOver ? "true" : undefined} data-folder-id={folder.id}>
      <div className="maho-row maho-folder__row">
        <button
          type="button"
          className="maho-folder__toggle"
          aria-label={`${collapsed ? "Expand" : "Collapse"} folder ${folder.name}`}
          aria-expanded={!collapsed}
          onClick={() => onToggleCollapsed(folder.id)}
        >
          {collapsed ? <FolderIcon aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}
        </button>
        {renaming ? (
          <form
            className="maho-folder__rename"
            onSubmit={(event) => {
              event.preventDefault();
              commitName();
            }}
          >
            <input
              className="maho-space-header__input"
              aria-label={`Rename folder ${folder.name}`}
              value={draftName}
              autoFocus
              onChange={(event) => setDraftName(event.target.value)}
              onBlur={commitName}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  setDraftName(folder.name);
                  setRenaming(false);
                }
              }}
            />
          </form>
        ) : (
          <button
            type="button"
            className="maho-folder__name"
            aria-label={`Rename folder ${folder.name}`}
            title="Rename folder"
            onClick={() => {
              setDraftName(folder.name);
              setRenaming(true);
            }}
          >
            {folder.name}
          </button>
        )}
        <button
          type="button"
          className="maho-row__action"
          aria-label={`Delete folder ${folder.name}`}
          title="Delete folder"
          onClick={() => store.deleteFolder(folder.id)}
        >
          <Trash2 aria-hidden="true" />
        </button>
      </div>
      {collapsed ? null : (
        <div className="maho-folder__tabs">
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
                  sectionLabel={`Pinned folder ${folder.name}`}
                  active={tab.browserId === selectedBrowserId}
                  onActivate={onActivateTab}
                  onClose={(browserId) => store.closeTab(browserId)}
                />
              ))}
            </SortableContext>
          )}
        </div>
      )}
    </div>
  );
}

export function MahoPinnedSection({ store = browserStore, spaceId, activeBrowserId, onActivateTab, showSpaceHeader = true }: MahoPinnedSectionProps) {
  const state = useBrowserStore((snapshot) => snapshot, store);
  const targetSpaceId = spaceId ?? state.activeSpaceId;
  const space = useMemo(
    () => selectSpaceList(state).find((candidate) => candidate.id === targetSpaceId) ?? selectSpaceList(state)[0],
    [state, targetSpaceId],
  );
  const folders = selectFolders(state, targetSpaceId).filter((folder) => folder.parentId === undefined);
  const pinnedTabs = selectPinnedTabs(state, targetSpaceId);
  const selectedBrowserId = activeBrowserId ?? selectActiveBrowserId(state, targetSpaceId);
  const [collapsedFolders, setCollapsedFolders] = useState<Readonly<Record<string, boolean>>>({});
  const [creatingFolder, setCreatingFolder] = useState(false);
  const [newFolderName, setNewFolderName] = useState("");
  const { active } = useDndContext();
  const { setNodeRef, isOver } = useDroppable({ id: mahoDragId({ kind: "section", section: "pinned" }) });

  const activate = (browserId: string) => {
    if (onActivateTab) {
      onActivateTab(browserId);
      return;
    }
    store.touch(browserId);
  };

  const submitFolder = () => {
    const name = newFolderName.trim();
    if (name) store.createFolder({ name, spaceId: targetSpaceId });
    setNewFolderName("");
    setCreatingFolder(false);
  };

  return (
    <section className="maho-pinned" aria-label="Pinned" data-empty={pinnedTabs.length === 0 && folders.length === 0 ? "true" : undefined}>
      {showSpaceHeader && space ? <MahoSpaceHeader store={store} space={space} /> : null}
      <div className="maho-section-header">
        <button
          type="button"
          className="maho-row__action"
          aria-label="New folder"
          title="New folder"
          aria-expanded={creatingFolder}
          onClick={() => setCreatingFolder((current) => !current)}
        >
          <FolderPlus aria-hidden="true" />
        </button>
      </div>
      {creatingFolder ? (
        <form
          className="maho-folder__rename"
          onSubmit={(event) => {
            event.preventDefault();
            submitFolder();
          }}
        >
          <input
            className="maho-space-header__input"
            aria-label="New folder name"
            placeholder="Folder name"
            value={newFolderName}
            autoFocus
            onChange={(event) => setNewFolderName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                setNewFolderName("");
                setCreatingFolder(false);
              }
            }}
          />
          <button type="submit" className="maho-row__action" aria-label="Create folder">
            Add
          </button>
        </form>
      ) : null}
      <div ref={setNodeRef} className="maho-pinned__body" data-over={isOver ? "true" : undefined} data-drag-active={active ? "true" : undefined}>
        <SortableContext
          items={pinnedTabs.map((tab) => mahoDragId({ kind: "tab", tabId: tab.browserId }))}
          strategy={verticalListSortingStrategy}
        >
          {pinnedTabs.map((tab) => (
            <MahoBrowserTabRow
              key={tab.browserId}
              store={store}
              tab={tab}
              sectionLabel="Pinned"
              active={tab.browserId === selectedBrowserId}
              onActivate={activate}
              onClose={(browserId) => store.closeTab(browserId)}
            />
          ))}
        </SortableContext>
        {folders.map((folder) => (
          <FolderRow
            key={folder.id}
            folder={folder}
            store={store}
            collapsed={collapsedFolders[folder.id] === true}
            onToggleCollapsed={(folderId) =>
              setCollapsedFolders((current) => ({ ...current, [folderId]: current[folderId] !== true }))
            }
            tabs={selectTabsInSection(state, targetSpaceId, "pinned", folder.id)}
            selectedBrowserId={selectedBrowserId}
            onActivateTab={activate}
          />
        ))}
      </div>
    </section>
  );
}
