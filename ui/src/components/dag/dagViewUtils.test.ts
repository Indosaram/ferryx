import { describe, expect, it } from "vitest";
import {
  deriveActiveWaveIndex,
  formatRouteText,
  getNodeStateGlyph,
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
});
