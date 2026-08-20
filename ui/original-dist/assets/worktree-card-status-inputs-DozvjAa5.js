function selectRuntimePaneTitlesForWorktree(state, worktreeId) {
	const out = {};
	for (const tab of state.tabsByWorktree[worktreeId] ?? []) {
		const paneTitles = state.runtimePaneTitlesByTabId[tab.id];
		if (paneTitles) out[tab.id] = paneTitles;
	}
	return out;
}
function selectLivePtyIdsForWorktree(state, worktreeId) {
	const out = {};
	for (const tab of state.tabsByWorktree[worktreeId] ?? []) {
		const ids = state.ptyIdsByTabId[tab.id];
		if (ids && ids.length > 0) out[tab.id] = ids;
	}
	return out;
}
function selectTerminalLayoutRootsForWorktree(state, worktreeId) {
	const out = {};
	for (const tab of state.tabsByWorktree[worktreeId] ?? []) out[tab.id] = state.terminalLayoutsByTabId[tab.id]?.root;
	return out;
}
export { selectRuntimePaneTitlesForWorktree as n, selectTerminalLayoutRootsForWorktree as r, selectLivePtyIdsForWorktree as t };
