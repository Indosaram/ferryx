function(post, reporter) {
  const key = '__ferryxMediaSessionBridge';
  let bridge = window[key];
  if (!bridge) {
    bridge = {
      handlers: new Map(),
      position: null,
      sessionId: `${performance.timeOrigin}:${Math.random().toString(36).slice(2)}`,
      source: '',
    };
    Object.defineProperty(window, key, { value: bridge });
    const session = navigator.mediaSession;
    if (session) {
      const setHandler = session.setActionHandler.bind(session);
      session.setActionHandler = (action, handler) => {
        setHandler(action, handler);
        if (handler) bridge.handlers.set(action, handler);
        else bridge.handlers.delete(action);
      };
      if (session.setPositionState) {
        const setPosition = session.setPositionState.bind(session);
        session.setPositionState = (position) => {
          setPosition(position);
          // A page may hand over a partial or impossible state; keep only what can be read
          // back as a finite number so the card never renders NaN or a negative time.
          const duration = Number(position?.duration);
          const value = Number(position?.position);
          const rate = Number(position?.playbackRate);
          bridge.position = Number.isFinite(duration) && duration > 0
            ? {
                duration,
                position: Number.isFinite(value) ? Math.min(Math.max(0, value), duration) : 0,
                playbackRate: Number.isFinite(rate) && rate > 0 ? rate : 1,
                at: performance.now(),
              }
            : null;
        };
      }
    }
  }

  // The dominant element is chosen exactly like the control script chooses it: the largest
  // playing one, else the largest one in document order.
  const dominant = () => {
    const elements = Array.from(document.querySelectorAll('audio,video'));
    if (elements.length === 0) return null;
    const area = (element) => {
      const rect = typeof element.getBoundingClientRect === 'function' ? element.getBoundingClientRect() : null;
      if (!rect) return 0;
      return Math.max(0, rect.width) * Math.max(0, rect.height);
    };
    const playing = (element) => !element.paused && !element.ended && element.readyState > 0;
    const active = elements.filter(playing);
    const candidates = active.length > 0 ? active : elements;
    return candidates.reduce((best, element) => (area(element) > area(best) ? element : best));
  };

  const secondsNow = (element, playing) => {
    if (element && Number.isFinite(element.currentTime)) return element.currentTime;
    const position = bridge.position;
    if (!position) return 0;
    if (!playing) return position.position;
    return position.position + ((performance.now() - position.at) / 1000) * position.playbackRate;
  };

  const durationOf = (element) => {
    if (element && Number.isFinite(element.duration) && element.duration > 0) return element.duration;
    return bridge.position ? bridge.position.duration : 0;
  };

  // Position, duration and capability, in the shape the host report carries. The capabilities
  // are what the card draws its controls from: a page that offers no previous track must not
  // get a previous button.
  const snapshot = (report) => {
    const element = dominant();
    const playing = report ? report.playing === true : Boolean(element && !element.paused && !element.ended);
    const duration = durationOf(element);
    const seconds = secondsNow(element, playing);
    return {
      positionMs: Number.isFinite(seconds) ? Math.max(0, Math.round(seconds * 1000)) : 0,
      durationMs: Number.isFinite(duration) ? Math.max(0, Math.round(duration * 1000)) : 0,
      canSeek: Boolean(duration > 0 || bridge.handlers.has('seekto')),
      canPrevious: bridge.handlers.has('previoustrack'),
      canNext: bridge.handlers.has('nexttrack'),
    };
  };

  // Seek, previous and next: the control script calls these by name. A page handler wins; the
  // element itself is the fallback only for seeking, because a track change cannot be invented.
  const dispatch = (action, seconds) => {
    if (action === 'seekto') {
      const handler = bridge.handlers.get('seekto');
      const element = dominant();
      if (handler) {
        handler({ action: 'seekto', seekTime: seconds });
        return true;
      }
      if (element && Number.isFinite(element.duration)) {
        element.currentTime = Math.min(Math.max(0, seconds), element.duration);
        return true;
      }
      return false;
    }
    const handler = bridge.handlers.get(action);
    if (!handler) return false;
    handler({ action });
    return true;
  };

  bridge.dominant = dominant;
  bridge.snapshot = snapshot;
  bridge.dispatch = dispatch;

  const augment = (report) => {
    if (!report) return report;
    const element = dominant();
    const source = element?.currentSrc || `${report.title}\u001f${report.artist}`;
    if (source !== bridge.source) {
      bridge.source = source;
      bridge.sessionId = `${performance.timeOrigin}:${Math.random().toString(36).slice(2)}`;
    }
    return { ...report, sessionId: bridge.sessionId, ...snapshot(report) };
  };

  if (typeof post === 'function') return reporter((report) => post(augment(report)));
  const raw = reporter(null);
  return raw ? JSON.stringify(augment(JSON.parse(raw))) : null;
}
