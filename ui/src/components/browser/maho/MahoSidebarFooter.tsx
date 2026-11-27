// Maho sidebar footer: Library toggle, the Space Dots switcher, and Create Space.
//
// One dot per Space in `order`, the active one drawn with the accent color from the
// Space palette. Create Space collects a name plus one of `SPACE_PRESETS`'s eight
// swatches, and lands on the new Space because that is what a person creating a
// Space wants next.

import { BookOpen, Plus, X } from "lucide-react";
import { useState } from "react";

import {
  browserStore,
  selectSpaceList,
  useBrowserStore,
  type BrowserStore,
} from "../../../state/browserStore";
import { DEFAULT_SPACE_PRESET_ID, SPACE_PRESETS } from "./mahoSpacePalette";

export type MahoSidebarFooterProps = {
  store?: BrowserStore;
  libraryOpen?: boolean;
  onToggleLibrary?: () => void;
};

export function MahoSidebarFooter({ store = browserStore, libraryOpen = false, onToggleLibrary }: MahoSidebarFooterProps) {
  const spaces = useBrowserStore(selectSpaceList, store);
  const activeSpaceId = useBrowserStore((state) => state.activeSpaceId, store);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [color, setColor] = useState(
    SPACE_PRESETS.find((preset) => preset.id === DEFAULT_SPACE_PRESET_ID)?.hex ?? SPACE_PRESETS[0].hex,
  );

  const submit = () => {
    const result = store.createSpace({ name, color });
    if (result.ok) store.setActiveSpace(result.value.id);
    setName("");
    setCreating(false);
  };

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
        <BookOpen aria-hidden="true" />
      </button>
      <div className="maho-footer__dots" role="group" aria-label="Spaces">
        {spaces.map((space) => (
          <button
            key={space.id}
            type="button"
            className="maho-space-dot"
            data-active={space.id === activeSpaceId ? "true" : undefined}
            aria-label={`Switch to Space ${space.name}`}
            aria-pressed={space.id === activeSpaceId}
            title={space.name}
            onClick={() => store.setActiveSpace(space.id)}
          >
            <span className="maho-space-dot__indicator" style={{ backgroundColor: space.color }} aria-hidden="true" />
          </button>
        ))}
        <button
          type="button"
          className="maho-space-dot maho-space-dot--create"
          aria-label="Create Space"
          aria-expanded={creating}
          title="Create Space"
          onClick={() => setCreating((current) => !current)}
        >
          <Plus aria-hidden="true" />
        </button>
      </div>
      {creating ? (
        <form
          className="maho-create-space"
          role="dialog"
          aria-label="Create Space"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <div className="maho-create-space__header">
            <span className="maho-create-space__title">New Space</span>
            <button
              type="button"
              className="maho-row__action"
              aria-label="Cancel Create Space"
              onClick={() => {
                setName("");
                setCreating(false);
              }}
            >
              <X aria-hidden="true" />
            </button>
          </div>
          <input
            className="maho-space-header__input"
            aria-label="Space name"
            placeholder="Space name"
            value={name}
            autoFocus
            onChange={(event) => setName(event.target.value)}
          />
          <div className="maho-space-colors" role="group" aria-label="Space color">
            {SPACE_PRESETS.map((preset) => (
              <button
                key={preset.id}
                type="button"
                className="maho-space-colors__swatch"
                style={{ backgroundColor: preset.hex }}
                aria-label={preset.label}
                aria-pressed={color === preset.hex}
                data-color={preset.hex}
                onClick={() => setColor(preset.hex)}
              />
            ))}
          </div>
          <button type="submit" className="maho-action-row__primary" disabled={!name.trim()}>
            <Plus aria-hidden="true" />
            <span>Create</span>
          </button>
        </form>
      ) : null}
    </div>
  );
}
