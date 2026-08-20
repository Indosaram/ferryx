function isCachedMergedBranchPRCurrentForWorktree(cachedPR, worktree) {
	return cachedPR?.state === "merged" && typeof cachedPR.headSha === "string" && cachedPR.headSha.length > 0 && typeof worktree.head === "string" && worktree.head.length > 0 && (cachedPR.headSha === worktree.head || cachedPR.confirmedContainedHeadOid === worktree.head);
}
function getLinkedReviewNumber(provider, links) {
	switch (provider) {
		case "github": return links.linkedPR;
		case "gitlab": return links.linkedGitLabMR;
		case "bitbucket": return links.linkedBitbucketPR;
		case "azure-devops": return links.linkedAzureDevOpsPR;
		case "gitea": return links.linkedGiteaPR;
	}
}
function makeLinkedReviewFallback(provider, number, review) {
	const label = provider === "gitlab" ? "MR" : "PR";
	return {
		provider,
		number,
		title: review === null ? `${label} details unavailable` : `Loading ${label}...`
	};
}
function getWorktreeCardPrDisplay(review, linkedPR, linkedGitLabMR = null, linkedBitbucketPR = null, linkedAzureDevOpsPR = null, linkedGiteaPR = null, options = {}) {
	const links = {
		linkedPR,
		linkedGitLabMR,
		linkedBitbucketPR,
		linkedAzureDevOpsPR,
		linkedGiteaPR
	};
	const hasLinkedReview = linkedPR !== null || linkedGitLabMR !== null || linkedBitbucketPR !== null || linkedAzureDevOpsPR !== null || linkedGiteaPR !== null;
	if (review) {
		if (review.provider === "unsupported") return review;
		const linkedReviewNumber = getLinkedReviewNumber(review.provider, links);
		if (linkedReviewNumber === null) {
			if (review.provider !== "github" && review.provider !== "gitlab") return review;
			if (!hasLinkedReview && review.provider === "github" && options.branchLookupGitHubPRNumber != null && options.branchLookupGitHubPRNumber === review.number) return review;
			return options.reviewHintKey === "" ? review : null;
		}
		if (review.number === linkedReviewNumber) return review;
		return makeLinkedReviewFallback(review.provider, linkedReviewNumber, void 0);
	}
	if (linkedPR !== null) return makeLinkedReviewFallback("github", linkedPR, review);
	if (linkedGitLabMR !== null) return makeLinkedReviewFallback("gitlab", linkedGitLabMR, review);
	if (linkedBitbucketPR !== null) return makeLinkedReviewFallback("bitbucket", linkedBitbucketPR, review);
	if (linkedAzureDevOpsPR !== null) return makeLinkedReviewFallback("azure-devops", linkedAzureDevOpsPR, review);
	if (linkedGiteaPR !== null) return makeLinkedReviewFallback("gitea", linkedGiteaPR, review);
	return null;
}
export { isCachedMergedBranchPRCurrentForWorktree as n, getWorktreeCardPrDisplay as t };
