import "@testing-library/jest-dom/vitest";

// NWSAPI does not implement :modal or :fullscreen. We delegate top-layer
// states without recursive loops while preserving compound selectors and
// full document.fullscreenElement state tracking.
if (typeof Element !== "undefined") {
  const originalMatches = Element.prototype.matches;
  const activeStateMatches = new WeakMap<Element, Set<string>>();

  function isFullscreen(el: Element): boolean {
    const doc = el.ownerDocument ?? (typeof document !== "undefined" ? document : null);
    return Boolean(doc && doc.fullscreenElement === el);
  }

  function isModal(el: Element): boolean {
    if (el.matches(":fullscreen")) return true;
    if (el.tagName === "DIALOG" && (el as HTMLDialogElement).open) {
      return el.getAttribute("aria-modal") === "true";
    }
    return false;
  }

  Element.prototype.matches = function (selector: string): boolean {
    if (!selector.includes(":modal") && !selector.includes(":fullscreen")) {
      return originalMatches.call(this, selector);
    }

    const active = activeStateMatches.get(this) ?? new Set<string>();
    if (active.has(selector)) return false;
    active.add(selector);
    activeStateMatches.set(this, active);

    try {
      if (selector === ":fullscreen") {
        return isFullscreen(this);
      }
      if (selector === ":modal") {
        return isModal(this);
      }

      if (selector.includes(",")) {
        return selector.split(",").some((part) => this.matches(part.trim()));
      }

      let currentSelector = selector;

      if (currentSelector.includes(":not(:fullscreen)")) {
        if (this.matches(":fullscreen")) return false;
        currentSelector = currentSelector.replaceAll(":not(:fullscreen)", "");
      }
      if (currentSelector.includes(":not(:modal)")) {
        if (this.matches(":modal")) return false;
        currentSelector = currentSelector.replaceAll(":not(:modal)", "");
      }

      if (currentSelector.includes(":fullscreen")) {
        if (!this.matches(":fullscreen")) return false;
        currentSelector = currentSelector.replaceAll(":fullscreen", "");
      }
      if (currentSelector.includes(":modal")) {
        if (!this.matches(":modal")) return false;
        currentSelector = currentSelector.replaceAll(":modal", "");
      }

      const remainder = currentSelector.trim();
      if (!remainder || remainder === "*") return true;
      return originalMatches.call(this, remainder);
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

// Deterministic capture delivery for JSDOM. Synthetic dispatch represents platform
// pointer input here; capture is applied before DOM propagation. Disconnection
// deliberately does not erase ownership: tests must observe explicit cleanup.
if (typeof Element !== "undefined") {
  const pointerCaptures = new Map<number, Element>();
  const dispatchEvent = EventTarget.prototype.dispatchEvent;

  function emitCaptureLoss(owner: Element, pointerId: number) {
    // The MouseEvent fallback ignores PointerEventInit.pointerId.
    const event = new PointerEvent("lostpointercapture", { bubbles: true, pointerId });
    Object.defineProperty(event, "pointerId", { value: pointerId });
    owner.dispatchEvent(event);
  }

  EventTarget.prototype.dispatchEvent = function (event: Event) {
    if ((event.type === "pointermove" || event.type === "pointerup" || event.type === "pointercancel") &&
        "pointerId" in event && typeof event.pointerId === "number") {
      const owner = pointerCaptures.get(event.pointerId);
      const result = dispatchEvent.call(owner ?? this, event);
      if (event.type !== "pointermove") {
        pointerCaptures.get(event.pointerId)?.releasePointerCapture(event.pointerId);
      }
      return result;
    }
    return dispatchEvent.call(this, event);
  };

  Element.prototype.hasPointerCapture = function (pointerId: number) {
    return pointerCaptures.get(pointerId) === this;
  };

  Element.prototype.setPointerCapture = function (pointerId: number) {
    if (!this.isConnected && typeof document !== "undefined" && !document.contains(this)) {
      throw new DOMException("The element is not connected to the document.", "InvalidStateError");
    }
    const prev = pointerCaptures.get(pointerId);
    if (prev && prev !== this) {
      pointerCaptures.delete(pointerId);
      emitCaptureLoss(prev, pointerId);
    }
    pointerCaptures.set(pointerId, this);
  };

  Element.prototype.releasePointerCapture = function (pointerId: number) {
    if (pointerCaptures.get(pointerId) === this) {
      pointerCaptures.delete(pointerId);
      emitCaptureLoss(this, pointerId);
    }
  };

  (globalThis as unknown as { __clearPointerCaptures?: () => void }).__clearPointerCaptures = () => {
    pointerCaptures.clear();
  };

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
  const clearCaptures = (globalThis as unknown as { __clearPointerCaptures?: () => void }).__clearPointerCaptures;
  if (clearCaptures) clearCaptures();
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
