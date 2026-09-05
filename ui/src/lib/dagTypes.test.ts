import { describe, expect, it } from "vitest";

import sampleJson from "../state/__fixtures__/dagRunSample.json";
import { deriveDagRunCounts, parseDagRunSnapshot, parseRoute } from "./dagTypes";
import type { DagNodeSnapshot } from "./dagTypes";

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

  it("parses a top-level rootSessionId string", () => {
    const snapshot = parseDagRunSnapshot({
      ...sampleJson,
      rootSessionId: "01a055f9-a8de-7619-a1f5-81ca62e3d3b1",
    });

    expect(snapshot?.rootSessionId).toBe("01a055f9-a8de-7619-a1f5-81ca62e3d3b1");
  });

  it("leaves rootSessionId undefined when missing, null, or invalid", () => {
    const { rootSessionId: _rootSessionId, ...withoutRootSessionId } = sampleJson;

    expect(parseDagRunSnapshot(withoutRootSessionId)?.rootSessionId).toBeUndefined();
    expect(parseDagRunSnapshot({ ...sampleJson, rootSessionId: null })?.rootSessionId).toBeUndefined();
    expect(parseDagRunSnapshot({ ...sampleJson, rootSessionId: 42 })?.rootSessionId).toBeUndefined();
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

  it("handles 'unknown' node state in parseDagRunSnapshot and deriveDagRunCounts without throwing", () => {
    // Given: fixture snapshot with a node having state "unknown"
    const snapshotWithUnknownNode = {
      ...sampleJson,
      nodes: [
        {
          ...(sampleJson.nodes[0] as Record<string, unknown>),
          state: "unknown",
        },
        ...sampleJson.nodes.slice(1),
      ],
    };

    // When: parsing the snapshot
    const snapshot = parseDagRunSnapshot(snapshotWithUnknownNode);

    // Then: parses successfully without throwing and preserves "unknown" state
    expect(snapshot).not.toBeNull();
    if (!snapshot) return;
    expect(snapshot.nodes[0]?.state).toBe("unknown");

    // Then: total includes the unknown node, but specific status counts ignore it
    expect(snapshot.counts.total).toBe(6);
    expect(snapshot.counts.completed).toBe(1);
    expect(snapshot.counts.cancelled).toBe(4);

    // When: calling deriveDagRunCounts directly with unknown state nodes
    const dummyNode: DagNodeSnapshot = {
      id: "node-unknown",
      label: "Unknown Node",
      state: "unknown",
      dependsOn: [],
      attempt: 1,
      route: { kind: "unknown" },
      startedAt: null,
      completedAt: null,
      error: null,
      taskId: null,
    };
    const counts = deriveDagRunCounts([dummyNode]);

    // Then: counts total is 1 and all status counts are 0 without throwing
    expect(counts).toEqual({
      total: 1,
      completed: 0,
      failed: 0,
      cancelled: 0,
      skipped: 0,
      running: 0,
    });
  });

  it("parses unknown route kind as { kind: 'unknown' } without throwing or returning null", () => {
    // When & Then: parseRoute parses "unknown" kind as { kind: "unknown" }
    expect(parseRoute({ kind: "unknown" })).toEqual({ kind: "unknown" });

    // When & Then: parseRoute parses any other unknown kind as { kind: "unknown" }
    expect(parseRoute({ kind: "custom", customField: 123 })).toEqual({ kind: "unknown" });
    expect(parseRoute({ kind: "service", endpoint: "http://localhost" })).toEqual({ kind: "unknown" });

    // Given: fixture with unknown route kinds on a node
    const snapshotWithUnknownRoute = {
      ...sampleJson,
      nodes: [
        {
          ...(sampleJson.nodes[0] as Record<string, unknown>),
          route: { kind: "external_service", serviceId: "svc-9" },
        },
        ...sampleJson.nodes.slice(1),
      ],
    };

    // When: parsing the snapshot
    const snapshot = parseDagRunSnapshot(snapshotWithUnknownRoute);

    // Then: node route is parsed gracefully as { kind: "unknown" }
    expect(snapshot).not.toBeNull();
    expect(snapshot?.nodes[0]?.route).toEqual({ kind: "unknown" });
  });
});
