export type Worktree = {
  path: string;
  head: string;
  branch: string | null;
  bare: boolean;
  detached: boolean;
  locked: string | null;
  prunable: string | null;
};

export type DirtyState = {
  is_dirty: boolean;
  files: Array<{
    status_code: string;
    path: string;
  }>;
};

export type AgentState = "working" | "waiting" | "done" | "idle";

export type ActiveAgent = {
  id: string;
  name: string;
  task: string;
  state: AgentState;
  worktreePath: string;
};

export type TerminalTab = {
  id: string;
  label: string;
  cwd: string;
};
