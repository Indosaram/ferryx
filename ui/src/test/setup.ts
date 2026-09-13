import "@testing-library/jest-dom/vitest";

// NWSAPI delegates these native states back to jsdom's matches(), which itself
// calls NWSAPI. Reject only recursive native delegation; the outer evaluation
// still parses selectors and checks document.fullscreenElement normally.
if (typeof Element !== "undefined") {
  const originalMatches = Element.prototype.matches;
  const activeStateMatches = new WeakMap<Element, Set<string>>();
  Element.prototype.matches = function (selector: string): boolean {
    if (selector !== ":modal" && selector !== ":fullscreen") {
      return originalMatches.call(this, selector);
    }
    const active = activeStateMatches.get(this) ?? new Set<string>();
    if (active.has(selector)) return false;
    active.add(selector);
    activeStateMatches.set(this, active);
    try {
      return originalMatches.call(this, selector);
    } finally {
      active.delete(selector);
      if (active.size === 0) activeStateMatches.delete(this);
    }
  };
}

class ResizeObserverStub implements ResizeObserver {
  disconnect() {}
  observe() {}
  unobserve() {}
}

Object.defineProperty(globalThis, "ResizeObserver", {
  configurable: true,
  value: ResizeObserverStub,
});

if (!globalThis.PointerEvent) {
  if (typeof MouseEvent !== "undefined") {
    Object.defineProperty(globalThis, "PointerEvent", {
      configurable: true,
      value: MouseEvent,
    });
  }
}

// Radix UI JSDOM stubs
if (typeof Element !== "undefined") {
  if (!Element.prototype.hasPointerCapture) {
    Element.prototype.hasPointerCapture = () => false;
  }
  if (!Element.prototype.setPointerCapture) {
    Element.prototype.setPointerCapture = () => {};
  }
  if (!Element.prototype.releasePointerCapture) {
    Element.prototype.releasePointerCapture = () => {};
  }
  if (!Element.prototype.scrollIntoView) {
    Element.prototype.scrollIntoView = () => {};
  }
}

// jsdom does not currently expose DragEvent. Testing Library falls back to a plain Event in
// that case, which silently drops MouseEvent coordinates such as clientX/clientY. Use the
// MouseEvent constructor as the closest available browser primitive so drag/drop tests exercise
// the same edge calculations as the real WebView. Testing Library supplies dataTransfer itself.
if (!globalThis.DragEvent) {
  if (typeof MouseEvent !== "undefined") {
    Object.defineProperty(globalThis, "DragEvent", {
      configurable: true,
      value: MouseEvent,
    });
  }
}

if (!globalThis.requestAnimationFrame) {
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    configurable: true,
    value: (callback: FrameRequestCallback) => globalThis.setTimeout(() => callback(performance.now()), 0),
  });
}

if (!globalThis.cancelAnimationFrame) {
  Object.defineProperty(globalThis, "cancelAnimationFrame", {
    configurable: true,
    value: (handle: number) => globalThis.clearTimeout(handle),
  });
}

const { beforeEach: setupBeforeEach } = await import("vitest");
const { clearHmrWorkspaceState } = await import("../state/hmrWorkspaceState");
const { clearWorkspaceSnapshot } = await import("../state/workspaceSnapshotCache");

setupBeforeEach(() => {
  clearHmrWorkspaceState();
  clearWorkspaceSnapshot();
  // jsdom has no media-query engine; tests that change density provide their own signals.
  if (typeof window !== "undefined" && typeof window.matchMedia !== "function") {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: (media: string): MediaQueryList => Object.assign(new EventTarget(), {
        media,
        matches: false,
        onchange: null,
        addListener() {},
        removeListener() {},
      }),
    });
  }
});
