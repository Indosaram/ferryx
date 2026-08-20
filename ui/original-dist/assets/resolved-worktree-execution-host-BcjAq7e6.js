import { $u as findIndexedRepoOwnerForHost, Pm as FLOATING_TERMINAL_WORKTREE_ID, Qu as findIndexedRepoOwner, Xu as findIndexedFolderWorkspaceOwner, Zu as findIndexedProjectGroupOwner, ed as findIndexedWorktreeOwner, hd as folderWorkspaceKey, td as findIndexedWorktreeOwnerForHost, vd as parseWorkspaceKey } from "./store-CgXrfmaH.js";
import { X as LOCAL_EXECUTION_HOST_ID, lt as toSshExecutionHostId, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
function getResolvedFolderHost(state, folderWorkspaceId) {
	const preferredHostId = state.activeWorktreeId === folderWorkspaceKey(folderWorkspaceId) ? state.activeWorkspaceExecutionHostId ?? void 0 : void 0;
	const folder = findIndexedFolderWorkspaceOwner(state.folderWorkspaces, folderWorkspaceId, preferredHostId);
	const group = folder ? findIndexedProjectGroupOwner(state.projectGroups, folder.projectGroupId, preferredHostId) : null;
	const explicitHost = parseExecutionHostId(folder?.executionHostId ?? group?.executionHostId);
	if (explicitHost) return explicitHost.id;
	const connectionId = folder?.connectionId?.trim() || group?.connectionId?.trim();
	if (connectionId) return toSshExecutionHostId(connectionId);
	const restoredHost = parseExecutionHostId(state.restoredRuntimeHostIdByWorkspaceSessionKey?.[folderWorkspaceKey(folderWorkspaceId)]);
	if (restoredHost?.kind === "runtime") return restoredHost.id;
	return folder && (group || preferredHostId) ? preferredHostId ?? "local" : null;
}
function getResolvedExecutionHostIdForWorktree(state, worktreeId) {
	if (!worktreeId) return null;
	if (worktreeId === "global-floating-terminal") return LOCAL_EXECUTION_HOST_ID;
	const scope = parseWorkspaceKey(worktreeId);
	if (scope?.type === "folder") return getResolvedFolderHost(state, scope.folderWorkspaceId);
	const preferredHostId = state.activeWorktreeId === worktreeId ? state.activeWorkspaceExecutionHostId ?? void 0 : void 0;
	const worktree = preferredHostId ? findIndexedWorktreeOwnerForHost(state.worktreesByRepo, worktreeId, preferredHostId) : findIndexedWorktreeOwner(state.worktreesByRepo, worktreeId);
	const worktreeHost = parseExecutionHostId(worktree?.hostId);
	if (worktreeHost) return worktreeHost.id;
	if (!worktree) return null;
	const repo = preferredHostId ? findIndexedRepoOwnerForHost(state.repos, worktree.repoId, preferredHostId) : findIndexedRepoOwner(state.repos, worktree.repoId);
	if (!repo) return null;
	const explicitRepoHost = parseExecutionHostId(repo.executionHostId);
	if (explicitRepoHost) return explicitRepoHost.id;
	return repo.connectionId?.trim() ? toSshExecutionHostId(repo.connectionId) : LOCAL_EXECUTION_HOST_ID;
}
export { getResolvedExecutionHostIdForWorktree as t };
