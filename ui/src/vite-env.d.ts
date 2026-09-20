/// <reference types="vite/client" />

/// Build identity injected by vite.config.ts: version, source revision, build clock. Shown in the
/// remote client so a stale phone bundle is visible at a glance instead of guessed at.
declare const __FERRYX_BUILD__: string;

declare module "jsdom";
