import { Lp as normalizeRuntimePathForComparison, Rp as normalizeRuntimePathSeparators } from "./store-CgXrfmaH.js";
const EXTERNAL_WORKTREE_VISIBILITY_ROLLOUT_AT = Date.UTC(2026, 4, 23);
const UNKNOWN_EXTERNAL_WORKTREE_PARENT_PATH = "Unknown location";
function trimRuntimePathTrailingSlash(value) {
	if (value === "/" || /^[A-Za-z]:\/$/.test(value)) return value;
	return value.replace(/\/+$/, "");
}
function getExternalWorktreeParentPath(worktreePath) {
	if (!worktreePath) return UNKNOWN_EXTERNAL_WORKTREE_PARENT_PATH;
	const normalized = trimRuntimePathTrailingSlash(normalizeRuntimePathSeparators(worktreePath));
	if (!normalized) return UNKNOWN_EXTERNAL_WORKTREE_PARENT_PATH;
	if (normalized.startsWith("//")) {
		const parts = normalized.slice(2).split("/").filter(Boolean);
		if (parts.length < 2) return UNKNOWN_EXTERNAL_WORKTREE_PARENT_PATH;
		if (parts.length === 2) return `//${parts[0]}/${parts[1]}`;
		return `//${parts.slice(0, -1).join("/")}`;
	}
	const lastSeparatorIndex = normalized.lastIndexOf("/");
	if (lastSeparatorIndex === -1) return UNKNOWN_EXTERNAL_WORKTREE_PARENT_PATH;
	if (lastSeparatorIndex === 0) return "/";
	if (/^[A-Za-z]:\/$/.test(normalized)) return normalized;
	if (/^[A-Za-z]:\/[^/]+$/.test(normalized)) return `${normalized.slice(0, 2)}/`;
	return normalized.slice(0, lastSeparatorIndex);
}
function isLegacyRepoForExternalWorktreeVisibility(repo) {
	if (typeof repo.externalWorktreeVisibilityLegacy === "boolean") return repo.externalWorktreeVisibilityLegacy;
	if (repo.externalWorktreeVisibility === void 0) return true;
	if (!Number.isFinite(repo.addedAt)) return true;
	return repo.addedAt < EXTERNAL_WORKTREE_VISIBILITY_ROLLOUT_AT;
}
function effectiveExternalWorktreeVisibility(repo, isLegacyRepoForVisibility) {
	if (repo.externalWorktreeVisibility) return repo.externalWorktreeVisibility;
	return isLegacyRepoForVisibility ? "show" : "hide";
}
function effectiveAgentWorktreeVisibility(repo) {
	return repo.agentWorktreeVisibility === "show" ? "show" : "hide";
}
function normalizeExternalWorktreeInboxPath(path) {
	return normalizeRuntimePathForComparison(path);
}
function mergeExternalWorktreeInboxPaths(existing, additions) {
	const seen = new Set((existing ?? []).map((path) => normalizeExternalWorktreeInboxPath(path)));
	const merged = [...existing ?? []];
	for (const path of additions) {
		const normalized = normalizeExternalWorktreeInboxPath(path);
		if (!normalized || seen.has(normalized)) continue;
		seen.add(normalized);
		merged.push(path);
	}
	return merged;
}
function getHiddenExternalWorktrees(detected) {
	if (detected?.authoritative !== true) return [];
	return detected.worktrees.filter((worktree) => !worktree.visible && isUserFacingExternalWorktree(worktree));
}
function isUserFacingExternalWorktree(worktree) {
	return !worktree.selectedCheckout && worktree.ownership !== "orca-managed" && worktree.ownership !== "agent-scratch";
}
function isImportableExternalWorktree(worktree) {
	return !worktree.selectedCheckout && worktree.ownership !== "orca-managed";
}
function getHiddenImportableExternalWorktrees(detected) {
	if (detected?.authoritative !== true) return [];
	return detected.worktrees.filter((worktree) => !worktree.visible && isImportableExternalWorktree(worktree));
}
function getVisibleNonOrcaWorktrees(detected) {
	if (detected?.authoritative !== true) return [];
	return detected.worktrees.filter((worktree) => worktree.visible && !worktree.selectedCheckout && worktree.ownership !== "orca-managed");
}
function isExternalWorktreeDiscoverySuppressed(repo) {
	return typeof repo.externalWorktreeDiscoverySuppressedAt === "number";
}
function hasCompletedInitialExternalWorktreeImportPrompt(repo) {
	return typeof repo.externalWorktreeVisibilityPromptDismissedAt === "number";
}
function shouldOfferNewExternalWorktreeInbox(repo) {
	if (isExternalWorktreeDiscoverySuppressed(repo)) return false;
	if (!hasCompletedInitialExternalWorktreeImportPrompt(repo)) return false;
	return effectiveExternalWorktreeVisibility(repo, isLegacyRepoForExternalWorktreeVisibility(repo)) === "hide";
}
function getNewExternalWorktreeInboxWorktrees(detected, repo) {
	if (!shouldOfferNewExternalWorktreeInbox(repo)) return [];
	const baseline = new Set((repo.externalWorktreeInboxBaselinePaths ?? []).map((path) => normalizeExternalWorktreeInboxPath(path)));
	return getHiddenExternalWorktrees(detected).filter((worktree) => !baseline.has(normalizeExternalWorktreeInboxPath(worktree.path)));
}
export { mergeExternalWorktreeInboxPaths as a, getExternalWorktreeParentPath as c, getVisibleNonOrcaWorktrees as i, isLegacyRepoForExternalWorktreeVisibility as l, getHiddenImportableExternalWorktrees as n, effectiveAgentWorktreeVisibility as o, getNewExternalWorktreeInboxWorktrees as r, effectiveExternalWorktreeVisibility as s, getHiddenExternalWorktrees as t };
