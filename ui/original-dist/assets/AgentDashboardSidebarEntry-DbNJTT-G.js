import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn } from "./button-DszXJEV6.js";
import { t as MessageCircleQuestionMark } from "./message-circle-question-mark-CFrAq4X1.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./useMountedRef-1omUd-IV.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./connection-context-BUPsamzR.js";
import "./worktree-agent-rows-C1pW_DbE.js";
import "./worktree-title-derived-agent-rows-xbcpjeY8.js";
import "./agent-row-conversation-name-DXwI1NP0.js";
import { t as DASHBOARD_BUCKET_ORDER } from "./dashboard-snapshot-B9IiTV8p.js";
import "./terminal-keyboard-protocol-De0UZ6qG.js";
import { t as buildDashboardSnapshot } from "./build-dashboard-snapshot-B3HmQPph.js";
var LayoutDashboard = createLucideIcon("layout-dashboard", [
	["rect", {
		width: "7",
		height: "9",
		x: "3",
		y: "3",
		rx: "1",
		key: "10lvy0"
	}],
	["rect", {
		width: "7",
		height: "5",
		x: "14",
		y: "3",
		rx: "1",
		key: "16une8"
	}],
	["rect", {
		width: "7",
		height: "9",
		x: "14",
		y: "12",
		rx: "1",
		key: "1hutg5"
	}],
	["rect", {
		width: "7",
		height: "5",
		x: "3",
		y: "16",
		rx: "1",
		key: "ldoo1y"
	}]
]);
var import_react = /* @__PURE__ */ __toESM(require_react());
var EMPTY_COUNTS = {
	attention: 0,
	working: 0,
	done: 0,
	idle: 0
};
function useAgentBucketCounts() {
	const { repos, worktreesByRepo, tabsByWorktree, agentStatusByPaneKey, retainedAgentsByPaneKey, migrationUnsupportedByPtyId, runtimeAgentOrchestrationByPaneKey, terminalLayoutsByTabId, ptyIdsByTabId, runtimePaneTitlesByTabId, folderWorkspaces, acknowledgedAgentsByPaneKey, agentStatusEpoch } = useAppStore(useShallow((s) => ({
		repos: s.repos,
		worktreesByRepo: s.worktreesByRepo,
		tabsByWorktree: s.tabsByWorktree,
		agentStatusByPaneKey: s.agentStatusByPaneKey,
		retainedAgentsByPaneKey: s.retainedAgentsByPaneKey,
		migrationUnsupportedByPtyId: s.migrationUnsupportedByPtyId,
		runtimeAgentOrchestrationByPaneKey: s.runtimeAgentOrchestrationByPaneKey,
		terminalLayoutsByTabId: s.terminalLayoutsByTabId,
		ptyIdsByTabId: s.ptyIdsByTabId,
		runtimePaneTitlesByTabId: s.runtimePaneTitlesByTabId,
		folderWorkspaces: s.folderWorkspaces,
		acknowledgedAgentsByPaneKey: s.acknowledgedAgentsByPaneKey,
		agentStatusEpoch: s.agentStatusEpoch
	})));
	return (0, import_react.useMemo)(() => {
		const snapshot = buildDashboardSnapshot({
			repos,
			worktreesByRepo,
			tabsByWorktree,
			agentStatusByPaneKey,
			retainedAgentsByPaneKey,
			migrationUnsupportedByPtyId,
			runtimeAgentOrchestrationByPaneKey,
			terminalLayoutsByTabId,
			ptyIdsByTabId,
			runtimePaneTitlesByTabId,
			folderWorkspaces,
			acknowledgedAgentsByPaneKey,
			settings: null
		}, Date.now(), {
			includeCardDetails: false,
			includeFilterOptions: false
		});
		if (snapshot.cards.length === 0) return EMPTY_COUNTS;
		const counts = {
			attention: 0,
			working: 0,
			done: 0,
			idle: 0
		};
		for (const card of snapshot.cards) counts[card.bucket] += 1;
		return counts;
	}, [
		repos,
		worktreesByRepo,
		tabsByWorktree,
		agentStatusByPaneKey,
		retainedAgentsByPaneKey,
		migrationUnsupportedByPtyId,
		runtimeAgentOrchestrationByPaneKey,
		terminalLayoutsByTabId,
		ptyIdsByTabId,
		runtimePaneTitlesByTabId,
		folderWorkspaces,
		acknowledgedAgentsByPaneKey,
		agentStatusEpoch
	]);
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var DASHBOARD_BUCKET_DOT_CLASS = {
	working: "bg-yellow-500",
	done: "bg-emerald-500",
	idle: "bg-neutral-500/50"
};
function dashboardBucketLabel(bucket) {
	switch (bucket) {
		case "attention": return translate("dashboardPopout.bucket.attention", "Needs You");
		case "working": return translate("dashboardPopout.bucket.working", "Working");
		case "done": return translate("dashboardPopout.bucket.done", "Done");
		case "idle": return translate("dashboardPopout.bucket.idle", "Idle");
	}
}
function DashboardBucketCounts({ counts, showIdle }) {
	const active = DASHBOARD_BUCKET_ORDER.filter((bucket) => counts[bucket] > 0 && (bucket !== "idle" || showIdle));
	if (active.length === 0) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: "flex items-center gap-1.5",
		children: active.map((bucket) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			"aria-label": `${dashboardBucketLabel(bucket)}: ${counts[bucket]}`,
			className: "inline-flex items-center gap-1 text-[10px] tabular-nums text-worktree-sidebar-foreground/55",
			children: [bucket === "attention" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MessageCircleQuestionMark, {
				className: "size-2.5 text-amber-500",
				"aria-hidden": true
			}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: cn("size-1.5 rounded-full", DASHBOARD_BUCKET_DOT_CLASS[bucket]) }), counts[bucket]]
		}, bucket))
	});
}
function AgentDashboardSidebarEntry() {
	const dashboardBucketCounts = useAgentBucketCounts();
	const showIdle = useAppStore((s) => s.settings?.experimentalAgentDashboardShowIdle === true);
	const openAsPopout = useAppStore((s) => s.settings?.experimentalAgentDashboardMode === "popout");
	const drawerOpen = useAppStore((s) => s.agentDashboardDrawerOpen);
	const setAgentDashboardDrawerOpen = useAppStore((s) => s.setAgentDashboardDrawerOpen);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
		type: "button",
		onClick: () => {
			if (openAsPopout) window.api.dashboard.openPopout();
			else setAgentDashboardDrawerOpen(!drawerOpen);
		},
		className: cn("flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] font-medium tracking-tight transition-colors", "text-worktree-sidebar-foreground/60 hover:bg-worktree-sidebar-foreground/8"),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LayoutDashboard, {
				className: "size-4 shrink-0 text-worktree-sidebar-foreground/30",
				strokeWidth: 1.75
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "flex-1",
				children: translate("dashboard.sidebar.label", "Agent Dashboard")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DashboardBucketCounts, {
				counts: dashboardBucketCounts,
				showIdle
			})
		]
	});
}
export { AgentDashboardSidebarEntry as default };
