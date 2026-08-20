import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { Gl as linkedReviewHintKey, Hl as getGitHubRepoCacheKey, Kg as isFolderRepo, Qn as prChecksCacheSuffix, Wl as getHostedReviewCacheKey, lr as hostedReviewInfoFromGitHubPRInfo } from "./store-CgXrfmaH.js";
import { t as getWorktreeGitIdentityDisplay } from "./worktree-git-identity-display-B29QxW_l.js";
import { t as getWorktreeCardPrDisplay } from "./worktree-card-pr-display-d-PZE8Kd.js";
import { n as canUseParentPrChecksGitHubPRCacheEntry, r as getParentPrChecksGitHubPRCacheEntry, t as canUseParentPrChecksHostedReviewCacheEntry } from "./parent-pr-checks-hosted-review-cache-Dw8LESfl.js";
const PARENT_PR_CHECKS_GROUP_LABELS = {
	get needsAttention() {
		return translate("auto.components.rightSidebar.parentPrChecks.groups.needsAttention", "Needs attention");
	},
	get pending() {
		return translate("auto.components.rightSidebar.parentPrChecks.groups.pending", "Pending");
	},
	get merged() {
		return translate("auto.components.rightSidebar.parentPrChecks.groups.merged", "Merged");
	},
	get passing() {
		return translate("auto.components.rightSidebar.parentPrChecks.groups.passing", "Passing");
	},
	get draftOrNoChecks() {
		return translate("auto.components.rightSidebar.parentPrChecks.groups.draftOrNoChecks", "Draft / no checks");
	},
	get noPr() {
		return translate("auto.components.rightSidebar.parentPrChecks.groups.noPr", "No PR");
	},
	get unavailable() {
		return translate("auto.components.rightSidebar.parentPrChecks.groups.unavailable", "Unavailable");
	}
};
const PARENT_PR_CHECKS_GROUP_ORDER = [
	"needsAttention",
	"pending",
	"merged",
	"passing",
	"draftOrNoChecks",
	"noPr",
	"unavailable"
];
function classifyParentPrChecksRowStatus({ isUnavailable, review, hasCacheEntry, outcome, hasFallbackReview }) {
	if (isUnavailable) return "unsupported";
	if (outcome?.kind === "loading") return review ? classifyKnownReviewStatus(review) : "loading";
	if (review) return classifyKnownReviewStatus(review);
	if (outcome?.kind === "error") return "refreshError";
	if (hasFallbackReview) return "linkedDetailsUnavailable";
	if (outcome?.kind === "unavailable") return "unavailable";
	if (outcome?.kind === "no-review") return "noReview";
	return hasCacheEntry ? "notFetched" : "notFetched";
}
function classifyKnownReviewStatus(review) {
	if (review.provider === "unsupported") return "unsupported";
	if (review.mergeable === "CONFLICTING") return "conflict";
	if (review.state === "merged") return "merged";
	if (review.state === "closed") return "closed";
	if (review.state === "draft") return "draft";
	if (review.status === "failure") return "failing";
	if (review.status === "pending") return "pending";
	if (review.status === "success") return "success";
	return "neutral";
}
function groupForRowStatus(status) {
	switch (status) {
		case "failing":
		case "conflict":
		case "closed":
		case "linkedDetailsUnavailable":
		case "refreshError": return "needsAttention";
		case "pending": return "pending";
		case "merged": return "merged";
		case "success": return "passing";
		case "draft":
		case "neutral": return "draftOrNoChecks";
		case "noReview": return "noPr";
		case "notFetched":
		case "loading":
		case "unsupported":
		case "unavailable": return "unavailable";
	}
}
function getRowCheckTone(status, review) {
	if ([
		"failing",
		"conflict",
		"closed",
		"linkedDetailsUnavailable",
		"refreshError"
	].includes(status)) return "failure";
	if (status === "pending" || status === "loading") return "pending";
	if (status === "success" || status === "merged") return "success";
	return review?.status ?? "neutral";
}
function getRowSummary(status, review, detailNames) {
	if (detailNames.length > 0 && (status === "failing" || status === "pending")) return status === "failing" ? translate("auto.components.rightSidebar.parentPrChecks.rowSummary.failingCount", "{{value0}} failing", { value0: detailNames.length }) : translate("auto.components.rightSidebar.parentPrChecks.rowSummary.pendingCount", "{{value0}} pending", { value0: detailNames.length });
	switch (status) {
		case "failing": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.checksFailing", "Checks failing");
		case "conflict": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.mergeConflicts", "Merge conflicts");
		case "pending": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.checksPending", "Checks pending");
		case "success": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.checksPassing", "Checks passing");
		case "merged": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.merged", "Merged");
		case "closed": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.closedWithoutMerge", "Closed without merge");
		case "draft": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.draftReview", "Draft review");
		case "neutral": return review ? translate("auto.components.rightSidebar.parentPrChecks.rowSummary.noCheckSignal", "No check signal") : translate("auto.components.rightSidebar.parentPrChecks.rowSummary.reviewUnavailable", "Review status unavailable");
		case "noReview": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.noPrLinked", "No PR linked");
		case "linkedDetailsUnavailable": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.detailsUnavailable", "Review details unavailable");
		case "refreshError": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.refreshFailed", "Refresh failed");
		case "loading": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.checking", "Checking review status…");
		case "notFetched": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.notFetched", "Status not fetched yet");
		case "unavailable": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.reviewUnavailable", "Review status unavailable");
		case "unsupported": return translate("auto.components.rightSidebar.parentPrChecks.rowSummary.unavailableWorktree", "Unavailable for this worktree");
	}
}
function buildParentPrChecksProjection(args) {
	const repoById = new Map(args.repos.map((repo) => [repo.id, repo]));
	const rows = buildParentPrChecksRows({
		...args,
		repoById
	});
	return {
		rows,
		groups: PARENT_PR_CHECKS_GROUP_ORDER.map((key) => ({
			key,
			label: PARENT_PR_CHECKS_GROUP_LABELS[key],
			rows: rows.filter((row) => row.group === key)
		})).filter((group) => group.rows.length > 0),
		summary: summarizeParentPrChecksRows(rows)
	};
}
function buildParentPrChecksRows(args) {
	return args.worktrees.map((worktree) => buildParentPrChecksRow({
		...args,
		worktree,
		repo: args.repoById.get(worktree.repoId) ?? null
	}));
}
function summarizeParentPrChecksRows(rows) {
	return {
		attached: rows.length,
		knownReview: rows.filter((row) => row.reviewLabel !== null && row.status !== "noReview").length,
		failing: rows.filter((row) => row.group === "needsAttention").length,
		pending: rows.filter((row) => row.group === "pending").length,
		passing: rows.filter((row) => row.group === "passing").length,
		noPr: rows.filter((row) => row.status === "noReview").length,
		unknown: rows.filter((row) => [
			"notFetched",
			"loading",
			"linkedDetailsUnavailable",
			"refreshError",
			"unsupported",
			"unavailable"
		].includes(row.status)).length
	};
}
function getParentPrChecksRefreshIdentity(worktree, repo, branch) {
	return [
		worktree.id,
		worktree.instanceId ?? "",
		repo?.id ?? worktree.repoId,
		branch ?? "",
		linkedReviewHintKey(getLinkedReviewHints(worktree))
	].join("::");
}
function buildParentPrChecksRow(args) {
	const branch = getBranchName(args.worktree);
	const refreshIdentity = getParentPrChecksRefreshIdentity(args.worktree, args.repo, branch);
	const outcome = args.refreshOutcomes?.get(refreshIdentity);
	const reviewSnapshot = getReviewSnapshot(args, branch, outcome);
	const fallbackDisplay = getWorktreeCardPrDisplay(reviewSnapshot.review, args.worktree.linkedPR, args.worktree.linkedGitLabMR ?? null, args.worktree.linkedBitbucketPR ?? null, args.worktree.linkedAzureDevOpsPR ?? null, args.worktree.linkedGiteaPR ?? null);
	const review = reviewSnapshot.review;
	const status = classifyParentPrChecksRowStatus({
		isUnavailable: !args.repo || isFolderRepo(args.repo) || args.worktree.isBare || !branch,
		review,
		hasCacheEntry: reviewSnapshot.hasCacheEntry,
		outcome,
		hasFallbackReview: fallbackDisplay !== null
	});
	const checkDetails = getCheckDetails(args, review, branch);
	const detailNames = getCheckDetailNames(checkDetails);
	return {
		id: args.worktree.id,
		refreshIdentity,
		worktree: args.worktree,
		repo: args.repo,
		branch,
		status,
		group: groupForRowStatus(status),
		checkTone: getRowCheckTone(status, review),
		title: getRowTitle(args.worktree, branch, review, fallbackDisplay?.title),
		reviewNumber: review?.number ?? fallbackDisplay?.number ?? null,
		reviewLabel: getReviewLabel(review, fallbackDisplay),
		reviewUrl: review?.url ?? fallbackDisplay?.url ?? null,
		reviewState: review?.state ?? fallbackDisplay?.state ?? null,
		reviewStatus: review?.status ?? fallbackDisplay?.status ?? null,
		provider: review?.provider ?? fallbackDisplay?.provider ?? null,
		githubRepository: review?.provider === "github" ? review.githubRepository ?? null : null,
		summary: getRowSummary(status, review, detailNames),
		detailNames,
		checks: checkDetails,
		isRefreshing: outcome?.kind === "loading",
		hasLinkedReview: hasLinkedReview(args.worktree)
	};
}
function getReviewSnapshot(args, branch, outcome) {
	if (outcome?.kind === "found") return {
		review: outcome.review,
		hasCacheEntry: true
	};
	if (!args.repo || !branch) return {
		review: void 0,
		hasCacheEntry: false
	};
	const scopedArgs = {
		...args,
		repo: args.repo
	};
	const hostedReviewEntry = args.hostedReviewCache[getHostedReviewKey(scopedArgs, branch)];
	if (hostedReviewEntry?.data && canUseParentPrChecksHostedReviewCacheEntry(args.worktree, hostedReviewEntry.data, hostedReviewEntry)) return {
		review: hostedReviewEntry.data,
		hasCacheEntry: true
	};
	const prEntry = getParentPrChecksGitHubPRCacheEntry({
		prCache: args.prCache,
		repo: args.repo,
		branch,
		settings: args.settings
	});
	if (canUseParentPrChecksGitHubPRCacheEntry(args.worktree, prEntry, hostedReviewEntry)) return {
		review: hostedReviewInfoFromGitHubPRInfo(prEntry.data),
		hasCacheEntry: true
	};
	return {
		review: hostedReviewEntry?.data === null ? null : void 0,
		hasCacheEntry: hostedReviewEntry !== void 0
	};
}
function getRowTitle(worktree, branch, review, fallbackTitle) {
	return review?.title ?? fallbackTitle ?? branch ?? worktree.displayName;
}
function getReviewLabel(review, fallback) {
	const provider = review?.provider ?? fallback?.provider;
	const number = review?.number ?? fallback?.number;
	if (provider === void 0 || number === void 0) return null;
	return provider === "gitlab" ? `!${number}` : `#${number}`;
}
function getCheckDetails(args, review, branch) {
	if (!args.repo || !branch || review?.provider !== "github") return [];
	return getGitHubChecksEntry({
		...args,
		repo: args.repo
	}, review)?.data ?? [];
}
function getCheckDetailNames(checks) {
	const interesting = checks.filter((check) => check.conclusion === "failure" || check.conclusion === "timed_out" || check.conclusion === "cancelled" || check.conclusion === "action_required" || check.conclusion === "pending" || check.conclusion === null || check.status === "queued" || check.status === "in_progress");
	return [...interesting.filter((check) => check.conclusion === "action_required"), ...interesting.filter((check) => check.conclusion !== "action_required")].slice(0, 2).map((check) => check.name);
}
function getGitHubChecksEntry(args, review) {
	const prRepo = review.githubRepository ?? null;
	const withHead = getGitHubRepoCacheKey(args.repo.path, args.repo.id, prChecksCacheSuffix(review.number, prRepo, review.headSha), args.settings, args.repo.connectionId, args.repo.executionHostId, true);
	const withoutHead = getGitHubRepoCacheKey(args.repo.path, args.repo.id, prChecksCacheSuffix(review.number, prRepo), args.settings, args.repo.connectionId, args.repo.executionHostId, true);
	return args.checksCache[withHead] ?? args.checksCache[withoutHead];
}
function getHostedReviewKey(args, branch) {
	return getHostedReviewCacheKey(args.repo.path, branch, args.settings, args.repo.id, args.repo.connectionId, args.repo.executionHostId, true);
}
function getBranchName(worktree) {
	const identity = getWorktreeGitIdentityDisplay(worktree);
	return identity?.kind === "branch" ? identity.branchName : null;
}
function hasLinkedReview(worktree) {
	return Boolean(worktree.linkedPR ?? worktree.linkedGitLabMR ?? worktree.linkedBitbucketPR ?? worktree.linkedAzureDevOpsPR ?? worktree.linkedGiteaPR ?? null);
}
function getLinkedReviewHints(worktree) {
	return {
		linkedGitHubPR: worktree.linkedPR ?? null,
		linkedGitLabMR: worktree.linkedGitLabMR ?? null,
		linkedBitbucketPR: worktree.linkedBitbucketPR ?? null,
		linkedAzureDevOpsPR: worktree.linkedAzureDevOpsPR ?? null,
		linkedGiteaPR: worktree.linkedGiteaPR ?? null
	};
}
export { buildParentPrChecksRows as n, getParentPrChecksRefreshIdentity as r, buildParentPrChecksProjection as t };
