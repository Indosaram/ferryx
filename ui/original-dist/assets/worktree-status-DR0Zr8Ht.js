import { jr as resolveRuntimePaneTitleLeafIdFromRoot } from "./store-CgXrfmaH.js";
import { I as containsAgentSpinnerGlyph, i as classifyTitleActivity, v as tabHasLivePty } from "./agent-status-3vUKbY6l.js";
import { n as resolveAgentTypeFromTerminalTitle } from "./worktree-title-derived-agent-rows-xbcpjeY8.js";
var STATUS_LABELS = {
	active: "Active",
	working: "Working",
	permission: "Needs permission",
	done: "Done",
	inactive: "Inactive"
};
function getWorktreeStatus(tabs, browserTabs, ptyIdsByTabId, runtimePaneTitlesByTabId = {}, options = {}) {
	const liveTabs = tabs.filter((tab) => tabHasLivePty(ptyIdsByTabId, tab.id));
	const hasStatus = (status) => liveTabs.some((tab) => tabHasStatus(tab, runtimePaneTitlesByTabId, status, options));
	if (options.liveAgentStatus === "permission" || hasStatus("permission")) return "permission";
	if (options.liveAgentStatus === "working" || hasStatus("working")) return "working";
	if (liveTabs.length > 0 || browserTabs.length > 0) return "active";
	return "inactive";
}
function tabHasStatus(tab, runtimePaneTitlesByTabId, status, options) {
	const agentStatusPaneIds = options.agentStatusPaneIdsByTabId?.[tab.id];
	const paneTitles = runtimePaneTitlesByTabId[tab.id];
	if (paneTitles && Object.keys(paneTitles).length > 0) {
		const tabLayoutRoot = options.terminalLayoutRootsByTabId?.[tab.id] ?? options.terminalLayoutsByTabId?.[tab.id]?.root;
		const paneTitleEntries = Object.entries(paneTitles);
		for (const [runtimePaneId, title] of paneTitleEntries) {
			const leafId = resolveRuntimePaneTitleLeafIdFromRoot(tabLayoutRoot, runtimePaneId);
			const hasSingleUnmappedAgentStatusPane = leafId === null && agentStatusPaneIds?.size === 1 && paneTitleEntries.length === 1;
			if (agentStatusPaneIds?.has(runtimePaneId) || leafId !== null && agentStatusPaneIds?.has(leafId) || hasSingleUnmappedAgentStatusPane) continue;
			if (classifyTitleActivity(title) === status && titleStatusIsAgentAttributable(title, tab.launchAgent)) return true;
		}
		return false;
	}
	if (agentStatusPaneIds && agentStatusPaneIds.size > 0) return false;
	return classifyTitleActivity(tab.title) === status && titleStatusIsAgentAttributable(tab.title, tab.launchAgent);
}
function titleStatusIsAgentAttributable(title, launchAgent) {
	if (resolveAgentTypeFromTerminalTitle(title) !== null) return true;
	return containsAgentSpinnerGlyph(title) && Boolean(launchAgent);
}
function getWorktreeStatusLabel(status) {
	return STATUS_LABELS[status];
}
function resolveWorktreeStatus(args) {
	const heuristic = getWorktreeStatus(args.tabs, args.browserTabs, args.ptyIdsByTabId, args.runtimePaneTitlesByTabId ?? {}, {
		agentStatusPaneIdsByTabId: args.agentStatusPaneIdsByTabId,
		terminalLayoutsByTabId: args.terminalLayoutsByTabId,
		terminalLayoutRootsByTabId: args.terminalLayoutRootsByTabId
	});
	if (args.hasPermission) return "permission";
	if (heuristic === "permission") return "permission";
	if (args.hasLiveWorking || heuristic === "working") return "working";
	if (args.hasLiveDone || args.hasRetainedDone) return "done";
	return heuristic;
}
export { getWorktreeStatusLabel as n, resolveWorktreeStatus as r, getWorktreeStatus as t };
