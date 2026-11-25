import { onInboundUrls, takeInboundUrls } from "./browserTauri";

/** The schemes the OS may hand Ferryx. Mirrors `classify_inbound_url` on the Rust side. */
const WEB_URL = /^https?:\/\/[^/?#\s]+/i;

/**
 * Keeps only the URLs that can open in a browser tab. The backend drops non-http(s) links before
 * queueing, so this is a second gate for values a different (or older) backend queued: a URL that
 * reached the queue must never become a tab on a scheme the app does not browse.
 */
export function webUrls(urls: readonly string[]): string[] {
  // Trimmed as well as filtered: the test must decide on what is opened, so a value padded with
  // whitespace is opened as the bare link rather than failing the tab's own URL parse.
  return urls.map((url) => url.trim()).filter((url) => WEB_URL.test(url));
}

export type InboundUrlDelivery = {
  /** Takes every queued URL, oldest first. Defaults to the Tauri command. */
  take?: () => Promise<string[]>;
  /** Subscribes to the "URLs are queued" event; resolves to its unlisten. Defaults to Tauri. */
  on?: (listener: () => void) => Promise<() => void>;
  /** Leaves URLs queued while this window cannot open local browser tabs. */
  canDrain?: () => boolean;
  /** Opens one URL in a new browser tab. */
  open: (url: string) => void;
};

/**
 * Delivers the http(s) links the OS asked Ferryx to open (the default-browser `open -a` hand-off)
 * into tabs, and returns a disposer and a trigger to retry delivery when this window is ready.
 *
 * The backend queues each link and then tells one window an event; the queue is drained with an
 * atomic take, so the window that drains gets each URL exactly once and no two windows can open
 * the same link. That is also why every window may run this: the focused window normally drains
 * first, and a second window finds the queue empty.
 *
 * Draining happens once on start (it covers links that arrived before this window listened, such
 * as the URL a cold launch was started with) and again on every event. A delivery that lands while
 * a drain is in flight is coalesced into that drain rather than dropped. After the disposer runs,
 * nothing opens: a take/listen that resolves late is ignored, and its listener is released.
 */
export function startInboundUrlDelivery({ take = takeInboundUrls, on = onInboundUrls, canDrain, open }: InboundUrlDelivery): { dispose: () => void; trigger: () => void } {
  let disposed = false;
  let draining = false;
  let queuedWhileDraining = false;
  let unlisten: (() => void) | undefined;

  const drain = async (): Promise<void> => {
    if (disposed) return;
    if (draining) {
      queuedWhileDraining = true;
      return;
    }
    draining = true;
    try {
      do {
        queuedWhileDraining = false;
        if (canDrain && !canDrain()) return;
        const urls = webUrls(await take());
        // Unmounted while the take was in flight: those URLs were drained, and opening tabs for a
        // window that is gone would put them nowhere the user can see.
        if (disposed) return;
        for (const url of urls) open(url);
      } while (queuedWhileDraining && !disposed);
    } catch {
      // The queue is unchanged on failure, so the next event or start drains it; a broken
      // delivery must not take the window's effects down with it.
    } finally {
      draining = false;
    }
  };

  // Subscribe before the first drain so an event in between is not missed; either path drains the
  // shared queue, and the in-flight guard keeps the two from opening the same URL twice.
  void on(() => void drain())
    .then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    })
    .catch(() => {});
  void drain();

  return {
    dispose: () => {
      disposed = true;
      unlisten?.();
      unlisten = undefined;
    },
    trigger: () => {
      if (!disposed) void drain();
    },
  };
}
