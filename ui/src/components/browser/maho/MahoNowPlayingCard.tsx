// Maho Now Playing card: the sidebar-bottom mini player for the tab that is playing.
//
// The media events come from the host (`browser_media_state`, one per tab) and land in
// `browserMediaStore`. The store outlives any single tab, so a tab closed while it was
// playing used to leave its entry behind and the card kept offering controls for a
// session that no longer existed. The card is the one surface that knows both sides -
// the media map and `browserStore.tabs` - so it is where that stale entry is dropped
// (`browserMediaStore.forget`), which is what fixes the lingering-track edge case.
//
// Everything the page reports (title, artist, artwork) is display text: the artwork is
// only rendered for http/https URLs, and a host failure is shown from its structured
// code (`extractBrowserErrorCode`), never from pattern-matching a message.

import { Music2, Pause, Play, Volume2, VolumeX } from "lucide-react";
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
import { MAHO_LAYOUT_TOKENS } from "./mahoTokens";

export type MahoNowPlayingCardProps = {
  store?: BrowserStore;
  /** Switch the browser to the playing tab. */
  onSelectTab: (browserId: string) => void;
};

/** Only a real web address is worth loading as an image; `data:`/`file:` are not artwork. */
function safeArtworkUrl(url: string): string | null {
  if (!url) return null;
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:" ? parsed.href : null;
  } catch {
    return null;
  }
}

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

  if (!current) return null;

  const tab = tabs[current.browserId];
  const title = current.title.trim() || (tab ? tab.title.trim() || displayHost(tab.url) : "Now playing");
  const artwork = safeArtworkUrl(current.artworkUrl);
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
      className="maho-now-playing flex items-center gap-2 rounded-xl bg-muted/60 px-2 py-1.5"
      style={{ borderRadius: MAHO_LAYOUT_TOKENS.rows.cornerRadius }}
      onClick={() => onSelectTab(current.browserId)}
      title={`Switch to ${title}`}
    >
      {artwork ? (
        <img
          src={artwork}
          alt=""
          data-testid="maho-now-playing-artwork"
          className="size-8 shrink-0 rounded-md object-cover"
        />
      ) : (
        <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
          <Music2 className="size-4" aria-hidden="true" />
        </span>
      )}

      <span className="min-w-0 flex-1">
        <span data-testid="maho-now-playing-title" className="block truncate text-xs text-foreground">
          {title}
        </span>
        {current.artist.trim() ? (
          <span data-testid="maho-now-playing-artist" className="block truncate text-[10px] text-muted-foreground">
            {current.artist}
          </span>
        ) : null}
        {errorCode ? (
          <span
            data-testid="maho-now-playing-error"
            className="block truncate font-mono text-[10px] text-destructive"
            title={errorCode}
          >
            {errorCode}
          </span>
        ) : null}
      </span>

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
  );
}
