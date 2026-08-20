import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { Jc as parsePaneKey, Nd as resolveHookCommandSourcePolicy, Qc as releaseAgentStartupDeliveryConsumed, Vu as createBrowserUuid, Xc as isAgentStartupDeliveryConsumed, Yc as agentStartupDeliveryKey, Zc as markAgentStartupDeliveryConsumed, t as useAppStore } from "./store-CgXrfmaH.js";
import { y as isShellProcess } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { n as tuiAgentToAgentKind } from "./agent-kind-Dfx6MnkP.js";
import { a as track } from "./telemetry-ZyUPyKMD.js";
import { i as pasteDraftToAgentPtyWhenReady, r as getSettingsForAgentTabRuntimeOwner } from "./agent-paste-draft-C2PA7vXu.js";
import { n as isExpectedAgentProcess, s as inspectRuntimeTerminalProcess, t as isAgentForegroundWrapperProcess, u as sendRuntimePtyInputVerified } from "./agent-process-recognition-BB0O3DaN.js";
import { a as parseGitHubIssueOrPRNumber, i as parseGitHubIssueOrPRLink, p as getLinearOrganizationUrlKeyFromIssueUrl } from "./github-links-C1M8w9wX.js";
async function sendFollowupPromptWhenAgentReady(args) {
	const { ptyId, expectedProcess, prompt, settings } = args;
	if (!await waitForAgentForeground(ptyId, expectedProcess, settings)) return false;
	try {
		return await sendRuntimePtyInputVerified(settings, ptyId, `${prompt}\r`);
	} catch {
		return false;
	}
}
async function waitForAgentForeground(ptyId, expectedProcess, settings) {
	for (let attempt = 0; attempt < 30; attempt += 1) {
		if (attempt > 0) await new Promise((resolve) => globalThis.setTimeout(resolve, 150));
		try {
			const process = await inspectRuntimeTerminalProcess(settings, ptyId);
			const foreground = process.foregroundProcess?.toLowerCase() ?? "";
			if (isExpectedAgentProcess(foreground, expectedProcess)) return true;
			if (attempt >= 4 && isAgentForegroundWrapperProcess(foreground) && !isShellProcess(foreground) && process.hasChildProcesses) return true;
		} catch {}
	}
	return false;
}
function showAutomationPromptNotSentToast(agent) {
	toast.message(translate("auto.lib.launch.agent.background.session.4ca0651d56", "Your automation prompt wasn't sent — open the workspace and paste it."));
	track("agent_error", {
		error_class: "paste_readiness_timeout",
		agent_kind: tuiAgentToAgentKind(agent)
	});
}
var pendingAgentStartupDeliveries = /* @__PURE__ */ new Map();
var staleStartupRecheckTimers = /* @__PURE__ */ new Map();
var unsubscribePendingAgentStartupDeliveries = null;
function resolveAgentStartupTabId(state, worktreeId, primaryTabId) {
	return primaryTabId ?? state.activeTabIdByWorktree[worktreeId] ?? state.tabsByWorktree[worktreeId]?.[0]?.id ?? null;
}
function getAgentStartupTabPtyId(state, tabId, launchToken) {
	const livePtyIds = new Set(state.ptyIdsByTabId[tabId] ?? []);
	if (livePtyIds.size === 0) return null;
	for (const [paneKey, entry] of Object.entries(state.agentLaunchConfigByPaneKey ?? {})) {
		const identity = entry.identity;
		if (identity.tabId !== tabId || identity.launchToken !== launchToken) continue;
		const leafId = identity.leafId ?? parsePaneKey(paneKey)?.leafId;
		if (!leafId) continue;
		const ptyId = state.terminalLayoutsByTabId[tabId]?.ptyIdsByLeafId?.[leafId];
		if (ptyId && livePtyIds.has(ptyId)) return ptyId;
	}
	return null;
}
function worktreeStillOwnsStartupTab(state, worktreeId, tabId) {
	return (state.tabsByWorktree[worktreeId] ?? []).some((tab) => tab.id === tabId);
}
function getPendingStartupLaunchToken(state, tabId) {
	return state.pendingStartupByTabId?.[tabId]?.launchToken;
}
function hasRegisteredStartupLaunch(state, tabId, launchToken) {
	return Object.values(state.agentLaunchConfigByPaneKey ?? {}).some((entry) => entry.identity.tabId === tabId && entry.identity.launchToken === launchToken);
}
function ensurePendingAgentStartupSubscription() {
	if (unsubscribePendingAgentStartupDeliveries) return;
	const initial = useAppStore.getState();
	let previousTabs = initial.tabsByWorktree;
	let previousPendingStartups = initial.pendingStartupByTabId;
	let previousLaunchConfigs = initial.agentLaunchConfigByPaneKey;
	let previousPtyIds = initial.ptyIdsByTabId;
	let previousLayouts = initial.terminalLayoutsByTabId;
	unsubscribePendingAgentStartupDeliveries = useAppStore.subscribe((state) => {
		if (state.tabsByWorktree === previousTabs && state.pendingStartupByTabId === previousPendingStartups && state.agentLaunchConfigByPaneKey === previousLaunchConfigs && state.ptyIdsByTabId === previousPtyIds && state.terminalLayoutsByTabId === previousLayouts) return;
		previousTabs = state.tabsByWorktree;
		previousPendingStartups = state.pendingStartupByTabId;
		previousLaunchConfigs = state.agentLaunchConfigByPaneKey;
		previousPtyIds = state.ptyIdsByTabId;
		previousLayouts = state.terminalLayoutsByTabId;
		flushPendingAgentStartupDeliveries();
	});
}
function stopPendingAgentStartupSubscriptionIfIdle() {
	if (pendingAgentStartupDeliveries.size > 0 || !unsubscribePendingAgentStartupDeliveries) return;
	unsubscribePendingAgentStartupDeliveries();
	unsubscribePendingAgentStartupDeliveries = null;
}
function queuePendingAgentStartupDelivery(delivery) {
	const key = agentStartupDeliveryKey(delivery);
	if (isAgentStartupDeliveryConsumed(key)) return;
	pendingAgentStartupDeliveries.set(key, delivery);
	ensurePendingAgentStartupSubscription();
	flushPendingAgentStartupDeliveries();
}
function beginAgentStartupDeliveryAttempt(args) {
	const key = agentStartupDeliveryKey(args);
	if (isAgentStartupDeliveryConsumed(key)) return false;
	markAgentStartupDeliveryConsumed(key);
	pendingAgentStartupDeliveries.delete(key);
	clearStaleStartupRecheck(key);
	return true;
}
function releaseAgentStartupDeliveryAttempt(args) {
	releaseAgentStartupDeliveryConsumed(agentStartupDeliveryKey(args));
}
function flushPendingAgentStartupDeliveries() {
	const state = useAppStore.getState();
	for (const [key, delivery] of pendingAgentStartupDeliveries) {
		const { tabId, launchToken } = delivery;
		if (!worktreeStillOwnsStartupTab(state, delivery.worktreeId, tabId)) {
			pendingAgentStartupDeliveries.delete(key);
			continue;
		}
		const queuedLaunchToken = getPendingStartupLaunchToken(state, tabId);
		const launchRegistered = hasRegisteredStartupLaunch(state, tabId, launchToken);
		if (queuedLaunchToken !== launchToken && !launchRegistered && queuedLaunchToken !== void 0) {
			pendingAgentStartupDeliveries.delete(key);
			clearStaleStartupRecheck(key);
			continue;
		}
		if (queuedLaunchToken === void 0 && !launchRegistered) {
			scheduleStaleStartupRecheck(key);
			continue;
		}
		const ptyId = getAgentStartupTabPtyId(state, tabId, launchToken);
		if (!ptyId) continue;
		if (beginAgentStartupDeliveryAttempt(delivery)) delivery.deliver(tabId, ptyId, delivery.startup).catch((error) => {
			console.warn("Queued agent startup delivery failed", error);
		});
	}
	stopPendingAgentStartupSubscriptionIfIdle();
}
function scheduleStaleStartupRecheck(key) {
	if (staleStartupRecheckTimers.has(key)) return;
	staleStartupRecheckTimers.set(key, globalThis.setTimeout(() => {
		staleStartupRecheckTimers.delete(key);
		const delivery = pendingAgentStartupDeliveries.get(key);
		if (!delivery) {
			stopPendingAgentStartupSubscriptionIfIdle();
			return;
		}
		const state = useAppStore.getState();
		if (getPendingStartupLaunchToken(state, delivery.tabId) === void 0 && !hasRegisteredStartupLaunch(state, delivery.tabId, delivery.launchToken)) pendingAgentStartupDeliveries.delete(key);
		else flushPendingAgentStartupDeliveries();
		stopPendingAgentStartupSubscriptionIfIdle();
	}, 1e3));
}
function clearStaleStartupRecheck(key) {
	const timer = staleStartupRecheckTimers.get(key);
	if (!timer) return;
	globalThis.clearTimeout(timer);
	staleStartupRecheckTimers.delete(key);
}
function foldWorkspaceNameWhitespaceToHyphen(input) {
	let result = "";
	let pendingHyphen = false;
	for (let index = 0; index < input.length; index += 1) {
		if (isWorkspaceNameWhitespace(input.charCodeAt(index))) {
			pendingHyphen = true;
			continue;
		}
		if (pendingHyphen) {
			result += "-";
			pendingHyphen = false;
		}
		result += input[index];
	}
	return result;
}
function collectCompactWorkspaceWords(input, maxWords, stopWords) {
	const words = [];
	let tokenStart = -1;
	for (let index = 0; index <= input.length; index += 1) {
		const isEnd = index === input.length;
		if (!isEnd && startsWithHttpUrl(input, index)) {
			index = finishCompactWorkspaceToken(input, tokenStart, index, words, maxWords, stopWords);
			tokenStart = -1;
			while (index < input.length && !isWorkspaceNameWhitespace(input.charCodeAt(index))) index += 1;
			if (words.length >= maxWords) break;
			continue;
		}
		if (!isEnd && !isCompactWorkspaceWordSeparator(input.charCodeAt(index))) {
			if (tokenStart === -1) tokenStart = index;
			continue;
		}
		if (tokenStart !== -1) {
			finishCompactWorkspaceToken(input, tokenStart, index, words, maxWords, stopWords);
			tokenStart = -1;
			if (words.length >= maxWords) break;
		}
	}
	return words;
}
function finishCompactWorkspaceToken(input, tokenStart, tokenEnd, words, maxWords, stopWords) {
	if (tokenStart === -1 || words.length >= maxWords) return tokenEnd;
	const word = input.slice(tokenStart, tokenEnd);
	if (word && !stopWords.has(word.toLowerCase())) words.push(word);
	return tokenEnd;
}
function startsWithHttpUrl(input, index) {
	return startsWithAsciiInsensitive(input, index, "http://") || startsWithAsciiInsensitive(input, index, "https://");
}
function startsWithAsciiInsensitive(input, index, prefix) {
	if (index + prefix.length > input.length) return false;
	for (let offset = 0; offset < prefix.length; offset += 1) if (toLowerAsciiCode(input.charCodeAt(index + offset)) !== prefix.charCodeAt(offset)) return false;
	return true;
}
function toLowerAsciiCode(code) {
	return code >= 65 && code <= 90 ? code + 32 : code;
}
function isCompactWorkspaceWordSeparator(code) {
	return isWorkspaceNameWhitespace(code) || code === 34 || code === 35 || code === 40 || code === 41 || code === 47 || code === 58 || code === 91 || code === 92 || code === 93 || code === 95 || code === 123 || code === 125 || code === 45;
}
function isWorkspaceNameWhitespace(code) {
	return code === 32 || code >= 9 && code <= 13 || code === 160 || code === 5760 || code >= 8192 && code <= 8202 || code === 8232 || code === 8233 || code === 8239 || code === 8287 || code === 12288 || code === 65279;
}
function normalizeApostrophes(input) {
	return input.replace(/[‘’]/g, "'");
}
function removeIntraWordApostrophes(input) {
	return normalizeApostrophes(input).replace(/([\p{L}\p{N}])'(?=[\p{L}\p{N}])/gu, "$1");
}
function stripDanglingDisplayApostrophes(input) {
	return normalizeApostrophes(input).replace(/(^|[^\p{L}\p{N}])'(?=[\p{L}\p{N}])/gu, "$1").replace(/([\p{L}\p{N}])'(?=$|[^\p{L}\p{N}])/gu, "$1");
}
function slugifyForWorkspaceName(input) {
	return foldWorkspaceNameWhitespaceToHyphen(removeIntraWordApostrophes(input).trim().toLowerCase().replace(/[\\/]+/g, "-")).replace(/[^a-z0-9._-]+/g, "-").replace(/-+/g, "-").replace(/\.{2,}/g, ".").replace(/^[.-]+|[.-]+$/g, "").slice(0, 48).replace(/[-._]+$/g, "");
}
function getLinkedWorkItemSuggestedName(item) {
	return slugifyForWorkspaceName(getLinkedWorkItemTitleSubject(item) || item.title.trim());
}
function getLinkedWorkItemTitleSubject(item) {
	return item.title.trim().replace(/^(?:issue|pr|pull request|mr|merge request)\s*[#!]?\d+\s*[:-]\s*/i, "").replace(/^#\d+\s*[:-]\s*/, "").replace(/\([#!]?\d+\)/g, "").replace(/\b#\d+\b/g, "").trim();
}
var ACTION_LABELS = [
	[/(?:^|[^a-z0-9_-])(?:fix(?:e[sd])?|resolve|repair)(?:$|[^a-z0-9_-])/i, "Fix"],
	[/(?:^|[^a-z0-9_-])(?:debug|diagnose)(?:$|[^a-z0-9_-])/i, "Debug"],
	[/(?:^|[^a-z0-9_-])(?:review|look\s+over|inspect|check|safe|safety)(?:$|[^a-z0-9_-])/i, "Review"],
	[/(?:^|[^a-z0-9_-])(?:implement|build|ship)(?:$|[^a-z0-9_-])/i, "Implement"],
	[/(?:^|[^a-z0-9_-])(?:investigate|understand|triage)(?:$|[^a-z0-9_-])/i, "Investigate"],
	[/(?:^|[^a-z0-9_-])(?:add|create)(?:$|[^a-z0-9_-])/i, "Add"],
	[/(?:^|[^a-z0-9_-])(?:update|change)(?:$|[^a-z0-9_-])/i, "Update"],
	[/(?:^|[^a-z0-9_-])(?:refactor|simplify)(?:$|[^a-z0-9_-])/i, "Refactor"],
	[/(?:^|[^a-z0-9_-])(?:test|verify|validate)(?:$|[^a-z0-9_-])/i, "Test"]
];
var STOP_WORDS = new Set([
	"a",
	"an",
	"and",
	"for",
	"from",
	"in",
	"is",
	"it",
	"of",
	"on",
	"or",
	"the",
	"this",
	"to",
	"with"
]);
function detectIntentAction(sourceText) {
	for (const [pattern, label] of ACTION_LABELS) if (pattern.test(sourceText)) return label;
	return null;
}
function titleCaseWord(word) {
	const normalized = normalizeApostrophes(word);
	if (/^[A-Z]{2,}\d*$/.test(normalized) || /^[A-Z]+-\d+$/i.test(normalized)) return normalized.toUpperCase();
	const acronymPossessive = normalized.match(/^([A-Z]{2,}\d*)'([sS])$/);
	if (acronymPossessive) return `${acronymPossessive[1].toUpperCase()}'s`;
	const lower = normalized.toLowerCase();
	const apostropheParts = lower.split("'");
	if (apostropheParts.length === 2 && apostropheParts[0].length === 1 && apostropheParts[1]) return `${apostropheParts[0].toUpperCase()}'${apostropheParts[1]}`;
	return lower.charAt(0).toUpperCase() + lower.slice(1);
}
function compactWords(input, maxWords = 4) {
	return collectCompactWorkspaceWords(stripDanglingDisplayApostrophes(input), maxWords, STOP_WORDS).map(titleCaseWord).join(" ");
}
function escapeRegExp(input) {
	return input.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
function compactWorkItemTitle(title, item) {
	const identifier = item.linearIdentifier ?? item.jiraIdentifier;
	let withoutPrefix = title.trim().replace(/^(?:issue|pr|pull request|mr|merge request)\s*[#!]?\d+\s*[:-]\s*/i, "").replace(/\([#!]?\d+\)/g, "").replace(/^[^:]{1,32}:\s*/, "").trim();
	if (item.number > 0) withoutPrefix = withoutPrefix.replace(new RegExp(`\\b[#!]?${item.number}\\b`, "g"), "").trim();
	if (identifier) withoutPrefix = withoutPrefix.replace(new RegExp(`^${escapeRegExp(identifier)}\\s*[:-]?\\s*`, "i"), "").trim();
	return compactWords(withoutPrefix || title, 3);
}
function workItemIdentity(item) {
	if (item.linearIdentifier) return item.linearIdentifier.toUpperCase();
	if (item.jiraIdentifier) return item.jiraIdentifier.toUpperCase();
	if (item.type === "pr") return `PR ${item.number}`;
	if (item.type === "mr") return `MR ${item.number}`;
	return `Issue ${item.number}`;
}
function getLinkedWorkItemWorkspaceName(item) {
	const identifier = item.linearIdentifier ?? item.jiraIdentifier;
	let subject = getLinkedWorkItemTitleSubject(item) || item.title.trim();
	if (identifier) subject = subject.replace(new RegExp(`^${escapeRegExp(identifier)}\\s*[:-]?\\s*`, "i"), "").trim();
	const displayName = [identifier, subject].filter(Boolean).join(" ") || workItemIdentity(item);
	const seedName = slugifyForWorkspaceName(displayName);
	if (!seedName) return null;
	return {
		displayName,
		seedName
	};
}
function defaultActionForWorkItem(item) {
	return item.type === "pr" || item.type === "mr" ? "Review" : null;
}
function getWorkspaceIntentName(args) {
	const sourceText = args.sourceText?.trim() ?? "";
	const item = args.workItem ?? null;
	let displayName = "";
	if (item) {
		const action = detectIntentAction(sourceText) ?? defaultActionForWorkItem(item);
		const identity = workItemIdentity(item);
		if (action) displayName = `${action} ${identity}`;
		else displayName = [identity, compactWorkItemTitle(item.title, item)].filter(Boolean).join(" ");
	} else if (sourceText) displayName = compactWords(sourceText, 5);
	if (!displayName && args.fallbackName?.trim()) displayName = args.fallbackName.trim();
	if (!displayName) return null;
	const seedName = slugifyForWorkspaceName(displayName);
	if (!seedName) return null;
	return {
		displayName,
		seedName
	};
}
function getLinearIssueWorkspaceName(issue) {
	const key = slugifyForWorkspaceName(issue.identifier);
	const titleSlug = getLinkedWorkItemSuggestedName(issue);
	if (!key) return titleSlug;
	let dedupedTitleSlug = titleSlug;
	if (titleSlug === key) dedupedTitleSlug = "";
	else if (titleSlug.startsWith(`${key}-`)) dedupedTitleSlug = titleSlug.slice(key.length + 1);
	return slugifyForWorkspaceName([key, dedupedTitleSlug].filter(Boolean).join("-"));
}
var GL_ITEM_PATH_FULL_RE = /^\/(.+)\/-\/(issues|work_items|merge_requests)\/(\d+)(?:\/.*)?$/i;
function parseGitLabIssueOrMRLink(input) {
	const trimmed = input.trim();
	if (!trimmed) return null;
	let url;
	try {
		url = new URL(trimmed);
	} catch {
		return null;
	}
	const match = GL_ITEM_PATH_FULL_RE.exec(url.pathname);
	if (!match) return null;
	const path = match[1];
	if (!path.includes("/")) return null;
	return {
		slug: {
			host: url.host,
			path
		},
		type: match[2].toLowerCase() === "merge_requests" ? "mr" : "issue",
		number: Number.parseInt(match[3], 10)
	};
}
const JIRA_ISSUE_KEY_PATTERN = /^[A-Za-z][A-Za-z0-9_]*-\d+$/;
function parseJiraIssueUrl(value) {
	let url;
	try {
		url = new URL(value.trim());
	} catch {
		return null;
	}
	if (url.protocol !== "http:" && url.protocol !== "https:" || url.username.length > 0 || url.password.length > 0) return null;
	const match = url.pathname.match(/^(.*)\/browse\/([^/]+)$/);
	if (!match || !JIRA_ISSUE_KEY_PATTERN.test(match[2])) return null;
	return {
		issueKey: match[2].toUpperCase(),
		origin: url.origin.toLowerCase(),
		sitePath: normalizeSitePath(match[1])
	};
}
function getMatchingJiraSites(parsed, sites) {
	return sites.filter((site) => {
		const identity = getJiraSiteIdentity(site.siteUrl);
		return identity !== null && identity.origin === parsed.origin && identity.sitePath === parsed.sitePath;
	});
}
function isResolvedJiraIssueMatch(parsed, site, issue) {
	const canonical = parseJiraIssueUrl(issue.url);
	return issue.key.toUpperCase() === parsed.issueKey && issue.siteId === site.id && canonical !== null && canonical.issueKey === parsed.issueKey && getMatchingJiraSites(canonical, [site]).length === 1;
}
function getJiraSiteIdentity(value) {
	let url;
	try {
		url = new URL(value.trim());
	} catch {
		return null;
	}
	if (url.protocol !== "http:" && url.protocol !== "https:" || url.username.length > 0 || url.password.length > 0 || url.search.length > 0 || url.hash.length > 0) return null;
	return {
		origin: url.origin.toLowerCase(),
		sitePath: normalizeSitePath(url.pathname)
	};
}
function normalizeSitePath(pathname) {
	const trimmed = pathname.replace(/\/+$/g, "");
	return trimmed === "/" ? "" : trimmed;
}
var LINEAR_ISSUE_URL_RE = /^https?:\/\/(?:www\.)?linear\.app\/[^/\s]+\/issue\/[^/\s]+(?:\/\S*)?$/i;
var GITHUB_ITEM_URL_IN_TEXT_RE = /https?:\/\/[^\s/]+\/[^\s/]+\/[^\s/]+\/(?:issues|pull)\/\d+[^\s]*/i;
var TRAILING_URL_PUNCTUATION_RE = /[),.;:!?]+$/;
function hasGitHubLookup(value) {
	if (parseGitHubIssueOrPRNumber(value) !== null || parseGitHubIssueOrPRLink(value) !== null) return true;
	const embedded = GITHUB_ITEM_URL_IN_TEXT_RE.exec(value)?.[0];
	return embedded ? parseGitHubIssueOrPRLink(embedded.replace(TRAILING_URL_PUNCTUATION_RE, "")) !== null : false;
}
function isWorkItemLookupText(value) {
	const trimmed = value.trim();
	if (!trimmed) return false;
	return hasGitHubLookup(trimmed) || parseGitLabIssueOrMRLink(trimmed) !== null || parseJiraIssueUrl(trimmed) !== null || LINEAR_ISSUE_URL_RE.test(trimmed);
}
var GITLAB_ISSUE_PATH_RE = /\/-\/(?:issues|work_items)\//i;
function isGitLabIssueUrl(url) {
	try {
		return GITLAB_ISSUE_PATH_RE.test(new URL(url).pathname);
	} catch {
		return GITLAB_ISSUE_PATH_RE.test(url);
	}
}
function isJiraIssueUrl(url) {
	try {
		const parsed = new URL(url);
		return /\.atlassian\.net$/i.test(parsed.hostname) || /\/browse\/[A-Z][A-Z0-9]+-\d+/i.test(parsed.pathname);
	} catch {
		return false;
	}
}
function getWorkspaceSourceProvider(item) {
	if (item.provider) return item.provider;
	if (item.linearIdentifier) return "linear";
	if (item.jiraIdentifier || isJiraIssueUrl(item.url)) return "jira";
	if (item.type === "mr" || isGitLabIssueUrl(item.url)) return "gitlab";
	if (item.number === 0 && !item.url.includes("github.com")) return "linear";
	return "github";
}
function buildGitHubWorkspaceSource(item) {
	return {
		provider: "github",
		...item
	};
}
function buildGitLabWorkspaceSource(item) {
	return {
		provider: "gitlab",
		...item
	};
}
function getUsableLinearBranchName(branchName) {
	return branchName?.trim() || void 0;
}
function buildLinearWorkspaceSource(issue) {
	const organizationUrlKey = getLinearOrganizationUrlKeyFromIssueUrl(issue.url);
	const branchName = getUsableLinearBranchName(issue.branchName);
	return {
		provider: "linear",
		type: "issue",
		number: 0,
		title: issue.title,
		url: issue.url,
		linearIdentifier: issue.identifier,
		...issue.workspaceId ? { linearWorkspaceId: issue.workspaceId } : {},
		...organizationUrlKey ? { linearOrganizationUrlKey: organizationUrlKey } : {},
		...branchName ? { linearBranchName: branchName } : {}
	};
}
function buildJiraWorkspaceSource(issue) {
	return {
		provider: "jira",
		type: "issue",
		number: 0,
		title: issue.title,
		url: issue.url,
		jiraIdentifier: issue.key
	};
}
function shouldApplyWorkspaceSourceAutoName(args) {
	return !args.currentName.trim() || args.currentName === args.lastAutoName || isWorkItemLookupText(args.currentName);
}
function toWorkspaceIntentItem(item) {
	return {
		...item,
		provider: getWorkspaceSourceProvider(item)
	};
}
function getWorkspaceSourceName(item) {
	const normalized = toWorkspaceIntentItem(item);
	const resolved = getLinkedWorkItemWorkspaceName(normalized);
	return {
		seedName: resolved?.seedName ?? getLinkedWorkItemSuggestedName(normalized),
		displayName: resolved?.displayName ?? item.title.trim()
	};
}
function buildWorkspaceSourceSelection(args) {
	const { linkedWorkItem, baseBranch } = args;
	if (!linkedWorkItem) return baseBranch ? {
		kind: "branch",
		label: baseBranch
	} : null;
	const provider = getWorkspaceSourceProvider(linkedWorkItem);
	return {
		kind: provider === "linear" ? "linear" : provider === "jira" ? "jira" : provider === "gitlab" ? linkedWorkItem.type === "mr" ? "gitlab-mr" : "gitlab-issue" : linkedWorkItem.type === "pr" ? "github-pr" : "github-issue",
		label: provider === "linear" || provider === "jira" || linkedWorkItem.number === 0 ? linkedWorkItem.title : `#${linkedWorkItem.number} ${linkedWorkItem.title}`,
		url: linkedWorkItem.url
	};
}
function shouldPreserveWorkspaceSourceOnRepoChange(item) {
	if (!item) return false;
	const provider = getWorkspaceSourceProvider(item);
	return provider === "linear" || provider === "jira";
}
const CLIENT_PLATFORM = navigator.userAgent.includes("Windows") ? "win32" : navigator.userAgent.includes("Mac") ? "darwin" : "linux";
function canUseIssueCommandForLinkedItemProvider(provider) {
	return provider === "github" || provider === "gitlab";
}
const DEFAULT_ISSUE_COMMAND_TEMPLATE = "Complete {{artifact_url}}";
function getDefaultTabCommandPreview(yamlHooks) {
	return (yamlHooks?.defaultTabs ?? []).map((tab, index) => {
		const command = tab.command?.trim();
		if (!command) return null;
		const label = tab.title ? ` ${tab.title}` : "";
		return `# defaultTabs[${index + 1}]${label}\n${command}`;
	}).filter((entry) => entry !== null).join("\n\n");
}
function getSetupConfigKind(hasSetup, hasDefaultTabCommands) {
	if (hasSetup && hasDefaultTabCommands) return "setup-and-default-tabs";
	if (hasDefaultTabCommands) return "default-tabs";
	return "setup";
}
function renderIssueCommandTemplate(template, vars) {
	const { issueNumber, artifactUrl } = vars;
	let rendered = template;
	if (artifactUrl !== null) rendered = rendered.replace(/\{\{artifact_url\}\}/g, artifactUrl);
	if (issueNumber !== null) rendered = rendered.replace(/\{\{issue\}\}/g, String(issueNumber));
	return rendered;
}
function buildAgentPromptWithContext(prompt, attachments, linkedUrls, linkedContextBlocks = []) {
	const trimmedPrompt = prompt.trim();
	if (attachments.length === 0 && linkedUrls.length === 0 && linkedContextBlocks.length === 0) return trimmedPrompt;
	const sections = [];
	if (attachments.length > 0) {
		const attachmentBlock = attachments.map((pathValue) => `- ${pathValue}`).join("\n");
		sections.push(`Attachments:\n${attachmentBlock}`);
	}
	if (linkedUrls.length > 0) {
		const linkBlock = linkedUrls.map((url) => `- ${url}`).join("\n");
		sections.push(`Linked work items:\n${linkBlock}`);
	}
	if (linkedContextBlocks.length > 0) sections.push(linkedContextBlocks.join("\n\n"));
	if (!trimmedPrompt) return sections.join("\n\n");
	return `${trimmedPrompt}\n\n${sections.join("\n\n")}`;
}
function getAttachmentLabel(pathValue) {
	return pathValue.split(/[/\\]/).at(-1) || pathValue;
}
function getSetupConfig(repo, yamlHooks) {
	const yamlSetup = yamlHooks?.scripts?.setup?.trim();
	const yamlDefaultTabCommands = getDefaultTabCommandPreview(yamlHooks);
	const localSetup = repo?.hookSettings?.scripts?.setup?.trim();
	const sourcePolicy = resolveHookCommandSourcePolicy(repo?.hookSettings?.commandSourcePolicy, { hasLocalScript: Boolean(localSetup) });
	if (sourcePolicy === "local-only") return localSetup ? {
		source: "local",
		command: localSetup,
		kind: "setup"
	} : null;
	const yamlCommand = [yamlSetup, yamlDefaultTabCommands].filter(Boolean).join("\n\n");
	if (sourcePolicy === "run-both" && yamlCommand && localSetup) return {
		source: "both",
		command: `${yamlCommand}\n\n${localSetup}`,
		kind: getSetupConfigKind(true, Boolean(yamlDefaultTabCommands))
	};
	if (yamlCommand) return {
		source: "yaml",
		command: yamlCommand,
		kind: getSetupConfigKind(Boolean(yamlSetup), Boolean(yamlDefaultTabCommands))
	};
	return null;
}
function getWorkspaceSeedName(args) {
	const { explicitName, prompt, linkedIssueNumber, linkedPR, fallbackName } = args;
	if (explicitName.trim()) return explicitName.trim();
	if (linkedPR !== null) return `pr-${linkedPR}`;
	if (linkedIssueNumber !== null) return `issue-${linkedIssueNumber}`;
	if (prompt.trim()) {
		const slug = slugifyForWorkspaceName(prompt);
		if (slug) return slug;
	}
	if (fallbackName && fallbackName.trim()) return fallbackName.trim();
	return "workspace";
}
async function ensureAgentStartupInTerminal(args) {
	const { worktreeId, primaryTabId, startup } = args;
	const draftPrompt = startup.draftPrompt ?? null;
	if (startup.followupPrompt === null && draftPrompt === null) return;
	const launchToken = ensureStartupLaunchToken(startup);
	let tabId = null;
	let ptyId = null;
	for (let attempt = 0; attempt < 30; attempt += 1) {
		if (attempt > 0) await new Promise((resolve) => globalThis.setTimeout(resolve, 150));
		const state = useAppStore.getState();
		tabId = resolveAgentStartupTabId(state, worktreeId, primaryTabId);
		if (!tabId) continue;
		ptyId = getAgentStartupTabPtyId(state, tabId, launchToken);
		if (ptyId) break;
	}
	if (!tabId || !ptyId) {
		if (tabId) queuePendingAgentStartupDelivery({
			worktreeId,
			tabId,
			launchToken,
			startup,
			deliver: deliverAgentStartupToTerminal
		});
		return;
	}
	if (beginAgentStartupDeliveryAttempt({
		worktreeId,
		tabId,
		launchToken
	})) await deliverAgentStartupToTerminal(tabId, ptyId, startup);
}
async function deliverAgentStartupToTerminal(tabId, ptyId, startup) {
	const draftPrompt = startup.draftPrompt ?? null;
	const runtimeSettings = getSettingsForAgentTabRuntimeOwner(tabId);
	if (startup.followupPrompt) {
		if (!await sendFollowupPromptWhenAgentReady({
			ptyId,
			expectedProcess: startup.expectedProcess,
			prompt: startup.followupPrompt,
			settings: runtimeSettings
		})) showAutomationPromptNotSentToast(startup.agent);
	}
	if (draftPrompt) await pasteDraftToAgentPtyWhenReady({
		tabId,
		ptyId,
		content: draftPrompt,
		agent: startup.agent,
		forcePaste: true,
		onTimeout: () => showAutomationPromptNotSentToast(startup.agent)
	});
}
function ensureStartupLaunchToken(startup) {
	if (!startup.launchToken) startup.launchToken = createBrowserUuid();
	return startup.launchToken;
}
function createNativeChatSessionOptionRecord(agent) {
	return {
		agent,
		valuesByModel: {}
	};
}
function cloneNativeChatSessionOptionRecord(record) {
	return {
		agent: record.agent,
		...record.model ? { model: { ...record.model } } : {},
		valuesByModel: Object.fromEntries(Object.entries(record.valuesByModel).map(([modelId, values]) => [modelId, Object.fromEntries(Object.entries(values).map(([id, tracked]) => [id, { ...tracked }]))]))
	};
}
function isFlipOnlyMidSession(midSession) {
	return midSession?.kind === "toggle-command";
}
function getTrackedSessionOption(record, modelId, optionId) {
	return modelId ? record.valuesByModel[modelId]?.[optionId] : void 0;
}
function clearTrackedSessionOption(record, modelId, optionId) {
	if (!modelId) return;
	const current = record.valuesByModel[modelId];
	if (!current || !(optionId in current)) return;
	const next = { ...current };
	delete next[optionId];
	if (Object.keys(next).length === 0) delete record.valuesByModel[modelId];
	else record.valuesByModel[modelId] = next;
}
function clearNativeChatSessionModel(record) {
	const modelId = typeof record.model?.value === "string" ? record.model.value : null;
	record.model = void 0;
	if (modelId) delete record.valuesByModel[modelId];
}
function setTrackedSessionOption(record, optionId, value, source, fallbackModelId = null) {
	if (optionId === "model") {
		record.model = {
			value,
			source
		};
		return typeof value === "string" ? value : null;
	}
	const modelId = (typeof record.model?.value === "string" ? record.model.value : null) ?? fallbackModelId;
	if (!modelId) return null;
	record.valuesByModel[modelId] = {
		...record.valuesByModel[modelId],
		[optionId]: {
			value,
			source
		}
	};
	return modelId;
}
function flattenNativeChatSessionOptionRecord(record, modelId) {
	return {
		model: modelId,
		...Object.fromEntries(Object.entries(record.valuesByModel[modelId] ?? {}).map(([id, tracked]) => [id, tracked.value]))
	};
}
function applyNativeChatReportedSessionOptions(record, values) {
	const modelId = typeof values.model === "string" ? values.model : null;
	if (!modelId) return false;
	const modelChanged = record.model?.value !== modelId;
	let changed = modelChanged || record.model?.source !== "reported";
	record.model = {
		value: modelId,
		source: "reported"
	};
	const modelValues = modelChanged ? {} : { ...record.valuesByModel[modelId] };
	for (const [id, value] of Object.entries(values)) {
		if (id === "model") continue;
		const current = modelValues[id];
		if (current?.value !== value || current.source !== "reported") changed = true;
		modelValues[id] = {
			value,
			source: "reported"
		};
	}
	record.valuesByModel[modelId] = modelValues;
	return changed;
}
function matchNativeChatCatalogModelId(catalog, reported) {
	const normalized = reported.trim().toLowerCase();
	if (!normalized) return null;
	const exact = catalog.models.find((model) => model.id.toLowerCase() === normalized);
	if (exact) return exact.id;
	const byLabel = catalog.models.find((model) => model.label.toLowerCase() === normalized);
	if (byLabel) return byLabel.id;
	return [...catalog.models].sort((left, right) => right.id.length - left.id.length).find((model) => normalized.includes(model.id.toLowerCase()))?.id ?? null;
}
function setBoundedScopeCacheEntry(cache, scopeKey, value) {
	cache.delete(scopeKey);
	cache.set(scopeKey, value);
	while (cache.size > 128) {
		const oldest = cache.keys().next().value;
		if (oldest === void 0) break;
		cache.delete(oldest);
	}
}
var sessionOptionCache = /* @__PURE__ */ new Map();
function readNativeChatSessionOptionCache(scopeKey, fallbackScopeKey) {
	const record = sessionOptionCache.get(scopeKey) ?? sessionOptionCache.get(fallbackScopeKey ?? "");
	return record ? cloneNativeChatSessionOptionRecord(record) : null;
}
function writeNativeChatSessionOptionCache(scopeKey, record) {
	setBoundedScopeCacheEntry(sessionOptionCache, scopeKey, cloneNativeChatSessionOptionRecord(record));
}
function seedNativeChatAppliedSessionOptions(scopeKey, agent, values) {
	const modelId = typeof values?.model === "string" ? values.model : null;
	if (!modelId) return;
	const record = createNativeChatSessionOptionRecord(agent);
	record.model = {
		value: modelId,
		source: "applied"
	};
	const modelValues = {};
	for (const [id, value] of Object.entries(values ?? {})) if (id !== "model") modelValues[id] = {
		value,
		source: "applied"
	};
	record.valuesByModel[modelId] = modelValues;
	writeNativeChatSessionOptionCache(scopeKey, record);
}
export { getWorkspaceSourceProvider as A, getLinearIssueWorkspaceName as B, buildGitHubWorkspaceSource as C, buildWorkspaceSourceSelection as D, buildLinearWorkspaceSource as E, JIRA_ISSUE_KEY_PATTERN as F, beginAgentStartupDeliveryAttempt as G, getLinkedWorkItemWorkspaceName as H, getMatchingJiraSites as I, releaseAgentStartupDeliveryAttempt as K, isResolvedJiraIssueMatch as L, shouldApplyWorkspaceSourceAutoName as M, shouldPreserveWorkspaceSourceOnRepoChange as N, getUsableLinearBranchName as O, isWorkItemLookupText as P, parseJiraIssueUrl as R, renderIssueCommandTemplate as S, buildJiraWorkspaceSource as T, getWorkspaceIntentName as U, getLinkedWorkItemSuggestedName as V, slugifyForWorkspaceName as W, canUseIssueCommandForLinkedItemProvider as _, applyNativeChatReportedSessionOptions as a, getSetupConfig as b, createNativeChatSessionOptionRecord as c, isFlipOnlyMidSession as d, matchNativeChatCatalogModelId as f, buildAgentPromptWithContext as g, DEFAULT_ISSUE_COMMAND_TEMPLATE as h, setBoundedScopeCacheEntry as i, isGitLabIssueUrl as j, getWorkspaceSourceName as k, flattenNativeChatSessionOptionRecord as l, CLIENT_PLATFORM as m, seedNativeChatAppliedSessionOptions as n, clearNativeChatSessionModel as o, setTrackedSessionOption as p, showAutomationPromptNotSentToast as q, writeNativeChatSessionOptionCache as r, clearTrackedSessionOption as s, readNativeChatSessionOptionCache as t, getTrackedSessionOption as u, ensureAgentStartupInTerminal as v, buildGitLabWorkspaceSource as w, getWorkspaceSeedName as x, getAttachmentLabel as y, parseGitLabIssueOrMRLink as z };
