import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useMemo, useSyncExternalStore } from "react";

/** Event the host emits for each tab whose Now Playing state changed. */
export const BROWSER_MEDIA_STATE_EVENT = "browser_media_state";

/** What a tab currently plays. Every string comes from the page, so it is display text only. */
export type BrowserMediaState = {
  browserId: string;
  title: string;
  artist: string;
  album: string;
  artworkUrl: string;
  playing: boolean;
  muted: boolean;
  hasMedia: boolean;
};

export type BrowserMediaStates = ReadonlyMap<string, BrowserMediaState>;

/** The five controls the Now Playing card can send. A closed set, matching the host command. */
export type BrowserMediaAction = "play" | "pause" | "toggle" | "mute" | "unmute";

export type BrowserMediaControlResult = {
  action: string;
  applied: boolean;
  playing?: boolean;
  muted?: boolean;
  hasMedia?: boolean;
  reason?: string | null;
};

type Listener = () => void;

/**
 * The whole-app Now Playing store: one entry per tab that has media, kept in MRU order of
 * report arrival so a card can show the tab that most recently started playing.
 */
export function createBrowserMediaStore() {
  let states: BrowserMediaStates = new Map();
  const listeners = new Set<Listener>();

  const notify = () => {
    for (const listener of listeners) listener();
  };

  return {
    getState: (): BrowserMediaStates => states,
    subscribe(listener: Listener): () => void {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    /** Applies one host report. A report without media removes the tab. */
    apply(state: BrowserMediaState): void {
      const next = new Map(states);
      if (!state.hasMedia) {
        if (!next.delete(state.browserId)) return;
      } else {
        // Re-insert so the map keeps report order.
        next.delete(state.browserId);
        next.set(state.browserId, state);
      }
      states = next;
      notify();
    },
    /** A closed tab stops being Now Playing. */
    forget(browserId: string): void {
      if (!states.has(browserId)) return;
      const next = new Map(states);
      next.delete(browserId);
      states = next;
      notify();
    },
    reset(): void {
      if (states.size === 0) return;
      states = new Map();
      notify();
    },
  };
}

export const browserMediaStore = createBrowserMediaStore();

/**
 * Subscribes `listener` to `browser_media_state`. Resolves to the unlisten function; a caller
 * that unmounts before the subscribe settles must call it once it resolves.
 */
export function onBrowserMediaState(
  listener: (payload: BrowserMediaState) => void,
): Promise<UnlistenFn> {
  return listen<BrowserMediaState>(BROWSER_MEDIA_STATE_EVENT, (event) => listener(event.payload));
}

/** play / pause / toggle / mute / unmute, routed through the host's page-script path. */
export function controlBrowserMedia(
  browserId: string,
  action: BrowserMediaAction,
): Promise<BrowserMediaControlResult> {
  return invoke<BrowserMediaControlResult>("cmd_browser_media_control", { browserId, action });
}

/**
 * Feeds every report into the shared store while the component is mounted, and returns the
 * current map. Several components can mount it; the store is the single source of truth and
 * the events are subscribed once per active mount.
 */
export function useBrowserMediaStates(): BrowserMediaStates {
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void onBrowserMediaState((payload) => {
      if (!disposed) browserMediaStore.apply(payload);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
  return useSyncExternalStore(browserMediaStore.subscribe, browserMediaStore.getState);
}

/** The one tab a Now Playing card should show: the most recently reported one with media. */
export function useActiveBrowserMedia(): BrowserMediaState | null {
  const states = useBrowserMediaStates();
  return useMemo(() => {
    let last: BrowserMediaState | null = null;
    for (const state of states.values()) last = state;
    return last;
  }, [states]);
}
