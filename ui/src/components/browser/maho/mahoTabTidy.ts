// Maho Tidy: deterministic grouping of Today by site.
//
// Tidy is offline and explainable: it never calls a service and never guesses. It
// reads the Today list, groups the tabs by host (`www.` stripped, unparseable and
// blank addresses ignored), and for every host with at least two tabs it creates - or
// reuses - a Pinned folder named after that host and moves the tabs in. A host with a
// single tab stays in Today: a folder holding one row is noise.
//
// Every change is recorded, so `undo()` can put the tabs back exactly where they were
// and delete only the folders this call created. Tabs the user moved or closed after
// Tidy are left alone: undo never resurrects a closed tab and never yanks back a tab
// the user has since filed somewhere else.

import {
  browserStore,
  selectFolders,
  selectTodayTabs,
  type BrowserFolder,
  type BrowserStore,
  type BrowserStoreTab,
} from "../../../state/browserStore";

export type TidyTodayTabsResult = {
  /** Folders this call created, in the order they were made. */
  foldersCreated: readonly BrowserFolder[];
  /** How many tabs moved into a folder. */
  movedCount: number;
  /** Restores the pre-Tidy placement of everything this call touched. */
  undo: () => void;
};

/**
 * The grouping key of one URL: its lowercased host without a leading `www.`.
 * `null` for a blank or unparseable address (`about:blank` is a new tab, not a site).
 */
export function tidyDomainOf(url: string): string | null {
  if (!url || !url.trim() || url === "about:blank") return null;
  try {
    const host = new URL(url).hostname.trim().toLowerCase();
    if (!host) return null;
    return host.startsWith("www.") ? host.slice(4) : host;
  } catch {
    return null;
  }
}

function folderNamed(folders: readonly BrowserFolder[], name: string): BrowserFolder | undefined {
  const wanted = name.toLowerCase();
  return folders.find((folder) => folder.name.trim().toLowerCase() === wanted);
}

/**
 * Groups `spaceId`'s Today tabs by host. Only hosts with two or more tabs are
 * returned, sorted by host so the folder order is reproducible.
 */
export function planTidyGroups(tabs: readonly BrowserStoreTab[]): Array<{ domain: string; tabs: BrowserStoreTab[] }> {
  const byDomain = new Map<string, BrowserStoreTab[]>();
  for (const tab of tabs) {
    const domain = tidyDomainOf(tab.url);
    if (!domain) continue;
    const bucket = byDomain.get(domain);
    if (bucket) bucket.push(tab);
    else byDomain.set(domain, [tab]);
  }
  return [...byDomain.entries()]
    .filter(([, bucket]) => bucket.length >= 2)
    .map(([domain, bucket]) => ({ domain, tabs: bucket }))
    .sort((left, right) => left.domain.localeCompare(right.domain));
}

/** Tidy Today for one Space. Pass no `spaceId` for the store's active Space. */
export function tidyTodayTabs(store: BrowserStore = browserStore, spaceId?: string): TidyTodayTabsResult {
  const state = store.getState();
  const targetSpaceId = spaceId ?? state.activeSpaceId;
  const today = selectTodayTabs(state, targetSpaceId);
  const groups = planTidyGroups(today);

  // Captured before any move: the exact Today sequence undo has to reproduce.
  const originalTodayOrder = today.map((tab) => tab.browserId);
  const foldersCreated: BrowserFolder[] = [];
  const moved: Array<{ browserId: string; folderId: string }> = [];

  for (const group of groups) {
    const existing = folderNamed(selectFolders(store.getState(), targetSpaceId), group.domain);
    const folder = existing ?? (() => {
      const created = store.createFolder({ name: group.domain, spaceId: targetSpaceId });
      if (!created.ok) return null;
      foldersCreated.push(created.value);
      return created.value;
    })();
    if (!folder) continue;

    for (const tab of group.tabs) {
      const result = store.moveTab(tab.browserId, { section: "pinned", folderId: folder.id });
      if (result.ok) moved.push({ browserId: tab.browserId, folderId: folder.id });
    }
  }

  const createdIds = foldersCreated.map((folder) => folder.id);

  return {
    foldersCreated,
    movedCount: moved.length,
    undo() {
      store.setState((prev) => {
        const tabs: Record<string, BrowserStoreTab> = { ...prev.tabs };
        for (const entry of moved) {
          const tab = tabs[entry.browserId];
          if (!tab) continue;
          // Only a tab still sitting in the folder Tidy made is ours to move back.
          if (tab.section !== "pinned" || tab.folderId !== entry.folderId) continue;
          const restored: BrowserStoreTab = { ...tab, section: "today" };
          delete restored.folderId;
          tabs[entry.browserId] = restored;
        }
        // Rebuild Today's numbering from the captured order; a tab opened after Tidy
        // is not in the list and keeps its own slot.
        const survivors = originalTodayOrder.filter((browserId) => {
          const tab = tabs[browserId];
          return tab !== undefined && tab.section === "today";
        });
        survivors.forEach((browserId, index) => {
          tabs[browserId] = { ...tabs[browserId], order: index };
        });
        const folders: Record<string, BrowserFolder> = { ...prev.folders };
        for (const folderId of createdIds) {
          const occupied = Object.values(tabs).some((tab) => tab.folderId === folderId);
          if (!occupied) delete folders[folderId];
        }
        return { ...prev, folders, tabs };
      });
    },
  };
}
