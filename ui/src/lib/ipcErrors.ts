import type { StructuredIpcError } from "./types";

export function worktreeErrorMessage(error: StructuredIpcError) {
  switch (error.code) {
    case "DIRTY_WORKTREE":
      return "The worktree has uncommitted changes. Commit or clean them before this operation.";
    case "WRITER_ALREADY_ACTIVE":
      return "The worktree is currently owned by another writer session.";
    case "GIT_ERROR":
      return "The selected base ref is not allowed or does not exist.";
    default:
      return error.message;
  }
}
