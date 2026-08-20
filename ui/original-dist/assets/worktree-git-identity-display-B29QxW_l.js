function shortGitHead(head) {
	return (head ?? "").trim().slice(0, 7);
}
function getDetachedHeadTooltip(shortHead) {
	return `Detached HEAD at ${shortHead}. You are viewing a commit, not a branch.`;
}
function getWorktreeGitIdentityDisplay(input) {
	const branchName = (input.branch ?? "").replace(/^refs\/heads\//, "").trim();
	if (branchName) return {
		kind: "branch",
		branchName
	};
	const shortHead = shortGitHead(input.head);
	if (!shortHead) return null;
	return {
		kind: "detached",
		shortHead,
		sidebarLabel: `Detached HEAD @ ${shortHead}`,
		sourceControlLabel: `Detached HEAD · ${shortHead}`,
		tooltip: getDetachedHeadTooltip(shortHead)
	};
}
export { getWorktreeGitIdentityDisplay as t };
