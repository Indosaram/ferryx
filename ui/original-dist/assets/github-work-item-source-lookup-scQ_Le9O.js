import { hu as getExplicitRuntimeOwnerEnvironmentId, op as getActiveRuntimeTarget, qi as getTaskSourceRuntimeSettings, rp as callRuntimeRpc } from "./store-CgXrfmaH.js";
import { st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { E as buildLinearWorkspaceSource, O as getUsableLinearBranchName } from "./native-chat-session-option-cache-DGE3h47U.js";
import { h as parseLinearIssueInput, p as getLinearOrganizationUrlKeyFromIssueUrl } from "./github-links-C1M8w9wX.js";
import { t as getWorktreeAttachmentLabel } from "./worktree-attachment-label-DjzymU5s.js";
function isLinearLinkedWorkItem(item) {
	return item?.provider === "linear" || Boolean(item?.linearIdentifier?.trim());
}
function getLinearLinkedWorkItemBranchName(item) {
	return isLinearLinkedWorkItem(item) ? getUsableLinearBranchName(item?.linearBranchName) : void 0;
}
function buildLinearIssueLinkedWorkItem(issue) {
	return buildLinearWorkspaceSource(issue);
}
function normalizeLinearIdentifier(value) {
	const trimmed = value?.trim();
	if (!trimmed) return null;
	return (parseLinearIssueInput(trimmed)?.identifier ?? trimmed).toUpperCase();
}
function scopeMatchScore(args) {
	const issueWorkspaceId = args.issueWorkspaceId?.trim() || null;
	const worktreeWorkspaceId = args.worktreeWorkspaceId?.trim() || null;
	if (issueWorkspaceId && worktreeWorkspaceId && issueWorkspaceId !== worktreeWorkspaceId) return null;
	const issueOrgKey = args.issueOrganizationUrlKey?.trim().toLowerCase() || null;
	const worktreeOrgKey = args.worktreeOrganizationUrlKey?.trim().toLowerCase() || null;
	if (issueOrgKey && worktreeOrgKey && issueOrgKey !== worktreeOrgKey) return null;
	return Number(Boolean(issueWorkspaceId && worktreeWorkspaceId)) + Number(Boolean(issueOrgKey && worktreeOrgKey));
}
function findScopedAttachment(candidates, issue) {
	const issueOrganizationUrlKey = getLinearOrganizationUrlKeyFromIssueUrl(issue.url);
	let best = null;
	let bestScore = -1;
	for (const worktree of candidates) {
		const score = scopeMatchScore({
			issueWorkspaceId: issue.workspaceId,
			worktreeWorkspaceId: worktree.linkedLinearIssueWorkspaceId,
			issueOrganizationUrlKey,
			worktreeOrganizationUrlKey: worktree.linkedLinearIssueOrganizationUrlKey
		});
		if (score != null && (score > bestScore || score === bestScore && best && worktree.lastActivityAt > best.lastActivityAt)) {
			best = worktree;
			bestScore = score;
		}
	}
	return best;
}
function findLinearIssueWorkspaceAttachment(worktrees, issue) {
	const identifier = normalizeLinearIdentifier(issue.identifier);
	if (!identifier) return null;
	return findScopedAttachment(worktrees.filter((worktree) => !worktree.isArchived && normalizeLinearIdentifier(worktree.linkedLinearIssue) === identifier), issue);
}
function buildLinearIssueWorkspaceAttachmentIndex(worktrees) {
	const index = /* @__PURE__ */ new Map();
	for (const worktree of worktrees) {
		if (worktree.isArchived) continue;
		const identifier = normalizeLinearIdentifier(worktree.linkedLinearIssue);
		if (!identifier) continue;
		const bucket = index.get(identifier);
		if (bucket) bucket.push(worktree);
		else index.set(identifier, [worktree]);
	}
	return index;
}
function findLinearIssueWorkspaceAttachmentInIndex(index, issue) {
	const identifier = normalizeLinearIdentifier(issue.identifier);
	if (!identifier) return null;
	const candidates = index.get(identifier);
	return candidates ? findScopedAttachment(candidates, issue) : null;
}
function getLinearIssueWorkspaceAttachmentLabel(worktree) {
	return getWorktreeAttachmentLabel(worktree);
}
function getGitHubSourceRuntimeHost(sourceContext) {
	if (sourceContext?.provider !== "github") return null;
	const parsedHost = parseExecutionHostId(sourceContext.hostId);
	return parsedHost?.kind === "runtime" ? parsedHost : null;
}
function getGitHubSourceRuntimeTarget(sourceContext) {
	return getActiveRuntimeTarget(getTaskSourceRuntimeSettings(sourceContext?.provider === "github" ? sourceContext : null));
}
function getGitHubMutationRoutingSettings(state, repoId, sourceContext) {
	return { activeRuntimeEnvironmentId: getGitHubSourceRuntimeHost(sourceContext)?.environmentId ?? getExplicitRuntimeOwnerEnvironmentId(state, repoId) };
}
function canUseGitHubRepoContext(repoPath, sourceContext) {
	return Boolean(repoPath) || getGitHubSourceRuntimeHost(sourceContext) !== null;
}
function getGitHubRuntimeRepoId(sourceContext, fallbackRepoId) {
	const fallback = fallbackRepoId ?? void 0;
	return sourceContext?.provider === "github" ? sourceContext.repoId ?? fallback : fallback;
}
function runtimeRepoId(args) {
	return getGitHubRuntimeRepoId(args.sourceContext, args.repoId);
}
async function lookupGitHubWorkItemForSource(args) {
	const target = getGitHubSourceRuntimeTarget(args.sourceContext);
	const item = target.kind === "environment" ? await callRuntimeRpc(target, "github.workItem", {
		repo: runtimeRepoId(args),
		number: args.number,
		type: args.type
	}, { timeoutMs: 3e4 }) : await window.api.gh.workItem({
		repoPath: args.repoPath,
		repoId: args.repoId,
		number: args.number,
		type: args.type
	});
	return item ? {
		...item,
		repoId: args.repoId
	} : null;
}
async function lookupGitHubWorkItemByOwnerRepoForSource(args) {
	const target = getGitHubSourceRuntimeTarget(args.sourceContext);
	const item = target.kind === "environment" ? await callRuntimeRpc(target, "github.workItemByOwnerRepo", {
		repo: runtimeRepoId(args),
		owner: args.owner,
		ownerRepo: args.repo,
		...args.host ? { host: args.host } : {},
		number: args.number,
		type: args.type
	}, { timeoutMs: 3e4 }) : await window.api.gh.workItemByOwnerRepo({
		repoPath: args.repoPath,
		repoId: args.repoId,
		owner: args.owner,
		repo: args.repo,
		...args.host ? { host: args.host } : {},
		number: args.number,
		type: args.type
	});
	return item ? {
		...item,
		repoId: args.repoId
	} : null;
}
function lookupGitHubWorkItemDetailsForSource(args) {
	const sourceContext = args.sourceContext;
	const runtimeHost = getGitHubSourceRuntimeHost(sourceContext);
	if (runtimeHost) return callRuntimeRpc({
		kind: "environment",
		environmentId: runtimeHost.environmentId
	}, "github.workItemDetails", {
		repo: getGitHubRuntimeRepoId(sourceContext, args.repoId),
		number: args.number,
		type: args.type
	}, { timeoutMs: 3e4 });
	return window.api.gh.workItemDetails({
		repoPath: args.repoPath,
		repoId: args.repoId,
		sourceContext,
		number: args.number,
		type: args.type
	});
}
export { getGitHubMutationRoutingSettings as a, getGitHubSourceRuntimeTarget as c, findLinearIssueWorkspaceAttachmentInIndex as d, getLinearIssueWorkspaceAttachmentLabel as f, isLinearLinkedWorkItem as g, getLinearLinkedWorkItemBranchName as h, canUseGitHubRepoContext as i, buildLinearIssueWorkspaceAttachmentIndex as l, buildLinearIssueLinkedWorkItem as m, lookupGitHubWorkItemDetailsForSource as n, getGitHubRuntimeRepoId as o, normalizeLinearIdentifier as p, lookupGitHubWorkItemForSource as r, getGitHubSourceRuntimeHost as s, lookupGitHubWorkItemByOwnerRepoForSource as t, findLinearIssueWorkspaceAttachment as u };
