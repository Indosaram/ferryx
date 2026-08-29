import { beforeEach, describe, expect, it, vi } from "vitest";

import sampleJson from "./__fixtures__/dagRunSample.json";
import { parseDagRunSnapshot, type DagRunSnapshot } from "../lib/dagTypes";
import { createDagStore, selectActiveRunIds, selectRunSummaries, type DagStore } from "./dagStore";

describe("dagStore", () => {
  let store: DagStore;
  let sampleSnapshot: DagRunSnapshot;

  beforeEach(() => {
    store = createDagStore();
    const parsed = parseDagRunSnapshot(sampleJson);
    if (!parsed) {
      throw new Error("Fixture failed to parse in test setup");
    }
    sampleSnapshot = parsed;
  });

  it("keeps exactly one entry when applying the same runId twice with updated content", () => {
    // Given: an initial snapshot applied for project "/repo/app"
    const projectPath = "/repo/app";
    store.applySnapshot(projectPath, sampleSnapshot);

    const initialRuns = store.getState().runsByProject[projectPath];
    expect(Object.keys(initialRuns ?? {})).toEqual([sampleSnapshot.runId]);

    // When: applying an updated version of the same runId (amendCount incremented, updatedAt changed)
    const updatedSnapshot: DagRunSnapshot = {
      ...sampleSnapshot,
      amendCount: 2,
      updatedAt: "2026-08-28T17:00:00.000Z",
    };
    store.applySnapshot(projectPath, updatedSnapshot);

    // Then: single entry exists for that runId with updated content
    const finalRuns = store.getState().runsByProject[projectPath];
    expect(Object.keys(finalRuns ?? {})).toEqual([sampleSnapshot.runId]);
    expect(finalRuns?.[sampleSnapshot.runId]?.amendCount).toBe(2);
    expect(finalRuns?.[sampleSnapshot.runId]?.updatedAt).toBe("2026-08-28T17:00:00.000Z");
  });

  it("returns empty activeRunIds for a cancelled run, but returns running runId when status is running", () => {
    // Given: a cancelled run snapshot in the store
    const projectPath = "/repo/app";
    store.applySnapshot(projectPath, sampleSnapshot);

    // When & Then: activeRunIds is empty for cancelled run
    expect(store.activeRunIds(projectPath)).toEqual([]);

    // When: a running run snapshot is applied
    const runningSnapshot: DagRunSnapshot = {
      ...sampleSnapshot,
      runId: "dag_running_123",
      status: "running",
    };
    store.applySnapshot(projectPath, runningSnapshot);

    // Then: activeRunIds contains the running runId only
    expect(store.activeRunIds(projectPath)).toEqual(["dag_running_123"]);
  });

  it("sorts runSummaries by updatedAt descending", () => {
    // Given: three runs with different updatedAt timestamps
    const projectPath = "/repo/app";
    const runOld: DagRunSnapshot = {
      ...sampleSnapshot,
      runId: "run_old",
      updatedAt: "2026-08-28T10:00:00.000Z",
    };
    const runMid: DagRunSnapshot = {
      ...sampleSnapshot,
      runId: "run_mid",
      updatedAt: "2026-08-28T12:00:00.000Z",
    };
    const runLatest: DagRunSnapshot = {
      ...sampleSnapshot,
      runId: "run_latest",
      updatedAt: "2026-08-28T16:00:00.000Z",
    };

    // When: applied in arbitrary order
    store.applySnapshot(projectPath, runOld);
    store.applySnapshot(projectPath, runLatest);
    store.applySnapshot(projectPath, runMid);

    // Then: runSummaries returns runs sorted by updatedAt desc
    const summaries = store.runSummaries(projectPath);
    expect(summaries.map((r) => r.runId)).toEqual(["run_latest", "run_mid", "run_old"]);
  });

  it("removes run when removeRun is called", () => {
    // Given: a run in the store
    const projectPath = "/repo/app";
    store.applySnapshot(projectPath, sampleSnapshot);
    expect(store.getState().runsByProject[projectPath]?.[sampleSnapshot.runId]).toBeDefined();

    // When: removing the run
    store.removeRun(projectPath, sampleSnapshot.runId);

    // Then: run is deleted
    expect(store.getState().runsByProject[projectPath]?.[sampleSnapshot.runId]).toBeUndefined();
  });

  it("isolates runs across distinct project paths", () => {
    // Given: two different project paths
    const projectA = "/repo/project-a";
    const projectB = "/repo/project-b";
    const runB: DagRunSnapshot = { ...sampleSnapshot, runId: "dag_proj_b", status: "running" };

    // When: snapshots are applied to respective projects
    store.applySnapshot(projectA, sampleSnapshot);
    store.applySnapshot(projectB, runB);

    // Then: each project only sees its own runs
    expect(store.activeRunIds(projectA)).toEqual([]);
    expect(store.activeRunIds(projectB)).toEqual(["dag_proj_b"]);
    expect(selectActiveRunIds(store.getState(), projectA)).toEqual([]);
    expect(selectRunSummaries(store.getState(), projectB)).toHaveLength(1);
  });

  it("notifies subscribers upon state change and unsubscribes cleanly", () => {
    // Given: a subscribed listener
    const listener = vi.fn();
    const unsubscribe = store.subscribe(listener);

    // When: applying a snapshot
    store.applySnapshot("/repo/app", sampleSnapshot);

    // Then: subscriber receives the update
    expect(listener).toHaveBeenCalledTimes(1);

    // When: unsubscribing and applying another snapshot
    unsubscribe();
    store.applySnapshot("/repo/app", { ...sampleSnapshot, runId: "dag_second" });

    // Then: subscriber is not called again
    expect(listener).toHaveBeenCalledTimes(1);
  });
});
