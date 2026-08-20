import { $a as isClipboardTextByteLengthOverLimit } from "./store-CgXrfmaH.js";
function buildLinearTeamUrl(args) {
	const organizationUrlKey = args.organizationUrlKey?.trim();
	const teamKey = args.teamKey?.trim();
	if (!organizationUrlKey || !teamKey) return null;
	return `https://linear.app/${encodeURIComponent(organizationUrlKey)}/team/${encodeURIComponent(teamKey)}/all`;
}
function buildLinearPersonalApiKeySettingsUrl(organizationUrlKey) {
	const trimmed = organizationUrlKey?.trim();
	return trimmed ? `https://linear.app/${encodeURIComponent(trimmed)}/settings/account/security` : "https://linear.app/settings/account/security";
}
function buildLinearWorkspaceApiSettingsUrl(organizationUrlKey) {
	const trimmed = organizationUrlKey?.trim();
	return trimmed ? `https://linear.app/${encodeURIComponent(trimmed)}/settings/api` : "https://linear.app/settings/api";
}
function buildLinearIssueUrl(args) {
	const identifier = args.identifier?.trim();
	const organizationUrlKey = args.organizationUrlKey?.trim();
	if (!identifier || !organizationUrlKey) return null;
	return `https://linear.app/${encodeURIComponent(organizationUrlKey)}/issue/${encodeURIComponent(identifier)}`;
}
function getLinearOrganizationUrlKeyFromIssueUrl(issueUrl) {
	if (!issueUrl) return null;
	try {
		const parsed = new URL(issueUrl);
		if (parsed.hostname !== "linear.app") return null;
		return parsed.pathname.split("/").find(Boolean) ?? null;
	} catch {
		return null;
	}
}
var LINEAR_IDENTIFIER_PATTERN = /^[A-Za-z][A-Za-z0-9_]*-\d+$/;
function parseLinearIssueInput(input) {
	const trimmed = input.trim();
	if (!trimmed) return null;
	if (LINEAR_IDENTIFIER_PATTERN.test(trimmed)) return { identifier: trimmed.toUpperCase() };
	try {
		const parsed = new URL(trimmed);
		if (parsed.protocol !== "https:" && parsed.protocol !== "http:") return null;
		if (parsed.hostname !== "linear.app") return null;
		const parts = parsed.pathname.split("/").filter(Boolean);
		const issueIndex = parts.indexOf("issue");
		const organizationUrlKey = parts[0];
		const rawIdentifier = issueIndex !== -1 ? parts[issueIndex + 1] : void 0;
		if (!organizationUrlKey || !rawIdentifier) return null;
		const identifier = decodeURIComponent(rawIdentifier).split(/[/?#]/)[0];
		if (!LINEAR_IDENTIFIER_PATTERN.test(identifier)) return null;
		return {
			identifier: identifier.toUpperCase(),
			organizationUrlKey: decodeURIComponent(organizationUrlKey)
		};
	} catch {
		return null;
	}
}
function parseLinearIssueUrlIntent(input) {
	const trimmed = input.trim();
	try {
		const url = new URL(trimmed);
		const pathMatch = /^\/([^/]+)\/issue\/([^/]+)(?:\/[^/]+)?\/?$/.exec(url.pathname);
		if (url.protocol !== "https:" && url.protocol !== "http:" || url.host !== "linear.app" || url.username !== "" || url.password !== "" || !pathMatch) return null;
		const organizationUrlKey = decodeURIComponent(pathMatch[1]);
		const identifier = decodeURIComponent(pathMatch[2]);
		if (!/^[A-Za-z0-9][A-Za-z0-9_-]*$/.test(organizationUrlKey) || !LINEAR_IDENTIFIER_PATTERN.test(identifier)) return null;
		return {
			identifier: identifier.toUpperCase(),
			organizationUrlKey
		};
	} catch {
		return null;
	}
}
function findLinearIssueWorkspaceId(intent, workspaces) {
	const organizationUrlKey = intent.organizationUrlKey.toLowerCase();
	return workspaces?.find((workspace) => workspace.organizationUrlKey?.toLowerCase() === organizationUrlKey)?.id ?? null;
}
function findLinearIssueWorkspaceIdFromStatus(intent, status) {
	const workspaceId = findLinearIssueWorkspaceId(intent, status.workspaces);
	if (workspaceId) return workspaceId;
	if (status.viewer?.organizationUrlKey?.toLowerCase() !== intent.organizationUrlKey.toLowerCase()) return null;
	return (status.selectedWorkspaceId && status.selectedWorkspaceId !== "all" ? status.selectedWorkspaceId : null) ?? status.activeWorkspaceId ?? null;
}
function findLinearIssueWorkspaceLookupIds(intent, status) {
	const ids = [];
	const seen = /* @__PURE__ */ new Set();
	const push = (id) => {
		if (!id || id === "all" || seen.has(id)) return;
		seen.add(id);
		ids.push(id);
	};
	push(findLinearIssueWorkspaceIdFromStatus(intent, status));
	for (const workspace of status.workspaces ?? []) if (!workspace.organizationUrlKey) push(workspace.id);
	if (!status.viewer?.organizationUrlKey && (status.workspaces?.length ?? 0) === 0) {
		push(status.selectedWorkspaceId && status.selectedWorkspaceId !== "all" ? status.selectedWorkspaceId : null);
		push(status.activeWorkspaceId);
	}
	return ids;
}
function isLinearIssueUrlResolutionMatch(intent, issue) {
	if (issue.identifier.toUpperCase() !== intent.identifier.toUpperCase()) return false;
	const issueOrganizationUrlKey = getLinearOrganizationUrlKeyFromIssueUrl(issue.url);
	return issueOrganizationUrlKey !== null && issueOrganizationUrlKey.toLowerCase() === intent.organizationUrlKey.toLowerCase();
}
const LINEAR_ISSUE_LINK_CLEARED = {
	linkedLinearIssue: null,
	linkedLinearIssueWorkspaceId: null,
	linkedLinearIssueOrganizationUrlKey: null
};
function buildLinearIssueLinkUpdates(input) {
	if (input.trim() === "") return { ...LINEAR_ISSUE_LINK_CLEARED };
	const parsed = parseLinearIssueInput(input);
	if (!parsed) return null;
	return {
		linkedLinearIssue: parsed.identifier,
		linkedLinearIssueWorkspaceId: null,
		linkedLinearIssueOrganizationUrlKey: parsed.organizationUrlKey ?? null
	};
}
var GH_ITEM_PATH_RE = /^\/([^/]+)\/([^/]+)\/(issues|pull)\/(\d+)(?:\/.*)?$/i;
function buildGitHubRepoUrl(slug) {
	if (!slug?.owner || !slug.repo) return null;
	return `https://${slug.host ?? "github.com"}/${encodeURIComponent(slug.owner)}/${encodeURIComponent(slug.repo)}`;
}
function matchGitHubItemPath(url) {
	return GH_ITEM_PATH_RE.exec(url.pathname.replace(/\/+$/, ""));
}
function parseGitHubItemNumber(value) {
	const parsed = Number.parseInt(value, 10);
	return parsed > 0 ? parsed : null;
}
function parseGitHubIssueOrPRNumber(input) {
	const trimmed = input.trim();
	if (!trimmed) return null;
	const numeric = trimmed.startsWith("#") ? trimmed.slice(1) : trimmed;
	if (/^\d+$/.test(numeric)) return parseGitHubItemNumber(numeric);
	let url;
	try {
		url = new URL(trimmed);
	} catch {
		return null;
	}
	if (url.protocol !== "https:" && url.protocol !== "http:") return null;
	const match = matchGitHubItemPath(url);
	if (!match) return null;
	return parseGitHubItemNumber(match[4]);
}
function parseGitHubIssueOrPRLink(input) {
	const trimmed = input.trim();
	if (!trimmed) return null;
	let url;
	try {
		url = new URL(trimmed);
	} catch {
		return null;
	}
	if (url.protocol !== "https:" && url.protocol !== "http:") return null;
	const match = matchGitHubItemPath(url);
	if (!match) return null;
	const number = parseGitHubItemNumber(match[4]);
	if (number === null) return null;
	return {
		slug: {
			owner: match[1],
			repo: match[2],
			host: url.host
		},
		type: match[3].toLowerCase() === "pull" ? "pr" : "issue",
		number
	};
}
const WORK_ITEM_LINK_QUERY_MAX_BYTES = 2 * 1024;
function isWorkItemLinkQueryTooLarge(query, maxBytes = WORK_ITEM_LINK_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
var HTTP_URL_PREFIX_RE = /^https?:\/\//i;
function normalizeGitHubLinkQuery(raw) {
	if (isWorkItemLinkQueryTooLarge(raw)) return {
		query: "",
		directNumber: null,
		tooLarge: true
	};
	const trimmed = raw.trim();
	if (!trimmed) return {
		query: "",
		directNumber: null
	};
	const direct = parseGitHubIssueOrPRNumber(trimmed);
	if (direct !== null && !HTTP_URL_PREFIX_RE.test(trimmed)) return {
		query: trimmed,
		directNumber: direct
	};
	const link = parseGitHubIssueOrPRLink(trimmed);
	if (!link) return {
		query: trimmed,
		directNumber: null
	};
	return {
		query: trimmed,
		directNumber: link.number,
		directLink: link
	};
}
export { parseGitHubIssueOrPRNumber as a, buildLinearIssueUrl as c, buildLinearWorkspaceApiSettingsUrl as d, findLinearIssueWorkspaceLookupIds as f, parseLinearIssueUrlIntent as g, parseLinearIssueInput as h, parseGitHubIssueOrPRLink as i, buildLinearPersonalApiKeySettingsUrl as l, isLinearIssueUrlResolutionMatch as m, isWorkItemLinkQueryTooLarge as n, LINEAR_ISSUE_LINK_CLEARED as o, getLinearOrganizationUrlKeyFromIssueUrl as p, buildGitHubRepoUrl as r, buildLinearIssueLinkUpdates as s, normalizeGitHubLinkQuery as t, buildLinearTeamUrl as u };
