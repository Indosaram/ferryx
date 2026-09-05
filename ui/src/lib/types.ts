import type { PaneDirection, PaneNode } from "../state/paneTree";

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
  daemonEpoch?: string | null;
};

export type AgentProviderSessionKey = "session_id" | "conversation_id";

export type AgentProviderSession = {
  readonly key: AgentProviderSessionKey;
  readonly id: string;
  readonly transcriptPath?: string;
};

export type TerminalLifecycle = "starting" | "working" | "waiting" | "exited" | "failed" | "running";

export type ReconnectLifecycle = "idle" | "validating" | "spawning" | "binding" | "failed";

export type TerminalSession = {
  /** Frontend-local stable identity used by pane leaves and terminal renderer ownership. */
  id: string;
  /** Best known process CWD. It may be nested below the worktree root after `cd`. */
  cwd: string;
  /** Stable worktree/repository root. Optional only for v1/test-state compatibility. */
  worktreePath?: string;
  workspaceId: string;
  worktree: WorktreeIdentity | null;
  /** Native PTY identity. This is intentionally distinct from `id`. */
  backendSessionId: string | null;
  lifecycle: TerminalLifecycle;
  ownerId?: string | null;
  daemonEpoch?: string | null;
  lastOutputSequence?: string | null;
  /** Agent type detected for this pane, e.g. "claude". Never minted by Ferryx. */
  agentType?: string | null;
  /** Session id the AGENT ITSELF generated. Ferryx never mints this. */
  agentSessionId?: string | null;
  /** Authoritative provider session reference. Never minted by Ferryx. */
  providerSession?: AgentProviderSession | null;
  /** Transient reconnect lifecycle state (idle | validating | spawning | binding | failed). Never persisted. */
  reconnectLifecycle?: ReconnectLifecycle;
  /** Transient structured error when reconnect fails. Never persisted. */
  reconnectError?: StructuredIpcError | null;
  reconnectRequestId?: string | null;
};

export type TerminalTab = {
  kind?: "terminal";
  id: string;
  label: string;
  /** Compatibility primary session; every real pane leaf is authoritative in TabPaneLayout. */
  sessionId: string;
  pinned?: boolean;
};

export type BrowserProfileId = string;

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
  zoomFactor?: number;
  loadError?: string | null;
  profileId?: string;
  worktreePath?: string;
  worktreeLabel?: string;
  pinned?: boolean;
};

export type WorkspaceTab = TerminalTab | BrowserTab;

export type CreateBrowserRequest = {
  browserId?: string | null;
  workspaceId?: string | null;
  worktreePath?: string | null;
  url: string;
  profile?: BrowserProfileId;
  zoomFactor?: number;
  bounds?: LogicalRect;
  visible?: boolean;
};

export type BrowserSessionSummary = {
  browserId: string;
  webviewLabel: string;
  workspaceId?: string | null;
  profileId?: string | null;
  url: string;
  title?: string | null;
  visible: boolean;
};

export type BrowserAutomationElement = {
  reference: string;
  role: string;
  name: string;
  tagName: string;
};

export type BrowserAutomationSnapshot = {
  browserId: string;
  generation: number;
  url: string;
  title: string;
  elements: BrowserAutomationElement[];
};

export type BrowserAutomationClickAction = {
  type: "click";
  reference: string;
};

export type BrowserAutomationFillAction = {
  type: "fill";
  reference: string;
  value: string;
};

export type BrowserAutomationKeypressAction = {
  type: "keypress";
  key: string;
};

export type BrowserAutomationAction =
  | BrowserAutomationClickAction
  | BrowserAutomationFillAction
  | BrowserAutomationKeypressAction;

export type BrowserAutomationRequest = {
  browserId: string;
  generation: number;
  action: BrowserAutomationAction;
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

export type TerminalPaneContent = {
  readonly kind: "terminal";
  readonly sessionId: string;
};

export type BrowserPaneState = {
  readonly browserId: string;
  readonly url: string;
  readonly title?: string | null;
  readonly loading?: boolean;
  readonly canGoBack?: boolean;
  readonly canGoForward?: boolean;
  readonly zoomFactor?: number;
  readonly loadError?: string | null;
  readonly profileId?: string;
  readonly worktreePath?: string;
  readonly worktreeLabel?: string;
};

export type BrowserPaneContent = {
  readonly kind: "browser";
  readonly browser?: BrowserPaneState;
  readonly browserId?: string;
  readonly url?: string;
  readonly title?: string | null;
  readonly loading?: boolean;
  readonly canGoBack?: boolean;
  readonly canGoForward?: boolean;
  readonly zoomFactor?: number;
  readonly loadError?: string | null;
  readonly profileId?: string;
  readonly worktreePath?: string;
  readonly worktreeLabel?: string;
};

export type DagPaneState = {
  readonly runId?: string | null;
};

export type DagPaneContent = {
  readonly kind: "dag";
  readonly dag?: DagPaneState;
  readonly runId?: string | null;
};

export type PaneContent = TerminalPaneContent | BrowserPaneContent | DagPaneContent;

export function createDagPaneContent(dag?: DagPaneState | { runId?: string | null }): DagPaneContent {
  const content: DagPaneContent = {
    kind: "dag",
    dag: dag ? { ...dag } : {},
  };
  Object.defineProperties(content, {
    runId: {
      get() {
        return this.dag?.runId ?? null;
      },
      enumerable: false,
    },
  });
  return content;
}

export function createBrowserPaneContent(browser: BrowserPaneState): BrowserPaneContent {
  const content: BrowserPaneContent = {
    kind: "browser",
    browser: { ...browser },
  };
  Object.defineProperties(content, {
    browserId: { get() { return this.browser.browserId; }, enumerable: false },
    url: { get() { return this.browser.url; }, enumerable: false },
    title: { get() { return this.browser.title; }, enumerable: false },
    loading: { get() { return this.browser.loading; }, enumerable: false },
    canGoBack: { get() { return this.browser.canGoBack; }, enumerable: false },
    canGoForward: { get() { return this.browser.canGoForward; }, enumerable: false },
    zoomFactor: { get() { return this.browser.zoomFactor; }, enumerable: false },
    loadError: { get() { return this.browser.loadError; }, enumerable: false },
    profileId: { get() { return this.browser.profileId; }, enumerable: false },
    worktreePath: { get() { return this.browser.worktreePath; }, enumerable: false },
    worktreeLabel: { get() { return this.browser.worktreeLabel; }, enumerable: false },
  });
  return content;
}

export function createTerminalPaneContent(sessionId: string): TerminalPaneContent {
  return {
    kind: "terminal",
    sessionId,
  };
}

/**
 * The split arrangement inside a single terminal tab. This is intentionally separate
 * from TabGroupLayoutNode: Orca keeps terminal panes inside a tab and split tab groups
 * as two different layout layers.
 */
export type TabPaneLayout = {
  root: PaneNode;
  /** Leaf that receives keyboard focus inside this tab. */
  activeLeafId: string | null;
  /** Leaf temporarily zoomed to fill the tab, or `null` when every pane is visible. */
  expandedLeafId: string | null;
  /** Frontend-local terminal session rendered by each leaf of `root`. */
  sessionIdsByLeafId: Record<string, string>;
  /** Authoritative content map for each leaf of `root`. */
  contentsByLeafId?: Record<string, PaneContent>;
};

export type TabGroup = {
  id: string;
  tabIds: string[];
  activeTabId: string | null;
};

export type TabGroupLayoutNode =
  | { type: "group"; groupId: string }
  | {
      type: "split";
      direction: PaneDirection;
      first: TabGroupLayoutNode;
      second: TabGroupLayoutNode;
      ratio: number;
    };

export type LayoutState = {
  tabs: WorkspaceTab[];
  primaryTabId?: string | null;
  secondaryTabId?: string | null;
  split?: SplitMode;
  nestedSplit?: NestedSplit | null;
  /** Active tab in the currently focused tab group. */
  activeTabId: string | null;
  /** Terminal-pane tree owned independently by each tab. */
  layoutsByTabId: Record<string, TabPaneLayout>;
  /** Orca-style tab groups. Optional only for backwards-compatible persisted/test state. */
  tabGroups?: Record<string, TabGroup>;
  /** Split tree whose leaves are tab groups, not terminal panes. */
  tabGroupLayout?: TabGroupLayoutNode | null;
  focusedGroupId?: string | null;
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
  sequence?: string | null;
  daemonEpoch?: string | null;
};

export type TerminalReplayGap = {
  requestedAfterSequence: string;
  availableFromSequence: string;
};

export type AttachTerminalRequest = {
  sessionId: string;
  afterSequence?: string | null;
};

export type AttachTerminalResponse = {
  sessionId: string;
  daemonEpoch?: string | null;
  historyStartSequence?: string | null;
  historyEndSequence?: string | null;
  history: string;
  gap?: TerminalReplayGap | null;
};

export type TerminalLifecyclePayload = {
  sessionId: string;
  state: "started" | "exited" | "failed";
  exitCode: number | null;
  reason: string | null;
};

/** Emitted per attached session by the native daemon stream pump, independent of pane mounting. */
export type NativeTerminalTitlePayload = {
  sessionId: string;
  title: string;
};

export type NativeTerminalAgentStatePayload = {
  sessionId: string;
  state: "working" | "blocked" | "idle";
  ruleId: string;
  manifestId: string;
  providerSession?: AgentProviderSession | null;
};

export type NativeTerminalBellPayload = {
  sessionId: string;
  count: number;
};

export type NativeTerminalScrollbarPayload = {
  sessionId: string;
  total: number;
  offset: number;
  len: number;
};

export type WorktreeChangedPayload = {
  workspaceId: string;
  worktree: WorktreeIdentity;
  kind: "created" | "deleted" | "destructivelyDeleted" | "dirtyChanged" | "pruned";
};

export type NotificationSource =
  | "agent-task-complete"
  | "terminal-bell"
  | "system"
  | "agentTaskComplete"
  | "terminalBell"
  | "test";
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
  sound?: "system" | "silent";
  attentionReason?: "waiting" | "done";
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

export interface NotificationBadgeResult {
  readonly supported: boolean;
  readonly count: number;
  readonly badgeLabel?: string;
}

export type SetBadgeCountResult = NotificationBadgeResult;

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

/** Version-2 terminal tab payload. All IDs in pane mappings are frontend-local session IDs. */
export interface PersistedTerminalTabState {
  primarySessionId: string;
  paneTree: PaneNode;
  sessionIdsByLeafId: Record<string, string>;
  contentsByLeafId?: Record<string, PaneContent>;
  activeLeafId: string | null;
  expandedLeafId: string | null;
}

export interface PersistedBrowserTabState {
  browserId: string;
  url: string;
  title?: string | null;
  loading?: boolean;
  canGoBack?: boolean;
  canGoForward?: boolean;
  zoomFactor?: number;
  profileId?: string;
  worktreePath?: string;
  worktreeLabel?: string;
}

/**
 * Version-2 tab record. Legacy v1 fields remain optional so persisted sessions can be
 * migrated in place without unsafe casts or a second DTO hierarchy.
 */
export interface PersistedTab {
  id: string;
  label: string;
  kind?: "terminal" | "browser";
  pinned?: boolean;
  terminal?: PersistedTerminalTabState;
  browser?: PersistedBrowserTabState;
  customTitle?: string;

  // v1 compatibility fields
  sessionId?: string;
  worktreePath?: string;
  paneTree?: PaneNode;
  sessionIdsByLeafId?: Record<string, string>;
  contentsByLeafId?: Record<string, PaneContent>;
  activeLeafId?: string | null;
  expandedLeafId?: string | null;
}

export interface PersistedTabGroup {
  id: string;
  tabIds: string[];
  activeTabId: string | null;
}

export interface PersistedLayout {
  splitMode: string;
  primaryTabId: string | null;
  secondaryTabId: string | null;
  activeTabId: string | null;
  tabs: PersistedTab[];
  tabGroups?: PersistedTabGroup[];
  tabGroupLayout?: TabGroupLayoutNode | null;
  focusedGroupId?: string | null;
  layoutsByTabId?: Record<string, TabPaneLayout>;
}

export interface PersistedTerminalSession {
  /** v2 stable frontend identity; map key remains authoritative. */
  localSessionId?: string;
  /** v2 native PTY identity. `null` means it must be respawned on restore. */
  backendSessionId?: string | null;
  /** v1 compatibility field (v1 accidentally stored the local id here). */
  sessionId?: string;
  worktreePath: string;
  cwd: string;
  lastCommand?: string;
  recentScrollback?: string;
  createdAt: number;
  daemonEpoch?: string | null;
  lastOutputSequence?: string | null;
  /** Agent type detected for this pane, e.g. "claude". Never minted by Ferryx. */
  agentType?: string | null;
  /** Session id the AGENT ITSELF generated. Ferryx never mints this. */
  agentSessionId?: string | null;
  /** Authoritative provider session reference. Never minted by Ferryx. */
  providerSession?: AgentProviderSession | null;
}

export interface PersistedWorkspace {
  workspaceId: string;
  repoRoot: string;
  worktrees: PersistedWorktree[];
  activeWorktreePath: string | null;
  layout: PersistedLayout;
  worktreeLayouts?: Record<string, PersistedLayout>;
  terminalSessions: Record<string, PersistedTerminalSession>;
  activityBySessionId?: Record<string, import("./activity").TerminalActivity>;
}

export interface PersistedWorkspaceSession {
  version: number;
  timestamp: number;
  activeWorkspaceId: string;
  workspaces: Record<string, PersistedWorkspace>;
}

export type CliLauncherStatus = {
  launcherPath: string;
  isInstalled: boolean;
  isSymlink: boolean;
  currentTarget: string | null;
  activeExecutable: string | null;
  isSupported: boolean;
};

export type { RegisteredProject } from "./tauri";

export type PermissionStatus = "granted" | "denied" | "not_determined" | "unsupported";

export interface PermissionItemStatus {
  status: PermissionStatus;
  granted: boolean;
  canRequest: boolean;
  description: string;
}

export interface SystemPermissionsStatus {
  platform: string;
  fullDiskAccess: PermissionItemStatus;
  accessibility: PermissionItemStatus;
  notifications: PermissionItemStatus;
  allGranted: boolean;
}

export interface OpenPermissionsSettingsResult {
  opened: boolean;
  target: string;
  reason?: string | null;
}

