import { Jc as parsePaneKey, qc as parseLegacyNumericPaneKey } from "./store-CgXrfmaH.js";
import { a as isExplicitAgentStatusFresh, u as AGENT_STATUS_STALE_AFTER_MS } from "./agent-status-3vUKbY6l.js";
import { r as resolveWorktreeStatus } from "./worktree-status-DR0Zr8Ht.js";
var flagsCache = null;
function getTerminalTabActivityFlags(agentStatusByPaneKey, agentStatusEpoch) {
	if (flagsCache && flagsCache.agentStatusByPaneKey === agentStatusByPaneKey && flagsCache.agentStatusEpoch === agentStatusEpoch) return flagsCache.flagsByTabId;
	const flagsByTabId = /* @__PURE__ */ new Map();
	const now = Date.now();
	for (const [paneKey, entry] of Object.entries(agentStatusByPaneKey ?? {})) {
		const identity = parseAgentStatusPaneKey(entry.paneKey || paneKey);
		if (!identity) continue;
		if (entry.restoredUnconfirmed) {
			getOrCreateTerminalTabActivityFlags(flagsByTabId, identity.tabId).paneIds.add(identity.paneId);
			continue;
		}
		if (!isExplicitAgentStatusFresh(entry, now, 18e5)) continue;
		const flags = getOrCreateTerminalTabActivityFlags(flagsByTabId, identity.tabId);
		flags.paneIds.add(identity.paneId);
		if (entry.state === "blocked" || entry.state === "waiting") flags.hasPermission = true;
		else if (entry.state === "working") flags.hasLiveWorking = true;
		else if (entry.state === "done") flags.hasLiveDone = true;
	}
	flagsCache = {
		agentStatusByPaneKey,
		agentStatusEpoch,
		flagsByTabId
	};
	return flagsByTabId;
}
function getOrCreateTerminalTabActivityFlags(flagsByTabId, tabId) {
	let flags = flagsByTabId.get(tabId);
	if (!flags) {
		flags = {
			hasPermission: false,
			hasLiveWorking: false,
			hasLiveDone: false,
			paneIds: /* @__PURE__ */ new Set()
		};
		flagsByTabId.set(tabId, flags);
	}
	return flags;
}
function parseAgentStatusPaneKey(paneKey) {
	const parsed = parsePaneKey(paneKey);
	if (parsed) return {
		tabId: parsed.tabId,
		paneId: parsed.leafId
	};
	const legacy = parseLegacyNumericPaneKey(paneKey);
	return legacy ? {
		tabId: legacy.tabId,
		paneId: legacy.numericPaneId
	} : null;
}
var EMPTY_PANE_IDS = /* @__PURE__ */ new Set();
function resolveTerminalTabActivityStatus({ tab, agentStatusByPaneKey, agentStatusEpoch, runtimePaneTitlesByTabId, ptyIdsByTabId, terminalLayout }) {
	const flags = getTerminalTabActivityFlags(agentStatusByPaneKey, agentStatusEpoch).get(tab.id);
	return resolveWorktreeStatus({
		tabs: [tab],
		browserTabs: [],
		ptyIdsByTabId: ptyIdsByTabId ?? {},
		runtimePaneTitlesByTabId: runtimePaneTitlesByTabId ?? {},
		agentStatusPaneIdsByTabId: { [tab.id]: flags?.paneIds ?? EMPTY_PANE_IDS },
		terminalLayoutsByTabId: terminalLayout ? { [tab.id]: terminalLayout } : void 0,
		hasPermission: flags?.hasPermission ?? false,
		hasLiveWorking: flags?.hasLiveWorking ?? false,
		hasLiveDone: flags?.hasLiveDone ?? false,
		hasRetainedDone: false
	});
}
function isTerminalTabActivityLive(status) {
	return status === "working" || status === "permission";
}
function resolveTerminalTabAttentionBadge({ status, hasUnread }) {
	if (status === "working") return "working";
	if (status === "permission") return "permission";
	if (hasUnread) return "unread";
	if (status === "done") return "done";
	return null;
}
function terminalTabActivityToAgentDotState(status) {
	switch (status) {
		case "working":
		case "permission":
		case "done": return status;
		case "active":
		case "inactive": return null;
	}
}
function terminalTabHasUnreadActivity({ terminalTabId, unreadTerminalTabs, unreadAgentCompletionPanes }) {
	return unreadTerminalTabs[terminalTabId] === true || hasUnreadAgentCompletionForTerminalTab(unreadAgentCompletionPanes, terminalTabId);
}
function hasUnreadAgentCompletionForTerminalTab(unreadAgentCompletionPanes, tabId) {
	for (const [paneKey, unread] of Object.entries(unreadAgentCompletionPanes ?? {})) {
		if (!unread) continue;
		const separatorIndex = paneKey.indexOf(":");
		if ((separatorIndex === -1 ? paneKey : paneKey.slice(0, separatorIndex)) === tabId) return true;
	}
	return false;
}
export { terminalTabHasUnreadActivity as a, terminalTabActivityToAgentDotState as i, resolveTerminalTabActivityStatus as n, resolveTerminalTabAttentionBadge as r, isTerminalTabActivityLive as t };
