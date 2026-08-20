import { invoke, isTauri } from "@tauri-apps/api/core";

import type { Worktree } from "./types";

export async function listWorktrees() {
  if (!isTauri()) return [];
  return invoke<Worktree[]>("cmd_worktree_list", { repoRoot: null });
}

export async function createWorktree(request: {
  wsId: string;
  slug: string;
  path: string;
  baseRef?: string;
}) {
  return invoke<Worktree>("cmd_worktree_create", {
    request: {
      repo_root: null,
      ws_id: request.wsId,
      slug: request.slug,
      path: request.path,
      base_ref: request.baseRef ?? null,
    },
  });
}
