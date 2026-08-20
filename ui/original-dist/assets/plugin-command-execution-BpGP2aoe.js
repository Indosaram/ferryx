import { $a as isClipboardTextByteLengthOverLimit, Cm as projectHostSetupProjectionFromRepos, Fn as linearStatus, Tc as resolveWorktreeDisplayName, Xn as issueCacheKey, qg as isGitRepoKind, wc as resolveWorktreeBranchLabel, wm as githubRepoIdentityKey, ym as getProjectIdentityKey } from "./store-CgXrfmaH.js";
import { y as isKeybindingActionId } from "./plugin-manifest-Bs-50M_g.js";
import { $ as getRepoExecutionHostId, Y as ALL_EXECUTION_HOSTS_SCOPE, nt as isRuntimeOwnedSshTargetId } from "./agent-status-3vUKbY6l.js";
import { R as parseJiraIssueUrl, z as parseGitLabIssueOrMRLink } from "./native-chat-session-option-cache-DGE3h47U.js";
import { f as findLinearIssueWorkspaceLookupIds, g as parseLinearIssueUrlIntent, i as parseGitHubIssueOrPRLink, m as isLinearIssueUrlResolutionMatch } from "./github-links-C1M8w9wX.js";
import { p as normalizeLinearIdentifier } from "./github-work-item-source-lookup-scQ_Le9O.js";
import { l as isLegacyRepoForExternalWorktreeVisibility, s as effectiveExternalWorktreeVisibility, t as getHiddenExternalWorktrees } from "./worktree-ownership-B1VtdtJF.js";
var listeners = /* @__PURE__ */ new Set();
function subscribeCmdJRowIndexJump(listener) {
	listeners.add(listener);
	return () => {
		listeners.delete(listener);
	};
}
function emitCmdJRowIndexJump(index) {
	for (const listener of listeners) listener(index);
}
function getHiddenImportedWorktrees(detected) {
	return getHiddenExternalWorktrees(detected);
}
function buildImportedWorktreesCardCandidates(args) {
	const visibleRepoIds = args.visibleWorktrees ? new Set(args.visibleWorktrees.map((worktree) => worktree.repoId)) : null;
	const filterRepoIds = args.filterRepoIds?.length ? new Set(args.filterRepoIds) : null;
	const candidates = /* @__PURE__ */ new Map();
	for (const repo of args.repos) {
		if (filterRepoIds && !filterRepoIds.has(repo.id)) continue;
		if (visibleRepoIds && !visibleRepoIds.has(repo.id)) continue;
		if (!isGitRepoKind(repo)) continue;
		if (typeof repo.externalWorktreeVisibilityPromptDismissedAt === "number") continue;
		if (effectiveExternalWorktreeVisibility(repo, isLegacyRepoForExternalWorktreeVisibility(repo)) !== "hide" && !args.forceVisibleRepoIds?.has(repo.id)) continue;
		const hiddenWorktrees = getHiddenImportedWorktrees(args.detectedWorktreesByRepo[repo.id]);
		if (hiddenWorktrees.length > 0) candidates.set(repo.id, {
			repo,
			hiddenWorktrees
		});
	}
	return candidates;
}
const WORKTREE_PALETTE_QUERY_MAX_BYTES = 2 * 1024;
function isWorktreePaletteQueryTooLarge(query, maxBytes = WORKTREE_PALETTE_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function extractWorktreePaletteCommentSnippet(comment, matchStart, matchEnd) {
	let snippetStart = Math.max(0, matchStart - 40);
	let snippetEnd = Math.min(comment.length, matchEnd + 40);
	for (let i = 0; i < 10 && snippetStart > 0; i++) {
		if (/\s/.test(comment[snippetStart - 1])) break;
		snippetStart--;
	}
	for (let i = 0; i < 10 && snippetEnd < comment.length; i++) {
		if (/\s/.test(comment[snippetEnd])) break;
		snippetEnd++;
	}
	const prefix = snippetStart > 0 ? "…" : "";
	const suffix = snippetEnd < comment.length ? "…" : "";
	return {
		text: `${prefix}${comment.slice(snippetStart, snippetEnd)}${suffix}`,
		matchRange: {
			start: prefix.length + matchStart - snippetStart,
			end: prefix.length + matchEnd - snippetStart
		}
	};
}
function matchWorktreePaletteReview(review, query, numericQuery) {
	const isMergeRequest = review.provider === "gitlab";
	const numberPrefix = isMergeRequest ? "MR !" : "PR #";
	const hasPullRequestSigil = query.startsWith("#");
	const hasMergeRequestSigil = query.startsWith("!");
	const sigilMatchesProvider = !hasPullRequestSigil && !hasMergeRequestSigil || hasPullRequestSigil && !isMergeRequest || hasMergeRequestSigil && isMergeRequest;
	const reviewNumericQuery = hasMergeRequestSigil ? query.slice(1) : numericQuery;
	const reviewNumberIndex = sigilMatchesProvider ? String(review.number).indexOf(reviewNumericQuery) : -1;
	if (reviewNumericQuery && reviewNumberIndex !== -1) return {
		labelKind: isMergeRequest ? "mr" : "pr",
		text: `${numberPrefix}${review.number}`,
		matchRange: {
			start: numberPrefix.length + reviewNumberIndex,
			end: numberPrefix.length + reviewNumberIndex + reviewNumericQuery.length
		}
	};
	const title = review.title ?? "";
	const titleIndex = title.toLowerCase().indexOf(query);
	if (titleIndex === -1) return null;
	return {
		labelKind: isMergeRequest ? "mr" : "pr",
		text: title,
		matchRange: {
			start: titleIndex,
			end: titleIndex + query.length
		}
	};
}
function gitLabProjectKey(slug) {
	return `${slug.host.replace(/:\d+$/, "").toLowerCase()}/${slug.path.replace(/^\/+/, "").replace(/\/+$/, "").replace(/\.git$/i, "").toLowerCase()}`;
}
function gitLabLinksEqual(left, right) {
	return left.type === right.type && left.number === right.number && gitLabProjectKey(left.slug) === gitLabProjectKey(right.slug);
}
function repoMatchesGitLabSlug(repo, slug) {
	const identity = repo?.gitRemoteIdentity;
	if (!identity?.canonicalKey) return "unknown";
	if (identity.canonicalKey.replace(/\/+$/, "").toLowerCase() === gitLabProjectKey(slug)) return true;
	return identity.remoteName === "upstream" ? "unknown" : false;
}
function worktreeMatchesGitLabUrl(worktree, link, repo, review) {
	const linkedUrl = worktree.linkedWorkItem?.url ? parseGitLabIssueOrMRLink(worktree.linkedWorkItem.url) : null;
	if (linkedUrl && gitLabLinksEqual(linkedUrl, link)) return true;
	const reviewUrl = review?.provider === "gitlab" && review.url ? parseGitLabIssueOrMRLink(review.url) : null;
	if (reviewUrl && gitLabLinksEqual(reviewUrl, link)) return true;
	const linkedItem = worktree.linkedWorkItem;
	if (!(linkedItem?.provider === "gitlab" && linkedItem.type === link.type && linkedItem.number === link.number || (link.type === "mr" ? worktree.linkedGitLabMR === link.number : worktree.linkedGitLabIssue === link.number))) return false;
	const repoMatch = repoMatchesGitLabSlug(repo, link.slug);
	if (repoMatch !== "unknown") return repoMatch;
	return !(linkedUrl && linkedUrl.type === link.type && linkedUrl.number === link.number);
}
function withResolvedCmdJGitHubPreview(preview, resolvedTitle, loading) {
	if (preview.provider !== "github") return preview;
	if (resolvedTitle) return {
		...preview,
		subtitle: resolvedTitle,
		createLabel: `${preview.createLabel}: ${resolvedTitle}`,
		loading: false
	};
	return loading ? {
		...preview,
		loading: true
	} : preview;
}
function githubIdentityKey(slug) {
	return githubRepoIdentityKey({
		owner: slug.owner,
		repo: slug.repo,
		host: slug.host?.replace(/^www\./i, "")
	});
}
function githubLinksEqual(left, right) {
	return left.type === right.type && left.number === right.number && githubIdentityKey(left.slug) === githubIdentityKey(right.slug);
}
function parseOwnerRepoDisplayName(value) {
	const match = /^([^/]+)\/([^/]+)$/.exec(value?.trim() ?? "");
	if (!match) return null;
	return {
		owner: match[1],
		repo: match[2]
	};
}
function repoMatchesGitHubSlug(repo, slug) {
	if (!repo) return "unknown";
	const fromName = parseOwnerRepoDisplayName(repo.displayName);
	if (fromName) return githubIdentityKey({
		...fromName,
		host: slug.host
	}) === githubIdentityKey(slug);
	if (repo.upstream?.owner && repo.upstream.repo) return githubIdentityKey(repo.upstream) === githubIdentityKey(slug);
	return "unknown";
}
function parseCmdJTaskSourceUrl(query) {
	const trimmed = query.trim();
	if (!trimmed || isWorktreePaletteQueryTooLarge(trimmed)) return null;
	const linear = parseLinearIssueUrlIntent(trimmed);
	if (linear) return {
		provider: "linear",
		intent: linear
	};
	const github = parseGitHubIssueOrPRLink(trimmed);
	if (github) return {
		provider: "github",
		link: github
	};
	const gitlab = parseGitLabIssueOrMRLink(trimmed);
	if (gitlab) return {
		provider: "gitlab",
		link: gitlab
	};
	const jira = parseJiraIssueUrl(trimmed);
	if (jira) return {
		provider: "jira",
		parsed: jira
	};
	return null;
}
function getCmdJTaskUrlCreatePreview(intent) {
	if (intent.provider === "linear") return null;
	if (intent.provider === "github") {
		const { slug, number, type } = intent.link;
		const repo = `${slug.owner}/${slug.repo}`;
		const kindLabel = type === "pr" ? "GitHub pull request" : "GitHub issue";
		return {
			provider: "github",
			identifier: `#${number}`,
			subtitle: repo,
			kindLabel,
			createLabel: `Create worktree from ${kindLabel} ${repo}#${number}`
		};
	}
	if (intent.provider === "gitlab") {
		const { slug, number, type } = intent.link;
		const project = `${slug.host}/${slug.path}`;
		const kindLabel = type === "mr" ? "GitLab merge request" : "GitLab issue";
		const identifier = type === "mr" ? `!${number}` : `#${number}`;
		return {
			provider: "gitlab",
			identifier,
			subtitle: project,
			kindLabel,
			createLabel: `Create worktree from ${kindLabel} ${project}${identifier}`
		};
	}
	return {
		provider: "jira",
		identifier: intent.parsed.issueKey,
		subtitle: intent.parsed.origin.replace(/^https?:\/\//, ""),
		kindLabel: "Jira issue",
		createLabel: `Create worktree from Jira issue ${intent.parsed.issueKey}`
	};
}
function supportingText(labelKind, text) {
	return {
		labelKind,
		text,
		matchRange: {
			start: 0,
			end: text.length
		}
	};
}
function result(worktreeId, matchedField, text) {
	return {
		worktreeId,
		matchedField,
		displayNameRange: null,
		branchRange: null,
		repoRange: null,
		supportingText: text
	};
}
function worktreeMatchesGitHubUrl(worktree, link, repo, review) {
	const linkedUrl = worktree.linkedWorkItem?.url ? parseGitHubIssueOrPRLink(worktree.linkedWorkItem.url) : null;
	if (linkedUrl && githubLinksEqual(linkedUrl, link)) return true;
	const reviewUrl = review?.url ? parseGitHubIssueOrPRLink(review.url) : null;
	if (reviewUrl && githubLinksEqual(reviewUrl, link)) return true;
	const linkedItem = worktree.linkedWorkItem;
	if (!(linkedItem?.provider === "github" && linkedItem.type === link.type && linkedItem.number === link.number || (link.type === "pr" ? worktree.linkedPR === link.number : worktree.linkedIssue === link.number))) return false;
	return repoMatchesGitHubSlug(repo, link.slug) !== false;
}
function worktreeMatchesLinearUrl(worktree, intent) {
	const identifier = normalizeLinearIdentifier(intent.identifier);
	const linkedIdentifier = normalizeLinearIdentifier(worktree.linkedLinearIssue) ?? normalizeLinearIdentifier(worktree.linkedWorkItem?.linearIdentifier);
	if (!identifier || linkedIdentifier !== identifier) {
		const linkedUrl = worktree.linkedWorkItem?.url ? parseLinearIssueUrlIntent(worktree.linkedWorkItem.url) : null;
		if (!linkedUrl || linkedUrl.identifier !== intent.identifier || linkedUrl.organizationUrlKey.toLowerCase() !== intent.organizationUrlKey.toLowerCase()) return false;
	}
	const worktreeOrg = worktree.linkedLinearIssueOrganizationUrlKey?.trim().toLowerCase();
	if (worktreeOrg && worktreeOrg !== intent.organizationUrlKey.toLowerCase()) return false;
	return true;
}
function worktreeMatchesJiraUrl(worktree, parsed) {
	if (worktree.linkedWorkItem?.jiraIdentifier?.toUpperCase() === parsed.issueKey) return true;
	const linkedUrl = worktree.linkedWorkItem?.url ? parseJiraIssueUrl(worktree.linkedWorkItem.url) : null;
	return linkedUrl !== null && linkedUrl.issueKey === parsed.issueKey && linkedUrl.origin === parsed.origin && linkedUrl.sitePath === parsed.sitePath;
}
function matchWorktreePaletteTaskUrl(args) {
	const { worktree, intent, repo, review } = args;
	if (intent.provider === "github") {
		if (!worktreeMatchesGitHubUrl(worktree, intent.link, repo, review)) return null;
		return result(worktree.id, intent.link.type === "pr" ? "pr" : "issue", supportingText(intent.link.type === "pr" ? "pr" : "issue", `${intent.link.type === "pr" ? "PR" : "Issue"} #${intent.link.number}`));
	}
	if (intent.provider === "linear") {
		if (!worktreeMatchesLinearUrl(worktree, intent.intent)) return null;
		return result(worktree.id, "issue", supportingText("issue", intent.intent.identifier));
	}
	if (intent.provider === "gitlab") {
		if (!worktreeMatchesGitLabUrl(worktree, intent.link, repo, review)) return null;
		return result(worktree.id, intent.link.type === "mr" ? "pr" : "issue", supportingText(intent.link.type === "mr" ? "mr" : "issue", `${intent.link.type === "mr" ? "MR" : "Issue"} #${intent.link.number}`));
	}
	if (!worktreeMatchesJiraUrl(worktree, intent.parsed)) return null;
	return result(worktree.id, "issue", supportingText("issue", intent.parsed.issueKey));
}
function getWorktreePaletteSearchScope(args) {
	if (!args.hasQuery) return [...args.emptyQueryWorktrees];
	return args.allWorktrees.filter((worktree) => !worktree.isArchived);
}
function makeResult(worktreeId, matchedField, overrides = {}) {
	return {
		worktreeId,
		matchedField,
		displayNameRange: null,
		branchRange: null,
		repoRange: null,
		supportingText: null,
		...overrides
	};
}
function searchWorktrees(worktrees, query, repoMap, prCache, issueCache, workspacePortsByWorktreeId, checksReviewByWorktree) {
	if (isWorktreePaletteQueryTooLarge(query)) return [];
	const trimmedQuery = query.trim();
	if (!trimmedQuery) return worktrees.map((worktree) => makeResult(worktree.id, null));
	const q = trimmedQuery.toLowerCase();
	const numericQuery = q.startsWith("#") ? q.slice(1) : q;
	const results = [];
	const taskSourceUrl = parseCmdJTaskSourceUrl(trimmedQuery);
	if (taskSourceUrl) {
		for (const worktree of worktrees) {
			const match = matchWorktreePaletteTaskUrl({
				worktree,
				intent: taskSourceUrl,
				repo: repoMap.get(worktree.repoId),
				review: checksReviewByWorktree?.get(worktree)
			});
			if (match) results.push(match);
		}
		return results;
	}
	const slashIndex = q.indexOf("/");
	const composite = slashIndex > 0 && slashIndex < q.length - 1 ? {
		repoPart: q.slice(0, slashIndex),
		branchPart: q.slice(slashIndex + 1)
	} : null;
	for (const worktree of worktrees) {
		if (composite) {
			const repoName = repoMap.get(worktree.repoId)?.displayName ?? "";
			const branch$1 = resolveWorktreeBranchLabel(worktree);
			const repoIdx = repoName.toLowerCase().indexOf(composite.repoPart);
			const branchIdx = branch$1.toLowerCase().indexOf(composite.branchPart);
			if (repoIdx !== -1 && branchIdx !== -1) {
				results.push(makeResult(worktree.id, "branch", {
					repoRange: {
						start: repoIdx,
						end: repoIdx + composite.repoPart.length
					},
					branchRange: {
						start: branchIdx,
						end: branchIdx + composite.branchPart.length
					}
				}));
				continue;
			}
		}
		const nameIndex = resolveWorktreeDisplayName(worktree).toLowerCase().indexOf(q);
		if (nameIndex !== -1) {
			results.push(makeResult(worktree.id, "displayName", { displayNameRange: {
				start: nameIndex,
				end: nameIndex + q.length
			} }));
			continue;
		}
		const branch = resolveWorktreeBranchLabel(worktree);
		const branchIndex = branch.toLowerCase().indexOf(q);
		if (branchIndex !== -1) {
			results.push(makeResult(worktree.id, "branch", { branchRange: {
				start: branchIndex,
				end: branchIndex + q.length
			} }));
			continue;
		}
		const repoIndex = (repoMap.get(worktree.repoId)?.displayName ?? "").toLowerCase().indexOf(q);
		if (repoIndex !== -1) {
			results.push(makeResult(worktree.id, "repo", { repoRange: {
				start: repoIndex,
				end: repoIndex + q.length
			} }));
			continue;
		}
		if (worktree.comment) {
			const commentIndex = worktree.comment.toLowerCase().indexOf(q);
			if (commentIndex !== -1) {
				const snippet = extractWorktreePaletteCommentSnippet(worktree.comment, commentIndex, commentIndex + q.length);
				results.push(makeResult(worktree.id, "comment", { supportingText: {
					labelKind: "comment",
					text: snippet.text,
					matchRange: snippet.matchRange
				} }));
				continue;
			}
		}
		if (!numericQuery) continue;
		const workspacePorts = workspacePortsByWorktreeId?.get(worktree.id) ?? [];
		let matchedPort = false;
		for (const port of workspacePorts) {
			const portText = String(port.port);
			const portIndex = portText.indexOf(numericQuery);
			if (portIndex !== -1) {
				const label = port.processName ? `${portText} · ${port.processName}` : portText;
				results.push(makeResult(worktree.id, "port", { supportingText: {
					labelKind: "port",
					text: label,
					matchRange: {
						start: portIndex,
						end: portIndex + numericQuery.length
					}
				} }));
				matchedPort = true;
				break;
			}
		}
		if (matchedPort) continue;
		const repo = repoMap.get(worktree.repoId);
		const checksReview = checksReviewByWorktree?.get(worktree);
		const hasChecksReviewEntry = checksReview !== void 0;
		if (checksReview) {
			const supportingText$1 = matchWorktreePaletteReview(checksReview, q, numericQuery);
			if (supportingText$1) {
				results.push(makeResult(worktree.id, "pr", { supportingText: supportingText$1 }));
				continue;
			}
		}
		const prKey = repo ? `${repo.path}::${branch}` : "";
		const pr = !hasChecksReviewEntry && prKey && prCache ? prCache[prKey]?.data : void 0;
		if (pr) {
			const supportingText$1 = matchWorktreePaletteReview({
				...pr,
				provider: "github"
			}, q, numericQuery);
			if (supportingText$1) {
				results.push(makeResult(worktree.id, "pr", { supportingText: supportingText$1 }));
				continue;
			}
		} else if (!hasChecksReviewEntry && worktree.linkedPR != null) {
			const prText = `PR #${worktree.linkedPR}`;
			const prNumberIndex = String(worktree.linkedPR).indexOf(numericQuery);
			if (prNumberIndex !== -1) {
				results.push(makeResult(worktree.id, "pr", { supportingText: {
					labelKind: "pr",
					text: prText,
					matchRange: {
						start: 4 + prNumberIndex,
						end: 4 + prNumberIndex + numericQuery.length
					}
				} }));
				continue;
			}
		}
		if (worktree.linkedIssue == null) continue;
		const issueText = `Issue #${worktree.linkedIssue}`;
		const issueNumberIndex = String(worktree.linkedIssue).indexOf(numericQuery);
		if (issueNumberIndex !== -1) {
			results.push(makeResult(worktree.id, "issue", { supportingText: {
				labelKind: "issue",
				text: issueText,
				matchRange: {
					start: 7 + issueNumberIndex,
					end: 7 + issueNumberIndex + numericQuery.length
				}
			} }));
			continue;
		}
		const issueKey = repo ? issueCacheKey(repo.path, repo.id, worktree.linkedIssue, void 0, repo.connectionId, repo.executionHostId) : "";
		const issue = issueKey && issueCache ? issueCache[issueKey]?.data : void 0;
		if (!issue?.title) continue;
		const issueTitleIndex = issue.title.toLowerCase().indexOf(q);
		if (issueTitleIndex !== -1) results.push(makeResult(worktree.id, "issue", { supportingText: {
			labelKind: "issue",
			text: issue.title,
			matchRange: {
				start: issueTitleIndex,
				end: issueTitleIndex + q.length
			}
		} }));
	}
	return results;
}
async function fetchMatchingLinearIssue(intent, workspaceId, sourceContext, fetchLinearIssue) {
	try {
		const issue = await fetchLinearIssue(intent.identifier, workspaceId, { sourceContext });
		return issue && isLinearIssueUrlResolutionMatch(intent, issue) ? issue : null;
	} catch {
		return null;
	}
}
async function lookupLinearIssueUrl({ intent, knownStatus, sourceContext, fetchLinearIssue, readLinearStatus = linearStatus }) {
	const triedWorkspaceIds = /* @__PURE__ */ new Set();
	const lookupInStatus = async (status) => {
		for (const workspaceId of findLinearIssueWorkspaceLookupIds(intent, status)) {
			if (triedWorkspaceIds.has(workspaceId)) continue;
			triedWorkspaceIds.add(workspaceId);
			const issue = await fetchMatchingLinearIssue(intent, workspaceId, sourceContext, fetchLinearIssue);
			if (issue) return issue;
		}
		return null;
	};
	const knownIssue = await lookupInStatus(knownStatus);
	if (knownIssue) return knownIssue;
	const currentStatus = await readLinearStatus(sourceContext).catch(() => null);
	return currentStatus ? lookupInStatus(currentStatus) : null;
}
function getNewWorkspaceDialogEligibleRepos(repos) {
	return repos.filter((repo) => Boolean(repo.path) && !isRuntimeOwnedSshTargetId(repo.connectionId));
}
function resolveNewWorkspaceDialogRepoId({ eligibleRepos, draftRepoId, initialRepoId, activeRepoId, focusedHostScope }) {
	const focusedHostRepo = focusedHostScope && focusedHostScope !== "all" ? eligibleRepos.find((repo) => getRepoExecutionHostId(repo) === focusedHostScope) : void 0;
	return (draftRepoId && eligibleRepos.find((repo) => repo.id === draftRepoId) || initialRepoId && eligibleRepos.find((repo) => repo.id === initialRepoId) || activeRepoId && eligibleRepos.find((repo) => repo.id === activeRepoId) || focusedHostRepo || eligibleRepos[0])?.id ?? "";
}
function resolveNewWorkspaceDialogGitRepoId(args) {
	const repoId = resolveNewWorkspaceDialogRepoId(args);
	const repo = repoId ? args.eligibleRepos.find((entry) => entry.id === repoId) : null;
	return repo && isGitRepoKind(repo) ? repo.id : null;
}
function getComposerEligibleRepos(repos) {
	return getNewWorkspaceDialogEligibleRepos(repos);
}
function resolveComposerActiveRepoId(repos, eligibleRepos, activeRepoId) {
	if (!activeRepoId) return activeRepoId ?? null;
	const activeRepo = repos.find((repo) => repo.id === activeRepoId);
	if (!activeRepo || !isRuntimeOwnedSshTargetId(activeRepo.connectionId)) return activeRepoId;
	const projectKey = getProjectIdentityKey(activeRepo);
	return eligibleRepos.find((repo) => getProjectIdentityKey(repo) === projectKey)?.id ?? activeRepoId;
}
function resolveComposerRepoId({ eligibleRepos, draftRepoId, initialRepoId, activeRepoId, focusedHostScope }) {
	return resolveNewWorkspaceDialogRepoId({
		eligibleRepos,
		draftRepoId,
		initialRepoId,
		activeRepoId,
		focusedHostScope
	});
}
function resolveComposerGitRepoId(args) {
	return resolveNewWorkspaceDialogGitRepoId(args);
}
function getProjectSetupModel({ eligibleRepos, projects, projectHostSetups }) {
	if (projects?.length || projectHostSetups?.length) return {
		projects: projects ?? [],
		setups: projectHostSetups ?? []
	};
	if (eligibleRepos.length === 0) return null;
	const projection = projectHostSetupProjectionFromRepos(eligibleRepos);
	return {
		projects: projection.projects,
		setups: projection.setups
	};
}
function isReadySetup(setup) {
	return setup.setupState === "ready";
}
function createTarget(setup, reposById) {
	const candidates = reposById.get(setup.repoId) ?? [];
	const repo = candidates.find((candidate) => getRepoExecutionHostId(candidate) === setup.hostId) ?? (candidates.length === 1 ? candidates[0] : null);
	if (!repo) return null;
	return {
		projectId: setup.projectId,
		hostId: setup.hostId,
		projectHostSetupId: setup.id,
		repoId: setup.repoId,
		repo,
		setup
	};
}
function findReadySetupTarget(setups, reposById, predicate) {
	for (const setup of setups) {
		if (!isReadySetup(setup) || !predicate(setup)) continue;
		const target = createTarget(setup, reposById);
		if (target) return target;
	}
	return null;
}
function resolveWorkspaceCreationTarget(input) {
	const { eligibleRepos, focusedHostScope, hostId, projectHostSetupId, projectId } = input;
	if (eligibleRepos.length === 0) return {
		status: "unavailable",
		reason: "no-eligible-repo"
	};
	const model = getProjectSetupModel(input);
	const reposById = /* @__PURE__ */ new Map();
	for (const repo of eligibleRepos) {
		const candidates = reposById.get(repo.id) ?? [];
		candidates.push(repo);
		reposById.set(repo.id, candidates);
	}
	const actionableHostIds = input.actionableHostIds;
	const allSetups = model?.setups ?? [];
	const setups = actionableHostIds ? allSetups.filter((setup) => actionableHostIds.has(setup.hostId)) : allSetups;
	if (projectHostSetupId) {
		const setup = allSetups.find((entry) => entry.id === projectHostSetupId);
		if (!setup) return {
			status: "unavailable",
			reason: "setup-not-found"
		};
		if (actionableHostIds && !actionableHostIds.has(setup.hostId)) return {
			status: "unavailable",
			reason: "setup-not-found"
		};
		if (!isReadySetup(setup)) return {
			status: "unavailable",
			reason: "setup-not-ready"
		};
		const canonical = findReadySetupTarget(setups, reposById, (entry) => entry.projectId === setup.projectId && entry.hostId === setup.hostId) ?? createTarget(setup, reposById);
		if (canonical) return {
			status: "ready",
			target: canonical
		};
		return {
			status: "unavailable",
			reason: "setup-not-found"
		};
	}
	if (projectId && !model?.projects.some((project) => project.id === projectId)) return {
		status: "unavailable",
		reason: "project-not-found"
	};
	if (projectId && hostId) {
		const hostSetup = setups.find((setup) => setup.projectId === projectId && setup.hostId === hostId);
		if (hostSetup && !isReadySetup(hostSetup)) return {
			status: "unavailable",
			reason: "setup-not-ready"
		};
		const target = findReadySetupTarget(setups, reposById, (setup) => setup.projectId === projectId && setup.hostId === hostId);
		if (target) return {
			status: "ready",
			target
		};
		return {
			status: "unavailable",
			reason: "project-not-set-up-on-host"
		};
	}
	if (projectId) {
		const focusedHostId = focusedHostScope && focusedHostScope !== "all" ? focusedHostScope : null;
		const focusedTarget = focusedHostId ? findReadySetupTarget(setups, reposById, (setup) => setup.projectId === projectId && setup.hostId === focusedHostId) : null;
		if (focusedTarget) return {
			status: "ready",
			target: focusedTarget
		};
		const target = findReadySetupTarget(setups, reposById, (setup) => setup.projectId === projectId);
		if (target) return {
			status: "ready",
			target
		};
		return {
			status: "unavailable",
			reason: "project-has-no-ready-setup"
		};
	}
	if (hostId) {
		const target = findReadySetupTarget(setups, reposById, (setup) => setup.hostId === hostId);
		if (target) return {
			status: "ready",
			target
		};
		return {
			status: "unavailable",
			reason: "project-not-set-up-on-host"
		};
	}
	const repoId = resolveComposerRepoId(input);
	const legacyCandidates = repoId ? reposById.get(repoId) ?? [] : [];
	const legacyRepo = (focusedHostScope && focusedHostScope !== "all" ? legacyCandidates.find((candidate) => getRepoExecutionHostId(candidate) === focusedHostScope) : null) ?? (legacyCandidates.length === 1 ? legacyCandidates[0] : null);
	let legacyTarget = null;
	if (legacyRepo) {
		const projectedLegacySetup = projectHostSetupProjectionFromRepos([legacyRepo]).setups[0];
		const legacyHostId = getRepoExecutionHostId(legacyRepo);
		const legacySetup = setups.find((setup) => setup.repoId === legacyRepo.id && setup.hostId === legacyHostId && isReadySetup(setup)) ?? (!actionableHostIds || actionableHostIds.has(projectedLegacySetup.hostId) ? projectedLegacySetup : null);
		legacyTarget = legacySetup ? createTarget(legacySetup, reposById) : null;
	} else if (repoId) legacyTarget = findReadySetupTarget(setups, reposById, (setup) => setup.repoId === repoId);
	if (legacyTarget) return {
		status: "ready",
		target: legacyTarget
	};
	const fallbackTarget = findReadySetupTarget(setups, reposById, () => true);
	return fallbackTarget ? {
		status: "ready",
		target: fallbackTarget
	} : {
		status: "unavailable",
		reason: legacyRepo ? "setup-not-found" : "no-eligible-repo"
	};
}
function resolveWorkspaceCreationRepoId(input) {
	const resolution = resolveWorkspaceCreationTarget(input);
	return resolution.status === "ready" ? resolution.target.repoId : "";
}
var currentDispatcher = null;
function registerAppCommandDispatcher(dispatcher) {
	currentDispatcher = dispatcher;
	return () => {
		if (currentDispatcher === dispatcher) currentDispatcher = null;
	};
}
function dispatchAppCommand(actionId, source) {
	return isKeybindingActionId(actionId) && currentDispatcher !== null ? currentDispatcher(actionId, source) : false;
}
async function executePluginCommand(command, source) {
	if (command.handler.type === "built-in") {
		if (!dispatchAppCommand(command.handler.action, source)) throw new Error("built-in action is unavailable in the current context");
		return;
	}
	await window.api.plugins.invokeCommand({
		pluginKey: command.pluginKey,
		commandId: command.id
	});
}
export { emitCmdJRowIndexJump as _, getComposerEligibleRepos as a, lookupLinearIssueUrl as c, getCmdJTaskUrlCreatePreview as d, parseCmdJTaskSourceUrl as f, getHiddenImportedWorktrees as g, buildImportedWorktreesCardCandidates as h, resolveWorkspaceCreationTarget as i, getWorktreePaletteSearchScope as l, isWorktreePaletteQueryTooLarge as m, registerAppCommandDispatcher as n, resolveComposerActiveRepoId as o, withResolvedCmdJGitHubPreview as p, resolveWorkspaceCreationRepoId as r, resolveComposerGitRepoId as s, executePluginCommand as t, searchWorktrees as u, subscribeCmdJRowIndexJump as v };
