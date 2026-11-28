// Maho Mini / Peek pane geometry: the card's own size and place, remembered across opens.
//
// Maho's Peek is a floating card over the canvas edge, not a layout column: the user can size it
// and slide it aside, and expects that to stick. This module is the only place that knows how a
// stored geometry is read: a missing, malformed or out-of-band value degrades to the docked
// default instead of throwing, and every read is clamped against the current viewport so a card
// saved on a 4K display still lands on a laptop screen with its close button reachable.
//
// Pure and injectable (`storage`, `viewport`) so a test never touches real localStorage.

import { MAHO_MINI_PANE_PREFS_STORAGE_KEY } from "./storageKeys";

/** The docked card's size: wide enough for a page's text column, short enough to leave the canvas. */
export const DEFAULT_MINI_PANE_WIDTH = 380;
export const DEFAULT_MINI_PANE_HEIGHT = 520;
export const MIN_MINI_PANE_WIDTH = 280;
export const MAX_MINI_PANE_WIDTH = 760;
export const MIN_MINI_PANE_HEIGHT = 200;
export const MAX_MINI_PANE_HEIGHT = 900;
/** Room the card keeps from every viewport edge, so its drag strip is never off-screen. */
export const MINI_PANE_VIEWPORT_MARGIN = 12;

/** Where the card sits and how big it is. `x`/`y` are viewport pixels from the top-left. */
export type MahoMiniPaneGeometry = {
  width: number;
  height: number;
  x: number;
  y: number;
};

export type MiniPaneViewport = { width: number; height: number };

/**
 * The card's opening geometry: docked to the canvas's right edge, vertically at the top of the
 * page area. `x` is resolved by the caller against the canvas width, so the stored value is the
 * offset the user last dragged it to rather than an absolute seat that breaks on a resize.
 */
export const DEFAULT_MINI_PANE_GEOMETRY: MahoMiniPaneGeometry = {
  width: DEFAULT_MINI_PANE_WIDTH,
  height: DEFAULT_MINI_PANE_HEIGHT,
  x: -1,
  y: -1,
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function defaultStorage(): Pick<Storage, "getItem" | "setItem"> | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    // Disabled storage keeps the in-memory geometry for this session.
    return null;
  }
}

/** Reads a finite number, or `fallback` for a missing/non-numeric/absurd value. */
function finiteOr(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

/**
 * Keeps a geometry usable: size inside its band, position inside the viewport with a margin.
 * A viewport too small for the stored card shrinks the card instead of pushing it off-screen.
 */
export function clampMiniPaneGeometry(
  geometry: Partial<MahoMiniPaneGeometry> | null | undefined,
  viewport: MiniPaneViewport | null = null,
): MahoMiniPaneGeometry {
  const rawWidth = finiteOr(geometry?.width, DEFAULT_MINI_PANE_WIDTH);
  const rawHeight = finiteOr(geometry?.height, DEFAULT_MINI_PANE_HEIGHT);
  const rawX = finiteOr(geometry?.x, DEFAULT_MINI_PANE_GEOMETRY.x);
  const rawY = finiteOr(geometry?.y, DEFAULT_MINI_PANE_GEOMETRY.y);

  const maxWidth = viewport && viewport.width > 0
    ? Math.max(MIN_MINI_PANE_WIDTH, Math.min(MAX_MINI_PANE_WIDTH, viewport.width - MINI_PANE_VIEWPORT_MARGIN * 2))
    : MAX_MINI_PANE_WIDTH;
  const maxHeight = viewport && viewport.height > 0
    ? Math.max(MIN_MINI_PANE_HEIGHT, Math.min(MAX_MINI_PANE_HEIGHT, viewport.height - MINI_PANE_VIEWPORT_MARGIN * 2))
    : MAX_MINI_PANE_HEIGHT;

  const width = clamp(Math.round(rawWidth), MIN_MINI_PANE_WIDTH, maxWidth);
  const height = clamp(Math.round(rawHeight), MIN_MINI_PANE_HEIGHT, maxHeight);

  // A negative x/y is the "docked" sentinel, not a position: only clamp a real one.
  const maxX = viewport && viewport.width > 0 ? Math.max(0, viewport.width - width - MINI_PANE_VIEWPORT_MARGIN) : null;
  const maxY = viewport && viewport.height > 0 ? Math.max(0, viewport.height - height - MINI_PANE_VIEWPORT_MARGIN) : null;
  const x = rawX < 0 ? DEFAULT_MINI_PANE_GEOMETRY.x : (maxX === null ? Math.round(rawX) : clamp(Math.round(rawX), MINI_PANE_VIEWPORT_MARGIN, maxX));
  const y = rawY < 0 ? DEFAULT_MINI_PANE_GEOMETRY.y : (maxY === null ? Math.round(rawY) : clamp(Math.round(rawY), MINI_PANE_VIEWPORT_MARGIN, maxY));

  return { width, height, x, y };
}

/** True when the stored geometry is the docked default, i.e. the card was never dragged. */
export function isDockedGeometry(geometry: MahoMiniPaneGeometry): boolean {
  return geometry.x < 0 || geometry.y < 0;
}

/** Parses one stored JSON blob; every non-conforming shape reads as the default. */
export function parseMiniPaneGeometry(raw: string | null): MahoMiniPaneGeometry {
  if (!raw) return DEFAULT_MINI_PANE_GEOMETRY;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return DEFAULT_MINI_PANE_GEOMETRY;
    const record = parsed as Record<string, unknown>;
    return clampMiniPaneGeometry({
      width: finiteOr(record.width, DEFAULT_MINI_PANE_WIDTH),
      height: finiteOr(record.height, DEFAULT_MINI_PANE_HEIGHT),
      x: finiteOr(record.x, DEFAULT_MINI_PANE_GEOMETRY.x),
      y: finiteOr(record.y, DEFAULT_MINI_PANE_GEOMETRY.y),
    });
  } catch {
    return DEFAULT_MINI_PANE_GEOMETRY;
  }
}

/**
 * The stored geometry, clamped to `viewport` when one is given.
 *
 * `viewport` is optional because a caller that has not measured the canvas yet still wants the
 * size; it can clamp again once it knows the box.
 */
export function loadMahoMiniPaneGeometry(
  storage: Pick<Storage, "getItem"> | null = defaultStorage(),
  viewport: MiniPaneViewport | null = null,
): MahoMiniPaneGeometry {
  if (!storage) return clampMiniPaneGeometry(DEFAULT_MINI_PANE_GEOMETRY, viewport);
  try {
    return clampMiniPaneGeometry(parseMiniPaneGeometry(storage.getItem(MAHO_MINI_PANE_PREFS_STORAGE_KEY)), viewport);
  } catch {
    return clampMiniPaneGeometry(DEFAULT_MINI_PANE_GEOMETRY, viewport);
  }
}

/** Persists the geometry; a storage failure keeps the in-memory card working for this session. */
export function saveMahoMiniPaneGeometry(
  geometry: MahoMiniPaneGeometry,
  storage: Pick<Storage, "setItem"> | null = defaultStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(MAHO_MINI_PANE_PREFS_STORAGE_KEY, JSON.stringify(clampMiniPaneGeometry(geometry)));
  } catch {
    // A full or disabled storage must not break dragging.
  }
}

/** Drops the stored geometry: the next open is the docked default again. */
export function resetMahoMiniPaneGeometry(
  storage: (Pick<Storage, "setItem"> & Partial<Pick<Storage, "removeItem">>) | null = defaultStorage(),
): void {
  if (!storage) return;
  try {
    storage.removeItem?.(MAHO_MINI_PANE_PREFS_STORAGE_KEY);
    if (!storage.removeItem) storage.setItem(MAHO_MINI_PANE_PREFS_STORAGE_KEY, JSON.stringify(DEFAULT_MINI_PANE_GEOMETRY));
  } catch {
    // ignore disabled storage
  }
}
