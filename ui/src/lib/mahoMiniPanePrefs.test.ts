// Maho Mini / Peek geometry persistence: the card's size and place across opens.
//
// The whole point of these cases is that a bad stored value is never fatal and never puts the
// close button off-screen: the pane reads geometry on mount and degrades to the docked default.

import { afterEach, describe, expect, it, vi } from "vitest";

import { MAHO_MINI_PANE_PREFS_STORAGE_KEY } from "./storageKeys";
import {
  DEFAULT_MINI_PANE_GEOMETRY,
  MAX_MINI_PANE_HEIGHT,
  MAX_MINI_PANE_WIDTH,
  MINI_PANE_VIEWPORT_MARGIN,
  MIN_MINI_PANE_HEIGHT,
  MIN_MINI_PANE_WIDTH,
  clampMiniPaneGeometry,
  isDockedGeometry,
  loadMahoMiniPaneGeometry,
  parseMiniPaneGeometry,
  resetMahoMiniPaneGeometry,
  saveMahoMiniPaneGeometry,
  type MahoMiniPaneGeometry,
} from "./mahoMiniPanePrefs";

/** A storage shim so no case touches the real localStorage. */
function memoryStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
    removeItem: (key: string) => {
      map.delete(key);
    },
    raw: map,
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("loadMahoMiniPaneGeometry", () => {
  it("opens docked at the default size when nothing is stored", () => {
    const geometry = loadMahoMiniPaneGeometry(memoryStorage(), { width: 1400, height: 900 });
    expect(geometry.width).toBe(DEFAULT_MINI_PANE_GEOMETRY.width);
    expect(geometry.height).toBe(DEFAULT_MINI_PANE_GEOMETRY.height);
    expect(isDockedGeometry(geometry)).toBe(true);
  });

  it("round-trips a saved geometry through the storage key", () => {
    const storage = memoryStorage();
    const saved: MahoMiniPaneGeometry = { width: 420, height: 610, x: 120, y: 96 };
    saveMahoMiniPaneGeometry(saved, storage);

    expect(storage.raw.has(MAHO_MINI_PANE_PREFS_STORAGE_KEY)).toBe(true);
    expect(loadMahoMiniPaneGeometry(storage, { width: 1400, height: 900 })).toEqual(saved);
  });

  it("reads a malformed, non-object or non-numeric blob as the docked default", () => {
    for (const raw of ["", "not json", "[]", "null", '"str"', '{"width":"wide"}', '{"width":null,"x":{}}']) {
      const storage = memoryStorage({ [MAHO_MINI_PANE_PREFS_STORAGE_KEY]: raw });
      const geometry = loadMahoMiniPaneGeometry(storage, { width: 1400, height: 900 });
      expect(geometry.width).toBe(DEFAULT_MINI_PANE_GEOMETRY.width);
      expect(geometry.height).toBe(DEFAULT_MINI_PANE_GEOMETRY.height);
      expect(isDockedGeometry(geometry)).toBe(true);
    }
  });

  it("clamps a card saved on a huge display back onto a small one", () => {
    const storage = memoryStorage({
      [MAHO_MINI_PANE_PREFS_STORAGE_KEY]: JSON.stringify({ width: 1200, height: 1600, x: 3400, y: 2000 }),
    });
    const geometry = loadMahoMiniPaneGeometry(storage, { width: 1280, height: 800 });

    expect(geometry.width).toBeLessThanOrEqual(1280 - MINI_PANE_VIEWPORT_MARGIN * 2);
    expect(geometry.height).toBeLessThanOrEqual(800 - MINI_PANE_VIEWPORT_MARGIN * 2);
    // The whole card stays on screen, margin included.
    expect(geometry.x + geometry.width).toBeLessThanOrEqual(1280);
    expect(geometry.y + geometry.height).toBeLessThanOrEqual(800);
    expect(geometry.x).toBeGreaterThanOrEqual(MINI_PANE_VIEWPORT_MARGIN);
    expect(geometry.y).toBeGreaterThanOrEqual(MINI_PANE_VIEWPORT_MARGIN);
  });

  it("does not throw when storage itself throws", () => {
    const hostile = {
      getItem: () => {
        throw new Error("storage disabled");
      },
    };
    expect(loadMahoMiniPaneGeometry(hostile, null)).toEqual(DEFAULT_MINI_PANE_GEOMETRY);
    expect(loadMahoMiniPaneGeometry(null, null)).toEqual(DEFAULT_MINI_PANE_GEOMETRY);
  });
});

describe("saveMahoMiniPaneGeometry", () => {
  it("persists the clamped value, not the raw one", () => {
    const storage = memoryStorage();
    saveMahoMiniPaneGeometry({ width: 5000, height: 1, x: -1, y: -1 }, storage);
    const stored = JSON.parse(String(storage.raw.get(MAHO_MINI_PANE_PREFS_STORAGE_KEY))) as MahoMiniPaneGeometry;
    expect(stored.width).toBe(MAX_MINI_PANE_WIDTH);
    expect(stored.height).toBe(MIN_MINI_PANE_HEIGHT);
  });

  it("swallows a storage failure instead of breaking the drag", () => {
    const hostile = {
      setItem: () => {
        throw new Error("quota");
      },
    };
    expect(() => saveMahoMiniPaneGeometry({ width: 400, height: 500, x: -1, y: -1 }, hostile)).not.toThrow();
    expect(() => saveMahoMiniPaneGeometry({ width: 400, height: 500, x: -1, y: -1 }, null)).not.toThrow();
  });
});

describe("resetMahoMiniPaneGeometry", () => {
  it("drops the stored value so the next open is docked again", () => {
    const storage = memoryStorage();
    saveMahoMiniPaneGeometry({ width: 500, height: 600, x: 40, y: 40 }, storage);
    resetMahoMiniPaneGeometry(storage);
    expect(storage.raw.has(MAHO_MINI_PANE_PREFS_STORAGE_KEY)).toBe(false);
    expect(isDockedGeometry(loadMahoMiniPaneGeometry(storage, { width: 1200, height: 800 }))).toBe(true);
  });
});

describe("clampMiniPaneGeometry", () => {
  it("keeps an in-band size and the negative docked sentinel untouched", () => {
    expect(clampMiniPaneGeometry({ width: 400, height: 500, x: -1, y: -1 }, null)).toEqual({
      width: 400,
      height: 500,
      x: -1,
      y: -1,
    });
  });

  it("bounds the size to its usable band", () => {
    expect(clampMiniPaneGeometry({ width: 10, height: 10, x: -1, y: -1 }, null))
      .toMatchObject({ width: MIN_MINI_PANE_WIDTH, height: MIN_MINI_PANE_HEIGHT });
    expect(clampMiniPaneGeometry({ width: 9999, height: 9999, x: -1, y: -1 }, null))
      .toMatchObject({ width: MAX_MINI_PANE_WIDTH, height: MAX_MINI_PANE_HEIGHT });
  });

  it("keeps a moved card inside the viewport even when the card fills it", () => {
    const geometry = clampMiniPaneGeometry({ width: 700, height: 800, x: 5, y: 5 }, { width: 720, height: 820 });
    expect(geometry.width).toBeLessThanOrEqual(720 - MINI_PANE_VIEWPORT_MARGIN * 2);
    expect(geometry.x).toBe(MINI_PANE_VIEWPORT_MARGIN);
    expect(geometry.x + geometry.width).toBeLessThanOrEqual(720);
  });

  it("survives NaN and non-numeric fields", () => {
    expect(clampMiniPaneGeometry({ width: Number.NaN, height: Number.POSITIVE_INFINITY, x: -1, y: -1 }, null))
      .toEqual(DEFAULT_MINI_PANE_GEOMETRY);
  });
});

describe("parseMiniPaneGeometry", () => {
  it("reads a partial blob by filling the missing fields with the defaults", () => {
    expect(parseMiniPaneGeometry('{"width":360}')).toMatchObject({
      width: 360,
      height: DEFAULT_MINI_PANE_GEOMETRY.height,
      x: -1,
      y: -1,
    });
  });
});
