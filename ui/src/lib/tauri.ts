import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  BranchDeletionPreview,
  DirtyState,
  StructuredIpcError,
  TerminalLifecyclePayload,
  TerminalOutputPayload,
  TerminalSessionSummary,
  TerminalSignal,
  Worktree,
  WorktreeChangedPayload,
  WorktreeIdentity,
} from "./types";

export const DEFAULT_WORKSPACE_ID = "default";

type WorktreeStatusRequest = {
  workspaceId: string;
  worktree: WorktreeIdentity;
};

type DeleteWorktreeRequest = WorktreeStatusRequest & {
  deleteBranch?: boolean | null;
};

export function isTauriRuntime() {
  return isTauri();
}

export async function listWorktrees(workspaceId: string) {
  if (!isTauri()) return [] as Worktree[];
  return invokeCommand<Worktree[]>("cmd_worktree_list", { workspaceId });
}

export async function createWorktree(request: {
  workspaceId: string;
  worktree: WorktreeIdentity;
  baseRef?: string | null;
}) {
  return invokeCommand<Worktree>("cmd_worktree_create", {
    request: {
      workspaceId: request.workspaceId,
      worktree: request.worktree,
      baseRef: request.baseRef ?? null,
    },
  });
}

export async function getWorktreeStatus(request: WorktreeStatusRequest) {
  return invokeCommand<DirtyState>("cmd_worktree_status", { request });
}

export async function previewWorktreeDelete(request: WorktreeStatusRequest) {
  return invokeCommand<BranchDeletionPreview>("cmd_worktree_delete_preview", { request });
}

export async function deleteWorktree(request: DeleteWorktreeRequest) {
  await invokeCommand<void>("cmd_worktree_delete", {
    request: {
      workspaceId: request.workspaceId,
      worktree: request.worktree,
      deleteBranch: request.deleteBranch ?? null,
    },
  });
}

export async function deleteWorktreeDestructive(request: DeleteWorktreeRequest) {
  await invokeCommand<void>("cmd_worktree_delete_destructive", {
    request: {
      workspaceId: request.workspaceId,
      worktree: request.worktree,
      deleteBranch: request.deleteBranch ?? null,
    },
  });
}

export async function spawnTerminal(request: { workspaceId: string; worktree: WorktreeIdentity | null }) {
  if (!isTauri()) return `preview:${request.workspaceId}:${request.worktree?.slug ?? "root"}:${crypto.randomUUID()}`;
  const response = await invokeCommand<{ sessionId: string }>("cmd_terminal_spawn", { request });
  return response.sessionId;
}

export async function writeTerminal(request: { sessionId: string; data: string }) {
  if (!isTauri()) return;
  await invokeCommand<void>("cmd_terminal_write", request);
}

export async function resizeTerminal(request: { sessionId: string; cols: number; rows: number }) {
  if (!isTauri()) return;
  await invokeCommand<void>("cmd_terminal_resize", request);
}

export async function signalTerminal(request: { sessionId: string; signal: TerminalSignal }) {
  if (!isTauri()) return;
  await invokeCommand<void>("cmd_terminal_signal", request);
}

export async function closeTerminal(sessionId: string) {
  if (!isTauri()) return;
  await invokeCommand<void>("cmd_terminal_close", { sessionId });
}

export async function listTerminalSessions() {
  if (!isTauri()) return [] as TerminalSessionSummary[];
  return invokeCommand<TerminalSessionSummary[]>("cmd_terminal_list");
}

export async function onTerminalOutput(handler: (payload: TerminalOutputPayload) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<TerminalOutputPayload>("terminal_output", (event) => handler(event.payload));
}

export async function onTerminalLifecycle(handler: (payload: TerminalLifecyclePayload) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<TerminalLifecyclePayload>("terminal_lifecycle", (event) => handler(event.payload));
}

export async function onWorktreeChanged(handler: (payload: WorktreeChangedPayload) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<WorktreeChangedPayload>("worktree_changed", (event) => handler(event.payload));
}

export function toIpcError(error: unknown): StructuredIpcError {
  if (isStructuredIpcError(error)) {
    return { code: error.code, message: error.message, details: error.details ?? {} };
  }
  return {
    code: "UNKNOWN",
    message: error instanceof Error ? error.message : "Unknown IPC error",
    details: {},
  };
}

export function isStructuredIpcError(error: unknown): error is StructuredIpcError {
  if (!error || typeof error !== "object") return false;
  const candidate = error as Partial<StructuredIpcError>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.message === "string" &&
    (candidate.details === undefined ||
      (typeof candidate.details === "object" && candidate.details !== null && !Array.isArray(candidate.details)))
  );
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toIpcError(error);
  }
}
