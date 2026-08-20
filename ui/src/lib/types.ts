export type Worktree = {
  worktreeId: string;
  wsId: string;
  path: string;
  head: string;
  branch: string | null;
  bare: boolean;
  detached: boolean;
  locked: string | null;
  prunable: string | null;
  isDirty: boolean;
};

export type DirtyFile = {
  statusCode: string;
  path: string;
};

export type DirtyState = {
  isDirty: boolean;
  files: DirtyFile[];
};

export type TerminalLifecycle = "starting" | "working" | "waiting" | "exited" | "failed";

export type TerminalSession = {
  id: string;
  cwd: string;
  workspaceId: string;
  worktreeId: string;
  backendSessionId: string | null;
  lifecycle: TerminalLifecycle;
  ownerId?: string | null;
};

export type TerminalTab = {
  id: string;
  label: string;
  sessionId: string;
};

export type Pane = {
  id: string;
  tabId: string;
};

export type SplitMode = "none" | "horizontal" | "vertical";

export type LayoutState = {
  tabs: TerminalTab[];
  primaryTabId: string | null;
  secondaryTabId: string | null;
  split: SplitMode;
};

export type AgentState = "starting" | "working" | "waiting" | "exited" | "failed";

export type ActiveAgent = {
  id: string;
  name: string;
  task: string;
  state: AgentState;
  worktreeId: string;
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
  action: "created" | "removed" | "dirty_changed" | "pruned" | "branch_changed";
  wsId: string;
  worktreeId: string;
};

export type StructuredIpcError = {
  code: string;
  message: string;
  details: Record<string, unknown>;
};
