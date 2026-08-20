import { Ht as getConnectionIdForFileFromState, Ut as getConnectionIdFromState, t as useAppStore, vd as parseWorkspaceKey } from "./store-CgXrfmaH.js";
function getConnectionId(worktreeId) {
	return getConnectionIdFromState(useAppStore.getState(), worktreeId);
}
function isWorktreeConnectionResolved(worktreeId) {
	if (!worktreeId) return true;
	if (parseWorkspaceKey(worktreeId)?.type === "folder") return true;
	return getConnectionId(worktreeId) !== void 0;
}
function getConnectionIdForFile(worktreeId, filePath) {
	return getConnectionIdForFileFromState(useAppStore.getState(), worktreeId, filePath);
}
export { getConnectionIdForFile as n, isWorktreeConnectionResolved as r, getConnectionId as t };
