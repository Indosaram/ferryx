// Maho's 42px action row: `+ New Tab`, `Tidy`, `Clear`.
//
// All three are callbacks. `Tidy` is wired in Todo 13 (the grouping orchestrator),
// and `Clear` is the one action with baked-in store semantics because its undo
// affordance has to mirror exactly what `clearToday` archived.

import { Plus, Sparkles, Trash2 } from "lucide-react";

import {
  browserStore,
  selectTodayTabs,
  useBrowserStore,
  type BrowserStore,
} from "../../../state/browserStore";
import { clearTodayWithUndo } from "./mahoBrowserTabActions";

export type MahoActionRowProps = {
  store?: BrowserStore;
  spaceId?: string;
  onNewTab?: () => void;
  /** Wired by Todo 13's Tidy orchestrator. */
  onTidy?: () => void;
};

export function MahoActionRow({ store = browserStore, spaceId, onNewTab, onTidy }: MahoActionRowProps) {
  const activeSpaceId = useBrowserStore((state) => state.activeSpaceId, store);
  const targetSpaceId = spaceId ?? activeSpaceId;
  const todayCount = useBrowserStore((state) => selectTodayTabs(state, targetSpaceId).length, store);

  return (
    <div className="maho-action-row" role="toolbar" aria-label="Tab actions">
      <button type="button" className="maho-action-row__primary" aria-label="New Tab" onClick={onNewTab}>
        <Plus aria-hidden="true" />
        <span>New Tab</span>
      </button>
      <span className="maho-action-row__spacer" />
      <button
        type="button"
        className="maho-action-row__button"
        aria-label="Tidy Today"
        title="Group Today by site"
        onClick={onTidy}
        disabled={!onTidy || todayCount === 0}
      >
        <Sparkles aria-hidden="true" />
      </button>
      <button
        type="button"
        className="maho-action-row__button"
        aria-label={`Clear Today (${todayCount})`}
        title={`Archive ${todayCount} Today ${todayCount === 1 ? "tab" : "tabs"}`}
        onClick={() => clearTodayWithUndo(store, targetSpaceId)}
        disabled={todayCount === 0}
      >
        <Trash2 aria-hidden="true" />
      </button>
    </div>
  );
}
