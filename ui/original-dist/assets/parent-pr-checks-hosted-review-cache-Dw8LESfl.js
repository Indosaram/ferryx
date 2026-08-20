import { Ul as getLegacyGitHubPRCacheKey, Vl as getGitHubPRCacheKey } from "./store-CgXrfmaH.js";
import { X as LOCAL_EXECUTION_HOST_ID, rt as normalizeExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { n as isCachedMergedBranchPRCurrentForWorktree, t as getWorktreeCardPrDisplay } from "./worktree-card-pr-display-d-PZE8Kd.js";
function canUseParentPrChecksGitHubPRCacheEntry(worktree, prEntry, hostedReviewEntry) {
	const pr = prEntry?.data;
	if (!pr) return false;
	const prFetchedAt = prEntry.fetchedAt;
	const hasLinkedGitHubPR = worktree.linkedPR !== null;
	if (hasLinkedGitHubPR && pr.number !== worktree.linkedPR) return false;
	if (!hasLinkedGitHubPR && hasNonGitHubLinkedReview(worktree)) return false;
	const mergedPrMatchesCurrentHead = isCachedMergedBranchPRCurrentForWorktree(pr, worktree);
	if (pr.state === "merged" && !mergedPrMatchesCurrentHead) return false;
	if (hostedReviewEntry?.data === null && !mergedPrMatchesCurrentHead && prFetchedAt <= hostedReviewEntry.fetchedAt) return false;
	return true;
}
function getParentPrChecksGitHubPRCacheEntry({ prCache, repo, branch, settings }) {
	const currentKey = getGitHubPRCacheKey(repo.path, repo.id, branch, settings, repo.connectionId, repo.executionHostId, true);
	const executionHostId = normalizeExecutionHostId(repo.executionHostId);
	const canUseLegacyPRCache = !repo.connectionId && (!executionHostId || executionHostId === "local");
	const legacyRepoKey = canUseLegacyPRCache ? getLegacyGitHubPRCacheKey(repo.path, repo.id, branch) : "";
	const legacyPathKey = canUseLegacyPRCache ? getLegacyGitHubPRCacheKey(repo.path, void 0, branch) : "";
	return prCache[currentKey] ?? (legacyRepoKey ? prCache[legacyRepoKey] : void 0) ?? (legacyPathKey ? prCache[legacyPathKey] : void 0);
}
function hasNonGitHubLinkedReview(worktree) {
	return worktree.linkedGitLabMR != null || worktree.linkedBitbucketPR != null || worktree.linkedAzureDevOpsPR != null || worktree.linkedGiteaPR != null;
}
function canUseParentPrChecksHostedReviewCacheEntry(worktree, review, entry) {
	if (review.state === "merged" && !mergedReviewMatchesHead(review, worktree)) return false;
	const linkedReviewNumber = getLinkedReviewNumberForProvider(worktree, review.provider);
	if (hasLinkedReview(worktree)) return linkedReviewNumber === review.number;
	if ((entry.linkedReviewHintKey ?? "") !== "") return false;
	const display = getWorktreeCardPrDisplay(review, worktree.linkedPR, worktree.linkedGitLabMR ?? null, worktree.linkedBitbucketPR ?? null, worktree.linkedAzureDevOpsPR ?? null, worktree.linkedGiteaPR ?? null, { reviewHintKey: entry.linkedReviewHintKey });
	return display?.provider === review.provider && display.number === review.number;
}
function mergedReviewMatchesHead(review, worktree) {
	return isCachedMergedBranchPRCurrentForWorktree({
		number: review.number,
		title: review.title,
		state: review.state,
		url: review.url,
		checksStatus: review.status,
		updatedAt: review.updatedAt,
		mergeable: review.mergeable,
		...review.headSha ? { headSha: review.headSha } : {},
		...review.confirmedContainedHeadOid ? { confirmedContainedHeadOid: review.confirmedContainedHeadOid } : {}
	}, worktree);
}
function getLinkedReviewNumberForProvider(worktree, provider) {
	switch (provider) {
		case "github": return worktree.linkedPR;
		case "gitlab": return worktree.linkedGitLabMR ?? null;
		case "bitbucket": return worktree.linkedBitbucketPR ?? null;
		case "azure-devops": return worktree.linkedAzureDevOpsPR ?? null;
		case "gitea": return worktree.linkedGiteaPR ?? null;
		case "unsupported": return null;
	}
}
function hasLinkedReview(worktree) {
	return worktree.linkedPR != null || worktree.linkedGitLabMR != null || worktree.linkedBitbucketPR != null || worktree.linkedAzureDevOpsPR != null || worktree.linkedGiteaPR != null;
}
export { canUseParentPrChecksGitHubPRCacheEntry as n, getParentPrChecksGitHubPRCacheEntry as r, canUseParentPrChecksHostedReviewCacheEntry as t };
