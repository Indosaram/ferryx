const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./AgentMap-DWOB9i-P.js","./preload-helper-Cgw39-ka.js","./button-DszXJEV6.js","./jsx-runtime-Cv_nyRjc.js","./chunk-Dhmk_5SA.js","./react-Da2TLWQy.js","./context-menu-D4RKI7hR.js","./dist-CUdeCwrc.js","./dist-Ca8cIakR.js","./classPrivateFieldGet2-CvaeS1Sp.js","./dist-BsNIAh1s.js","./floating-ui.dom-i2UEqmZo.js","./dist-BvH-oDES.js","./dist-DGfr86jh.js","./dist-DW1EJH6e.js","./react-dom-Da8MQai-.js","./dist-B1f0G6s_.js","./dist-G_cmV6EA.js","./es2015-B5WZ-7WO.js","./chevron-right-CZtMe6Ev.js","./circle-DumnR8X3.js","./popover-CgR1mzy7.js","./tooltip-DPmd1AoJ.js","./dist-DS2hPbHS.js","./repo-icon-Dcv6msBx.js","./bot-nZ1Hb9j2.js","./box-CsJZiF6f.js","./braces-h3B7q7My.js","./code-xml-Q_QLKUSg.js","./database-xHap7ese.js","./folder-CYUB3i-Q.js","./layers-C1KP-86j.js","./globe-c32i33v9.js","./package-CI_g0v6D.js","./palette-WtHqBYpp.js","./server-DYdwnXME.js","./sparkles-DTr27w4B.js","./square-terminal-rgWG-Apn.js","./wrench-DbYIAUyT.js","./localized-catalog-DubKHKUR.js","./minus-Byrkh1sN.js","./moon-Cw6GyiDZ.js","./plus-Db0kWPVa.js","./icons-jFAuHbv9.js","./agent-catalog-CBF2CV5Q.js","./agent-status-3vUKbY6l.js","./agent-kind-Dfx6MnkP.js","./AgentStateDot-DFt63YGw.js","./circle-check-CmH3uVJy.js","./message-circle-question-mark-CFrAq4X1.js","./AgentWorkingSpinner-BpnTWNKF.js","./DashboardHostBadge-DXORvYCI.js","./agent-map-filter-CTyDhUZY.js","./dashboard-snapshot-B9IiTV8p.js","./agent-map-workspace-identity-DOM8S3VE.js","./usePrefersReducedMotion-TEdW-TWP.js","./lazy-with-retry-pSZJrSfN.js","./defineProperty-BAtR-r70.js","./AgentMap-CH1cI5LN.css"])))=>i.map(i=>d[i]);
import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as lazyWithRetry } from "./lazy-with-retry-pSZJrSfN.js";
import { n as cn } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import { i as EMPTY_DASHBOARD_FILTERS, n as AgentTerminalPanel, o as filterDashboardWorkspaces, r as AgentDashboardToolbar } from "./AgentTerminalDialog-BBP0-dz2.js";
import "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./dropdown-menu-Dth6LPK-.js";
import "./useMountedRef-1omUd-IV.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./localized-catalog-DubKHKUR.js";
import "./terminal-appearance-D3oO-Ew5.js";
import "./ShortcutKeyCombo-Ch456Md0.js";
import "./dialog-BbelfMSB.js";
import "./input-DV5rpysh.js";
import "./AgentWorkingSpinner-BpnTWNKF.js";
import "./AgentStateDot-DFt63YGw.js";
import "./icons-jFAuHbv9.js";
import "./agent-catalog-CBF2CV5Q.js";
import "./crash-diagnostics-DaEtfKCs.js";
import "./use-system-prefers-dark-QSo6mmSW.js";
import "./paste-payload-metadata-pr3nuODB.js";
import "./terminal-shortcut-policy-BOkUsz_T.js";
/* empty css               */
import { a as agentMapWorktreeIdentityFromParts } from "./agent-map-workspace-identity-DOM8S3VE.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function selectAgentlessMapWorkspaces({ cards, workspaces, query, filters }) {
	const occupiedWorkspaceIds = new Set(cards.map((card) => agentMapWorktreeIdentityFromParts(card.worktreeId, card.executionHostId)));
	return filterDashboardWorkspaces(workspaces, query, filters).filter((workspace) => !occupiedWorkspaceIds.has(agentMapWorktreeIdentityFromParts(workspace.worktreeId, workspace.executionHostId)));
}
var ALL_AGENT_STATES = [
	"attention",
	"working",
	"done",
	"idle"
];
function useAgentMapStateFilter() {
	const [agentStates, setAgentStates] = (0, import_react.useState)(() => new Set(ALL_AGENT_STATES));
	return {
		agentStates,
		toggleAgentState: (0, import_react.useCallback)((state) => {
			setAgentStates((current) => {
				const next = new Set(current);
				if (!next.delete(state)) next.add(state);
				return next;
			});
		}, []),
		resetAgentStates: (0, import_react.useCallback)(() => {
			setAgentStates(new Set(ALL_AGENT_STATES));
		}, [])
	};
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var AgentMap = lazyWithRetry(() => __vitePreload(() => import("./AgentMap-DWOB9i-P.js"), __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58]), import.meta.url).then((module) => ({ default: module.AgentMap })), { reloadKey: "agent-map" });
function AgentDashboardMapView({ snapshot, cards, query, onQueryChange, filters, onFiltersChange, searchInputRef, now, dialogCard, onDialogOpenChange, onRevealAgent, onOpenTerminal, onSpawnAgent, onSleepWorkspace, workspaceContextMenusEnabled, onWorkspaceContextMenuOpenChange }) {
	const { agentStates, toggleAgentState, resetAgentStates } = useAgentMapStateFilter();
	const [showAgentlessWorkspaces, setShowAgentlessWorkspaces] = (0, import_react.useState)(false);
	const [showOrchestrationLinks, setShowOrchestrationLinks] = (0, import_react.useState)(true);
	const agentlessWorkspaces = (0, import_react.useMemo)(() => selectAgentlessMapWorkspaces({
		cards: snapshot.cards,
		workspaces: snapshot.workspaces ?? [],
		query: "",
		filters: EMPTY_DASHBOARD_FILTERS
	}), [snapshot.cards, snapshot.workspaces]);
	const visibleAgentlessWorkspaces = (0, import_react.useMemo)(() => showAgentlessWorkspaces ? filterDashboardWorkspaces(agentlessWorkspaces, query, filters) : [], [
		agentlessWorkspaces,
		filters,
		query,
		showAgentlessWorkspaces
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentDashboardToolbar, {
		cards: snapshot.cards,
		filterOptions: snapshot.filterOptions,
		filteredCount: cards.length,
		query,
		onQueryChange,
		filters,
		onFiltersChange,
		agentStates,
		onAgentStateToggle: toggleAgentState,
		onAgentStatesReset: resetAgentStates,
		showAgentlessWorkspaces,
		agentlessWorkspaceCount: agentlessWorkspaces.length,
		onShowAgentlessWorkspacesChange: setShowAgentlessWorkspaces,
		showOrchestrationLinks,
		onShowOrchestrationLinksChange: setShowOrchestrationLinks,
		searchInputRef
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("flex min-h-0 flex-1", dialogCard && "flex-row-reverse"),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_react.Suspense, {
			fallback: null,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMap, {
				cards,
				workspaces: visibleAgentlessWorkspaces,
				repoIconsByRepoId: snapshot.repoIconsByRepoId,
				now,
				className: dialogCard ? "w-1/2 flex-none" : void 0,
				compact: dialogCard !== null,
				selectedPaneKey: dialogCard?.paneKey,
				enabledStates: agentStates,
				showOrchestrationLinks,
				launchableAgentsByWorktreeId: snapshot.launchableAgentsByWorktreeId,
				workspaceContextMenusEnabled,
				onWorkspaceContextMenuOpenChange,
				onOpenTerminal,
				onSpawnAgent,
				onSleepWorkspace
			})
		}), dialogCard ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentTerminalPanel, {
			card: dialogCard,
			onOpenChange: onDialogOpenChange,
			onReveal: onRevealAgent,
			className: "mr-0 animate-in fade-in-0 slide-in-from-left-2 duration-200 motion-reduce:animate-none"
		}) : null]
	})] });
}
export { AgentDashboardMapView };
