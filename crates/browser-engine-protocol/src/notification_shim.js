// Page-world Notification API for Ferryx browser tabs (both engines).
// The host decides permission per origin and pushes it with __ferryxNotificationPermission when
// a page finished loading; this shim only forwards requests and shows what the host allows.
// `post` is substituted by the host: a function taking one JSON string.
(() => {
  if (window.__ferryxNotificationShim) return;
  Object.defineProperty(window, '__ferryxNotificationShim', { value: true });
  const post = __FERRYX_NOTIFY_POST__;
  const send = (message) => {
    try {
      post(JSON.stringify(message));
    } catch (error) {
      if (typeof console !== 'undefined') console.warn('[ferryx] notification bridge failed', error);
    }
  };
  let permission = 'default';
  const waiting = [];
  Object.defineProperty(window, '__ferryxNotificationPermission', {
    value: (state) => {
      if (state !== 'granted' && state !== 'denied' && state !== 'default') return;
      permission = state;
      while (waiting.length) {
        const resolve = waiting.shift();
        try {
          resolve(state);
        } catch (error) {
          if (typeof console !== 'undefined') console.warn('[ferryx] notification permission callback failed', error);
        }
      }
    },
  });
  const text = (value, limit) => String(value == null ? '' : value).slice(0, limit);
  class FerryxNotification extends EventTarget {
    constructor(title, options) {
      super();
      const opts = options && typeof options === 'object' ? options : {};
      this.title = text(title, 256);
      this.body = text(opts.body, 1024);
      this.tag = text(opts.tag, 128);
      this.onclick = null;
      this.onshow = null;
      this.onclose = null;
      this.onerror = null;
      if (permission !== 'granted') {
        queueMicrotask(() => {
          const event = new Event('error');
          this.dispatchEvent(event);
          if (typeof this.onerror === 'function') this.onerror(event);
        });
        return;
      }
      send({ op: 'show', title: this.title, body: this.body, tag: this.tag });
      queueMicrotask(() => {
        const event = new Event('show');
        this.dispatchEvent(event);
        if (typeof this.onshow === 'function') this.onshow(event);
      });
    }
    close() {
      const event = new Event('close');
      this.dispatchEvent(event);
      if (typeof this.onclose === 'function') this.onclose(event);
    }
    static get permission() {
      return permission;
    }
    static requestPermission(callback) {
      return new Promise((resolve) => {
        const done = (state) => {
          if (typeof callback === 'function') callback(state);
          resolve(state);
        };
        if (permission !== 'default') {
          done(permission);
          return;
        }
        waiting.push(done);
        send({ op: 'request' });
      });
    }
  }
  Object.defineProperty(FerryxNotification, 'maxActions', { value: 0 });
  try {
    Object.defineProperty(window, 'Notification', { value: FerryxNotification, configurable: true, writable: true });
  } catch (error) {
    if (typeof console !== 'undefined') console.warn('[ferryx] could not install Notification', error);
  }
})();
