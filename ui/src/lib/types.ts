import type { PaneNode } from "../state/paneTree";

export type WorktreeIdentity = {
  wsId: string;
  slug: string;
};

export type Worktree = {
  path: string;
  head: string;
  branch: string | null;
  bare: boolean;
  detached: boolean;
  locked: string | null;
  prunable: string | null;
};

export function worktreeIdentity(worktree: Worktree): WorktreeIdentity | null {
  const branch = worktree.branch?.replace(/^refs\/heads\//, "");
  const parts = branch?.split("/");
  if (!parts || parts.length < 3 || parts[0] !== "orca") return null;

  return { wsId: parts[1], slug: parts.slice(2).join("/") };
}

export type DirtyFile = {
  statusCode: string;
  path: string;
};

export type DirtyState = {
  isDirty: boolean;
  files: DirtyFile[];
};

export type BranchDeletionPreview = {
  branch: string;
  head: string;
  upstream: string | null;
  merged: boolean;
  ahead: number | null;
  behind: number | null;
};

export type TerminalSignal = "interrupt" | "terminate" | "kill";

export type TerminalSessionSummary = {
  sessionId: string;
  worktreePath: string | null;
};

export type TerminalLifecycle = "starting" | "working" | "waiting" | "exited" | "failed";

export type TerminalSession = {
  id: string;
  cwd: string;
  workspaceId: string;
  worktree: WorktreeIdentity | null;
  backendSessionId: string | null;
  lifecycle: TerminalLifecycle;
  ownerId?: string | null;
};

export type TerminalTab = {
  kind?: "terminal";
  id: string;
  label: string;
  sessionId: string;
  pinned?: boolean;
};

export type BrowserProfileId = "default" | "private";

export type LogicalRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export type BrowserState = {
  browserId: string;
  webviewLabel: string;
  workspaceId?: string | null;
  worktreePath?: string | null;
  profileId: BrowserProfileId;
  generation: number;
  url: string;
  title?: string | null;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  zoomFactor: number;
  loadError?: string | null;
  visible: boolean;
};

export type BrowserTab = {
  kind: "browser";
  id: string;
  label: string;
  browserId: string;
  url: string;
  title?: string | null;
  loading?: boolean;
  canGoBack?: boolean;
  canGoForward?: boolean;
  pinned?: boolean;
};

export type WorkspaceTab = TerminalTab | BrowserTab;

export type CreateBrowserRequest = {
  workspaceId?: string | null;
  worktreePath?: string | null;
  url: string;
  profile?: BrowserProfileId;
  bounds?: LogicalRect;
  visible?: boolean;
};

export type BrowserSessionSummary = {
  browserId: string;
  webviewLabel: string;
  workspaceId?: string | null;
  url: string;
  title?: string | null;
  visible: boolean;
};

export type Pane = {
  id: string;
  tabId: string;
};

export type SplitMode = "none" | "horizontal" | "vertical";

export type NestedSplit = {
  orientation: Exclude<SplitMode, "none">;
  tabId: string;
};

/**
 * The split arrangement of a single tab. Every tab owns an independent pane tree,
 * so splitting one tab never disturbs the panes of another.
 */
export type TabPaneLayout = {
  root: PaneNode;
  /** Leaf that receives keyboard focus inside this tab. */
  activeLeafId: string | null;
  /** Leaf temporarily zoomed to fill the tab, or `null` when every pane is visible. */
  expandedLeafId: string | null;
  /** Terminal session rendered by each leaf of `root`. */
  sessionIdsByLeafId: Record<string, string>;
};

export type LayoutState = {
  tabs: WorkspaceTab[];
  primaryTabId?: string | null;
  secondaryTabId?: string | null;
  split?: SplitMode;
  nestedSplit?: NestedSplit | null;
  activeTabId: string | null;
  layoutsByTabId: Record<string, TabPaneLayout>;
};

export type AgentState = "starting" | "working" | "waiting" | "exited" | "failed";

export type ActiveAgent = {
  id: string;
  name: string;
  task: string;
  state: AgentState;
  worktree: WorktreeIdentity | null;
  worktreePath: string;
  sessionId: string;
};

export type TerminalOutputPayload = {
  sessionId: string;
  data: string;
};

export type TerminalLifecyclePayload = {
  sessionId: string;
  state: "started" | "exited" | "failed";
  exitCode: number | null;
  reason: string | null;
};

export type WorktreeChangedPayload = {
  workspaceId: string;
  worktree: WorktreeIdentity;
  kind: "created" | "deleted" | "destructivelyDeleted" | "dirtyChanged" | "pruned";
};

export type NotificationSource = "agent-task-complete" | "terminal-bell" | "system";
export type NotificationAuthorization = "not-determined" | "denied" | "authorized" | "provisional" | "unknown";
export type NotificationPlatform = "macos" | "windows" | "linux" | "unknown";

export interface NotificationPermissionStatus {
  platform?: NotificationPlatform;
  supported?: boolean;
  authorization: NotificationAuthorization;
  alertsEnabled?: boolean | null;
  soundsEnabled?: boolean | null;
  requested?: boolean;
  authoritative?: boolean;
  canOpenSettings?: boolean;
  status?: string;
}

export interface NotificationPermissionRequest {
  granted: boolean;
  status?: NotificationPermissionStatus;
  error?: string | null;
}

export interface DispatchNotificationArgs {
  source?: NotificationSource | string;
  title?: string;
  body?: string;
  notificationId?: string;
  workspaceLabel?: string;
  worktreeLabel?: string;
  terminalTitle?: string;
  agentLabel?: string;
}

export interface DispatchNotificationResult {
  submitted?: boolean;
  delivered?: boolean;
  reason?: "permission-required" | "blocked-by-system" | "unsupported" | "backend-error" | string;
}

export interface NotificationProbeResult {
  outcome?: "submitted" | "ready" | "permission-required" | "blocked-by-system" | "unsupported" | "failed" | string;
  status?: NotificationPermissionStatus;
  testSubmitted?: boolean;
  success?: boolean;
}

export interface OpenSystemSettingsResult {
  opened: boolean;
  reason?: string | null;
}

export interface PlaySoundResult {
  played: boolean;
  reason?: string | null;
}

export interface PickedAudioFile {
  path: string;
  displayName?: string;
  extension?: string;
  sizeBytes?: number;
}

export type StructuredIpcError = {
  code: string;
  message: string;
  details?: Record<string, unknown>;
};

export interface PersistedWorktree {
  path: string;
  branch: string;
  head: string;
  isMain: boolean;
  isLocked: boolean;
}

export interface PersistedTab {
  id: string;
  sessionId: string;
  label: string;
  customTitle?: string;
  worktreePath: string;
  paneTree?: PaneNode;
  sessionIdsByLeafId?: Record<string, string>;
  activeLeafId?: string | null;
}

export interface PersistedLayout {
  splitMode: string;
  primaryTabId: string | null;
  secondaryTabId: string | null;
  activeTabId: string | null;
  tabs: PersistedTab[];
}

export interface PersistedTerminalSession {
  sessionId: string;
  worktreePath: string;
  cwd: string;
  lastCommand?: string;
  recentScrollback?: string;
  createdAt: number;
}

export interface PersistedWorkspace {
  workspaceId: string;
  repoRoot: string;
  worktrees: PersistedWorktree[];
  activeWorktreePath: string | null;
  layout: PersistedLayout;
  terminalSessions: Record<string, PersistedTerminalSession>;
}

export interface PersistedWorkspaceSession {
  version: number;
  timestamp: number;
  activeWorkspaceId: string;
  workspaces: Record<string, PersistedWorkspace>;
}
