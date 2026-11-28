// Maho sidebar footer: Library toggle, the Space Dots switcher, and Create Space.
//
// Dot size and opacity follow MahoSidebarSpaceDotView; color comes from the
// primary-text and neutral-glyph palette roles, not each Space's swatch.

import { Archive, Plus } from "lucide-react";
import { useState } from "react";

import {
  browserStore,
  isSpaceIdentityIcon,
  selectSpaceList,
  useBrowserStore,
  type BrowserStore,
} from "../../../state/browserStore";
import { MahoCreateSpace } from "./MahoCreateSpace";
import { MahoSpaceIdentity } from "./MahoSpaceIdentity";

export type MahoSidebarFooterProps = {
  store?: BrowserStore;
  libraryOpen?: boolean;
  onToggleLibrary?: () => void;
  /** Owned by the caller; when set, the caller renders the Create Space pane itself. */
  onCreateSpace?: () => void;
  /** Whether the caller's Create Space pane is open, so the control can report it. */
  creating?: boolean;
};

export function MahoSidebarFooter({ store = browserStore, libraryOpen = false, onToggleLibrary, onCreateSpace, creating: creatingProp }: MahoSidebarFooterProps) {
  const spaces = useBrowserStore(selectSpaceList, store);
  const activeSpaceId = useBrowserStore((state) => state.activeSpaceId, store);
  const [creatingLocal, setCreatingLocal] = useState(false);
  const creating = creatingProp ?? creatingLocal;
  const toggleCreating = () => { if (onCreateSpace) onCreateSpace(); else setCreatingLocal((current) => !current); };

  return (
    <div className="maho-footer">
      <button
        type="button"
        className="maho-footer__control"
        aria-label="Library"
        aria-pressed={libraryOpen}
        title="Library"
        onClick={onToggleLibrary}
      >
        <Archive aria-hidden="true" />
      </button>
      <div className="maho-footer__dots" role="group" aria-label="Spaces">
        {spaces.map((space) => (
          <button
            key={space.id}
            type="button"
            className="maho-space-dot"
            data-active={space.id === activeSpaceId ? "true" : undefined}
            data-has-icon={isSpaceIdentityIcon(space.icon) ? "true" : undefined}
            aria-label={`Switch to Space ${space.name}`}
            aria-pressed={space.id === activeSpaceId}
            title={space.name}
            onClick={() => store.setActiveSpace(space.id)}
          >
            <span className="maho-space-dot__indicator" aria-hidden="true" />
            <MahoSpaceIdentity icon={space.icon} />
          </button>
        ))}
      </div>
      <button
        type="button"
        className="maho-footer__control maho-space-dot--create"
        aria-label="Create Space"
        aria-expanded={creating}
        title="Create Space"
        onClick={toggleCreating}
      >
        <Plus aria-hidden="true" />
      </button>
      {creating && !onCreateSpace ? <MahoCreateSpace store={store} onDone={() => setCreatingLocal(false)} /> : null}
    </div>
  );
}
