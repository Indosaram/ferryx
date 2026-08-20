function agentMapCardTopologyIdentity(card) {
	const parentPaneKey = card.parentPaneKey ?? "";
	const parentWorktreeId = card.parentWorktreeId ?? "";
	const executionHostId = card.executionHostId ?? "";
	return `${card.repoId.length}:${card.repoId}${card.worktreeId.length}:${card.worktreeId}${executionHostId.length}:${executionHostId}${card.paneKey.length}:${card.paneKey}${parentPaneKey.length}:${parentPaneKey}${parentWorktreeId.length}:${parentWorktreeId}`;
}
function agentMapWorkspaceTopologyIdentity(workspace) {
	const parentWorktreeId = workspace.parentWorktreeId ?? "";
	return `${workspace.repoId.length}:${workspace.repoId}${workspace.worktreeId.length}:${workspace.worktreeId}${workspace.executionHostId.length}:${workspace.executionHostId}${parentWorktreeId.length}:${parentWorktreeId}`;
}
function agentMapWorktreeIdentityFromParts(worktreeId, executionHostId) {
	const hostId = executionHostId ?? "";
	return `${worktreeId.length}:${worktreeId}${hostId.length}:${hostId}`;
}
function agentMapWorktreeIdentity(card) {
	return agentMapWorktreeIdentityFromParts(card.worktreeId, card.executionHostId);
}
function agentMapWorkspaceIdentity(workspace) {
	return agentMapWorktreeIdentityFromParts(workspace.worktreeId, workspace.executionHostId);
}
export { agentMapWorktreeIdentityFromParts as a, agentMapWorktreeIdentity as i, agentMapWorkspaceIdentity as n, agentMapWorkspaceTopologyIdentity as r, agentMapCardTopologyIdentity as t };
