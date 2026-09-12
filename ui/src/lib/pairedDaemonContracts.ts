import type { RunTarget } from "./scopedContracts";

export class MachineDecodeError extends Error {
  constructor(readonly code: "INVALID_REQUEST" | "PAYLOAD_TOO_LARGE") {
    super(code);
  }
}

export function decodeRunTarget(value: unknown): RunTarget {
  const v = object(value);
  switch (v.kind) {
    case "local": return { kind: "local" };
    case "ssh": return { kind: "ssh", hostId: text(v.hostId) };
    case "pairedDaemon": return { kind: "pairedDaemon", hostId: text(v.hostId) };
    default: throw new MachineDecodeError("INVALID_REQUEST");
  }
}

function invalid(): never { throw new MachineDecodeError("INVALID_REQUEST"); }
function object(v: unknown): Record<string, unknown> {
  if (v === null || typeof v !== "object" || Array.isArray(v)) return invalid();
  return Object.fromEntries(Object.entries(v));
}
function text(v: unknown): string { return typeof v === "string" && v.trim() ? v : invalid(); }
function bool(v: unknown): boolean { return typeof v === "boolean" ? v : invalid(); }
function uint(v: unknown): number { return typeof v === "number" && Number.isInteger(v) && v >= 0 && v <= 4294967295 ? v : invalid(); }
function dimension(v: unknown): number { const n = uint(v); return n <= 65535 ? n : invalid(); }
function requestId(v: unknown): string {
  const s = text(v); return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(s) ? s : invalid();
}
function exitMetadata(value: unknown) {
  const v = object(value);
  return { code: nullable(v.code, n => typeof n === "number" && Number.isInteger(n) && n >= -2147483648 && n <= 2147483647 ? n : invalid()), signal: nullable(v.signal, text) };
}
function machineError(value: unknown) {
  const e = object(value);
  return { code: text(e.code), message: text(e.message), retryable: bool(e.retryable), requestId: requestId(e.requestId), details: object(e.details) };
}
function startup(value: unknown) {
  const v = object(value);
  switch (v.kind) {
    case "shell": return { kind: "shell" };
    case "agentResume": return { kind: "agentResume", agentType: text(v.agentType), providerSession: provider(v.providerSession) };
    default: return invalid();
  }
}
function nullable<T>(v: unknown, parse: (v: unknown) => T): T | null { return v === null ? null : parse(v); }
function array<T>(v: unknown, parse: (v: unknown) => T): T[] { return Array.isArray(v) ? v.map(parse) : invalid(); }
function choice<const T extends readonly string[]>(v: unknown, choices: T): T[number] {
  for (const item of choices) if (item === v) return item;
  return invalid();
}
export function decodeU64(v: unknown): string {
  const s = text(v);
  return /^(0|[1-9][0-9]*)$/.test(s) && s.length <= 20 && BigInt(s) <= 18446744073709551615n ? s : invalid();
}
export function decodeRemoteTarget(value: unknown) {
  const v = object(value);
  return { machineId: text(v.machineId), daemonEpoch: decodeU64(v.daemonEpoch), sessionId: text(v.sessionId) };
}
function identity(value: unknown) {
  const v = object(value); return { wsId: text(v.wsId), slug: text(v.slug) };
}
function project(value: unknown) {
  const v = object(value);
  return { workspaceId: text(v.workspaceId), repoRoot: text(v.repoRoot), gitRoot: nullable(v.gitRoot, text),
    gitCommonDir: nullable(v.gitCommonDir, text), gitRemote: nullable(v.gitRemote, text),
    gitBranch: nullable(v.gitBranch, text), gitHead: nullable(v.gitHead, text),
    availability: choice(v.availability, ["ready", "missing", "permissionDenied", "invalid"]), revision: decodeU64(v.revision) };
}
function worktree(value: unknown) {
  const v = object(value);
  return { workspaceId: text(v.workspaceId), identity: nullable(v.identity, identity), path: text(v.path),
    head: typeof v.head === "string" ? v.head : invalid(), branch: nullable(v.branch, text),
    bare: bool(v.bare), detached: bool(v.detached), locked: nullable(v.locked, text), prunable: nullable(v.prunable, text), managed: bool(v.managed) };
}
function provider(value: unknown) {
  const v = object(value);
  return { key: choice(v.key, ["session_id", "conversation_id"]), id: text(v.id),
    ...(v.transcriptPath === undefined ? {} : { transcriptPath: text(v.transcriptPath) }) };
}
function session(value: unknown) {
  const v = object(value);
  return { target: decodeRemoteTarget(v.target), workspaceId: text(v.workspaceId), worktree: nullable(v.worktree, identity),
    cwd: text(v.cwd), cols: dimension(v.cols), rows: dimension(v.rows), running: bool(v.running), providerSession: nullable(v.providerSession, provider),
    startSequence: decodeU64(v.startSequence), endSequence: decodeU64(v.endSequence) };
}
export function decodePairedProject(value: unknown) {
  const v = object(value), target = decodeRunTarget(v.target);
  if (target.kind !== "pairedDaemon") return invalid();
  const workspaceId = text(v.workspaceId);
  if (!/^daemon:[a-f0-9]{64}$/.test(workspaceId)) return invalid();
  return { workspaceId, repoRoot: text(v.repoRoot), target, remoteWorkspaceId: text(v.remoteWorkspaceId) };
}
export const MACHINE_JSON_MAX_BYTES = 64 * 1024;
export const DIRECTORY_JSON_MAX_BYTES = 256 * 1024;
export const CONTROL_JSON_MAX_BYTES = 16 * 1024;
export function decodeProxyDescriptor(value: unknown) {
  const v = object(value), native = object(v.nativeAttachment);
  const workspaceId = text(v.workspaceId), localProxyId = text(v.localProxyId);
  if (!/^daemon:[a-f0-9]{64}$/.test(workspaceId) || !/^daemon-session:[a-f0-9]{64}$/.test(localProxyId)) return invalid();
  return { hostId: text(v.hostId), workspaceId, remoteWorkspaceId: text(v.remoteWorkspaceId), localProxyId,
    worktree: nullable(v.worktree, identity), remoteTarget: decodeRemoteTarget(v.remoteTarget), remoteCursor: decodeU64(v.remoteCursor),
    nativeAttachment: { daemonEpoch: decodeU64(native.daemonEpoch), lastOutputSequence: decodeU64(native.lastOutputSequence) },
    cols: dimension(v.cols), rows: dimension(v.rows), requestId: requestId(v.requestId),
    processStatus: choice(v.processStatus, ["running", "exited", "expired", "unknown"]),
    transportStatus: choice(v.transportStatus, ["disconnected", "reconnecting", "revoked"]) };
}
export function decodeMachineJson(kind: string, json: string) {
  const limit = kind === "directories" ? DIRECTORY_JSON_MAX_BYTES : kind === "attached" || kind === "control" ? CONTROL_JSON_MAX_BYTES : MACHINE_JSON_MAX_BYTES;
  if (new TextEncoder().encode(json).length > limit) throw new MachineDecodeError("PAYLOAD_TOO_LARGE");
  let value: unknown;
  try { value = JSON.parse(json); } catch (error) { if (error instanceof SyntaxError) return invalid(); throw error; }
  const v = object(value);
  switch (kind) {
    case "target": return decodeRunTarget(v);
    case "pairedProject": return decodePairedProject(v);
    case "descriptor": return decodeProxyDescriptor(v);
    case "capabilities": {
      const limits = object(v.limits);
      if (v.apiVersion !== 1) return invalid();
      return { apiVersion: 1, machineId: text(v.machineId), daemonEpoch: decodeU64(v.daemonEpoch),
        platform: choice(v.platform, ["linux", "macos", "windows"]), accessScope: choice(v.accessScope, ["mirror", "machine"]),
        permission: choice(v.permission, ["view", "control"]), capabilities: array(v.capabilities, text),
        limits: { directoryEntries: uint(limits.directoryEntries), terminalSessions: uint(limits.terminalSessions) } };
    }
    case "directories": return { path: text(v.path), parentPath: nullable(v.parentPath, text), homePath: text(v.homePath),
      entries: array(v.entries, entry => { const e = object(entry); return { name: text(e.name), path: text(e.path), hidden: bool(e.hidden) }; }), truncated: bool(v.truncated) };
    case "project": return project(v);
    case "projects": return { revision: decodeU64(v.revision), completeness: choice(v.completeness, ["complete", "partial"]),
      projects: array(v.projects, project), unavailableWorkspaceIds: array(v.unavailableWorkspaceIds, text) };
    case "worktree": return worktree(v);
    case "worktrees": return { revision: decodeU64(v.revision), worktrees: array(v.worktrees, worktree) };
    case "session": return session(v);
    case "sessions": return { revision: decodeU64(v.revision), completeness: choice(v.completeness, ["complete", "partial"]),
      sessions: array(v.sessions, session), unavailableWorkspaceIds: array(v.unavailableWorkspaceIds, text) };
    case "error": return { error: machineError(v.error) };
    case "worktreeStatus": {
      const dirty = object(v.dirty);
      return { workspaceId: text(v.workspaceId), worktree: identity(v.worktree),
        dirty: { isDirty: bool(dirty.isDirty), files: array(dirty.files, file => { const f = object(file); return { statusCode: text(f.statusCode), path: text(f.path) }; }) },
        dirtyCount: uint(v.dirtyCount), branchDeletion: nullable(v.branchDeletion, preview => {
          const p = object(preview); return { branch: text(p.branch), head: text(p.head), upstream: nullable(p.upstream, text), merged: bool(p.merged), ahead: nullable(p.ahead, uint), behind: nullable(p.behind, uint) };
        }), locked: nullable(v.locked, text), prunable: nullable(v.prunable, text), liveSessionIds: array(v.liveSessionIds, text), revision: decodeU64(v.revision) };
    }
    case "sessionDetail": switch (v.status) {
      case "running": return { status: "running", session: session(v.session) };
      case "exited": return { status: "exited", session: session(v.session), exit: exitMetadata(v.exit) };
      case "expired": return { status: "expired", target: decodeRemoteTarget(v.target) };
      default: return invalid();
    }
    case "operation": {
      const id = requestId(v.requestId);
      switch (v.state) {
        case "pending": case "outcomeUnknown": case "resultExpired": return { state: v.state, requestId: id };
        case "completed": {
          const o = object(v.outcome);
          switch (o.kind) {
            case "noContent": return { state: "completed", requestId: id, outcome: { kind: "noContent" } };
            case "project": return { state: "completed", requestId: id, outcome: { kind: "project", project: project(o.project) } };
            case "worktree": return { state: "completed", requestId: id, outcome: { kind: "worktree", worktree: worktree(o.worktree) } };
            case "session": return { state: "completed", requestId: id, outcome: { kind: "session", session: session(o.session) } };
            case "error": return { state: "completed", requestId: id, outcome: { kind: "error", error: machineError(o.error) } };
            default: return invalid();
          }
        }
        default: return invalid();
      }
    }
    case "control": switch (v.type) {
      case "resize": return { type: "resize", generation: decodeU64(v.generation), cols: dimension(v.cols), rows: dimension(v.rows) };
      case "signal": return { type: "signal", generation: decodeU64(v.generation), signal: choice(v.signal, ["interrupt"]) };
      case "ping": case "pong": return { type: v.type };
      case "error": return { type: "error", error: machineError(v.error) };
      case "status": return { type: "status", target: decodeRemoteTarget(v.target), status: choice(v.status, ["running", "exited", "expired"]) };
      case "exit": return { type: "exit", target: decodeRemoteTarget(v.target), exit: exitMetadata(v.exit) };
      default: return invalid();
    }
    case "registerRequest": return { requestId: requestId(v.requestId), repoPath: text(v.repoPath) };
    case "unregisterRequest": return { requestId: requestId(v.requestId), expectedRevision: decodeU64(v.expectedRevision) };
    case "createWorktreeRequest": return { requestId: requestId(v.requestId), workspaceId: text(v.workspaceId), worktree: identity(v.worktree), ...(v.baseRef === undefined ? {} : { baseRef: text(v.baseRef) }) };
    case "deleteWorktreeRequest": return { requestId: requestId(v.requestId), workspaceId: text(v.workspaceId), worktree: identity(v.worktree), deleteBranch: bool(v.deleteBranch), expectedRevision: decodeU64(v.expectedRevision) };
    case "createSessionRequest": return { requestId: requestId(v.requestId), workspaceId: text(v.workspaceId), worktree: nullable(v.worktree, identity), cols: dimension(v.cols), rows: dimension(v.rows), inheritFromSessionId: nullable(v.inheritFromSessionId, text), cwdRelative: nullable(v.cwdRelative, text), startup: startup(v.startup) };
    case "closeSessionRequest": return { requestId: requestId(v.requestId), daemonEpoch: decodeU64(v.daemonEpoch) };
    case "attached": {
      if (v.type !== "attached") return invalid();
      return { type: "attached", target: decodeRemoteTarget(v.target), generation: decodeU64(v.generation), cols: dimension(v.cols), rows: dimension(v.rows),
        startSequence: decodeU64(v.startSequence), endSequence: decodeU64(v.endSequence),
        replayGap: nullable(v.replayGap, gap => { const g = object(gap); return { requestedAfterSequence: decodeU64(g.requestedAfterSequence), availableFromSequence: decodeU64(g.availableFromSequence) }; }) };
    }
    default: return invalid();
  }
}

export type RemoteTerminalTarget = ReturnType<typeof decodeRemoteTarget>;
export type MachineProject = ReturnType<typeof project>;
export type MachineSession = ReturnType<typeof session>;
export type MachineWorktree = ReturnType<typeof worktree>;
