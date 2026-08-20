import { Oa as focusTerminalTabSurface, co as focusRuntimeTerminalSurface, t as useAppStore } from "./store-CgXrfmaH.js";
function resolveActivatedWorkspaceTerminalTabId(worktreeId, activation) {
	const state = useAppStore.getState();
	if (activation && activation.primaryTabId) return activation.primaryTabId;
	if (state.activeWorktreeId !== worktreeId || state.activeView !== "terminal" || state.activeTabType !== "terminal") return null;
	return state.activeTabId;
}
function queueWorkspaceActivationTerminalFocus(worktreeId, activation) {
	const tabId = resolveActivatedWorkspaceTerminalTabId(worktreeId, activation);
	if (!tabId) return false;
	requestAnimationFrame(() => {
		const state = useAppStore.getState();
		if (state.activeWorktreeId !== worktreeId || state.activeView !== "terminal" || state.activeTabType !== "terminal" || state.activeTabId !== tabId) return;
		if (!focusRuntimeTerminalSurface(tabId)) focusTerminalTabSurface(tabId);
	});
	return true;
}
export { queueWorkspaceActivationTerminalFocus as t };
