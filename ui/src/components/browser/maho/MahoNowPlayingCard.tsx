// Maho Now Playing card: the sidebar-bottom mini player for the tab that is playing.
//
// The media events come from the host (`browser_media_state`, one per tab) and land in
// `browserMediaStore`. The store outlives any single tab, so a tab closed while it was
// playing used to leave its entry behind and the card kept offering controls for a
// session that no longer existed. The card is the one surface that knows both sides -
// the media map and `browserStore.tabs` - so it is where that stale entry is dropped
// (`browserMediaStore.forget`), which is what fixes the lingering-track edge case.
//
// Source mapping: maho_now_playing_card.cc ignores artwork and expands metadata on hover.

import { Music2, Pause, Play, Volume2, VolumeX, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import {
  browserMediaStore,
  controlBrowserMedia,
  useBrowserMediaStates,
  type BrowserMediaState,
} from "../../../lib/browserMedia";
import { extractBrowserErrorCode } from "../../../lib/browserTauri";
import {
  browserStore,
  useBrowserStore,
  type BrowserStore,
} from "../../../state/browserStore";
import { displayHost } from "./mahoBrowserTabActions";

export type MahoNowPlayingCardProps = {
  store?: BrowserStore;
  /** Switch the browser to the playing tab. */
  onSelectTab: (browserId: string) => void;
};

/**
 * The media entry the card shows: the most recently reported tab that still exists in
 * the store. `null` when nothing open is playing.
 */
export function selectNowPlaying(
  states: ReadonlyMap<string, BrowserMediaState>,
  tabs: Readonly<Record<string, unknown>>,
): BrowserMediaState | null {
  let current: BrowserMediaState | null = null;
  for (const state of states.values()) {
    if (!tabs[state.browserId]) continue;
    if (!state.hasMedia) continue;
    current = state;
  }
  return current;
}

export function MahoNowPlayingCard({ store = browserStore, onSelectTab }: MahoNowPlayingCardProps) {
  const mediaStates = useBrowserMediaStates();
  const tabs = useBrowserStore((state) => state.tabs, store);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [dismissedId, setDismissedId] = useState<string | null>(null);

  // A media entry whose tab is gone describes a session that cannot be controlled:
  // drop it instead of drawing controls that would fail.
  useEffect(() => {
    for (const browserId of mediaStates.keys()) {
      if (!tabs[browserId]) browserMediaStore.forget(browserId);
    }
  }, [mediaStates, tabs]);

  const current = useMemo(() => selectNowPlaying(mediaStates, tabs), [mediaStates, tabs]);

  const control = useCallback(async (browserId: string, action: "play" | "pause" | "mute" | "unmute") => {
    setErrorCode(null);
    try {
      const result = await controlBrowserMedia(browserId, action);
      // The host reports a refusal in-band; surfacing it beats a button that silently
      // does nothing.
      if (!result.applied && result.reason) setErrorCode(result.reason);
    } catch (error) {
      setErrorCode(extractBrowserErrorCode(error));
    }
  }, []);

  if (!current || current.browserId === dismissedId) return null;

  const tab = tabs[current.browserId];
  const title = current.title.trim() || (tab ? tab.title.trim() || displayHost(tab.url) : "Now playing");
  const artist = current.artist.trim() || displayHost(tab.url);
  const toggleLabel = current.playing ? "Pause" : "Play";
  const muteLabel = current.muted ? "Unmute" : "Mute";

  return (
    <div
      role="group"
      aria-label="Now playing"
      data-testid="maho-now-playing"
      data-browser-id={current.browserId}
      data-playing={current.playing ? "true" : "false"}
      data-muted={current.muted ? "true" : "false"}
      className="maho-now-playing"
    >
      <div className="maho-now-playing__metadata">
        <div className="maho-now-playing__title-row">
          <span data-testid="maho-now-playing-title" className="min-w-0 flex-1 truncate">{title}</span>
          <button type="button" aria-label="Dismiss Now Playing" title="Dismiss" onClick={() => setDismissedId(current.browserId)}><X aria-hidden="true" /></button>
        </div>
        <span data-testid="maho-now-playing-artist" className="maho-now-playing__artist block truncate">{artist}</span>
      </div>
      {errorCode ? (
          <span
            data-testid="maho-now-playing-error"
            className="block truncate font-mono text-[10px] text-destructive"
            title={errorCode}
          >
            {errorCode}
          </span>
        ) : null}
      <div className="maho-now-playing__controls">
        <button type="button" className="maho-now-playing__source" aria-label="Jump to source tab" title="Jump to source tab" onClick={() => onSelectTab(current.browserId)}>
          {tab.faviconUrl ? <img src={tab.faviconUrl} alt="" /> : <Music2 aria-hidden="true" />}
          {current.playing ? <span className="maho-now-playing__notes" aria-hidden="true">♪</span> : null}
        </button>
        <span className="flex-1" />
      <button
        type="button"
        aria-label={toggleLabel}
        title={toggleLabel}
        data-testid="maho-now-playing-toggle"
        className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
        onClick={(event) => {
          event.stopPropagation();
          void control(current.browserId, current.playing ? "pause" : "play");
        }}
      >
        {current.playing ? <Pause className="size-4" aria-hidden="true" /> : <Play className="size-4" aria-hidden="true" />}
      </button>
      <span className="flex-1" />
      <button
        type="button"
        aria-label={muteLabel}
        title={muteLabel}
        data-testid="maho-now-playing-mute"
        className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
        onClick={(event) => {
          event.stopPropagation();
          void control(current.browserId, current.muted ? "unmute" : "mute");
        }}
      >
        {current.muted ? <VolumeX className="size-4" aria-hidden="true" /> : <Volume2 className="size-4" aria-hidden="true" />}
      </button>
      </div>
    </div>
  );
}
