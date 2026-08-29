import { describe, expect, it } from "vitest";

import sampleJson from "../state/__fixtures__/dagRunSample.json";
import { parseDagRunSnapshot } from "./dagTypes";

describe("DagRunSnapshot validator", () => {
  it("parses the real fixture snapshot and derives accurate counts and literals", () => {
    // Given: real dagRunSample fixture JSON
    const rawData = sampleJson;

    // When: parsing the snapshot with parseDagRunSnapshot
    const snapshot = parseDagRunSnapshot(rawData);

    // Then: returns valid typed snapshot with exact literals from fixture
    expect(snapshot).not.toBeNull();
    if (!snapshot) return;

    expect(snapshot.status).toBe("cancelled");
    expect(snapshot.nodes).toHaveLength(6);
    expect(snapshot.waves).toHaveLength(4);
    expect(snapshot.criticalPath).toEqual(["extract", "render", "d5-fix", "verify"]);
    expect(snapshot.bottlenecks[0]).toEqual({ nodeId: "extract", blockedCount: 3 });

    // Then: counts are accurately derived from the 6 nodes (1 completed, 5 cancelled)
    expect(snapshot.counts).toEqual({
      total: 6,
      completed: 1,
      failed: 0,
      cancelled: 5,
      skipped: 0,
      running: 0,
    });
    expect(snapshot.amendCount).toBe(1);
  });

  it("returns null when nodes array is missing or deleted", () => {
    // Given: a snapshot payload with nodes field removed
    const { nodes: _deletedNodes, ...corrupted } = sampleJson;

    // When: attempting to validate the corrupted payload
    const snapshot = parseDagRunSnapshot(corrupted);

    // Then: validator detects the structural mismatch and returns null
    expect(snapshot).toBeNull();
  });

  it("returns null for non-object, null, or malformed top-level payload", () => {
    // Given: primitives and invalid non-object values
    const nonObjects = [null, undefined, "not an object", 42, [], true];

    // When & Then: each fails validation gracefully
    for (const invalid of nonObjects) {
      expect(parseDagRunSnapshot(invalid)).toBeNull();
    }
  });

  it("returns null when a node has an invalid state or missing route", () => {
    // Given: fixture with one node having invalid state
    const badState = {
      ...sampleJson,
      nodes: [
        {
          ...(sampleJson.nodes[0] as Record<string, unknown>),
          state: "invalid-state-value",
        },
      ],
    };

    // When: validating bad node state
    const resultBadState = parseDagRunSnapshot(badState);

    // Then: returns null
    expect(resultBadState).toBeNull();

    // Given: fixture with one node missing route
    const missingRoute = {
      ...sampleJson,
      nodes: [
        {
          ...(sampleJson.nodes[0] as Record<string, unknown>),
          route: undefined,
        },
      ],
    };

    // When: validating missing route
    const resultMissingRoute = parseDagRunSnapshot(missingRoute);

    // Then: returns null
    expect(resultMissingRoute).toBeNull();
  });
});
