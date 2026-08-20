import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon } from "./button-DszXJEV6.js";
import { $a as isClipboardTextByteLengthOverLimit, Ea as isWebClientLocation, ga as getWindowsTerminalCapabilityOwnerKey, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as createLocalizedCatalog } from "./localized-catalog-DubKHKUR.js";
import { n as getAgentCatalog } from "./agent-catalog-CBF2CV5Q.js";
import { n as translateSearchKeyword, r as uniqueKeywords, t as searchKeywords } from "./settings-search-keywords-Cutqc_5t.js";
import { n as getAgentAwakeSearchKeywords, r as getAgentAwakeTitle, t as getAgentAwakeDescription } from "./agent-awake-copy-ClRvhNkR.js";
var Blocks = createLucideIcon("blocks", [["path", {
	d: "M10 22V7a1 1 0 0 0-1-1H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-5a1 1 0 0 0-1-1H2",
	key: "1ah6g2"
}], ["rect", {
	x: "14",
	y: "2",
	width: "8",
	height: "8",
	rx: "1",
	key: "88lufb"
}]]);
const SETTINGS_SEARCH_QUERY_MAX_BYTES = 2 * 1024;
var SETTINGS_SEARCH_NO_MATCH_SCORE = 0;
var SETTINGS_SEARCH_EMPTY_QUERY_SCORE = 1;
var PANE_TITLE_SCORE = {
	exact: 900,
	prefix: 850,
	substring: 800
};
var ENTRY_TITLE_SCORE = {
	exact: 700,
	prefix: 650,
	substring: 600
};
var DESCRIPTION_SCORE = {
	exact: 500,
	prefix: 450,
	substring: 400
};
var KEYWORD_SCORE = {
	exact: 300,
	prefix: 250,
	substring: 200
};
function isSettingsSearchQueryTooLarge(query, maxBytes = SETTINGS_SEARCH_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function normalizeSettingsSearchQuery(query) {
	return query.trim().toLowerCase();
}
function scoreSettingsSearchText(normalizedQuery, value, tier) {
	if (!value) return SETTINGS_SEARCH_NO_MATCH_SCORE;
	const normalizedValue = value.toLowerCase();
	if (normalizedValue === normalizedQuery) return tier.exact;
	if (normalizedValue.startsWith(normalizedQuery)) return tier.prefix;
	if (normalizedValue.includes(normalizedQuery)) return tier.substring;
	return SETTINGS_SEARCH_NO_MATCH_SCORE;
}
function scoreSettingsSearchValues(normalizedQuery, values, tier) {
	return (values ?? []).reduce((score, value) => Math.max(score, scoreSettingsSearchText(normalizedQuery, value, tier)), SETTINGS_SEARCH_NO_MATCH_SCORE);
}
function scoreSettingsSearch(query, entries) {
	if (isSettingsSearchQueryTooLarge(query)) return SETTINGS_SEARCH_NO_MATCH_SCORE;
	const normalizedQuery = normalizeSettingsSearchQuery(query);
	if (!normalizedQuery) return SETTINGS_SEARCH_EMPTY_QUERY_SCORE;
	return (Array.isArray(entries) ? entries : [entries]).reduce((score, entry, index) => {
		const titleScore = index === 0 ? PANE_TITLE_SCORE : ENTRY_TITLE_SCORE;
		return Math.max(score, scoreSettingsSearchText(normalizedQuery, entry.title, titleScore), scoreSettingsSearchText(normalizedQuery, entry.description, DESCRIPTION_SCORE), scoreSettingsSearchValues(normalizedQuery, entry.keywords, KEYWORD_SCORE));
	}, SETTINGS_SEARCH_NO_MATCH_SCORE);
}
function getSettingsSectionSearchEntries(section) {
	return [{
		title: section.title,
		description: section.description
	}, ...section.searchEntries];
}
function rankSettingsSearchItems(query, items, getEntries) {
	if (isSettingsSearchQueryTooLarge(query)) return [];
	if (!normalizeSettingsSearchQuery(query)) return items.map((item) => ({
		item,
		score: SETTINGS_SEARCH_EMPTY_QUERY_SCORE
	}));
	return items.map((item, index) => ({
		item,
		index,
		score: scoreSettingsSearch(query, getEntries(item))
	})).filter((candidate) => candidate.score > SETTINGS_SEARCH_NO_MATCH_SCORE).sort((a, b) => b.score - a.score || a.index - b.index).map(({ item, score }) => ({
		item,
		score
	}));
}
function matchesSettingsSearch(query, entries) {
	return scoreSettingsSearch(query, entries) > SETTINGS_SEARCH_NO_MATCH_SCORE;
}
const getAgentCacheTimerSearchEntries = createLocalizedCatalog(() => [{
	title: translate("auto.components.settings.general.search.1e0f28c6f1", "Prompt Cache Timer"),
	description: translate("auto.components.settings.general.search.40c9585e43", "Countdown timer showing time until prompt cache expires (Claude agents)."),
	keywords: [
		...translateSearchKeyword("auto.components.settings.general.search.b2601a778c", "cache"),
		...translateSearchKeyword("auto.components.settings.general.search.939b80f5fd", "timer"),
		...translateSearchKeyword("auto.components.settings.general.search.0efc9d96ad", "prompt"),
		...translateSearchKeyword("auto.components.settings.general.search.585beac3f8", "ttl"),
		...translateSearchKeyword("auto.components.settings.general.search.95b63edde7", "claude"),
		...translateSearchKeyword("auto.components.settings.general.search.660528b048", "cost"),
		...translateSearchKeyword("auto.components.settings.general.search.3462308bd3", "tokens")
	]
}]);
var AGENT_GENERATED_TAB_TITLES_TITLE_KEY = "auto.components.settings.agent-generated-tab-title-copy.19ad21615a";
var AGENT_GENERATED_TAB_TITLES_DESCRIPTION_KEY = "auto.components.settings.agent-generated-tab-title-copy.b036c7a409";
function getAgentGeneratedTabTitlesTitle() {
	return translate(AGENT_GENERATED_TAB_TITLES_TITLE_KEY, "Auto-generate tab titles");
}
function getAgentGeneratedTabTitlesDescription() {
	return translate(AGENT_GENERATED_TAB_TITLES_DESCRIPTION_KEY, "Derive short stable tab names from the first known agent prompt. Manual renames always win.");
}
function getAgentGeneratedTabTitlesSearchKeywords() {
	return searchKeywords([
		{
			key: "auto.components.settings.agents.search.96ba2373b6",
			fallback: "agent"
		},
		{
			key: "auto.components.settings.agents.search.be7ea3553b",
			fallback: "tab"
		},
		{
			key: "auto.components.settings.agents.search.6956646a1e",
			fallback: "title"
		},
		{
			key: "auto.components.settings.agents.search.32836788b0",
			fallback: "generated title"
		},
		{
			key: "auto.components.settings.agents.search.966890236d",
			fallback: "name"
		},
		{
			key: "auto.components.settings.agents.search.848dcae8d3",
			fallback: "generated"
		},
		{
			key: "auto.components.settings.agents.search.52115d0d7c",
			fallback: "auto"
		},
		{
			key: "auto.components.settings.agents.search.c64059f50d",
			fallback: "prompt"
		},
		{
			key: "auto.components.settings.agents.search.5784ae8c43",
			fallback: "rename"
		},
		{
			key: "auto.components.settings.agents.search.8a17fd6026",
			fallback: "stable"
		},
		{
			key: "auto.components.settings.agents.search.a79d266f71",
			fallback: "session"
		},
		{
			key: "auto.components.settings.agents.search.afbf35be68",
			fallback: "stable session"
		}
	]);
}
var AGENT_STATUS_HOOKS_TITLE_KEY = "auto.components.settings.agent-status-hooks-copy.7707c15abb";
var AGENT_STATUS_HOOKS_DESCRIPTION_KEY = "auto.components.settings.agent-status-hooks-copy.a68a642835";
function getAgentStatusHooksTitle() {
	return translate(AGENT_STATUS_HOOKS_TITLE_KEY, "Agent status hooks");
}
function getAgentStatusHooksDescription() {
	return translate(AGENT_STATUS_HOOKS_DESCRIPTION_KEY, "Shows working, waiting, and done states in Orca. Turn off to remove Orca-managed hooks and stop reinstalling them.");
}
function getAgentStatusHooksSearchKeywords() {
	return searchKeywords([
		{
			key: "auto.components.settings.agents.search.0d752916f8",
			fallback: "hooks"
		},
		{
			key: "auto.components.settings.agents.search.6984d4291a",
			fallback: "status"
		},
		{
			key: "auto.components.settings.agents.search.affbf130f6",
			fallback: "working"
		},
		{
			key: "auto.components.settings.agents.search.13b20636a6",
			fallback: "waiting"
		},
		{
			key: "auto.components.settings.agents.search.8599603496",
			fallback: "done"
		},
		{
			key: "auto.components.settings.agents.search.ea71995548",
			fallback: "remove"
		},
		{
			key: "auto.components.settings.agents.search.c1317fe641",
			fallback: "restore"
		},
		{
			key: "auto.components.settings.agents.search.5963143e00",
			fallback: "settings"
		},
		{
			key: "auto.components.settings.agents.search.042c551bc5",
			fallback: "config"
		},
		{
			key: "auto.components.settings.agents.search.f412abbba5",
			fallback: "claude",
			englishOnly: true
		},
		{
			key: "auto.components.settings.agents.search.5ded38b843",
			fallback: "codex",
			englishOnly: true
		}
	]);
}
function buildAgentSettingsKeywords() {
	const keywords = searchKeywords([
		{
			key: "auto.components.settings.agents.search.96ba2373b6",
			fallback: "agent"
		},
		{
			key: "auto.components.settings.agents.search.d8f3a8b8a0",
			fallback: "default"
		},
		{
			key: "auto.components.settings.agents.search.167daeb5e9",
			fallback: "command"
		},
		{
			key: "auto.components.settings.agents.search.be59907510",
			fallback: "override"
		},
		{
			key: "auto.components.settings.agents.search.a6d594c17d",
			fallback: "install"
		},
		{
			key: "auto.components.settings.agents.search.f2932bf22b",
			fallback: "detected"
		},
		{
			key: "auto.components.settings.agents.search.2afd3b5858",
			fallback: "enable"
		},
		{
			key: "auto.components.settings.agents.search.60393e1b17",
			fallback: "disable"
		},
		{
			key: "auto.components.settings.agents.search.2e188c771c",
			fallback: "hide"
		},
		{
			key: "auto.components.settings.agents.search.87fffe6c20",
			fallback: "show"
		},
		{
			key: "auto.components.settings.agents.search.permission",
			fallback: "permission"
		},
		{
			key: "auto.components.settings.agents.search.permissions",
			fallback: "permissions"
		},
		{
			key: "auto.components.settings.agents.search.yolo",
			fallback: "yolo",
			englishOnly: true
		},
		{
			key: "auto.components.settings.agents.search.manual",
			fallback: "manual"
		},
		{
			key: "auto.components.settings.agents.search.e2b7c0dcd7",
			fallback: "github",
			englishOnly: true
		}
	]);
	for (const agent of getAgentCatalog()) {
		keywords.push(...expandAgentSearchText(agent.id), ...expandAgentSearchText(agent.label));
		keywords.push(...expandAgentSearchText(agent.cmd));
	}
	return uniqueKeywords(keywords);
}
function expandAgentSearchText(value) {
	const spaced = value.replace(/([a-z0-9])([A-Z])/g, "$1 $2").replace(/[-_]+/g, " ").trim();
	return spaced === value ? [value] : [value, spaced];
}
var AGENT_AWAKE_SEARCH_ENTRY_ID = "agent-awake";
var AGENT_RUNTIME_SEARCH_ENTRY_ID = "agent-runtime";
var getAllAgentsPaneSearchEntries = createLocalizedCatalog(() => [
	{
		title: translate("auto.components.settings.agents.search.bb9ad95777", "Agents"),
		description: translate("auto.components.settings.agents.search.01926b9d8c", "Configure AI coding agents, default agent, and command overrides."),
		keywords: buildAgentSettingsKeywords()
	},
	{
		title: translate("auto.components.settings.agents.search.agentRuntime", "Agent Runtime"),
		id: AGENT_RUNTIME_SEARCH_ENTRY_ID,
		description: translate("auto.components.settings.agents.search.agentRuntimeDescription", "Choose whether agents are detected and launched on Windows or in WSL by default."),
		keywords: [
			...translateSearchKeyword("auto.components.settings.agents.search.96ba2373b6", "agent"),
			...translateSearchKeyword("auto.components.settings.agents.search.runtime", "runtime"),
			...translateSearchKeyword("auto.components.settings.agents.search.d2952dfd74", "location"),
			...translateSearchKeyword("auto.components.settings.agents.search.agentLocation", "agent location"),
			...translateSearchKeyword("auto.components.settings.agents.search.77c02fa3c3", "windows"),
			...translateSearchKeyword("auto.components.settings.agents.search.d608654c03", "wsl"),
			...translateSearchKeyword("auto.components.settings.agents.search.f622b8eb2a", "linux"),
			...translateSearchKeyword("auto.components.settings.agents.search.839e82c81f", "detect"),
			...translateSearchKeyword("auto.components.settings.agents.search.2814401339", "installed"),
			...translateSearchKeyword("auto.components.settings.agents.search.installedAgentsWsl", "installed agents in wsl"),
			...translateSearchKeyword("auto.components.settings.agents.search.719f53350c", "path")
		]
	},
	{
		title: getAgentStatusHooksTitle(),
		description: getAgentStatusHooksDescription(),
		keywords: getAgentStatusHooksSearchKeywords()
	},
	{
		title: getAgentGeneratedTabTitlesTitle(),
		description: getAgentGeneratedTabTitlesDescription(),
		keywords: getAgentGeneratedTabTitlesSearchKeywords()
	},
	{
		title: getAgentAwakeTitle(),
		id: AGENT_AWAKE_SEARCH_ENTRY_ID,
		description: getAgentAwakeDescription(),
		keywords: getAgentAwakeSearchKeywords()
	},
	{
		title: translate("auto.components.settings.agents.search.agentPermissions", "Agent Permissions"),
		description: translate("auto.components.settings.agents.search.agentPermissionsDescription", "Switch agent permission defaults between Yolo and Manual."),
		keywords: [
			...translateSearchKeyword("auto.components.settings.agents.search.permission", "permission"),
			...translateSearchKeyword("auto.components.settings.agents.search.permissions", "permissions"),
			...translateSearchKeyword("auto.components.settings.agents.search.yolo", "yolo"),
			...translateSearchKeyword("auto.components.settings.agents.search.manual", "manual"),
			...translateSearchKeyword("auto.components.settings.agents.search.skip", "skip"),
			...translateSearchKeyword("auto.components.settings.agents.search.checks", "checks")
		]
	},
	...getAgentCacheTimerSearchEntries()
]);
function getAgentsPaneSearchEntries({ includeAgentAwake = true, includeAgentRuntime = true } = {}) {
	return getAllAgentsPaneSearchEntries().filter((entry) => (!("id" in entry) || entry.id !== AGENT_RUNTIME_SEARCH_ENTRY_ID || includeAgentRuntime) && (!("id" in entry) || entry.id !== AGENT_AWAKE_SEARCH_ENTRY_ID || includeAgentAwake));
}
function resolveWindowsTerminalCapabilityOwnerKey(args) {
	const activeEnvironmentId = args.activeRuntimeEnvironmentId?.trim() || null;
	const environment = args.isWebClient ? args.runtimeEnvironments[0] ?? null : null;
	const ownerKey = getWindowsTerminalCapabilityOwnerKey(environment?.id ?? activeEnvironmentId, args.sshConnectionId);
	if (!environment) return ownerKey;
	return `${ownerKey}:pairing:${environment.pairingRevision ?? environment.createdAt}:connection:${args.runtimeStatusByEnvironmentId?.get(environment.id)?.connectionGeneration ?? 0}`;
}
function useWindowsTerminalCapabilityOwnerKey(activeRuntimeEnvironmentId, sshConnectionId) {
	const isWebClient = isWebClientLocation();
	return useAppStore((state) => resolveWindowsTerminalCapabilityOwnerKey({
		activeRuntimeEnvironmentId,
		isWebClient,
		runtimeEnvironments: state.runtimeEnvironments ?? [],
		runtimeStatusByEnvironmentId: state.runtimeStatusByEnvironmentId,
		sshConnectionId
	}));
}
export { getAgentGeneratedTabTitlesDescription as a, getSettingsSectionSearchEntries as c, rankSettingsSearchItems as d, scoreSettingsSearch as f, getAgentStatusHooksTitle as i, matchesSettingsSearch as l, getAgentsPaneSearchEntries as n, getAgentGeneratedTabTitlesTitle as o, Blocks as p, getAgentStatusHooksDescription as r, getAgentCacheTimerSearchEntries as s, useWindowsTerminalCapabilityOwnerKey as t, normalizeSettingsSearchQuery as u };
