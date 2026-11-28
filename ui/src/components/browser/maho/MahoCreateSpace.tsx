import { Plus, Sparkles } from "lucide-react";
import { useState } from "react";
import { supportedBrowserProfiles, useBrowserSettings } from "../../../lib/browserSettings";
import { SPACE_IDENTITY_ICONS, type BrowserStore } from "../../../state/browserStore";
import { DEFAULT_SPACE_PRESET_ID, SPACE_PRESETS } from "./mahoSpacePalette";
import { MahoSpaceIdentity } from "./MahoSpaceIdentity";

export function MahoCreateSpace({ store, onDone }: { store: BrowserStore; onDone: () => void }) {
  const { settings } = useBrowserSettings();
  const [name, setName] = useState("");
  const [icon, setIcon] = useState<string>();
  const [color, setColor] = useState(SPACE_PRESETS.find((preset) => preset.id === DEFAULT_SPACE_PRESET_ID)!.hex);
  const [profileId, setProfileId] = useState(settings.defaultProfileId);
  const [pickingIcon, setPickingIcon] = useState(false);
  const [error, setError] = useState("");
  return (
    <form className="maho-create-space-pane" role="dialog" aria-label="Create Space" onSubmit={(event) => {
      event.preventDefault();
      const result = store.createSpace({ name, color, icon, profileId });
      if (!result.ok) { setError("Could not create Space. Enter a name and try again."); return; }
      store.setActiveSpace(result.value.id);
      onDone();
    }}>
      <div className="maho-create-space-pane__hero" aria-hidden="true"><Sparkles /></div>
      <h2>Create a Space</h2>
      <p>Separate your tabs for life, work, projects, and more.</p>
      <div className="maho-create-space-pane__name">
        <button type="button" aria-label="Choose space icon" aria-expanded={pickingIcon} onClick={() => setPickingIcon(!pickingIcon)}>{icon ? <MahoSpaceIdentity icon={icon} /> : <Plus />}</button>
        <input aria-label="Space name" placeholder="Space name" autoFocus value={name} onChange={(event) => setName(event.target.value)} />
      </div>
      {pickingIcon ? <div className="maho-create-space-pane__icons" role="group" aria-label="Space identity icon">
        <button type="button" aria-label="No icon" aria-pressed={!icon} onClick={() => { setIcon(undefined); setPickingIcon(false); }}>None</button>
        {SPACE_IDENTITY_ICONS.map((candidate) => <button type="button" key={candidate} aria-label={candidate} aria-pressed={icon === candidate} onClick={() => { setIcon(candidate); setPickingIcon(false); }}><MahoSpaceIdentity icon={candidate} /></button>)}
      </div> : null}
      <label className="maho-create-space-pane__profile">Profile<select aria-label="Choose profile" value={profileId} onChange={(event) => setProfileId(event.target.value)}>{supportedBrowserProfiles(settings).map((profile) => <option key={profile.id} value={profile.id}>{profile.name}</option>)}</select></label>
      <fieldset><legend>Choose a Theme</legend><div className="maho-space-colors" role="group" aria-label="Space color">{SPACE_PRESETS.map((preset) => <button type="button" key={preset.id} className="maho-space-colors__swatch" aria-label={preset.label} aria-pressed={color === preset.hex} style={{ backgroundColor: preset.hex }} onClick={() => setColor(preset.hex)} />)}</div></fieldset>
      <div className="maho-create-space-pane__bottom">
        {error ? <p role="alert">{error}</p> : null}
        <button type="submit" disabled={!name.trim()}>Create</button>
        <button type="button" onClick={onDone}>Cancel</button>
      </div>
    </form>
  );
}
