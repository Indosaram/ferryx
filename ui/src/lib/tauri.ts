export const DEFAULT_TERMINAL_FONT_STACK = '"Geist Mono", "JetBrains Mono", "MesloLGS NF", "Noto Sans KR", monospace';
import { defaultRemoteClient, getRemoteAuthToken } from "./remoteClient";
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

export type RegisteredProject = {
  workspaceId: string;
  repoRoot: string;
};

export type LocalBranch = {
  name: string;
  isCurrent: boolean;
};

export type TerminalThemeColors = {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  selectionBackground: string;
  selectionForeground?: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
  extendedAnsi?: string[];
};

export type TerminalPreferences = {
  fontFamily: string;
  fontSize: number;
  macosOptionAsAlt: boolean;
  cursorStyle: string;
  theme: TerminalThemeColors;
  source: "defaults" | "ghostty";
  status: "imported" | "absent" | "malformed";
  sourcePath: string | null;
};

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

export async function registerProject(request: { workspaceId: string; repoPath: string }) {
  return invokeCommand<RegisteredProject>("cmd_project_register", { request });
}

export async function listProjectBranches(workspaceId: string) {
  return invokeCommand<LocalBranch[]>("cmd_project_branches", { request: { workspaceId } });
}

export async function getTerminalPreferences(): Promise<TerminalPreferences> {
  if (!isTauri()) {
    if (getRemoteAuthToken()) {
      try {
        return await defaultRemoteClient.fetchJson<TerminalPreferences>("/api/v1/terminal/preferences");
      } catch {
        // Fallback below
      }
    }
    return {
      fontFamily: DEFAULT_TERMINAL_FONT_STACK,
      fontSize: 13,
      macosOptionAsAlt: false,
      cursorStyle: "block",
      theme: {
        background: "#0a0a0a",
        foreground: "#d4d4d4",
        cursor: "#e5e5e5",
        cursorAccent: "#0a0a0a",
        selectionBackground: "#52525299",
        black: "#171717",
        red: "#f87171",
        green: "#86efac",
        yellow: "#fde68a",
        blue: "#93c5fd",
        magenta: "#d8b4fe",
        cyan: "#67e8f9",
        white: "#e5e5e5",
        brightBlack: "#737373",
        brightRed: "#fca5a5",
        brightGreen: "#bbf7d0",
        brightYellow: "#fef08a",
        brightBlue: "#bfdbfe",
        brightMagenta: "#e9d5ff",
        brightCyan: "#a5f3fc",
        brightWhite: "#fafafa",
        extendedAnsi: [],
      },
      source: "defaults",
      status: "absent",
      sourcePath: null,
    };
  }
  return invokeCommand<TerminalPreferences>("cmd_terminal_preferences");
}

export async function listWorktrees(workspaceId: string) {
  if (!isTauri()) {
    if (getRemoteAuthToken()) {
      return defaultRemoteClient.listWorktrees(workspaceId);
    }
    return [] as Worktree[];
  }
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

export async function waitForTerminalExit(_sessionId: string, _timeoutMs = 5000) {
  return;
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


export type RemoteNetworkMode = "off" | "localNetwork" | "tailscale";

export type TailscaleStatus = {
  installed: boolean;
  running: boolean;
  tailnetName: string | null;
  selfDns: string | null;
  serveActive: boolean;
};

export type RemoteGatewayStatus = {
  enabled: boolean;
  mode: RemoteNetworkMode;
  port: number;
  boundAddress: string | null;
  localIp: string | null;
  tailscale: TailscaleStatus;
};

export type DeviceInfo = {
  id: string;
  name: string;
  permission: "view" | "control";
  createdAt: number;
  lastSeenAt: number;
  revoked: boolean;
};

export type CreatePairingCodeResponse = {
  code: string;
  expiresInSeconds: number;
};

export async function getRemoteStatus(): Promise<RemoteGatewayStatus> {
  if (!isTauri()) {
    return {
      enabled: false,
      mode: "off",
      port: 43821,
      boundAddress: null,
      localIp: null,
      tailscale: {
        installed: false,
        running: false,
        tailnetName: null,
        selfDns: null,
        serveActive: false,
      },
    };
  }
  return invokeCommand<RemoteGatewayStatus>("cmd_remote_status");
}

export async function enableRemoteGateway(request: {
  mode: RemoteNetworkMode;
  port?: number;
  allowControl?: boolean;
}): Promise<RemoteGatewayStatus> {
  return invokeCommand<RemoteGatewayStatus>("cmd_remote_enable", { request });
}

export async function disableRemoteGateway(): Promise<RemoteGatewayStatus> {
  return invokeCommand<RemoteGatewayStatus>("cmd_remote_disable");
}

export async function createPairingCode(permission?: "view" | "control"): Promise<CreatePairingCodeResponse> {
  return invokeCommand<CreatePairingCodeResponse>("cmd_remote_pairing_create", { permission });
}

export async function listRemoteDevices(): Promise<DeviceInfo[]> {
  if (!isTauri()) return [];
  return invokeCommand<DeviceInfo[]>("cmd_remote_devices");
}

export async function revokeRemoteDevice(deviceId: string): Promise<boolean> {
  return invokeCommand<boolean>("cmd_remote_device_revoke", { deviceId });
}

export async function getTailscaleStatus(): Promise<TailscaleStatus> {
  if (!isTauri()) {
    return {
      installed: false,
      running: false,
      tailnetName: null,
      selfDns: null,
      serveActive: false,
    };
  }
  return invokeCommand<TailscaleStatus>("cmd_tailscale_status");
}


export async function dispatchNotification(req: import('./types').DispatchNotificationArgs): Promise<import('./types').DispatchNotificationResult> {
  return invokeCommand<import('./types').DispatchNotificationResult>('cmd_notification_dispatch', { req });
}

export async function getNotificationPermissionStatus(): Promise<import('./types').NotificationPermissionStatus> {
  return invokeCommand<import('./types').NotificationPermissionStatus>('cmd_notification_get_permission_status');
}

export async function requestNotificationPermission(): Promise<import('./types').NotificationPermissionRequest> {
  return invokeCommand<import('./types').NotificationPermissionRequest>('cmd_notification_request_permission');
}

export async function probeNotificationDelivery(force?: boolean): Promise<import('./types').NotificationProbeResult> {
  return invokeCommand<import('./types').NotificationProbeResult>('cmd_notification_probe_delivery', { force });
}

export async function openNotificationSystemSettings(): Promise<import('./types').OpenSystemSettingsResult> {
  return invokeCommand<import('./types').OpenSystemSettingsResult>('cmd_notification_open_system_settings');
}

export async function playNotificationSound(args: {
  soundId: string;
  customSoundPath?: string | null;
  volume?: number;
  force?: boolean;
}): Promise<import('./types').PlaySoundResult> {
  return invokeCommand<import('./types').PlaySoundResult>('cmd_notification_play_sound', {
    soundId: args.soundId,
    customSoundPath: args.customSoundPath ?? null,
    volume: args.volume ?? 1.0,
    force: args.force ?? false,
  });
}

export async function pickNotificationAudio(): Promise<import('./types').PickedAudioFile | null> {
  return invokeCommand<import('./types').PickedAudioFile | null>('cmd_notification_pick_audio');
}

export async function saveSession(session: import('./types').PersistedWorkspaceSession): Promise<void> {
  if (!isTauri()) return;
  return invokeCommand<void>('cmd_session_save', { session });
}

export async function loadSession(): Promise<import('./types').PersistedWorkspaceSession | null> {
  if (!isTauri()) return null;
  return invokeCommand<import('./types').PersistedWorkspaceSession | null>('cmd_session_load');
}

export async function clearSession(): Promise<void> {
  if (!isTauri()) return;
  return invokeCommand<void>('cmd_session_clear');
}

