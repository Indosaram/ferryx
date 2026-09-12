import { describe, expect, it } from "vitest";
import {
  calculateEdgePath,
  calculateEffectiveMinScale,
  calculateFitCamera,
  calculateZoomAtAnchor,
  clampScaleWithRecovery,
  deriveActiveWaveIndex,
  formatRouteText,
  getNodeStateGlyph,
  normalizeWheelDeltaPixels,
} from "./dagViewUtils";
import type { DagNodeRoute, DagNodeState, DagRunSnapshot } from "../../lib/dagTypes";

describe("dagViewUtils", () => {
  describe("getNodeStateGlyph", () => {
    it('returns "?" for unknown state', () => {
      expect(getNodeStateGlyph("unknown" as DagNodeState)).toBe("?");
    });

    it("returns correct glyphs for known states", () => {
      expect(getNodeStateGlyph("completed")).toBe("✓");
      expect(getNodeStateGlyph("failed")).toBe("✗");
      expect(getNodeStateGlyph("running")).toBe("▶");
      expect(getNodeStateGlyph("pending")).toBe("◌");
    });
  });

  describe("formatRouteText", () => {
    it('returns "unknown" when route kind is unknown', () => {
      expect(formatRouteText({ kind: "unknown" } as unknown as DagNodeRoute)).toBe("unknown");
    });

    it("returns formatted category route", () => {
      expect(formatRouteText({ kind: "category", category: "backend" })).toBe("category:backend");
    });

    it("returns formatted agent route", () => {
      expect(formatRouteText({ kind: "agent", agent: "coder" })).toBe("agent:coder");
    });
  });

  describe("deriveActiveWaveIndex", () => {
    it("returns correct sorted wave index when waves are unsorted", () => {
      const run: DagRunSnapshot = {
        runId: "run-1",
        name: "test-run",
        status: "running",
        nodes: [
          { id: "node-0", label: "Node 0", state: "completed", dependsOn: [], attempt: 1, route: { kind: "unknown" }, startedAt: null, completedAt: null, error: null, taskId: null },
          { id: "node-1", label: "Node 1", state: "running", dependsOn: ["node-0"], attempt: 1, route: { kind: "unknown" }, startedAt: null, completedAt: null, error: null, taskId: null },
        ],
        waves: [
          { index: 1, nodeIds: ["node-1"] },
          { index: 0, nodeIds: ["node-0"] },
        ],
        counts: { total: 2, completed: 1, running: 1, failed: 0, cancelled: 0, skipped: 0 },
        runKey: "key-1",
        startedAt: null,
        completedAt: null,
        updatedAt: null,
        amendCount: 0,
        edges: [],
        criticalPath: [],
        bottlenecks: [],
      };

      // In unsorted array, wave with index 1 is at [0] and wave with index 0 is at [1].
      // Sorted waves: [wave0, wave1].
      // wave1 (running) is at sorted index 1.
      expect(deriveActiveWaveIndex(run)).toBe(1);
    });

    it("returns wave index with running node even if unsorted order would place it differently", () => {
      const run: DagRunSnapshot = {
        runId: "run-2",
        name: "test-run-2",
        status: "running",
        nodes: [
          { id: "node-0", label: "Node 0", state: "running", dependsOn: [], attempt: 1, route: { kind: "unknown" }, startedAt: null, completedAt: null, error: null, taskId: null },
          { id: "node-1", label: "Node 1", state: "pending", dependsOn: ["node-0"], attempt: 1, route: { kind: "unknown" }, startedAt: null, completedAt: null, error: null, taskId: null },
        ],
        waves: [
          { index: 1, nodeIds: ["node-1"] },
          { index: 0, nodeIds: ["node-0"] },
        ],
        counts: { total: 2, completed: 0, running: 1, failed: 0, cancelled: 0, skipped: 0 },
        runKey: "key-2",
        startedAt: null,
        completedAt: null,
        updatedAt: null,
        amendCount: 0,
        edges: [],
        criticalPath: [],
        bottlenecks: [],
      };

      // Sorted waves: [wave0 (running), wave1 (pending)].
      // Active wave should be sorted index 0.
      expect(deriveActiveWaveIndex(run)).toBe(0);
    });

    it("returns sorted wave index for active nodes when none are running", () => {
      const run: DagRunSnapshot = {
        runId: "run-3",
        name: "test-run-3",
        status: "running",
        nodes: [
          { id: "node-0", label: "Node 0", state: "completed", dependsOn: [], attempt: 1, route: { kind: "unknown" }, startedAt: null, completedAt: null, error: null, taskId: null },
          { id: "node-1", label: "Node 1", state: "scheduled", dependsOn: ["node-0"], attempt: 1, route: { kind: "unknown" }, startedAt: null, completedAt: null, error: null, taskId: null },
        ],
        waves: [
          { index: 1, nodeIds: ["node-1"] },
          { index: 0, nodeIds: ["node-0"] },
        ],
        counts: { total: 2, completed: 1, running: 0, failed: 0, cancelled: 0, skipped: 0 },
        runKey: "key-3",
        startedAt: null,
        completedAt: null,
        updatedAt: null,
        amendCount: 0,
        edges: [],
        criticalPath: [],
        bottlenecks: [],
      };

      expect(deriveActiveWaveIndex(run)).toBe(1);
    });

    it("handles empty or missing waves", () => {
      const run: DagRunSnapshot = {
        runId: "run-4",
        name: "test-run-4",
        status: "running",
        nodes: [],
        waves: [],
        counts: { total: 0, completed: 0, running: 0, failed: 0, cancelled: 0, skipped: 0 },
        runKey: "key-4",
        startedAt: null,
        completedAt: null,
        updatedAt: null,
        amendCount: 0,
        edges: [],
        criticalPath: [],
        bottlenecks: [],
      };

      expect(deriveActiveWaveIndex(run)).toBe(0);
    });
  });

  describe("calculateEdgePath", () => {
    it("draws direct horizontal S-curve for adjacent columns", () => {
      const fromPos = { x: 32, y: 44 };
      const toPos = { x: 324, y: 44 };
      const path = calculateEdgePath(fromPos, toPos);
      expect(path).toMatch(/^M 252 72 C \d+(\.\d+)? 72, \d+(\.\d+)? 72, 324 72$/);
    });

    it("draws an upward arc for multi-hop edges that skip columns", () => {
      const fromPos = { x: 32, y: 44 };
      const toPos = { x: 908, y: 44 }; // skips 2 intermediate columns (colSpan 3)
      const path = calculateEdgePath(fromPos, toPos);
      // y1 is 72, control point cy should be significantly above 72 (e.g. <= 20)
      const match = path.match(/^M 252 72 C [0-9.]+ (-?[0-9.]+), [0-9.]+ (-?[0-9.]+), 908 72$/);
      expect(match).not.toBeNull();
      const cy1 = Number(match![1]);
      const cy2 = Number(match![2]);
      expect(cy1).toBeLessThan(44); // strictly above intermediate card top (y=44)
      expect(cy2).toBeLessThan(44);
    });

    it("distributes target ports when multiple edges enter the same node", () => {
      const target = { x: 908, y: 44 };
      const p1 = calculateEdgePath({ x: 32, y: 44 }, target, { targetIndex: 0, totalTargets: 3 });
      const p2 = calculateEdgePath({ x: 324, y: 44 }, target, { targetIndex: 1, totalTargets: 3 });
      const p3 = calculateEdgePath({ x: 616, y: 44 }, target, { targetIndex: 2, totalTargets: 3 });

      // Each path must end at a different target Y
      const endY1 = Number(p1.split(" ").slice(-1)[0]);
      const endY2 = Number(p2.split(" ").slice(-1)[0]);
      const endY3 = Number(p3.split(" ").slice(-1)[0]);

      expect(endY1).toBeLessThan(endY2);
      expect(endY2).toBeLessThan(endY3);
    });
  });

  describe("Camera math functions", () => {
    it("calculateFitCamera centers and bounds content within viewport with margin", () => {
      const cam = calculateFitCamera(1000, 700, 1160, 284);
      // margin = 24. fitScale = min(1, 952/1160, 652/284) = 952/1160 = 0.820689...
      expect(cam.scale).toBeCloseTo(0.8207, 3);
      expect(cam.x).toBeCloseTo(24, 1);
      expect(cam.y).toBeGreaterThan(0);
    });

    it("calculateFitCamera does not upscale content smaller than viewport", () => {
      const cam = calculateFitCamera(1000, 700, 200, 100);
      expect(cam.scale).toBe(1.0);
      expect(cam.x).toBe((1000 - 200) / 2);
      expect(cam.y).toBe((700 - 100) / 2);
    });

    it("calculateFitCamera handles zero and negative dimensions safely", () => {
      expect(calculateFitCamera(0, 700, 100, 100)).toEqual({ x: 0, y: 0, scale: 1 });
      expect(calculateFitCamera(1000, 0, 100, 100)).toEqual({ x: 0, y: 0, scale: 1 });
      expect(calculateFitCamera(1000, 700, 0, 100)).toEqual({ x: 0, y: 0, scale: 1 });
    });

    it("calculateEffectiveMinScale floors at min(0.1, fitScale)", () => {
      expect(calculateEffectiveMinScale(0.5)).toBe(0.1);
      expect(calculateEffectiveMinScale(0.05)).toBe(0.05);
    });

    it("calculateZoomAtAnchor maintains cursor anchor world coordinates", () => {
      const initialCam = { x: 100, y: 50, scale: 1.0 };
      const cursor = { x: 500, y: 350 };
      // World point under cursor: wX = (500 - 100) / 1 = 400, wY = (350 - 50) / 1 = 300
      const zoomed = calculateZoomAtAnchor(initialCam, 1.5, cursor, 0.1, 3.0);
      expect(zoomed.scale).toBe(1.5);
      const newScreenX = zoomed.x + 1.5 * 400;
      const newScreenY = zoomed.y + 1.5 * 300;
      expect(newScreenX).toBeCloseTo(cursor.x, 2);
      expect(newScreenY).toBeCloseTo(cursor.y, 2);
    });

    it("calculateZoomAtAnchor clamps to limits without drift", () => {
      const atMax = { x: 100, y: 50, scale: 3.0 };
      const cursor = { x: 500, y: 350 };
      const overMax = calculateZoomAtAnchor(atMax, 3.5, cursor, 0.1, 3.0);
      expect(overMax.scale).toBe(3.0);
      expect(overMax.x).toBe(atMax.x);
      expect(overMax.y).toBe(atMax.y);
    });

    it("clampScaleWithRecovery obeys direction-preserving recovery below min and above max", () => {
      // Below minScale (0.1)
      expect(clampScaleWithRecovery(0.05, 0.04, 0.1)).toBe(0.05); // outward: no-op
      expect(clampScaleWithRecovery(0.05, 0.08, 0.1)).toBe(0.08); // inward: advances toward min
      expect(clampScaleWithRecovery(0.05, 0.15, 0.1)).toBe(0.1); // inward past min: clamps to min

      // Above maxScale (3.0)
      expect(clampScaleWithRecovery(3.5, 3.8, 0.1, 3.0)).toBe(3.5); // outward (larger): no-op
      expect(clampScaleWithRecovery(3.5, 3.2, 0.1, 3.0)).toBe(3.2); // inward: advances toward max
      expect(clampScaleWithRecovery(3.5, 2.5, 0.1, 3.0)).toBe(3.0); // inward past max: clamps to max

      // Within range
      expect(clampScaleWithRecovery(1.0, 1.2, 0.1, 3.0)).toBe(1.2);
      expect(clampScaleWithRecovery(1.0, 0.05, 0.1, 3.0)).toBe(0.1);
      expect(clampScaleWithRecovery(1.0, 3.5, 0.1, 3.0)).toBe(3.0);
    });

    it("normalizeWheelDeltaPixels scales lines and pages properly", () => {
      expect(normalizeWheelDeltaPixels(10, 0, 800)).toBe(10);
      expect(normalizeWheelDeltaPixels(2, 1, 800)).toBe(32);
      expect(normalizeWheelDeltaPixels(1, 2, 800)).toBe(800);
    });
  });
});
