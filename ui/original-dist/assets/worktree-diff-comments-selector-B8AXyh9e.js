import { Ju as findFolderWorkspaceOwner, Kt as getIndexedWorktreeById, vd as parseWorkspaceKey } from "./store-CgXrfmaH.js";
var EMPTY_DIFF_COMMENTS = Object.freeze([]);
function selectWorktreeDiffComments(state, worktreeId) {
	if (!worktreeId) return;
	const scope = parseWorkspaceKey(worktreeId);
	if (scope?.type === "folder") return findFolderWorkspaceOwner(state, scope.folderWorkspaceId)?.diffComments;
	return getIndexedWorktreeById(state.worktreesByRepo, worktreeId)?.diffComments;
}
function selectWorktreeDiffCommentsOrEmpty(state, worktreeId) {
	return selectWorktreeDiffComments(state, worktreeId) ?? EMPTY_DIFF_COMMENTS;
}
export { selectWorktreeDiffCommentsOrEmpty as n, selectWorktreeDiffComments as t };
