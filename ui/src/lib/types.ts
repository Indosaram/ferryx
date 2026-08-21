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

export type StructuredIpcError = {
  code: string;
  message: string;
  details?: Record<string, unknown>;
};
