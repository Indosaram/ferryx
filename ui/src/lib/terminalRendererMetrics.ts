export type WebglLifecycleCounters = {
  created: number;
  active: number;
  disposed: number;
  contextLosses: number;
  loadFailures: number;
  canvasFallbacks: number;
};

declare global {
  var __ORCA_WEBGL_LIFECYCLE__: Readonly<WebglLifecycleCounters> | undefined;
}

const counters: WebglLifecycleCounters = {
  created: 0,
  active: 0,
  disposed: 0,
  contextLosses: 0,
  loadFailures: 0,
  canvasFallbacks: 0,
};

export function getWebglLifecycleCounters(): Readonly<WebglLifecycleCounters> {
  return { ...counters };
}

export function recordWebglCreated() {
  counters.created += 1;
  counters.active += 1;
  publish();
}

export function recordWebglDisposed() {
  counters.disposed += 1;
  counters.active = Math.max(0, counters.active - 1);
  publish();
}

export function recordWebglContextLoss() {
  counters.contextLosses += 1;
  publish();
}

export function recordWebglLoadFailure() {
  counters.loadFailures += 1;
  publish();
}

export function recordCanvasFallback() {
  counters.canvasFallbacks += 1;
  publish();
}

export function resetWebglLifecycleCountersForTests() {
  counters.created = 0;
  counters.active = 0;
  counters.disposed = 0;
  counters.contextLosses = 0;
  counters.loadFailures = 0;
  counters.canvasFallbacks = 0;
  publish();
}

function publish() {
  globalThis.__ORCA_WEBGL_LIFECYCLE__ = Object.freeze(getWebglLifecycleCounters());
}

publish();
