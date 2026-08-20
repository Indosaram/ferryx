import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as I18nProvider } from "./I18nProvider-Fh0iT-Ow.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as useTranslation } from "./useTranslation-DX5IRIhk.js";
import "./lazy-with-retry-pSZJrSfN.js";
import "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import "./repo-icon-Dcv6msBx.js";
import { t as AgentKanbanBoard } from "./AgentKanbanBoard-poUnfROE.js";
import "./AgentTerminalDialog-BBP0-dz2.js";
import { pc as recordRendererCrashBreadcrumb, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import { u as AGENT_STATUS_STALE_AFTER_MS } from "./agent-status-3vUKbY6l.js";
import { t as buildAppFontFamily } from "./app-font-family-CyNkxn1D.js";
import { t as require_react_dom } from "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./dropdown-menu-Dth6LPK-.js";
import "./tooltip-DPmd1AoJ.js";
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
import { t as installRendererCrashDiagnostics } from "./crash-diagnostics-DaEtfKCs.js";
import "./use-system-prefers-dark-QSo6mmSW.js";
import "./react-error-boundary-reporting-CkObujra.js";
import "./paste-payload-metadata-pr3nuODB.js";
import { t as RecoverableRenderErrorBoundary } from "./RecoverableRenderErrorBoundary-B5zK7owr.js";
import "./plugin-language-packs-dIIzfC0p.js";
import { t as applyDocumentTheme } from "./document-theme-66WaD9Gm.js";
import "./client-XKKXQWGM.js";
import { t as getOrCreateRendererRoot } from "./react-renderer-root--8UCRBuZ.js";
import { a as EMPTY_DASHBOARD_SNAPSHOT, o as dashboardCardDisplayState } from "./dashboard-snapshot-B9IiTV8p.js";
import "./DashboardHostBadge-DXORvYCI.js";
import "./terminal-shortcut-policy-BOkUsz_T.js";
/* empty css               */
import { t as dashboardBucketForDotState } from "./dashboard-card-bucket-DVJn5pXm.js";
var import_react_dom = require_react_dom();
var import_react = /* @__PURE__ */ __toESM(require_react());
function patchedSubagents(card, event) {
	if (event.subagents === void 0) return card.subagents;
	return event.subagents.map((subagent) => ({
		id: `${card.paneKey}\u0000subagent:${subagent.id}`,
		name: subagent.description || subagent.agentType || "unknown",
		dotState: subagent.state
	}));
}
function patchDashboardSnapshotFromAgentStatus(snapshot, event) {
	if (event.providerSessionOnly) return {
		matched: true,
		snapshot
	};
	const index = snapshot.cards.findIndex((card$1) => card$1.paneKey === event.paneKey);
	if (index === -1) return {
		matched: false,
		snapshot
	};
	const card = snapshot.cards[index];
	if (event.receivedAt <= (card.statusUpdatedAt ?? 0) || event.worktreeId !== void 0 && event.worktreeId !== card.worktreeId) return {
		matched: true,
		snapshot
	};
	const stateChanged = event.stateStartedAt > card.stateChangedAt;
	const unseen = stateChanged ? card.startedAt !== 0 : card.unseen;
	const dotState = event.state;
	const bucket = dashboardBucketForDotState(dashboardCardDisplayState({
		dotState,
		unseen
	}));
	const nextCard = {
		...card,
		...event.agentType ? { agentType: event.agentType } : {},
		...event.prompt ? {
			task: event.prompt,
			lastUserMessage: event.prompt
		} : {},
		...event.lastAssistantMessage !== void 0 ? { lastAgentMessage: event.lastAssistantMessage || void 0 } : {},
		...event.orchestration?.parentPaneKey ? { parentPaneKey: event.orchestration.parentPaneKey } : {},
		bucket,
		dotState,
		unseen,
		stateChangedAt: stateChanged ? event.stateStartedAt : card.stateChangedAt,
		statusUpdatedAt: event.receivedAt,
		finishedAt: dotState === "done" && stateChanged ? event.stateStartedAt : card.finishedAt,
		askSummary: bucket === "attention" ? event.interactivePrompt !== void 0 ? event.interactivePrompt || void 0 : card.askSummary : void 0,
		subagents: patchedSubagents(card, event)
	};
	const cards = snapshot.cards.slice();
	cards[index] = nextCard;
	return {
		matched: true,
		snapshot: {
			...snapshot,
			generatedAt: Math.max(snapshot.generatedAt, event.receivedAt),
			cards
		}
	};
}
var TOPOLOGY_REFRESH_DEBOUNCE_MS = 250;
var TOPOLOGY_REFRESH_MAX_WAIT_MS = 1e3;
function columnSignature(snapshot) {
	return snapshot.cards.map((card) => `${card.paneKey}:${card.bucket}`).sort().join(",");
}
function prefersReducedMotion() {
	return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}
function terminalDialogIsOpen() {
	return document.querySelector("[role=\"dialog\"][data-state=\"open\"]") !== null;
}
function useDashboardSnapshot() {
	const [snapshot, setSnapshot] = (0, import_react.useState)(EMPTY_DASHBOARD_SNAPSHOT);
	const columnSignatureRef = (0, import_react.useRef)("");
	const retainedRepoIconsRef = (0, import_react.useRef)(void 0);
	const snapshotRef = (0, import_react.useRef)(snapshot);
	(0, import_react.useEffect)(() => {
		let topologyRefreshTimer = null;
		let topologyRefreshStartedAt = null;
		let staleRefreshTimer = null;
		const transientClearWatermarks = /* @__PURE__ */ new Map();
		const requestTopologyRefresh = () => {
			if (topologyRefreshTimer) clearTimeout(topologyRefreshTimer);
			const now = Date.now();
			topologyRefreshStartedAt ?? (topologyRefreshStartedAt = now);
			const remainingMaxWait = Math.max(0, TOPOLOGY_REFRESH_MAX_WAIT_MS - (now - topologyRefreshStartedAt));
			topologyRefreshTimer = setTimeout(() => {
				topologyRefreshTimer = null;
				topologyRefreshStartedAt = null;
				window.api.dashboard.requestSnapshot();
			}, Math.min(TOPOLOGY_REFRESH_DEBOUNCE_MS, remainingMaxWait));
		};
		const scheduleStaleRefresh = (next) => {
			if (staleRefreshTimer) {
				clearTimeout(staleRefreshTimer);
				staleRefreshTimer = null;
			}
			let nextDeadline = Number.POSITIVE_INFINITY;
			for (const card of next.cards) if (card.statusUpdatedAt !== void 0 && (card.dotState === "working" || card.dotState === "blocked" || card.dotState === "waiting")) nextDeadline = Math.min(nextDeadline, card.statusUpdatedAt + AGENT_STATUS_STALE_AFTER_MS);
			if (Number.isFinite(nextDeadline)) staleRefreshTimer = setTimeout(requestTopologyRefresh, Math.max(0, nextDeadline - Date.now() + 1));
		};
		const apply = (incoming) => {
			const next = incoming.repoIconsByRepoId === void 0 && retainedRepoIconsRef.current ? {
				...incoming,
				repoIconsByRepoId: retainedRepoIconsRef.current
			} : incoming;
			if (next.repoIconsByRepoId !== void 0) retainedRepoIconsRef.current = next.repoIconsByRepoId;
			snapshotRef.current = next;
			scheduleStaleRefresh(next);
			const nextSignature = columnSignature(next);
			const layoutChanged = nextSignature !== columnSignatureRef.current;
			columnSignatureRef.current = nextSignature;
			const startViewTransition = document.startViewTransition?.bind(document);
			if (!layoutChanged || prefersReducedMotion() || terminalDialogIsOpen() || !startViewTransition) {
				setSnapshot(next);
				return;
			}
			startViewTransition(() => {
				(0, import_react_dom.flushSync)(() => setSnapshot(next));
			});
		};
		const unsubscribe = window.api.dashboard.onSnapshot(apply);
		const unsubscribeStatus = window.api.agentStatus.onSet((event) => {
			if (typeof event.connectionId === "string" && event.receivedAt <= (transientClearWatermarks.get(event.connectionId) ?? -1)) return;
			const result = patchDashboardSnapshotFromAgentStatus(snapshotRef.current, event);
			if (!result.matched) {
				if (snapshotRef.current.generatedAt !== 0) requestTopologyRefresh();
				return;
			}
			if (result.snapshot !== snapshotRef.current) apply(result.snapshot);
		});
		const unsubscribeClear = window.api.agentStatus.onClear((event) => {
			if ("transient" in event && event.transient) transientClearWatermarks.set(event.connectionId, Math.max(transientClearWatermarks.get(event.connectionId) ?? -1, event.clearedAt));
			requestTopologyRefresh();
		});
		window.api.dashboard.requestSnapshot();
		return () => {
			unsubscribe();
			unsubscribeStatus();
			unsubscribeClear();
			if (topologyRefreshTimer) clearTimeout(topologyRefreshTimer);
			if (staleRefreshTimer) clearTimeout(staleRefreshTimer);
		};
	}, []);
	return snapshot;
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function DashboardPopoutRoot(_props) {
	const snapshot = useDashboardSnapshot();
	const [view, setView] = (0, import_react.useState)(() => _props.view === "map" || _props.view === "rings" ? "map" : "board");
	(0, import_react.useEffect)(() => window.api.dashboard.onViewRequested(setView), []);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentKanbanBoard, {
		snapshot,
		initialView: view
	}, view);
}
recordRendererCrashBreadcrumb("popout_bootstrap_started", { dev: false });
installRendererCrashDiagnostics("dashboard-popout");
function applyPopoutAppearance(settings) {
	applyDocumentTheme(settings?.theme ?? "system", { disableTransitions: false });
	document.documentElement.style.setProperty("--app-font-family", buildAppFontFamily(settings?.appFontFamily));
}
var startupSettings = null;
try {
	startupSettings = window.api.settings.getSync();
} catch {}
if (startupSettings) useAppStore.setState({ settings: startupSettings });
applyPopoutAppearance(startupSettings);
var rootElement = document.getElementById("root");
if (!rootElement) {
	recordRendererCrashBreadcrumb("popout_root_missing");
	throw new Error("Pop-out root element not found.");
}
var requestedView = new URLSearchParams(window.location.search).get("view");
function PopoutSettingsSync() {
	const settings = useAppStore((state) => state.settings);
	(0, import_react.useEffect)(() => {
		let disposed = false;
		useAppStore.getState().fetchKeybindings();
		const setSettings = (next) => {
			if (!disposed) useAppStore.setState({ settings: next });
		};
		const offChanged = window.api.settings.onChanged((updates) => {
			const current = useAppStore.getState().settings;
			if (current) setSettings({
				...current,
				...updates
			});
		});
		window.api.settings.get().then(setSettings).catch(() => void 0);
		return () => {
			disposed = true;
			offChanged();
		};
	}, []);
	(0, import_react.useEffect)(() => {
		applyPopoutAppearance(settings);
		if (settings?.theme !== "system") return;
		const media = window.matchMedia("(prefers-color-scheme: dark)");
		const handleChange = () => applyDocumentTheme("system");
		media.addEventListener("change", handleChange);
		return () => media.removeEventListener("change", handleChange);
	}, [settings]);
	return null;
}
function PopoutRoot() {
	useTranslation();
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RecoverableRenderErrorBoundary, {
		boundaryId: "dashboard-popout.root",
		surface: "dashboard-popout",
		title: translate("dashboardPopout.recoverableError.title", "Orca dashboard hit an error."),
		description: translate("dashboardPopout.recoverableError.description", "The dashboard could not finish rendering. Retry to remount it, or reopen it."),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DashboardPopoutRoot, { view: requestedView })
	});
}
getOrCreateRendererRoot(rootElement, void 0).render(/* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_react.StrictMode, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(I18nProvider, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoutSettingsSync, {}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoutRoot, {})] }) }));
recordRendererCrashBreadcrumb("popout_bootstrap_rendered");
