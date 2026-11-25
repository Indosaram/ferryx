// Media-session observation for the Now Playing card, shared by both browser engines.
//
// This file holds ONE function expression (not an IIFE): the caller decides how it is used.
//   * push: the system webview's guest bridge installs it with a `post` callback; it reports on
//     change, at most twice a second, as a `ferryx-intent` that the isolated-world broker
//     forwards to the host as a `media` trusted-bridge message. `post` receives one plain
//     report object.
//   * pull: the Chromium (CEF) engine has no page-world initialization hook Ferryx can install
//     without changing the engine binary, so the host evaluates this same function with
//     `post = null` through the existing `Runtime.evaluate` page-script path and reads one
//     snapshot back as JSON text.
//
// Everything here is read-only towards the page: it never evaluates page-provided strings. The
// values it reports (`navigator.mediaSession.metadata`, element state) are page-controlled, so
// the host treats them as untrusted text: it clamps lengths and validates the artwork URL.
function (post) {
  let topFrame = false;
  try {
    topFrame = window.top === window;
  } catch (error) {
    topFrame = false;
  }
  if (!topFrame) return null;

  const MAX_REPORTS_PER_SECOND = 2;
  const POLL_MS = 500;
  const TITLE_LIMIT = 300;
  const ARTIST_LIMIT = 200;
  const ALBUM_LIMIT = 200;
  const ARTWORK_LIMIT = 2048;

  const clampText = (value, limit) => {
    if (typeof value !== 'string') return '';
    // Control characters would break the one-line host protocol; page text is untrusted.
    return value.replace(/[\u0000-\u001f\u007f]+/g, ' ').trim().slice(0, limit);
  };
  const clampUrl = (value) => {
    const text = clampText(value, ARTWORK_LIMIT);
    if (!text) return '';
    if (!/^https?:\/\//i.test(text) && !/^data:image\//i.test(text)) return '';
    return text;
  };

  const isMedia = (node) => Boolean(node) && (node.tagName === 'AUDIO' || node.tagName === 'VIDEO');
  const area = (element) => {
    const rect = typeof element.getBoundingClientRect === 'function' ? element.getBoundingClientRect() : null;
    if (!rect) return 0;
    return Math.max(0, rect.width) * Math.max(0, rect.height);
  };
  const playing = (element) => !element.paused && !element.ended && element.readyState > 0;

  const push = typeof post === 'function';
  // Push mode learns "the last element that played" from play events. Pull mode is a stateless
  // single evaluation, so it uses playback progress instead.
  const startedAt = new WeakMap();
  let startOrder = 0;
  const mediaElements = () => Array.from(document.querySelectorAll('audio,video')).filter(isMedia);

  // The dominant element: the largest one playing, else the most recent one, else the largest.
  const dominant = () => {
    const elements = mediaElements();
    if (elements.length === 0) return null;
    const active = elements.filter(playing);
    if (active.length > 0) {
      return active.reduce((best, element) => (area(element) > area(best) ? element : best));
    }
    if (push) {
      const ordered = elements
        .filter((element) => startedAt.has(element))
        .sort((left, right) => startedAt.get(right) - startedAt.get(left));
      if (ordered.length > 0) return ordered[0];
    } else {
      const progressed = elements
        .filter((element) => !element.ended)
        .sort((left, right) => (right.currentTime || 0) - (left.currentTime || 0));
      if (progressed.length > 0) return progressed[0];
    }
    return elements.reduce((best, element) => (area(element) > area(best) ? element : best));
  };

  const metadataOf = () => {
    let metadata = null;
    try {
      metadata = navigator.mediaSession ? navigator.mediaSession.metadata : null;
    } catch (error) {
      metadata = null;
    }
    if (!metadata) return { title: '', artist: '', album: '', artworkUrl: '' };
    let artworkUrl = '';
    try {
      const artwork = Array.isArray(metadata.artwork) ? metadata.artwork : [];
      const first = artwork[0];
      artworkUrl = clampUrl(first && typeof first.src === 'string' ? first.src : '');
    } catch (error) {
      artworkUrl = '';
    }
    return {
      title: clampText(metadata.title, TITLE_LIMIT),
      artist: clampText(metadata.artist, ARTIST_LIMIT),
      album: clampText(metadata.album, ALBUM_LIMIT),
      artworkUrl,
    };
  };

  const emptyReport = () => ({
    title: '',
    artist: '',
    album: '',
    artworkUrl: '',
    playing: false,
    muted: false,
    hasMedia: false,
  });

  const snapshot = () => {
    const element = dominant();
    const metadata = metadataOf();
    const hasMedia = Boolean(element) || Boolean(metadata.title) || Boolean(metadata.artworkUrl);
    if (!hasMedia) return null;
    return {
      // The document title is the fallback name when the page publishes no media-session
      // metadata (YouTube sets one; a bare <video src> does not).
      title: metadata.title || (element ? clampText(document.title, TITLE_LIMIT) : ''),
      artist: metadata.artist,
      album: metadata.album,
      artworkUrl: metadata.artworkUrl,
      playing: element ? playing(element) : false,
      muted: element ? element.muted === true : false,
      hasMedia: true,
    };
  };

  // Pull mode: one snapshot, no listeners and no timers.
  if (!push) {
    return JSON.stringify(snapshot() || emptyReport());
  }

  const recordStart = (element) => {
    if (!isMedia(element)) return;
    startOrder += 1;
    startedAt.set(element, startOrder);
  };
  ['play', 'playing'].forEach((type) => {
    document.addEventListener(type, (event) => recordStart(event.target), true);
  });

  let lastKey = null;
  let windowStart = 0;
  let windowCount = 0;
  let reportedMedia = false;

  const send = (report) => {
    const key = JSON.stringify(report);
    if (key === lastKey) return false;
    const now = Date.now();
    if (now - windowStart >= 1000) {
      windowStart = now;
      windowCount = 0;
    }
    if (windowCount >= MAX_REPORTS_PER_SECOND) return false;
    windowCount += 1;
    lastKey = key;
    try {
      post(report);
    } catch (error) {
      // The channel is gone (a reload, a navigation); the next document installs a new one.
      lastKey = null;
    }
    return true;
  };

  // A tab that had media and lost it reports the empty state once, so the card can go away.
  const report = () => {
    const current = snapshot();
    if (!current) {
      if (reportedMedia) {
        reportedMedia = false;
        send(emptyReport());
      }
      return;
    }
    reportedMedia = true;
    send(current);
  };

  ['play', 'playing', 'pause', 'ended', 'volumechange', 'emptied', 'loadedmetadata', 'durationchange'].forEach((type) => {
    document.addEventListener(type, (event) => {
      if (isMedia(event.target)) report();
    }, true);
  });
  document.addEventListener('DOMContentLoaded', report, true);
  window.addEventListener('pagehide', () => {
    lastKey = null;
  });

  setInterval(report, POLL_MS);
  report();
  return 'reporter';
}
