const MANUAL_TERMINAL_WORKTREE_PARK_EVENT = "orca-manual-terminal-worktree-park";
var pendingWorktreeIds = /* @__PURE__ */ new Set();
function requestManualTerminalWorktreePark(worktreeId) {
	if (!worktreeId) return;
	pendingWorktreeIds.add(worktreeId);
	window.dispatchEvent(new CustomEvent(MANUAL_TERMINAL_WORKTREE_PARK_EVENT, { detail: { worktreeId } }));
}
function takePendingManualTerminalWorktreePark(worktreeId) {
	return pendingWorktreeIds.delete(worktreeId);
}
function takeAllPendingManualTerminalWorktreeParks() {
	const pending = [...pendingWorktreeIds];
	pendingWorktreeIds.clear();
	return pending;
}
export { takePendingManualTerminalWorktreePark as i, requestManualTerminalWorktreePark as n, takeAllPendingManualTerminalWorktreeParks as r, MANUAL_TERMINAL_WORKTREE_PARK_EVENT as t };
