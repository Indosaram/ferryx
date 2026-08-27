import { Copy } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import {
  resolveSupportedBrowserProfileId,
  supportedBrowserProfiles,
  useBrowserSettings,
} from "../lib/browserSettings";
import type { BrowserTab } from "../lib/types";

export function BrowserDuplicateControl({
  tab,
  onDuplicate,
}: {
  tab: BrowserTab;
  onDuplicate: (profileId: string) => void;
}) {
  const { settings } = useBrowserSettings();
  const profiles = useMemo(() => supportedBrowserProfiles(settings), [settings]);
  const [profileId, setProfileId] = useState(() => resolveSupportedBrowserProfileId(tab.profileId, settings));

  useEffect(() => {
    setProfileId(resolveSupportedBrowserProfileId(tab.profileId, settings));
  }, [settings, tab.id, tab.profileId]);

  return (
    <div className="flex items-center gap-1 px-1 py-1">
      <button
        type="button"
        role="menuitem"
        onClick={() => onDuplicate(profileId)}
        className="flex min-w-0 flex-1 items-center gap-2 rounded-sm px-1 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground"
      >
        <Copy className="size-3.5 shrink-0" />
        Duplicate browser tab
      </button>
      <select
        aria-label="Duplicate browser profile"
        value={profileId}
        onChange={(event) => setProfileId(event.target.value)}
        onClick={(event) => event.stopPropagation()}
        className="max-w-24 rounded border border-border bg-background px-1 py-0.5 text-[10px] text-foreground outline-none focus:border-ring"
      >
        {profiles.map((profile) => (
          <option key={profile.id} value={profile.id}>{profile.name}</option>
        ))}
      </select>
    </div>
  );
}
