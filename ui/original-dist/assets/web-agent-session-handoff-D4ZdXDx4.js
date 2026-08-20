var handoffByProvisionalTab = /* @__PURE__ */ new Map();
function handoffKey(args) {
	return `${args.environmentId}\0${args.worktreeId}\0${args.provisionalTabId}`;
}
function recordWebAgentSessionHandoff(args) {
	if (!args.environmentId.trim() || !args.worktreeId.trim() || !args.provisionalTabId.trim() || !args.hostTabId.trim() || !args.hostTerminalHandle.trim()) return;
	handoffByProvisionalTab.set(handoffKey(args), {
		hostTabId: args.hostTabId,
		hostTerminalHandle: args.hostTerminalHandle,
		postCreateSnapshotConfirmed: false
	});
}
function resolveWebAgentSessionHandoff(args) {
	return handoffByProvisionalTab.get(handoffKey(args))?.hostTabId ?? null;
}
function isWebAgentSessionHandoffPostCreateSnapshotConfirmed(args) {
	return handoffByProvisionalTab.get(handoffKey(args))?.postCreateSnapshotConfirmed === true;
}
function confirmWebAgentSessionHandoffAfterCreate(args) {
	const key = handoffKey(args);
	const handoff = handoffByProvisionalTab.get(key);
	if (handoff?.hostTabId === args.hostTabId && handoff.hostTerminalHandle === args.hostTerminalHandle) handoffByProvisionalTab.set(key, {
		...handoff,
		postCreateSnapshotConfirmed: true
	});
}
function clearWebAgentSessionHandoff(args) {
	handoffByProvisionalTab.delete(handoffKey(args));
}
function clearWebAgentSessionHandoffsForWorktree(environmentId, worktreeId) {
	const prefix = `${environmentId}\0${worktreeId}\0`;
	for (const key of handoffByProvisionalTab.keys()) if (key.startsWith(prefix)) handoffByProvisionalTab.delete(key);
}
function clearWebAgentSessionHandoffsForEnvironment(environmentId) {
	const prefix = `${environmentId}\0`;
	for (const key of handoffByProvisionalTab.keys()) if (key.startsWith(prefix)) handoffByProvisionalTab.delete(key);
}
export { isWebAgentSessionHandoffPostCreateSnapshotConfirmed as a, confirmWebAgentSessionHandoffAfterCreate as i, clearWebAgentSessionHandoffsForEnvironment as n, recordWebAgentSessionHandoff as o, clearWebAgentSessionHandoffsForWorktree as r, resolveWebAgentSessionHandoff as s, clearWebAgentSessionHandoff as t };
