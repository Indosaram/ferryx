// One machine's sidebar as its desktop draws it: the model the desktop published, or, when the
// desktop has not published one (or predates the route), the same model rebuilt with the shared
// builder from the machine's project inventory.
import { buildSidebarModel, type SidebarModel } from "../components/sidebar/sidebarModel";
import type { RegisteredProject } from "../lib/tauri";
import type { AgentState, Worktree } from "../lib/types";
import { AGENT_STATES } from "../components/sidebar/sidebarSessionSignals";
import type { SupervisorFetch, SupervisorHttpResponse } from "./remoteSupervisor";

/** Same adapter as the supervisor feed: the device token is already bound, callers pass a path. */
export type SidebarFetch = SupervisorFetch;

export type RemoteSidebarResult =
  | { kind: "ok"; model: SidebarModel }
  | { kind: "error"; code: string; message: string };

export const SIDEBAR_OFFLINE = "OFFLINE";

class SidebarRequestError extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = "SidebarRequestError";
    this.code = code;
  }
}

type Answer = { readonly status: number; readonly data: unknown };
type UnknownRecord = Record<string, unknown>;

const objectOf = (value: unknown): UnknownRecord | null =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? (value as UnknownRecord) : null;
const textOf = (value: unknown): string | null => (typeof value === "string" && value.trim() ? value : null);
const ok = (answer: Answer) => answer.status >= 200 && answer.status < 300;

function decode(body: SupervisorHttpResponse["body"]): unknown {
  try {
    return JSON.parse(typeof body === "string" ? body : new TextDecoder().decode(body));
  } catch (err) {
    if (err instanceof SyntaxError) return null;
    throw err;
  }
}

async function request(fetchLike: SidebarFetch, path: string): Promise<Answer> {
  let res: SupervisorHttpResponse;
  try {
    res = await fetchLike(path);
  } catch (err) {
    throw new SidebarRequestError(SIDEBAR_OFFLINE, err instanceof Error ? err.message : String(err));
  }
  return { status: res.status, data: decode(res.body) };
}

async function requireOk(fetchLike: SidebarFetch, path: string): Promise<UnknownRecord> {
  const answer = await request(fetchLike, path);
  const data = objectOf(answer.data);
  if (!ok(answer) || !data) throw new SidebarRequestError("HTTP_ERROR", `Sidebar inventory request failed (${answer.status})`);
  return data;
}

const isString = (value: unknown): value is string => typeof value === "string";
const isNullableString = (value: unknown) => value === null || isString(value);
const isStringList = (value: unknown) => Array.isArray(value) && value.every(isString);
const MEMBER_KINDS: readonly unknown[] = ["local", "ssh", "paired"];
const STATUS_KINDS: readonly unknown[] = ["online", "offline", "checking", "relay", "failure"];

function isMemberNode(value: unknown): boolean {
  const m = objectOf(value);
  return m !== null && isString(m.workspaceId) && MEMBER_KINDS.includes(m.kind) && isNullableString(m.machineId);
}

function isWorktreeNode(value: unknown): boolean {
  const w = objectOf(value);
  return w !== null && isString(w.key) && isString(w.workspaceId) && isString(w.path) && isNullableString(w.worktreeSlug) &&
    isString(w.label) && isNullableString(w.hostLabel) && typeof w.stale === "boolean" && isNullableString(w.listingFailure);
}

function isProjectNode(value: unknown): boolean {
  const p = objectOf(value);
  if (!p) return false;
  const status = p.status === null ? null : objectOf(p.status);
  return isString(p.groupId) && isString(p.label) && isNullableString(p.hostLabel) && isString(p.primaryWorkspaceId) &&
    (p.status === null || (status !== null && STATUS_KINDS.includes(status.kind) && isString(status.label))) &&
    Array.isArray(p.members) && p.members.every(isMemberNode) && isStringList(p.filterLabels) &&
    Array.isArray(p.worktrees) && p.worktrees.every(isWorktreeNode) && isNullableString(p.worktreesEmptyHint);
}

const isFlagRecord = (value: unknown) => {
  const r = objectOf(value);
  return r !== null && Object.values(r).every((flag) => typeof flag === "boolean");
};
const isAgentStateRecord = (value: unknown) => {
  const r = objectOf(value);
  return r !== null && Object.values(r).every((state) => AGENT_STATES.includes(state as AgentState));
};

const ACTIVITY_COUNTS = ["workingCount", "waitingCount", "doneCount", "runningCount"] as const;
const ACTIVITY_FLAGS = ["hasWorking", "hasWaiting", "hasDone", "hasUnread"] as const;

function isActivitySummary(value: unknown): boolean {
  const a = objectOf(value);
  return a !== null && ACTIVITY_COUNTS.every((field) => typeof a[field] === "number") &&
    ACTIVITY_FLAGS.every((field) => typeof a[field] === "boolean") && (a.agentType === undefined || isString(a.agentType));
}

const isActivityRecord = (value: unknown) => {
  const r = objectOf(value);
  return r !== null && Object.values(r).every(isActivitySummary);
};

function isSessionSignals(value: unknown): boolean {
  const s = objectOf(value);
  return s !== null && isFlagRecord(s.unreadWorktreePaths) && isAgentStateRecord(s.agentStateByPath) &&
    (s.activityByWorktreePath === undefined || isActivityRecord(s.activityByWorktreePath));
}

/** Structural guard for the wire model: one malformed node rejects the whole snapshot. */
function isSidebarModel(value: unknown): value is SidebarModel {
  const m = objectOf(value);
  return m !== null && m.schemaVersion === 1 && typeof m.revision === "number" && isString(m.generatedAt) &&
    (m.source === "desktop" || m.source === "inventory") && isStringList(m.collapsedGroupIds) &&
    typeof m.advancedFeatures === "boolean" && (m.emptyState === null || m.emptyState === "noProjects") &&
    Array.isArray(m.projects) && m.projects.every(isProjectNode) &&
    // Optional: snapshots from desktops older than this field still parse.
    (m.signals === undefined || isSessionSignals(m.signals));
}

function projectOf(raw: unknown): { project: RegisteredProject; ready: boolean } | null {
  const p = objectOf(raw);
  const workspaceId = textOf(p?.workspaceId);
  const repoRoot = textOf(p?.repoRoot);
  if (!p || !workspaceId || !repoRoot) return null;
  return {
    project: {
      workspaceId, repoRoot, gitRoot: textOf(p.gitRoot), gitCommonDir: textOf(p.gitCommonDir),
      gitRemote: textOf(p.gitRemote), gitBranch: textOf(p.gitBranch), gitHead: textOf(p.gitHead),
    },
    ready: p.availability === "ready",
  };
}

function worktreeOf(raw: unknown): Worktree | null {
  const w = objectOf(raw);
  const path = textOf(w?.path);
  if (!w || !path) return null;
  const identity = objectOf(w.identity);
  const wsId = textOf(identity?.wsId);
  const slug = textOf(identity?.slug);
  // Local rows carry no owner, exactly like the desktop's own listing, so row keys match it.
  return {
    path, head: textOf(w.head) ?? "", branch: textOf(w.branch), bare: w.bare === true, detached: w.detached === true,
    locked: textOf(w.locked), prunable: textOf(w.prunable), identity: wsId && slug ? { wsId, slug } : null,
  };
}

/** SSH projects live only in the gateway's workspace state; it names the folder, never the path. */
function sshProjectsOf(state: unknown, known: ReadonlySet<string>): RegisteredProject[] {
  const list = objectOf(state)?.projects;
  return (Array.isArray(list) ? list : []).flatMap((raw): RegisteredProject[] => {
    const p = objectOf(raw);
    const workspaceId = textOf(p?.workspaceId);
    if (!p || !workspaceId?.startsWith("ssh:") || known.has(workspaceId)) return [];
    const worktrees = Array.isArray(p.worktrees) ? p.worktrees : [];
    const folder = (textOf(objectOf(worktrees[0])?.worktreeLabel) ?? "").split(/[\\/]/).filter(Boolean).at(-1) ?? workspaceId;
    const hostLabel = textOf(p.hostLabel)?.trim() ?? null;
    return [{ workspaceId, repoRoot: folder, target: { kind: "ssh", hostId: hostLabel ?? "SSH" }, ...(hostLabel ? { hostLabel } : {}) }];
  });
}

async function inventoryModel(fetchLike: SidebarFetch): Promise<SidebarModel> {
  let catalog = await requireOk(fetchLike, "/api/v1/workspace/projects");
  if (catalog.desktopLocalSnapshotExists === true) {
    catalog = await requireOk(fetchLike, "/api/v1/workspace/projects?scope=desktopLocal");
  }
  const parsed = (Array.isArray(catalog.projects) ? catalog.projects : []).flatMap((raw) => projectOf(raw) ?? []);
  const listingFailures: Record<string, string> = {};
  for (const { project, ready } of parsed) if (!ready) listingFailures[project.workspaceId] = "Project unavailable";
  const ready = parsed.filter((entry) => entry.ready).map((entry) => entry.project);
  const rows: Record<string, Worktree[]> = {};
  const stateRequest = request(fetchLike, "/api/v1/workspace/state").catch(() => null);
  // At most four listings are in flight, like discovery; one project's failure marks only it.
  let next = 0;
  const worker = async () => {
    while (next < ready.length) {
      const { workspaceId } = ready[next++];
      try {
        const data = await requireOk(fetchLike, `/api/v1/workspace/worktrees?workspaceId=${encodeURIComponent(workspaceId)}`);
        rows[workspaceId] = (Array.isArray(data.worktrees) ? data.worktrees : []).flatMap((raw) => worktreeOf(raw) ?? []);
      } catch (err) {
        if (!(err instanceof SidebarRequestError)) throw err;
        if (err.code === SIDEBAR_OFFLINE) throw err;
        listingFailures[workspaceId] = err.message;
      }
    }
  };
  await Promise.all(Array.from({ length: Math.min(4, ready.length) }, worker));
  const stateAnswer = await stateRequest;
  const projects = parsed.map((entry) => entry.project);
  const ssh = stateAnswer && ok(stateAnswer) ? sshProjectsOf(stateAnswer.data, new Set(projects.map((p) => p.workspaceId))) : [];
  return buildSidebarModel({
    source: "inventory",
    generatedAt: new Date().toISOString(),
    projects: [...projects, ...ssh],
    activeProjectId: undefined,
    worktrees: [],
    inactiveProjectWorktrees: rows,
    hosts: [],
    remoteState: { hosts: {}, activeHostId: null, discovering: false },
    reachability: () => ({ supported: false, entry: null }),
    worktreeOrder: {},
    collapsedGroupIds: new Set(),
    advancedFeatures: false,
    listingFailures,
  });
}

/** GET /api/v1/workspace/sidebar; `{model:null}`, a 404 or an unknown schema fall back to the inventory. */
export async function loadRemoteSidebar(fetchLike: SidebarFetch): Promise<RemoteSidebarResult> {
  try {
    const answer = await request(fetchLike, "/api/v1/workspace/sidebar");
    if (answer.status !== 404 && !ok(answer)) {
      return { kind: "error", code: "HTTP_ERROR", message: `Sidebar request failed (${answer.status})` };
    }
    const published = ok(answer) ? objectOf(answer.data)?.model : null;
    return { kind: "ok", model: isSidebarModel(published) ? published : await inventoryModel(fetchLike) };
  } catch (err) {
    if (err instanceof SidebarRequestError) return { kind: "error", code: err.code, message: err.message };
    throw err;
  }
}
