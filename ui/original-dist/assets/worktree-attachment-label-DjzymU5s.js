import { Ec as basename } from "./store-CgXrfmaH.js";
function getWorktreeAttachmentLabel(worktree) {
	const displayName = worktree.displayName.trim();
	if (displayName) return displayName;
	const branch = getBranchLabel(worktree.branch);
	if (branch) return branch;
	return basename(worktree.path) || worktree.path;
}
function getBranchLabel(branch) {
	const trimmed = branch?.trim();
	if (!trimmed) return null;
	if (trimmed.startsWith("refs/heads/")) return trimmed.slice(11);
	return trimmed;
}
export { getWorktreeAttachmentLabel as t };
