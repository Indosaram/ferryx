import type {
  DagEdge,
  DagNodeSnapshot,
  DagNodeState,
  DagRunSnapshot,
} from "../lib/dagTypes";
import { deriveDagRunCounts } from "../lib/dagTypes";

/**
 * Test-only fixtures for the DAG viewport QA browser harness (dag-viewport-qa.html).
 * They are seeded through the REAL dagStore APIs by dagViewportQaMain.tsx and never
 * imported by production code.
 */
export const QA_PROJECT_PATH = "/qa/dag-viewport";
export const QA_OWNER_SESSION_ID = "qa-owner";

function node(
  id: string,
  state: DagNodeState,
  dependsOn: readonly string[] = [],
): DagNodeSnapshot {
  return {
    id,
    label: `QA node ${id}`,
    state,
    dependsOn,
    attempt: 1,
    route: { kind: "agent", agent: "omo" },
    startedAt: null,
    completedAt: null,
    error: null,
    taskId: null,
  };
}

function run(
  runId: string,
  name: string,
  overrides: Partial<DagRunSnapshot> & {
    nodes: readonly DagNodeSnapshot[];
    edges: readonly DagEdge[];
  },
): DagRunSnapshot {
  const nodes = overrides.nodes;
  return {
    runId,
    runKey: `qa-${runId}`,
    name,
    rootSessionId: overrides.rootSessionId ?? null,
    status: overrides.status ?? "running",
    startedAt: "2026-09-12T09:00:00Z",
    completedAt: null,
    updatedAt: overrides.updatedAt ?? "2026-09-12T09:00:00Z",
    amendCount: 0,
    nodes,
    edges: overrides.edges,
    waves: overrides.waves ?? [],
    criticalPath: overrides.criticalPath ?? [],
    bottlenecks: overrides.bottlenecks ?? [],
    counts: overrides.counts ?? deriveDagRunCounts(nodes),
  };
}

function edges(...pairs: Array<readonly [string, string]>): readonly DagEdge[] {
  return pairs.map(([from, to]) => ({ from, to }));
}

/** Fixture A: waves [a,b],[c],[d],[e]; edges a->c, b->c, c->d, a->e, d->e. Owner qa-owner. */
export function buildQaDagRunA(): DagRunSnapshot {
  const nodes = [
    node("a", "running"),
    node("b", "pending"),
    node("c", "pending", ["a", "b"]),
    node("d", "pending", ["c"]),
    node("e", "pending", ["a", "d"]),
  ];
  return run("qa-dag-a", "QA viewport run A", {
    rootSessionId: QA_OWNER_SESSION_ID,
    status: "running",
    updatedAt: "2026-09-12T12:00:00Z",
    nodes,
    edges: edges(["a", "c"], ["b", "c"], ["c", "d"], ["a", "e"], ["d", "e"]),
    waves: [
      { index: 0, nodeIds: ["a", "b"] },
      { index: 1, nodeIds: ["c"] },
      { index: 2, nodeIds: ["d"] },
      { index: 3, nodeIds: ["e"] },
    ],
    criticalPath: ["a", "c", "d", "e"],
  });
}

/** Fixture B: same owner, different dimensions (2 waves x 3 rows). */
export function buildQaDagRunB(): DagRunSnapshot {
  const nodes = [
    node("b-1", "running"), node("b-2", "pending"), node("b-3", "pending"),
    node("b-4", "pending"), node("b-5", "pending"), node("b-6", "pending"),
  ];
  return run("qa-dag-b", "QA viewport run B", {
    rootSessionId: QA_OWNER_SESSION_ID,
    status: "running",
    updatedAt: "2026-09-12T10:00:00Z",
    nodes,
    edges: edges(["b-1", "b-4"], ["b-2", "b-5"], ["b-3", "b-6"], ["b-4", "b-6"]),
    waves: [
      { index: 0, nodeIds: ["b-1", "b-2", "b-3"] },
      { index: 1, nodeIds: ["b-4", "b-5", "b-6"] },
    ],
  });
}

/** Big fixture: 100 columns, one node each, chain plus first-to-last edge. */
export function buildBigDagRun(): DagRunSnapshot {
  const nodeIds = Array.from({ length: 100 }, (_, i) => `n${i}`);
  const nodes = nodeIds.map((id, i) => node(id, i === 0 ? "running" : "pending", i > 0 ? [`n${i - 1}`] : []));
  return run("qa-dag-big", "QA big 100-column run", {
    rootSessionId: null,
    nodes,
    edges: edges(...nodeIds.slice(1).map((id, i) => [`n${i}`, id] as const), ["n0", "n99"]),
    waves: nodeIds.map((id, i) => ({ index: i, nodeIds: [id] })),
  });
}

/** Tall fixture: one column, 100 nodes. */
export function buildTallDagRun(): DagRunSnapshot {
  const nodeIds = Array.from({ length: 100 }, (_, i) => `t${i}`);
  const nodes = nodeIds.map((id, i) => node(id, i === 0 ? "running" : "pending", i > 0 ? [`t${i - 1}`] : []));
  return run("qa-dag-tall", "QA tall 100-node run", {
    rootSessionId: null,
    nodes,
    edges: edges(...nodeIds.slice(1).map((id, i) => [`t${i}`, id] as const)),
    waves: [{ index: 0, nodeIds }],
  });
}

/** Empty fixture: no nodes, no waves. */
export function buildEmptyDagRun(): DagRunSnapshot {
  return run("qa-dag-empty", "QA empty run", {
    rootSessionId: null,
    status: "running",
    nodes: [],
    edges: [],
    waves: [],
    counts: { total: 0, completed: 0, failed: 0, cancelled: 0, skipped: 0, running: 0 },
  });
}

/** Same-run live update for fixture A: identical runId, one node state flips. */
export function buildQaDagRunAUpdated(): DagRunSnapshot {
  const base = buildQaDagRunA();
  return {
    ...base,
    updatedAt: "2026-09-12T12:05:00Z",
    nodes: base.nodes.map((n) => (n.id === "b" ? { ...n, state: "completed" as const } : n)),
    counts: deriveDagRunCounts(base.nodes.map((n) => (n.id === "b" ? { ...n, state: "completed" as const } : n))),
  };
}
