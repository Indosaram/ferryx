export interface Worktree {
  path: string;
  head: string;
  branch: string | null;
  bare: boolean;
  detached: boolean;
  locked: string | null;
  prunable: string | null;
}

export interface DirtyFile {
  status_code: string;
  path: string;
}

export interface DirtyState {
  is_dirty: boolean;
  files: DirtyFile[];
}

export interface TerminalTab {
  id: string;
  title: string;
  worktreePath?: string;
  sessionId?: string;
}
