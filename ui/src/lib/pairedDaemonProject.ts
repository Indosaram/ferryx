import { invoke, isTauri } from "@tauri-apps/api/core";
import { remoteHostStore, type RemoteHostStore } from "../state/remoteHostStore";
import { decodeMachineJson, decodePairedProject, decodeU64 } from "./pairedDaemonContracts";

export interface PairedHostContext { readonly hostId: string; readonly generation: string }
export interface WorktreeIdentity { wsId: string; slug: string }
export interface RegisterProjectRequest { requestId: string; repoPath: string }
export interface UnregisterProjectRequest { requestId: string; expectedRevision: string }
export interface CreateWorktreeRequest { requestId: string; workspaceId: string; worktree: WorktreeIdentity; baseRef?: string }
export interface DeleteWorktreeRequest extends Omit<CreateWorktreeRequest, "baseRef"> { deleteBranch: boolean; expectedRevision: string }
export interface CreateSessionRequest {
  requestId: string; workspaceId: string; worktree: WorktreeIdentity | null; cols: number; rows: number;
  inheritFromSessionId: string | null; cwdRelative: string | null;
  startup: { kind: "shell" } | { kind: "agentResume"; agentType: string; providerSession: { key: "session_id" | "conversation_id"; id: string; transcriptPath?: string } };
}
export interface CloseSessionRequest { requestId: string; daemonEpoch: string }
export type PairedHostOperation =
  | { kind: "capabilities" } | { kind: "directories"; path?: string; includeHidden: boolean }
  | { kind: "projects" } | { kind: "registerProject"; request: RegisterProjectRequest }
  | { kind: "unregisterProject"; workspaceId: string; request: UnregisterProjectRequest }
  | { kind: "worktrees"; workspaceId: string }
  | { kind: "worktreeStatus"; workspaceId: string; worktree: WorktreeIdentity }
  | { kind: "createWorktree"; request: CreateWorktreeRequest }
  | { kind: "deleteWorktree"; request: DeleteWorktreeRequest }
  | { kind: "sessions"; workspaceId?: string }
  | { kind: "session"; sessionId: string; daemonEpoch: string }
  | { kind: "createSession"; request: CreateSessionRequest }
  | { kind: "closeSession"; sessionId: string; request: CloseSessionRequest }
  | { kind: "operation"; requestId: string };
export interface PairedHostOperationRequest extends PairedHostContext { operation: PairedHostOperation }
export type PairedHostInvoke = (command: "paired_host_operation", args: { request: PairedHostOperationRequest }) => Promise<unknown>;
export class PairedOperationError extends Error {
  constructor(readonly code: string, readonly retryable = false, readonly requestId?: string,
    readonly details: Record<string, unknown> = {}, readonly ambiguous = false) { super(code); this.name = "PairedOperationError"; }
}
function invalid(code = "INVALID_RESPONSE"): never { throw new PairedOperationError(code); }
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return invalid();
  return Object.fromEntries(Object.entries(value));
}
function remoteId(value: unknown): string {
  if (typeof value !== "string" || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,255}$/.test(value) || value === "." || value === "..") return invalid("INVALID_REMOTE_WORKSPACE_ID");
  return value;
}
function desktopId(value: unknown): string {
  if (typeof value !== "string" || !/^daemon:[a-f0-9]{64}$/.test(value)) return invalid();
  return value;
}
function json(kind: string, value: unknown) { return decodeMachineJson(kind, JSON.stringify(value)); }
function capabilities(value: unknown) { const v = json("capabilities", value); if (!("apiVersion" in v) || v.apiVersion === undefined) return invalid(); return v; }
function directories(value: unknown) { const v = json("directories", value); if (!("homePath" in v) || v.homePath === undefined) return invalid(); return v; }
function projectMetadata(value: unknown) { const v = json("project", value); if (!("availability" in v) || v.availability === undefined) return invalid(); return v; }
function projectList(value: unknown) { const v = json("projects", value); if (!("projects" in v) || v.projects === undefined) return invalid(); return v; }
function worktrees(value: unknown) { const v = json("worktrees", value); if (!("worktrees" in v) || v.worktrees === undefined) return invalid(); return v; }
function worktree(value: unknown) { const v = json("worktree", value); if (!("managed" in v) || v.managed === undefined) return invalid(); return v; }
function worktreeStatus(value: unknown) { const v = json("worktreeStatus", value); if (!("dirtyCount" in v) || v.dirtyCount === undefined) return invalid(); return v; }
function sessions(value: unknown) { const v = json("sessions", value); if (!("sessions" in v) || v.sessions === undefined) return invalid(); return v; }
function session(value: unknown) { const v = json("session", value); if (!("startSequence" in v) || v.startSequence === undefined) return invalid(); return v; }
function sessionDetail(value: unknown) { const v = json("sessionDetail", value); if (!("status" in v) || v.status === undefined) return invalid(); return v; }
function operation(value: unknown) { const v = json("operation", value); if (!("state" in v) || v.state === undefined) return invalid(); return v; }
function machineError(value: unknown) { const v = json("error", value); if (!("error" in v) || v.error === undefined) return invalid(); return v; }
function pairedProject(value: unknown, hostId: string) {
  const base = decodePairedProject(value);
  if (base.target.hostId !== hostId) return invalid("CROSS_HOST_RESULT");
  remoteId(base.remoteWorkspaceId);
  return { ...projectMetadata(value), ...base };
}
function mappedProjects(value: unknown, hostId: string) {
  const list = projectList(value), raw = object(value);
  if (!Array.isArray(raw.projects)) return invalid();
  return { ...list, projects: raw.projects.map(row => pairedProject(row, hostId)),
    unavailableWorkspaceIds: list.unavailableWorkspaceIds.map(desktopId) };
}
const requiredCapability = {
  directories: "directoryBrowseV1", projects: "machineWorkspaceV1", registerProject: "machineWorkspaceV1",
  unregisterProject: "machineWorkspaceV1", worktrees: "managedWorktreesV1", worktreeStatus: "managedWorktreesV1",
  createWorktree: "managedWorktreesV1", deleteWorktree: "managedWorktreesV1", sessions: "terminalCreateV1",
  session: "terminalCreateV1", createSession: "terminalCreateV1", closeSession: "terminalCreateV1", operation: "machineWorkspaceV1",
};
const knownCapabilities = new Set([...Object.values(requiredCapability), "terminalStreamV1", "machineEventsV1"]);
function safeError(value: unknown): PairedOperationError {
  try {
    const native = object(value);
    const envelope = "machineError" in native ? { error: native.machineError } : native;
    if ("machineError" in native && native.machineError === null) {
      if (typeof native.code !== "string" || !/^[A-Z][A-Z0-9_]{0,63}$/.test(native.code)
        || typeof native.ambiguous !== "boolean" || !(native.requestId === null || typeof native.requestId === "string")) return invalid();
      if (native.requestId !== null) json("operation", { state: "pending", requestId: native.requestId });
      return new PairedOperationError(native.code, false, native.requestId ?? undefined, {}, native.ambiguous);
    }
    const parsed = machineError(envelope).error;
    if (!/^[A-Z][A-Z0-9_]{0,63}$/.test(parsed.code)) return invalid();
    // Native sanitizes machine details; never expose arbitrary rejection text or body.
    return new PairedOperationError(parsed.code, parsed.retryable, parsed.requestId, parsed.details, native.ambiguous === true);
  } catch { return new PairedOperationError("PAIRED_HOST_UNAVAILABLE"); }
}
/** Captured owner only. No active-host routing, credential access, retries, or UI feature enablement. */
export function createPairedDaemonProjectAdapter(
  context: PairedHostContext,
  dependencies: { store?: RemoteHostStore; invoke?: PairedHostInvoke; isNative?: () => boolean } = {},
) {
  const hostId = context.hostId, generation = decodeU64(context.generation);
  const store = dependencies.store ?? remoteHostStore;
  const native = dependencies.isNative ?? isTauri;
  const call = dependencies.invoke ?? ((command, args) => invoke<unknown>(command, args));
  let negotiated: ReturnType<typeof capabilities> | undefined;
  function current() {
    if (!native() || store.getState().nativeStatus !== "ready") return invalid("NATIVE_CONTEXT_REQUIRED");
    const host = store.getState().hosts[hostId];
    if (!host || host.generation !== generation) return invalid("STALE_HOST_GENERATION");
    if (host.authStatus !== "paired" || host.grantScope !== "machine") return invalid("MACHINE_GRANT_REQUIRED");
    if (!host.machineId) return invalid("NATIVE_CONTEXT_REQUIRED");
    return host;
  }
  async function execute<T>(operation: PairedHostOperation, parse: (value: unknown) => T): Promise<T> {
    // A rolled-back or incompatible peer must not inherit earlier admission.
    if (operation.kind === "capabilities") negotiated = undefined;
    const host = current();
    if (operation.kind !== "capabilities" && (!negotiated || !negotiated.capabilities.includes(requiredCapability[operation.kind]))) return invalid("UNSUPPORTED_CAPABILITY");
    if (operation.kind === "operation") json("operation", { state: "pending", requestId: operation.requestId });
    if ("workspaceId" in operation && operation.workspaceId !== undefined) remoteId(operation.workspaceId);
    if ("request" in operation) {
      const kinds = { registerProject: "registerRequest", unregisterProject: "unregisterRequest", createWorktree: "createWorktreeRequest", deleteWorktree: "deleteWorktreeRequest", createSession: "createSessionRequest", closeSession: "closeSessionRequest" };
      json(kinds[operation.kind], operation.request);
      if ("workspaceId" in operation.request) remoteId(operation.request.workspaceId);
    }
    let raw: unknown;
    try { raw = await call("paired_host_operation", { request: { hostId, generation, operation } }); }
    catch (error) { current(); throw safeError(error); }
    current();
    const envelope = object(raw);
    if (envelope.hostId !== hostId || envelope.generation !== generation) return invalid("CROSS_HOST_RESULT");
    const result = object(envelope.result);
    if (result.kind !== operation.kind) return invalid();
    // Check every remote session target, including journal outcomes, before parsing/adoption.
    function provenance(value: unknown): void {
      if (!value || typeof value !== "object") return;
      const row = object(value);
      if ("machineId" in row && row.machineId !== host.machineId) return invalid("CROSS_HOST_RESULT");
      if ("remoteWorkspaceId" in row) remoteId(row.remoteWorkspaceId);
      for (const child of Object.values(row)) {
        if (Array.isArray(child)) child.forEach(provenance); else provenance(child);
      }
    }
    provenance(result.data);
    if (operation.kind === "session") {
      const detail = object(result.data);
      const target = object(detail.status === "expired" ? detail.target : object(detail.session).target);
      if (target.sessionId !== operation.sessionId || target.daemonEpoch !== operation.daemonEpoch) return invalid("CROSS_HOST_RESULT");
    }
    return parse(result.data);
  }
  return {
    context: Object.freeze({ hostId, generation }),
    capabilities: () => execute({ kind: "capabilities" }, value => {
      const result = capabilities(value);
      if (result.accessScope !== "machine" || result.permission !== "control" || result.capabilities.some(c => !knownCapabilities.has(c))) return invalid("UNSUPPORTED_CAPABILITY");
      negotiated = result;
      return result;
    }),
    directories: (path?: string, includeHidden = false) => execute({ kind: "directories", path, includeHidden }, directories),
    projects: () => execute({ kind: "projects" }, value => mappedProjects(value, hostId)),
    registerProject: (request: RegisterProjectRequest) => execute({ kind: "registerProject", request }, value => pairedProject(value, hostId)),
    unregisterProject: (workspaceId: string, request: UnregisterProjectRequest) => execute({ kind: "unregisterProject", workspaceId, request }, noContent),
    worktrees: (workspaceId: string) => execute({ kind: "worktrees", workspaceId }, worktrees),
    worktreeStatus: (workspaceId: string, worktree: WorktreeIdentity) => execute({ kind: "worktreeStatus", workspaceId, worktree }, worktreeStatus),
    createWorktree: (request: CreateWorktreeRequest) => execute({ kind: "createWorktree", request }, worktree),
    deleteWorktree: (request: DeleteWorktreeRequest) => execute({ kind: "deleteWorktree", request }, noContent),
    sessions: (workspaceId?: string) => execute({ kind: "sessions", workspaceId }, sessions),
    session: (sessionId: string, daemonEpoch: string) => execute({ kind: "session", sessionId, daemonEpoch: decodeU64(daemonEpoch) }, sessionDetail),
    createSession: (request: CreateSessionRequest) => execute({ kind: "createSession", request }, session),
    closeSession: (sessionId: string, request: CloseSessionRequest) => execute({ kind: "closeSession", sessionId, request }, noContent),
    operation: (requestId: string) => execute({ kind: "operation", requestId }, value => {
      const parsed = operation(value);
      if (parsed.requestId !== requestId) return invalid();
      // Journal outcomes are remote machine DTOs, not registered desktop references.
      // Only registerProject/projects return Rust-mapped desktop project IDs.
      return parsed;
    }),
  };
}
function noContent(value: unknown): null { if (value !== null) return invalid(); return null; }
export type PairedDaemonProjectAdapter = ReturnType<typeof createPairedDaemonProjectAdapter>;
