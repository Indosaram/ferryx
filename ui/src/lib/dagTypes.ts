export type DagRunStatus = "running" | "completed" | "failed" | "cancelled" | "paused";

export type DagNodeState =
  | "pending"
  | "scheduled"
  | "blocked"
  | "running"
  | "completed"
  | "failed"
  | "skipped"
  | "cancelled"
  | "paused";

export type DagNodeRoute =
  | { readonly kind: "category"; readonly category: string }
  | { readonly kind: "agent"; readonly agent: string; readonly model?: string };

export type DagNodeError = {
  readonly code: string;
  readonly message: string;
  readonly nodeId?: string;
  readonly at?: string;
};

export type DagNodeSnapshot = {
  readonly id: string;
  readonly label: string | null;
  readonly state: DagNodeState;
  readonly dependsOn: readonly string[];
  readonly attempt: number;
  readonly route: DagNodeRoute;
  readonly startedAt: string | null;
  readonly completedAt: string | null;
  readonly error: DagNodeError | null;
  readonly taskId: string | null;
};

export type DagEdge = { readonly from: string; readonly to: string };
export type DagWave = { readonly index: number; readonly nodeIds: readonly string[] };
export type DagBottleneck = { readonly nodeId: string; readonly blockedCount: number };

export type DagRunCounts = {
  readonly total: number;
  readonly completed: number;
  readonly failed: number;
  readonly cancelled: number;
  readonly skipped: number;
  readonly running: number;
};

export type DagRunSummary = Pick<
  DagRunSnapshot,
  "runId" | "runKey" | "name" | "status" | "startedAt" | "updatedAt" | "counts"
>;

export type DagRunSnapshot = {
  readonly runId: string;
  readonly runKey: string;
  readonly name: string;
  readonly status: DagRunStatus;
  readonly startedAt: string | null;
  readonly completedAt: string | null;
  readonly updatedAt: string | null;
  readonly amendCount: number;
  readonly nodes: readonly DagNodeSnapshot[];
  readonly edges: readonly DagEdge[];
  readonly waves: readonly DagWave[];
  readonly criticalPath: readonly string[];
  readonly bottlenecks: readonly DagBottleneck[];
  readonly counts: DagRunCounts;
};

const VALID_RUN_STATUSES = new Set<string>(["running", "completed", "failed", "cancelled", "paused"]);
const VALID_NODE_STATES = new Set<string>([
  "pending", "scheduled", "blocked", "running", "completed", "failed", "skipped", "cancelled", "paused",
]);

function assertNever(x: never): never {
  throw new Error(`Unexpected object: ${String(x)}`);
}

export function deriveDagRunCounts(nodes: readonly DagNodeSnapshot[]): DagRunCounts {
  let completed = 0;
  let failed = 0;
  let cancelled = 0;
  let skipped = 0;
  let running = 0;

  for (const node of nodes) {
    switch (node.state) {
      case "completed": completed += 1; break;
      case "failed": failed += 1; break;
      case "cancelled": cancelled += 1; break;
      case "skipped": skipped += 1; break;
      case "running": running += 1; break;
      case "pending":
      case "scheduled":
      case "blocked":
      case "paused": break;
      default: return assertNever(node.state);
    }
  }
  return { total: nodes.length, completed, failed, cancelled, skipped, running };
}

function isRecord(val: unknown): val is Record<string, unknown> {
  return typeof val === "object" && val !== null && !Array.isArray(val);
}

function parseStrOrNull(val: unknown): string | null | undefined {
  if (val === null || val === undefined) return null;
  return typeof val === "string" ? val : undefined;
}

function parseRoute(val: unknown): DagNodeRoute | null {
  if (!isRecord(val)) return null;
  if (val["kind"] === "category" && typeof val["category"] === "string") {
    return { kind: "category", category: val["category"] };
  }
  if (val["kind"] === "agent" && typeof val["agent"] === "string") {
    const route: { kind: "agent"; agent: string; model?: string } = { kind: "agent", agent: val["agent"] };
    if (typeof val["model"] === "string") route.model = val["model"];
    return route;
  }
  return null;
}

function parseNodeError(val: unknown): DagNodeError | null | undefined {
  if (val === null || val === undefined) return null;
  if (!isRecord(val) || typeof val["code"] !== "string" || typeof val["message"] !== "string") return undefined;
  const err: { code: string; message: string; nodeId?: string; at?: string } = {
    code: val["code"],
    message: val["message"],
  };
  if (typeof val["nodeId"] === "string") err.nodeId = val["nodeId"];
  if (typeof val["at"] === "string") err.at = val["at"];
  return err;
}

function parseStringArray(val: unknown): readonly string[] | null {
  if (val === undefined || val === null) return [];
  return Array.isArray(val) && val.every((x) => typeof x === "string") ? (val as readonly string[]) : null;
}

function parseArrayOf<T>(val: unknown, itemParser: (item: unknown) => T | null): readonly T[] | null {
  if (val === undefined || val === null) return [];
  if (!Array.isArray(val)) return null;
  const res: T[] = [];
  for (const item of val) {
    const parsed = itemParser(item);
    if (parsed === null) return null;
    res.push(parsed);
  }
  return res;
}

function parseNode(val: unknown): DagNodeSnapshot | null {
  if (!isRecord(val) || typeof val["id"] !== "string") return null;
  const label = parseStrOrNull(val["label"]);
  const stateStr = val["state"];
  if (label === undefined || typeof stateStr !== "string" || !VALID_NODE_STATES.has(stateStr)) return null;

  const dependsOn = parseStringArray(val["dependsOn"]);
  const attempt = val["attempt"] === undefined ? 0 : typeof val["attempt"] === "number" ? val["attempt"] : null;
  const route = parseRoute(val["route"]);
  const startedAt = parseStrOrNull(val["startedAt"]);
  const completedAt = parseStrOrNull(val["completedAt"]);
  const error = parseNodeError(val["error"]);
  const taskId = parseStrOrNull(val["taskId"]);

  if (!dependsOn || attempt === null || !route || startedAt === undefined || completedAt === undefined || error === undefined || taskId === undefined) {
    return null;
  }
  return { id: val["id"], label, state: stateStr as DagNodeState, dependsOn, attempt, route, startedAt, completedAt, error, taskId };
}

export function parseDagRunSnapshot(raw: unknown): DagRunSnapshot | null {
  if (!isRecord(raw) || typeof raw["runId"] !== "string" || typeof raw["runKey"] !== "string" || typeof raw["name"] !== "string") {
    return null;
  }
  const statusStr = raw["status"];
  if (typeof statusStr !== "string" || !VALID_RUN_STATUSES.has(statusStr)) return null;

  const startedAt = parseStrOrNull(raw["startedAt"]);
  const completedAt = parseStrOrNull(raw["completedAt"]);
  const updatedAt = parseStrOrNull(raw["updatedAt"]);
  if (startedAt === undefined || completedAt === undefined || updatedAt === undefined) return null;

  let amendCount = 0;
  if (typeof raw["amendCount"] === "number") {
    amendCount = raw["amendCount"];
  } else if (raw["amendCount"] === undefined) {
    if (Array.isArray(raw["amendHistory"])) amendCount = raw["amendHistory"].length;
  } else {
    return null;
  }

  if (!Array.isArray(raw["nodes"])) return null;
  const nodes = parseArrayOf(raw["nodes"], parseNode);
  const edges = parseArrayOf(raw["edges"], (i) => (isRecord(i) && typeof i["from"] === "string" && typeof i["to"] === "string" ? { from: i["from"], to: i["to"] } : null));
  const waves = parseArrayOf(raw["waves"], (i) => {
    if (!isRecord(i) || typeof i["index"] !== "number") return null;
    const nodeIds = parseStringArray(i["nodeIds"]);
    return nodeIds && i["nodeIds"] !== undefined ? { index: i["index"], nodeIds } : null;
  });
  const criticalPath = parseStringArray(raw["criticalPath"]);
  const bottlenecks = parseArrayOf(raw["bottlenecks"], (i) => (isRecord(i) && typeof i["nodeId"] === "string" && typeof i["blockedCount"] === "number" ? { nodeId: i["nodeId"], blockedCount: i["blockedCount"] } : null));

  if (!nodes || !edges || !waves || !criticalPath || !bottlenecks) return null;

  return {
    runId: raw["runId"],
    runKey: raw["runKey"],
    name: raw["name"],
    status: statusStr as DagRunStatus,
    startedAt,
    completedAt,
    updatedAt,
    amendCount,
    nodes,
    edges,
    waves,
    criticalPath,
    bottlenecks,
    counts: deriveDagRunCounts(nodes),
  };
}
