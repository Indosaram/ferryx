import type { Worktree } from "./types";

export function branchName(worktree: Worktree): string {
  return worktree.branch?.replace(/^refs\/heads\//, "") ?? "detached HEAD";
}

export function displayWorkspaceTitle(worktree: Worktree): string {
  const parts = branchName(worktree).split("/");
  if (parts[0] === "orca" && parts.length > 2) {
    return parts.slice(2).join("/");
  }
  const branch = branchName(worktree);
  if (branch && branch !== "detached HEAD" && branch !== "ferryx" && branch !== "rorca" && branch !== "orca-lite") {
    return branch;
  }
  return "main";
}

export const workspaceName = displayWorkspaceTitle;
