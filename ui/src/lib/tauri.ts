import type { DagRunSnapshot } from "./dagTypes";
export const DEFAULT_TERMINAL_FONT_STACK = 'MesloLGS NF, "Noto Sans KR", monospace';
import { defaultRemoteClient, getRemoteAuthToken } from "./remoteClient";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type {
  AttachTerminalRequest,
  AttachTerminalResponse,
  CliLauncherStatus,
  NativeTerminalAgentStatePayload,
  NativeTerminalScrollbarPayload,
  NotificationBadgeResult,
  SetBadgeCountResult,
  TerminalLifecyclePayload,
  TerminalOutputPayload,
  TerminalReplayGap,
  TerminalSessionSummary,
  TerminalSignal,
  WorktreeChangedPayload,
};

import type {
  AgentProviderSession,
  AttachTerminalRequest,
  AttachTerminalResponse,
  BranchDeletionPreview,
  CliLauncherStatus,
  DirtyState,
  NativeTerminalAgentStatePayload,
  NativeTerminalBellPayload,
  NativeTerminalScrollbarPayload,
  NativeTerminalTitlePayload,
  NotificationBadgeResult,
  SetBadgeCountResult,
  StructuredIpcError,
  TerminalLifecyclePayload,
  TerminalOutputPayload,
  TerminalReplayGap,
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
  gitRoot?: string | null;
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
  defaultShell?: string | null;
};

type WorktreeStatusRequest = {
  workspaceId: string;
  worktree: WorktreeIdentity;
};

type DeleteWorktreeRequest = WorktreeStatusRequest & {
  deleteBranch?: boolean | null;
};

export type SpawnTerminalRequest = {
  workspaceId: string;
  worktree: WorktreeIdentity | null;
  cwd?: string | null;
  clientRequestId?: string | null;
  shell?: string | null;
  /** Inherit the live CWD of this backend session when `cwd` is not pinned. */
  inheritFromSessionId?: string | null;
  startup?: {
    kind: "agentResume";
    agentType: string;
    providerSession: AgentProviderSession;
  } | null;
};

export type SpawnTerminalResult = {
  sessionId: string;
  daemonEpoch: string;
  session: {
    sessionId: string;
    workspaceId?: string | null;
    worktree?: WorktreeIdentity | null;
    cwd?: string | null;
    cols: number;
    rows: number;
    running: boolean;
  };
};

export function isTauriRuntime() {
  return isTauri();
}

export async function registerProject(request: { workspaceId: string; repoPath: string }) {
  return invokeCommand<RegisteredProject>("cmd_project_register", { request });
}

export async function unregisterProject(request: { workspaceId: string }) {
  if (!isTauri()) return;
  return invokeCommand<void>("cmd_project_unregister", { request });
}

export async function revealPath(path: string) {
  return invokeCommand<void>("cmd_path_reveal", { path });
}

export async function getInitialProject() {
  return invokeCommand<RegisteredProject>("cmd_project_initial");
}

export async function bootTrace(
  stage: string,
  details?: Record<string, unknown>,
) {
  if (!isTauri()) return;
  await invokeCommand("cmd_boot_trace", {
    request: { stage, details: details ?? {} },
  }).catch(() => {});
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
      defaultShell: null,
    };
  }
  return invokeCommand<TerminalPreferences>("cmd_terminal_preferences");
}

export type TerminalOverrides = {
  fontFamily: string | null;
  fontSize: number | null;
  macosOptionAsAlt: boolean | null;
  shell?: string | null;
};

export type TerminalOverridesRequest = TerminalOverrides;

export async function applyTerminalOverrides(
  overrides: TerminalOverrides,
): Promise<TerminalPreferences | null> {
  if (!isTauri()) return null;
  return invokeCommand<TerminalPreferences>("cmd_terminal_apply_overrides", {
    overrides: {
      ...overrides,
      shell: overrides.shell ?? null,
    },
  });
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

export async function spawnTerminalDetailed(request: SpawnTerminalRequest): Promise<SpawnTerminalResult> {
  if (!isTauri()) {
    throw {
      code: "INTERNAL_ERROR",
      message: "Terminal spawning is available only in the Ferryx desktop runtime",
      details: { runtime: "web" },
    } satisfies StructuredIpcError;
  }
  return invokeCommand<SpawnTerminalResult>("cmd_terminal_spawn", {
    request: {
      ...request,
      cwd: request.cwd ?? null,
      clientRequestId: request.clientRequestId ?? null,
      shell: request.shell ?? null,
      startup: request.startup ?? null,
      inheritFromSessionId: request.inheritFromSessionId ?? null,
    },
  });
}

export type SpawnTerminalBatchEntry = {
  index: number;
  sessionId: string | null;
  error: string | null;
};

export async function spawnTerminalsBatch(
  spawns: SpawnTerminalRequest[],
): Promise<SpawnTerminalBatchEntry[]> {
  if (!isTauri()) {
    throw {
      code: "INTERNAL_ERROR",
      message: "Terminal spawning is available only in the Ferryx desktop runtime",
      details: { runtime: "web" },
    } satisfies StructuredIpcError;
  }
  return invokeCommand<SpawnTerminalBatchEntry[]>("cmd_terminal_spawn_batch", {
    request: {
      spawns: spawns.map((request) => ({
        ...request,
        cwd: request.cwd ?? null,
        clientRequestId: request.clientRequestId ?? null,
        shell: request.shell ?? null,
        startup: request.startup ?? null,
        inheritFromSessionId: request.inheritFromSessionId ?? null,
      })),
    },
  });
}

export async function spawnTerminal(request: SpawnTerminalRequest): Promise<string> {
  return (await spawnTerminalDetailed(request)).sessionId;
}

export async function attachTerminal(
  requestOrSessionId: string | AttachTerminalRequest,
  afterSequence?: string | null,
): Promise<AttachTerminalResponse> {
  const req: AttachTerminalRequest =
    typeof requestOrSessionId === "string"
      ? { sessionId: requestOrSessionId, afterSequence: afterSequence ?? null }
      : { sessionId: requestOrSessionId.sessionId, afterSequence: requestOrSessionId.afterSequence ?? null };

  if (!isTauri()) {
    return {
      sessionId: req.sessionId,
      daemonEpoch: null,
      historyStartSequence: null,
      historyEndSequence: null,
      history: "",
      gap: null,
    };
  }
  return invokeCommand<AttachTerminalResponse>("cmd_terminal_attach", {
    sessionId: req.sessionId,
    afterSequence: req.afterSequence ?? null,
  });
}

export async function getTerminalCwd(sessionId: string): Promise<string | null> {
  if (!isTauri()) return null;
  const response = await invokeCommand<{ cwd: string }>("cmd_terminal_get_cwd", { sessionId });
  return response.cwd || null;
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
  await invokeCommand<void>("cmd_native_terminal_close", { sessionId }).catch(() => undefined);
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

/**
 * Native title and bell events are emitted from the daemon stream pump task, one per attached
 * session, so they arrive for background tabs whose panes are unmounted. Subscribe once at the
 * store rather than per pane: `TerminalSplitView` only mounts the active tab's panes, and a
 * per-pane listener would therefore observe only the foreground session.
 */
export async function onNativeTerminalTitle(
  handler: (payload: NativeTerminalTitlePayload) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<NativeTerminalTitlePayload>("native_terminal_title", (event) => handler(event.payload));
}

export async function onNativeTerminalBell(
  handler: (payload: NativeTerminalBellPayload) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<NativeTerminalBellPayload>("native_terminal_bell", (event) => handler(event.payload));
}

export async function onNativeTerminalAgentState(
  handler: (payload: NativeTerminalAgentStatePayload) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<NativeTerminalAgentStatePayload>("native_terminal_agent_state", (event) => handler(event.payload));
}

export async function onNativeTerminalScrollbar(
  handler: (payload: NativeTerminalScrollbarPayload) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<NativeTerminalScrollbarPayload>("native_terminal_scrollbar", (event) => handler(event.payload));
}

export async function setNativeTerminalScrollbarOverlay(
  sessionId: string,
  visible: boolean,
): Promise<void> {
  if (!isTauri()) return;
  return invokeCommand<void>("cmd_native_terminal_set_scrollbar_overlay", {
    sessionId,
    visible,
  });
}

export async function setNativeTerminalAttentionFrame(
  sessionId: string,
  attention: boolean,
): Promise<void> {
  if (!isTauri()) return;
  return invokeCommand<void>("cmd_native_terminal_set_attention_frame", {
    sessionId,
    attention,
  });
}

export async function onNativeTerminalFocus(
  handler: (sessionId: string) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<string>("native_terminal_focus", (event) => handler(event.payload));
}

/**
 * AppKit can consume Cmd+V before Korean IME/WebKit emits either `keydown` or `paste`.
 * The native key monitor forwards that physical shortcut here.
 */
export async function onNativeTerminalPaste(handler: () => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<void>("native_terminal_paste", () => handler());
}

/**
 * AppKit consumes Cmd+C via the native Edit menu before WebKit emits `keydown`.
 * The native key monitor forwards that physical shortcut here so the terminal pane
 * can copy any active selection or interrupt (clear the input line) when empty.
 */
export async function onNativeTerminalCopyOrInterrupt(handler: () => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<void>("native_terminal_copy_or_interrupt", () => handler());
}

export async function onNewTerminalTabMenu(handler: () => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<void>("menu_new_terminal_tab", () => handler());
}

export async function onCloseTabMenu(handler: () => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<void>("menu_close_tab", () => handler());
}

/**
 * The macOS Window menu owns Cmd+1..9, so AppKit consumes those events before the
 * webview receives a keydown. The native key monitor forwards the pressed digit
 * here instead.
 */
export async function onSelectWorktreeMenu(handler: (digit: number) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<number>("menu_select_worktree", (event) => handler(event.payload));
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

function safeStringify(value: unknown): string {
  let raw: string;
  try {
    if (typeof value === "string") {
      raw = value;
    } else if (value instanceof Error) {
      raw = value.stack || value.message || String(value);
    } else {
      raw = JSON.stringify(value) ?? String(value);
    }
  } catch {
    raw = String(value);
  }
  return raw.length > 500 ? `${raw.slice(0, 500)}...` : raw;
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    const ipcError = toIpcError(error);
    ipcError.details = {
      ...ipcError.details,
      command,
      raw: safeStringify(error),
    };
    throw ipcError;
  }
}


export type AgentDetection = {
  name: string;
  available: boolean;
  path?: string;
};

export async function detectAgents(names: string[]): Promise<AgentDetection[]> {
  if (!isTauri()) return names.map((name) => ({ name, available: false }));
  return invokeCommand<AgentDetection[]>("cmd_agents_detect", { names });
}

export async function discoverAgentProviderSession(sessionId: string, agentType: string): Promise<string | null> {
  if (!isTauri()) return null;
  return invokeCommand<string | null>("cmd_agent_session_discover", {
    sessionId,
    agentType,
  });
}

export type RemoteNetworkMode = "off" | "localNetwork" | "tailscale";

export type RemoteGatewayStatus = {
  enabled: boolean;
  mode: RemoteNetworkMode;
  port: number;
  boundAddress: string | null;
  localIp: string | null;
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


export async function dispatchNotification(req: import('./types').DispatchNotificationArgs): Promise<import('./types').DispatchNotificationResult> {
  return invokeCommand<import('./types').DispatchNotificationResult>('cmd_notification_dispatch', { request: req });
}

export async function getNotificationPermissionStatus(): Promise<import('./types').NotificationPermissionStatus> {
  return invokeCommand<import('./types').NotificationPermissionStatus>('cmd_notification_get_permission_status');
}

export async function requestNotificationPermission(): Promise<import('./types').NotificationPermissionRequest> {
  return invokeCommand<import('./types').NotificationPermissionRequest>('cmd_notification_request_permission');
}

export async function probeNotificationDelivery(sendTest?: boolean): Promise<import('./types').NotificationProbeResult> {
  return invokeCommand<import('./types').NotificationProbeResult>('cmd_notification_probe_delivery', { sendTest });
}

export async function openNotificationSystemSettings(): Promise<import('./types').OpenSystemSettingsResult> {
  return invokeCommand<import('./types').OpenSystemSettingsResult>('cmd_notification_open_system_settings');
}

export async function getSystemPermissionsStatus(): Promise<import('./types').SystemPermissionsStatus> {
  if (!isTauri()) {
    return {
      platform: 'web',
      fullDiskAccess: {
        status: 'unsupported',
        granted: false,
        canRequest: false,
        description: 'Permissions are managed by the host desktop application.',
      },
      accessibility: {
        status: 'unsupported',
        granted: false,
        canRequest: false,
        description: 'Permissions are managed by the host desktop application.',
      },
      notifications: {
        status: 'unsupported',
        granted: false,
        canRequest: false,
        description: 'Permissions are managed by the host desktop application.',
      },
      allGranted: true,
    };
  }
  return invokeCommand<import('./types').SystemPermissionsStatus>('cmd_permissions_get_status');
}

export async function openPermissionsSystemSettings(
  target: 'full_disk_access' | 'accessibility' | 'notifications' | string
): Promise<import('./types').OpenPermissionsSettingsResult> {
  if (!isTauri()) {
    return { opened: false, target, reason: 'unsupported outside desktop environment' };
  }
  return invokeCommand<import('./types').OpenPermissionsSettingsResult>('cmd_permissions_open_settings', { target });
}

export async function requestAccessibilityPermission(): Promise<boolean> {
  if (!isTauri()) {
    return false;
  }
  return invokeCommand<boolean>('cmd_permissions_request_accessibility');
}

export async function playNotificationSound(args: {
  soundId: string;
  customSoundPath?: string | null;
  volume?: number;
  force?: boolean;
}): Promise<import('./types').PlaySoundResult> {
  // The backend player resolves a file path. The built-in "system" sound rides the
  // OS banner and "none" is muted, so without a custom file there is nothing to play
  // and invoking would only fail the required `path` argument.
  if (!args.customSoundPath) {
    return { played: false };
  }
  return invokeCommand<import('./types').PlaySoundResult>('cmd_notification_play_sound', {
    path: args.customSoundPath,
    volume: Math.round((args.volume ?? 1) * 100),
    force: args.force ?? false,
  });
}

export async function pickNotificationAudio(): Promise<import('./types').PickedAudioFile | null> {
  return invokeCommand<import('./types').PickedAudioFile | null>('cmd_notification_pick_audio');
}

const MAX_U32_BADGE_COUNT = 4_294_967_295;

export function normalizeBadgeCount(count: number): number {
  if (!Number.isFinite(count)) return 0;
  return Math.min(MAX_U32_BADGE_COUNT, Math.max(0, Math.floor(count)));
}

export async function setBadgeCount(count: number): Promise<SetBadgeCountResult> {
  const normalizedCount = normalizeBadgeCount(count);
  if (!isTauri()) {
    return { supported: false, count: normalizedCount };
  }
  return invokeCommand<SetBadgeCountResult>("cmd_notification_set_badge_count", { count: normalizedCount });
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

export type RemoteTerminalTabInfo = {
  id: string;
  label: string;
  activityState?: "working" | "waiting" | "done";
  agentType?: string;
  worktreeSlug?: string;
  worktreeLabel?: string;
  sessionId?: string;
};

export type FocusedTerminalPayload = {
  workspaceId: string;
  worktreeSlug?: string | null;
  worktreeLabel?: string | null;
  backendSessionId?: string | null;
  activeTabId?: string | null;
  tabId?: string | null;
  tabs?: RemoteTerminalTabInfo[];
  terminalTabs?: RemoteTerminalTabInfo[];
};

export async function publishFocusedTerminal(payload: FocusedTerminalPayload | null): Promise<void> {
  if (!isTauri()) return;
  const terminalTabs = payload?.terminalTabs ?? payload?.tabs ?? [];
  const tabId = payload?.tabId ?? payload?.activeTabId ?? null;
  return invokeCommand<void>("cmd_remote_set_active_selection", {
    request: {
      workspaceId: payload?.workspaceId ?? null,
      worktreeSlug: payload?.worktreeSlug ?? null,
      worktreeLabel: payload?.worktreeLabel ?? null,
      sessionId: payload?.backendSessionId ?? null,
      tabId,
      terminalTabs,
    },
  });
}

export type RemoteSelectionRequestedPayload = {
  workspaceId: string;
  worktreeSlug?: string | null;
  worktreeLabel?: string | null;
  sessionId?: string | null;
  tabId?: string | null;
  activeTabId?: string | null;
};

export async function onRemoteSelectionRequested(
  handler: (payload: RemoteSelectionRequestedPayload) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<RemoteSelectionRequestedPayload>("remote_selection_requested", (event) => handler(event.payload));
}

export async function getCliLauncherStatus(): Promise<CliLauncherStatus> {
  if (!isTauri()) {
    return {
      launcherPath: "~/.local/bin/ferryx",
      isInstalled: false,
      isSymlink: false,
      currentTarget: null,
      activeExecutable: null,
      isSupported: false,
    };
  }
  return invokeCommand<CliLauncherStatus>("cmd_cli_launcher_status");
}

export async function installCliLauncher(): Promise<CliLauncherStatus> {
  if (!isTauri()) {
    throw new Error("Ferryx CLI installation is available only in the desktop app");
  }
  return invokeCommand<CliLauncherStatus>("cmd_cli_launcher_install");
}

export type DagRunUpdatedEvent = {
  projectPath: string;
  snapshot: DagRunSnapshot;
};

export async function listenDagRunUpdated(
  handler: (event: DagRunUpdatedEvent) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<DagRunUpdatedEvent>("dag-run-updated", (event) => handler(event.payload));
}

export type DagWatchProjectResult = {
  projectPath: string;
  runs: DagRunSnapshot[];
};

export async function watchDagProject(projectPath: string): Promise<DagWatchProjectResult> {
  if (!isTauri()) return { projectPath, runs: [] };
  return invokeCommand<DagWatchProjectResult>("dag_watch_project", { projectPath });
}
