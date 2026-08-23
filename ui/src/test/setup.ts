import "@testing-library/jest-dom/vitest";

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

setupBeforeEach(() => {
  clearHmrWorkspaceState();
});
