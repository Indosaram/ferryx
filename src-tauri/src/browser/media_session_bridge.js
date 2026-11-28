function(post, reporter) {
  const key = '__ferryxMediaSessionBridge';
  let bridge = window[key];
  if (!bridge) {
    bridge = { handlers: new Map(), position: null, sessionId: `${performance.timeOrigin}:${Math.random().toString(36).slice(2)}`, source: '' };
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
          bridge.position = position ? { ...position, at: performance.now() } : null;
        };
      }
    }
  }
  const augment = (report) => {
    if (!report) return report;
    const elements = Array.from(document.querySelectorAll('audio,video'));
    const active = elements.filter((element) => !element.paused && !element.ended);
    const candidates = active.length ? active : elements;
    const area = (element) => { const rect = element.getBoundingClientRect(); return rect.width * rect.height; };
    const element = candidates.reduce((best, next) => !best || area(next) > area(best) ? next : best, null);
    const source = element?.currentSrc || `${report.title}\u001f${report.artist}`;
    if (source !== bridge.source) {
      bridge.source = source;
      bridge.sessionId = `${performance.timeOrigin}:${Math.random().toString(36).slice(2)}`;
    }
    const position = bridge.position;
    const duration = element?.duration ?? position?.duration ?? 0;
    const seconds = element?.currentTime ?? (position ? position.position + (report.playing ? (performance.now() - position.at) / 1000 * (position.playbackRate ?? 1) : 0) : 0);
    return { ...report, sessionId: bridge.sessionId, durationMs: Number.isFinite(duration) ? Math.max(0, Math.round(duration * 1000)) : 0, positionMs: Number.isFinite(seconds) ? Math.max(0, Math.round(seconds * 1000)) : 0, canSeek: Boolean((element && Number.isFinite(duration) && duration > 0) || bridge.handlers.has('seekto')), canPrevious: bridge.handlers.has('previoustrack'), canNext: bridge.handlers.has('nexttrack') };
  };
  if (typeof post === 'function') return reporter((report) => post(augment(report)));
  const raw = reporter(null);
  return raw ? JSON.stringify(augment(JSON.parse(raw))) : null;
}
