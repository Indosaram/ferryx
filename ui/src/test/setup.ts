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
