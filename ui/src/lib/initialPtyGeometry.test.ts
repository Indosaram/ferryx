import { afterEach, describe, expect, it, vi } from "vitest";
import {
  computeInitialPtyGrid,
  initialGeometryElement,
  initialGridFields,
  measureInitialPtyGrid,
} from "./initialPtyGeometry";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: () => false }));

function elementWithRect(rect: { x: number; y: number; width: number; height: number }): Element {
  const element = document.createElement("div");
  element.getBoundingClientRect = () => ({ ...rect, top: rect.y, left: rect.x, right: rect.x + rect.width, bottom: rect.y + rect.height, toJSON: () => rect }) as DOMRect;
  return element;
}

describe("computeInitialPtyGrid", () => {
  it("floors snapped physical pane size by native cell metrics", () => {
    // 800x600 CSS px at 2x = 1600x1200 physical; 16x34 physical cells.
    expect(computeInitialPtyGrid({ x: 0, y: 0, width: 800, height: 600 }, 2, { widthPx: 16, heightPx: 34 }))
      .toEqual({ cols: 100, rows: 35 });
  });

  it("snaps fractional split edges before dividing, matching the native layout", () => {
    // Edges snap to 320 and 1271 at 1x: width 951 px / 9 px = 105 columns.
    expect(computeInitialPtyGrid({ x: 319.6, y: 44, width: 951.4, height: 900 }, 1, { widthPx: 9, heightPx: 18 }))
      .toEqual({ cols: 105, rows: 50 });
  });

  it.each([
    ["zero width", { x: 0, y: 0, width: 0, height: 600 }, 1, { widthPx: 8, heightPx: 16 }],
    ["sub-cell pane", { x: 0, y: 0, width: 5, height: 600 }, 1, { widthPx: 8, heightPx: 16 }],
    ["zero cell", { x: 0, y: 0, width: 800, height: 600 }, 1, { widthPx: 0, heightPx: 16 }],
    ["bad scale", { x: 0, y: 0, width: 800, height: 600 }, Number.NaN, { widthPx: 8, heightPx: 16 }],
    ["over limit", { x: 0, y: 0, width: 9000, height: 600 }, 1, { widthPx: 8, heightPx: 16 }],
  ])("returns null for %s", (_label, rect, scale, cell) => {
    expect(computeInitialPtyGrid(rect, scale, cell)).toBeNull();
  });
});

describe("measureInitialPtyGrid", () => {
  afterEach(() => { document.body.innerHTML = ""; });

  it("uses the element rect and native metrics at the window scale", async () => {
    vi.stubGlobal("devicePixelRatio", 2);
    const fetchMetrics = vi.fn(async () => ({ widthPx: 16, heightPx: 32 }));
    await expect(measureInitialPtyGrid(elementWithRect({ x: 10, y: 20, width: 400, height: 320 }), fetchMetrics))
      .resolves.toEqual({ cols: 50, rows: 20 });
    expect(fetchMetrics).toHaveBeenCalledWith(2);
    vi.unstubAllGlobals();
  });

  it("reports unavailable without guessing when element or metrics are missing", async () => {
    const fetchMetrics = vi.fn(async () => null);
    await expect(measureInitialPtyGrid(null, fetchMetrics)).resolves.toBeNull();
    expect(fetchMetrics).not.toHaveBeenCalled();
    await expect(measureInitialPtyGrid(elementWithRect({ x: 0, y: 0, width: 400, height: 300 }), fetchMetrics))
      .resolves.toBeNull();
    await expect(measureInitialPtyGrid(elementWithRect({ x: 0, y: 0, width: 0, height: 0 }), vi.fn(async () => ({ widthPx: 8, heightPx: 16 }))))
      .resolves.toBeNull();
  });

  it("returns null outside the desktop runtime through the default metrics fetcher", async () => {
    await expect(measureInitialPtyGrid(elementWithRect({ x: 0, y: 0, width: 400, height: 300 }))).resolves.toBeNull();
  });
});

describe("initialGeometryElement", () => {
  afterEach(() => { document.body.innerHTML = ""; });

  it("finds the session viewport and the tab group body", () => {
    document.body.innerHTML = `
      <div data-testid="native-terminal-viewport" data-frontend-session-id="session-a"></div>
      <div data-tab-group-body-id="group-1"></div>`;
    expect(initialGeometryElement({ kind: "session", sessionId: "session-a" })?.getAttribute("data-frontend-session-id")).toBe("session-a");
    expect(initialGeometryElement({ kind: "session", sessionId: "session-b" })).toBeNull();
    expect(initialGeometryElement({ kind: "newTab", groupId: "group-1" })?.getAttribute("data-tab-group-body-id")).toBe("group-1");
    expect(initialGeometryElement({ kind: "newTab", groupId: null })).toBeNull();
  });
});

describe("initialGridFields", () => {
  it("adds fields only for measured geometry", () => {
    expect(initialGridFields({ cols: 120, rows: 40 })).toEqual({ cols: 120, rows: 40 });
    expect(initialGridFields(null)).toEqual({});
    expect("cols" in initialGridFields(undefined)).toBe(false);
  });
});
