import { Ai as isWorkspaceOldForCleanup, Kg as isFolderRepo, ki as getPersistedWorkspaceCleanupActivityAt } from "./store-CgXrfmaH.js";
function countEstimatedInactiveWorkspaces(worktrees, repoById, now) {
	let count = 0;
	for (const worktree of worktrees) {
		const repo = repoById.get(worktree.repoId);
		if (!repo || isFolderRepo(repo) || worktree.isMainWorktree) continue;
		const lastActivityAt = getPersistedWorkspaceCleanupActivityAt(worktree);
		if (lastActivityAt > 0 && isWorkspaceOldForCleanup({
			isArchived: worktree.isArchived,
			lastActivityAt
		}, now)) count += 1;
	}
	return count;
}
export { countEstimatedInactiveWorkspaces as t };
