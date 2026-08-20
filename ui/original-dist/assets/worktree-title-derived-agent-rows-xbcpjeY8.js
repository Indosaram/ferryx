import { Gc as isTerminalLeafId, Kc as makePaneKey } from "./store-CgXrfmaH.js";
import { E as isClaudeIdentityFrameTitle, L as isClaudeManagementTitle, R as isCursorAgentTitle, c as resolveTitleActivityLabel, i as classifyTitleActivity, r as formatAgentTypeLabel, v as tabHasLivePty } from "./agent-status-3vUKbY6l.js";
import { i as resolveCompatibleAgentTypeForOwner, r as normalizeCompatibleAgentTitleForOwner, t as resolvePaneAgentOwner } from "./pane-agent-owner-BPfoVAtS.js";
var EMPTY_RUNTIME_TITLES = {};
var EMPTY_LIVE_PTY_IDS = {};
var EMPTY_TERMINAL_LAYOUTS = {};
var TITLE_AGENT_LABEL_TO_TYPE = {
	"Claude Code": "claude",
	OpenClaude: "openclaude",
	Codex: "codex",
	"Gemini CLI": "gemini",
	"GitHub Copilot": "copilot",
	Grok: "grok",
	Devin: "devin",
	Antigravity: "antigravity",
	OpenCode: "opencode",
	Aider: "aider",
	Cursor: "cursor",
	Droid: "droid",
	Hermes: "hermes",
	Pi: "pi",
	OMP: "omp"
};
var CLAUDE_AGENT_TOKEN_RE = /(?<![\w./\\-])claude(?![\w./\\-])/i;
function buildTitleDerivedAgentRows(args) {
	const rows = [];
	const runtimePaneTitlesByTabId = args.runtimePaneTitlesByTabId ?? EMPTY_RUNTIME_TITLES;
	const ptyIdsByTabId = args.ptyIdsByTabId ?? EMPTY_LIVE_PTY_IDS;
	const terminalLayoutsByTabId = args.terminalLayoutsByTabId ?? EMPTY_TERMINAL_LAYOUTS;
	for (const tab of args.tabs) {
		if (!tabHasLivePty(ptyIdsByTabId, tab.id)) continue;
		const layout = terminalLayoutsByTabId[tab.id];
		const paneTitles = runtimePaneTitlesByTabId[tab.id];
		const paneTitleEntries = paneTitles && Object.keys(paneTitles).length > 0 ? Object.entries(paneTitles).sort(([a], [b]) => Number(a) - Number(b)) : [];
		if (paneTitleEntries.length > 0) {
			for (const [paneId, title] of paneTitleEntries) {
				const leafId$1 = resolveLeafIdForTitleFallback({
					layout,
					paneTitleEntries,
					paneId: Number(paneId),
					title
				});
				if (!leafId$1) continue;
				const row$1 = buildTitleDerivedAgentRow({
					tab,
					leafId: leafId$1,
					title,
					ownerAgentType: resolveTitleDerivedPaneOwner(tab, layout, leafId$1),
					now: args.now,
					runtimeAgentOrchestrationByPaneKey: args.runtimeAgentOrchestrationByPaneKey
				});
				if (!row$1 || args.seenPaneKeys.has(row$1.paneKey)) continue;
				rows.push(row$1);
				args.seenPaneKeys.add(row$1.paneKey);
			}
			continue;
		}
		const leafId = layout?.activeLeafId ?? collectLeafIds(layout?.root ?? null)[0];
		if (!leafId) continue;
		const row = buildTitleDerivedAgentRow({
			tab,
			leafId,
			title: tab.title,
			ownerAgentType: resolveTitleDerivedPaneOwner(tab, layout, leafId),
			now: args.now,
			runtimeAgentOrchestrationByPaneKey: args.runtimeAgentOrchestrationByPaneKey
		});
		if (!row || args.seenPaneKeys.has(row.paneKey)) continue;
		rows.push(row);
		args.seenPaneKeys.add(row.paneKey);
	}
	return rows;
}
function buildTitleDerivedAgentRow(args) {
	const title = normalizeCompatibleAgentTitleForOwner(args.title, args.tab.launchAgent);
	const isClaudeAgentsTitle = isClaudeManagementTitle(title);
	const status = isClaudeAgentsTitle ? "idle" : classifyTitleActivity(title) ?? (isCursorAgentTitle(title) ? "idle" : null);
	const label = isClaudeAgentsTitle ? "Claude Code" : resolveTitleActivityLabel(title);
	if (!status || !label) return null;
	if (!isTerminalLeafId(args.leafId)) return null;
	const paneKey = makePaneKey(args.tab.id, args.leafId);
	const orchestration = args.runtimeAgentOrchestrationByPaneKey?.[paneKey];
	const titleAgentType = isClaudeAgentsTitle ? "claude" : resolveTitleDerivedAgentType(title, label, args.ownerAgentType);
	const agentType = titleAgentType ?? args.ownerAgentType;
	if (!agentType) return null;
	const rowLabel = titleAgentType ? label : formatAgentTypeLabel(agentType);
	const rowState = titleStatusToRowState(status);
	const secondary = status === "permission" ? "Needs input" : status === "working" ? "Running" : "Idle";
	return {
		paneKey,
		entry: {
			paneKey,
			state: rowState === "waiting" ? "waiting" : "working",
			prompt: rowLabel,
			updatedAt: args.now,
			stateStartedAt: args.now,
			stateHistory: [],
			agentType,
			terminalTitle: title,
			lastAssistantMessage: secondary,
			...orchestration ? { orchestration } : {}
		},
		tab: args.tab,
		agentType,
		rowSource: "live",
		state: rowState,
		startedAt: 0
	};
}
function resolveTitleDerivedAgentType(title, label, ownerAgentType) {
	const agentType = TITLE_AGENT_LABEL_TO_TYPE[label] ?? "unknown";
	if (agentType !== "claude") return agentType;
	if (!CLAUDE_AGENT_TOKEN_RE.test(title)) return null;
	const owner = ownerAgentType && ownerAgentType !== "unknown" ? ownerAgentType : null;
	if (owner && owner !== "claude" && !isClaudeIdentityFrameTitle(title)) return null;
	return agentType;
}
function resolveTitleDerivedPaneOwner(tab, layout, leafId) {
	if (layout?.root?.type !== "leaf" || layout.root.leafId !== leafId) return null;
	return resolvePaneAgentOwner({ launchAgent: tab.launchAgent });
}
function resolveAgentTypeFromTerminalTitle(title, ownerAgentType) {
	if (!title) return null;
	const normalizedTitle = normalizeCompatibleAgentTitleForOwner(title, ownerAgentType);
	const label = resolveTitleActivityLabel(normalizedTitle);
	return label ? resolveCompatibleAgentTypeForOwner(resolveTitleDerivedAgentType(normalizedTitle, label, ownerAgentType), ownerAgentType) ?? null : null;
}
function titleStatusToRowState(status) {
	if (status === "permission") return "waiting";
	if (status === "working") return "working";
	return "idle";
}
function resolveLeafIdForTitleFallback(args) {
	const matchingTitleLeafIds = Object.entries(args.layout?.titlesByLeafId ?? {}).filter(([, title]) => title === args.title).map(([leafId]) => leafId);
	if (matchingTitleLeafIds.length === 1) return matchingTitleLeafIds[0];
	const leafIds = collectLeafIds(args.layout?.root ?? null);
	if (leafIds.length === 1) return leafIds[0];
	const paneIndex = args.paneTitleEntries.findIndex(([paneId]) => Number(paneId) === args.paneId);
	return paneIndex !== -1 ? leafIds[paneIndex] ?? null : null;
}
function collectLeafIds(node) {
	if (!node) return [];
	if (node.type === "leaf") return [node.leafId];
	return [...collectLeafIds(node.first), ...collectLeafIds(node.second)];
}
export { resolveAgentTypeFromTerminalTitle as n, buildTitleDerivedAgentRows as t };
