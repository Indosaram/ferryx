import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn, t as Button } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import { t as Bell } from "./bell-D5DYvUTg.js";
import { t as EllipsisVertical } from "./ellipsis-vertical-C91ShGFg.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { r as activateAndRevealWorktree } from "./worktree-activation-BDsaiyMf.js";
import { t as MessageSquareText } from "./message-square-text-DY1JGciX.js";
import { t as Search } from "./search-DK1nVA6d.js";
import { t as SquareTerminal } from "./square-terminal-rgWG-Apn.js";
import { $a as isClipboardTextByteLengthOverLimit, Jc as parsePaneKey, Pm as FLOATING_TERMINAL_WORKTREE_ID, aa as orchestrationLabelsMatchLiveDispatch, ia as isOrcaDispatchPrompt, ra as getAgentRowPrimaryText, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import { a as isExplicitAgentStatusFresh, n as agentTypeToIconAgent, r as formatAgentTypeLabel, u as AGENT_STATUS_STALE_AFTER_MS } from "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { i as DropdownMenuItem, l as DropdownMenuSeparator, m as DropdownMenuTrigger, n as DropdownMenuCheckboxItem, r as DropdownMenuContent, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
import { a as SelectTrigger, n as SelectContent, o as SelectValue, r as SelectItem, t as Select } from "./select-B67U0C6J.js";
import { t as Toggle } from "./toggle-CoxCWEA5.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
import { i as getWorktreeMapFromState, r as getRepoMapFromState } from "./selectors-XOBeaOSb.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./web-session-tabs-sync-CYKZbAxS.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import "./connection-context-BUPsamzR.js";
import { t as migrationUnsupportedToAgentStatusEntry } from "./migration-unsupported-agent-entry-BJ_0rXR-.js";
import "./localized-catalog-DubKHKUR.js";
import { t as activateTabAndFocusPane } from "./activate-tab-and-focus-pane-dvS5VCkm.js";
import { t as useSidebarResize } from "./useSidebarResize-BhlGhEjK.js";
import { n as RepoBadgeMark } from "./RepoBadgeLabel-BMcVlWTu.js";
import "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
import { i as FilledBellIcon } from "./WorktreeCardHelpers-Detnezco.js";
import "./AgentWorkingSpinner-BpnTWNKF.js";
import "./lib-CtirWBBB.js";
import "./lib-D08jHVMa.js";
import "./purify.es-C_rn83UJ.js";
import "./MermaidBlock-gW3wAx0A.js";
import { t as CommentMarkdown_default } from "./CommentMarkdown-bsrexQcY.js";
import { n as agentStateLabel, t as AgentStateDot } from "./AgentStateDot-DFt63YGw.js";
import "./icons-jFAuHbv9.js";
import { t as AgentIcon } from "./agent-catalog-CBF2CV5Q.js";
import { t as formatUiRelativeTime } from "./relative-time-format-BdBnutwN.js";
import { n as setActivityTerminalPortals } from "./activity-terminal-portal-DhEMOZUG.js";
var BellDot = createLucideIcon("bell-dot", [
	["path", {
		d: "M10.268 21a2 2 0 0 0 3.464 0",
		key: "vwvbt9"
	}],
	["path", {
		d: "M11.68 2.009A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673c-.824-.85-1.678-1.731-2.21-3.348",
		key: "xaq59h"
	}],
	["circle", {
		cx: "18",
		cy: "5",
		r: "3",
		key: "gq8acd"
	}]
]);
var import_react = /* @__PURE__ */ __toESM(require_react());
function reconcileActivityPortalThreads(args) {
	const { selectedThread, displayedThread, selectedHasLiveTab, displayedHasLiveTab } = args;
	const displayedIsSelectedTerminal = Boolean(selectedThread && displayedThread && displayedThread.worktree.id === selectedThread.worktree.id && displayedThread.tab.id === selectedThread.tab.id);
	const visibleThread = selectedThread && selectedHasLiveTab ? displayedThread && displayedHasLiveTab && displayedThread.paneKey !== selectedThread.paneKey ? displayedIsSelectedTerminal ? selectedThread : displayedThread : selectedThread : null;
	return {
		displayedIsSelectedTerminal,
		visibleThread,
		stagedThread: selectedThread && selectedHasLiveTab && visibleThread && visibleThread.paneKey !== selectedThread.paneKey && !displayedIsSelectedTerminal ? selectedThread : null
	};
}
function resolveActivityPortalSwap(args) {
	const { selectedThread, selectedHasLiveTab, visibleThread, stagedThread, visiblePortalReady, stagedPortalReady, stagedPortalUnavailable } = args;
	if (!selectedThread || !selectedHasLiveTab) return { kind: "clear" };
	if (stagedThread && (stagedPortalReady || stagedPortalUnavailable)) return {
		kind: "swap-staged",
		paneKey: stagedThread.paneKey
	};
	if (!stagedThread && visibleThread?.paneKey === selectedThread.paneKey && visiblePortalReady) return {
		kind: "settle-visible",
		paneKey: selectedThread.paneKey
	};
	return null;
}
function createActivityPortalChurnBudget(args) {
	const { limit, windowMs, now = () => Date.now() } = args;
	let eventsAt = [];
	const prune = (at) => {
		eventsAt = eventsAt.filter((eventAt) => eventAt > at - windowMs && eventAt <= at);
	};
	return {
		record() {
			const at = now();
			prune(at);
			eventsAt.push(at);
			if (eventsAt.length > limit) eventsAt.shift();
			return eventsAt.length >= limit;
		},
		isSpent() {
			prune(now());
			return eventsAt.length >= limit;
		},
		clear() {
			eventsAt = [];
		}
	};
}
function createActivityPortalReadinessLatch(now = () => Date.now()) {
	let lastStatus = null;
	const flipBudget = createActivityPortalChurnBudget({
		limit: 8,
		windowMs: 500,
		now
	});
	return { next(status) {
		if (status === "ready") {
			lastStatus = status;
			flipBudget.clear();
			return status;
		}
		const flipped = lastStatus !== null && lastStatus !== status;
		lastStatus = status;
		return (flipped ? flipBudget.record() : flipBudget.isSpent()) ? "unavailable" : status;
	} };
}
var TERSE_FOLLOW_UP_PATTERN = /^(yes|no|ok|yep|nope|sure|thanks|thank you|please|proceed|continue|go ahead|lgtm|done|looks good|ok proceed)\.?$/i;
function isTerseAgentFollowUpPrompt(prompt) {
	const trimmed = prompt.trim();
	if (!trimmed) return true;
	if (trimmed.length > 24) return false;
	return TERSE_FOLLOW_UP_PATTERN.test(trimmed);
}
function taskTitleFromPrompt(prompt) {
	if (isOrcaDispatchPrompt(prompt)) return getAgentRowPrimaryText({ prompt }) || null;
	const trimmed = prompt.trim();
	if (!trimmed || isTerseAgentFollowUpPrompt(trimmed)) return null;
	return trimmed;
}
function bestTaskPromptFromHistory(history) {
	let best = null;
	let bestStartedAt = Number.NEGATIVE_INFINITY;
	for (const historyEntry of history) {
		const candidate = taskTitleFromPrompt(historyEntry.prompt);
		if (!candidate) continue;
		if (historyEntry.startedAt >= bestStartedAt) {
			best = candidate;
			bestStartedAt = historyEntry.startedAt;
		}
	}
	return best;
}
function orchestrationLabelForEntry(entry) {
	const label = entry.orchestration?.displayName?.trim() || entry.orchestration?.taskTitle?.trim() || "";
	if (!label) return null;
	if (isOrcaDispatchPrompt(entry.prompt)) return orchestrationLabelsMatchLiveDispatch(entry) ? label : null;
	if (taskTitleFromPrompt(entry.prompt)) return null;
	return label;
}
function getActivityThreadWorkspaceTitle(worktree) {
	const displayName = worktree.displayName?.trim();
	const branch = worktree.branch?.trim();
	if (displayName) return displayName;
	return branch || "Workspace";
}
function getActivityThreadTaskTitle(args) {
	const customTitle = args.tab.customTitle?.trim();
	if (customTitle) return customTitle;
	const orchestrationLabel = orchestrationLabelForEntry(args.entry);
	if (orchestrationLabel) return orchestrationLabel;
	const generatedTitle = args.generatedTitlesEnabled ? args.tab.generatedTitle?.trim() : "";
	if (generatedTitle) return generatedTitle;
	const liveTitle = taskTitleFromPrompt(args.entry.prompt);
	if (liveTitle) return liveTitle;
	const historical = bestTaskPromptFromHistory(args.entry.stateHistory);
	if (historical) return historical;
	const liveTabTitle = args.tab.title?.trim();
	const defaultTabTitle = args.tab.defaultTitle?.trim();
	if (liveTabTitle && liveTabTitle !== defaultTabTitle) return liveTabTitle;
	return defaultTabTitle || liveTabTitle || "Terminal";
}
function isMislabeledUserPrompt(text, entry) {
	const trimmed = text.trim();
	if (!trimmed) return true;
	if (isTerseAgentFollowUpPrompt(trimmed)) return true;
	if (trimmed === entry.prompt.trim()) return true;
	return false;
}
function getActivityThreadStatusPreview(entry, agentState) {
	if (entry.interrupted === true) return "Interrupted by user";
	if ((agentState ?? entry.state) === "working") {
		const toolName = entry.toolName?.trim() ?? "";
		const toolInput = entry.toolInput?.trim() ?? "";
		if (toolName && toolInput) return `${toolName}: ${toolInput}`;
		if (toolName) return toolName;
	}
	const assistant = entry.lastAssistantMessage?.trim() ?? "";
	if (assistant && !isMislabeledUserPrompt(assistant, entry)) return assistant;
	return "";
}
function resolveActivityThreadStatusPreview(entry, agentState, previousPreview) {
	const next = getActivityThreadStatusPreview(entry, agentState);
	if (next) return next;
	if (!isTerseAgentFollowUpPrompt(entry.prompt)) return "";
	const previous = previousPreview?.trim() ?? "";
	if (previous && !isMislabeledUserPrompt(previous, entry)) return previous;
	return "";
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var ACTIVITY_TERMINAL_LOADING_LABEL_DELAY_MS = 180;
var ACTIVITY_THREAD_RESPONSE_RENDER_PREVIEW_MAX_LENGTH = 320;
var ACTIVITY_STATUS_GROUP_ORDER = [
	"working",
	"blocked",
	"waiting",
	"done",
	"interrupted"
];
var STANDALONE_ACTIVITY_WORKTREE_REPO_ID = "__activity_standalone__";
var absoluteDateFormatter = new Intl.DateTimeFormat(void 0, {
	year: "numeric",
	month: "short",
	day: "numeric",
	hour: "numeric",
	minute: "2-digit"
});
function formatAbsoluteDate(timestamp) {
	return absoluteDateFormatter.format(new Date(timestamp));
}
function formatRelativeTime(timestamp) {
	return formatUiRelativeTime(timestamp - Date.now());
}
function findActivityTerminalPane(root, leafId) {
	let foundAnyPane = false;
	for (const candidate of root.querySelectorAll("[data-leaf-id]")) {
		foundAnyPane = true;
		if (candidate.dataset.leafId === leafId) return {
			foundAnyPane,
			pane: candidate
		};
	}
	return {
		foundAnyPane,
		pane: null
	};
}
function hasInlineDisplayNoneBetween(element, root) {
	let current = element;
	while (current) {
		if (current.style.display === "none") return true;
		if (current === root) return false;
		current = current.parentElement;
	}
	return false;
}
function hasUnhiddenSiblingPane(root, selectedPane) {
	for (const candidate of root.querySelectorAll("[data-leaf-id]")) if (candidate !== selectedPane && !hasInlineDisplayNoneBetween(candidate, root)) return true;
	return false;
}
function truncatePreservingSurrogates(value, maxLength) {
	if (value.length <= maxLength) return value;
	const truncated = value.slice(0, maxLength);
	const lastCode = truncated.charCodeAt(truncated.length - 1);
	if (lastCode >= 55296 && lastCode <= 56319) return truncated.slice(0, -1);
	return truncated;
}
function activityThreadResponseRenderPreview({ responsePreview }) {
	const trimmed = responsePreview.trim();
	if (trimmed.length <= ACTIVITY_THREAD_RESPONSE_RENDER_PREVIEW_MAX_LENGTH) return trimmed;
	return `${truncatePreservingSurrogates(trimmed, ACTIVITY_THREAD_RESPONSE_RENDER_PREVIEW_MAX_LENGTH).trimEnd()}...`;
}
function getSelectedActivityTerminalPortalStatus(target, paneKey) {
	const parsed = parsePaneKey(paneKey);
	if (!parsed) return {
		ready: false,
		unavailable: true
	};
	let selectedRoot = null;
	for (const candidate of target.querySelectorAll("[data-terminal-tab-id]")) if (candidate.dataset.terminalTabId === parsed.tabId) {
		selectedRoot = candidate;
		break;
	}
	if (!selectedRoot) return {
		ready: false,
		unavailable: false
	};
	const { foundAnyPane, pane: selectedPane } = findActivityTerminalPane(selectedRoot, parsed.leafId);
	if (!selectedPane) return {
		ready: false,
		unavailable: foundAnyPane
	};
	const unavailable = hasInlineDisplayNoneBetween(selectedPane, selectedRoot);
	const hasUnisolatedSibling = hasUnhiddenSiblingPane(selectedRoot, selectedPane);
	const isVisibleRoot = !unavailable && (selectedPane.offsetParent !== null || selectedPane.getClientRects().length > 0);
	const hasPtyBinding = selectedPane.hasAttribute("data-pty-id") || selectedPane.querySelector("[data-pty-id]") !== null;
	const hasXtermScreen = selectedPane.querySelector(".xterm-screen") !== null;
	return {
		ready: isVisibleRoot && !hasUnisolatedSibling && hasPtyBinding && hasXtermScreen,
		unavailable
	};
}
function useActivityTerminalPortalStatus(target, paneKey, forceUnavailable = false) {
	const [readiness, setReadiness] = (0, import_react.useState)({
		target: null,
		paneKey: null,
		status: "loading"
	});
	const readinessLatchRef = (0, import_react.useRef)(null);
	(0, import_react.useLayoutEffect)(() => {
		let disposed = false;
		let readinessFrame = null;
		let readinessReleaseTimer = null;
		let pendingStatus = null;
		const scheduleReadiness = (status) => {
			if (disposed) return;
			pendingStatus = status;
			if (readinessFrame !== null) return;
			readinessFrame = requestAnimationFrame(() => {
				readinessFrame = null;
				const nextStatus = pendingStatus;
				pendingStatus = null;
				if (disposed || nextStatus === null) return;
				setReadiness((prev) => prev.target === target && prev.paneKey === paneKey && prev.status === nextStatus ? prev : {
					target,
					paneKey,
					status: nextStatus
				});
			});
		};
		const disposeFrame = () => {
			disposed = true;
			if (readinessFrame !== null) {
				cancelAnimationFrame(readinessFrame);
				readinessFrame = null;
			}
			if (readinessReleaseTimer !== null) {
				window.clearTimeout(readinessReleaseTimer);
				readinessReleaseTimer = null;
			}
		};
		if (!target || !paneKey) {
			scheduleReadiness("loading");
			return disposeFrame;
		}
		if (forceUnavailable) {
			scheduleReadiness("unavailable");
			return disposeFrame;
		}
		const readinessLatch = readinessLatchRef.current ?? (readinessLatchRef.current = createActivityPortalReadinessLatch());
		const updateReadiness = (status) => {
			const nextStatus = readinessLatch.next(status);
			scheduleReadiness(nextStatus);
			if (readinessReleaseTimer !== null) {
				window.clearTimeout(readinessReleaseTimer);
				readinessReleaseTimer = null;
			}
			if (nextStatus !== status) readinessReleaseTimer = window.setTimeout(checkReadiness, 500);
		};
		const checkReadiness = () => {
			const status = getSelectedActivityTerminalPortalStatus(target, paneKey);
			if (status.unavailable) {
				updateReadiness("unavailable");
				return;
			}
			if (status.ready) {
				updateReadiness("ready");
				return;
			}
			updateReadiness("loading");
		};
		checkReadiness();
		const observer = new MutationObserver(checkReadiness);
		observer.observe(target, {
			childList: true,
			subtree: true,
			attributes: true,
			attributeFilter: [
				"data-terminal-tab-id",
				"data-leaf-id",
				"data-pty-id",
				"style"
			]
		});
		return () => {
			disposeFrame();
			observer.disconnect();
		};
	}, [
		target,
		paneKey,
		forceUnavailable
	]);
	return readiness.target === target && readiness.paneKey === paneKey ? readiness.status : "loading";
}
function otherActivityTerminalSlot(slotId) {
	return slotId === "primary" ? "secondary" : "primary";
}
function useActivityTerminalLoadingLabel(loading) {
	const [visible, setVisible] = (0, import_react.useState)(false);
	const [visibleLoading, setVisibleLoading] = (0, import_react.useState)(loading);
	if (visibleLoading !== loading) {
		setVisibleLoading(loading);
		if (visible) setVisible(false);
	}
	(0, import_react.useEffect)(() => {
		if (!loading) return;
		const timer = setTimeout(() => setVisible(true), ACTIVITY_TERMINAL_LOADING_LABEL_DELAY_MS);
		return () => clearTimeout(timer);
	}, [loading]);
	return loading && visible;
}
function agentTitle(event) {
	if (event.state === "done") return event.entry.interrupted ? "Agent interrupted" : "Agent finished";
	return event.state === "waiting" ? "Agent waiting for input" : "Agent needs input";
}
function agentSummary(event) {
	const prompt = getAgentRowPrimaryText(event.entry);
	if (event.state === "done") return event.entry.lastAssistantMessage?.trim() || prompt || "Completed the current turn.";
	return prompt || event.entry.lastAssistantMessage?.trim() || "The agent paused for user input.";
}
function agentMeta(event) {
	const agent = formatAgentTypeLabel(event.agentType);
	if (event.state === "done") return event.entry.interrupted ? `${agent} interrupted` : `${agent} completed`;
	return event.state === "waiting" ? `${agent} waiting` : `${agent} blocked`;
}
function paneTitleForEntry(entry, tab, generatedTitlesEnabled) {
	return getActivityThreadTaskTitle({
		entry,
		tab,
		generatedTitlesEnabled
	});
}
function paneTitleForEvent(event, generatedTitlesEnabled) {
	return paneTitleForEntry(event.entry, event.tab, generatedTitlesEnabled);
}
function statusPreviewForEntry(entry, agentState, previousPreview) {
	return resolveActivityThreadStatusPreview(entry, agentState, previousPreview);
}
function isActivityEventState(state) {
	return state === "done" || state === "blocked" || state === "waiting";
}
function isActivityLiveAgentState(state) {
	return state === "working" || state === "blocked" || state === "waiting";
}
function freshActivityLiveAgentState(entry, now) {
	if (!isActivityLiveAgentState(entry.state)) return null;
	return isExplicitAgentStatusFresh(entry, now, 18e5) ? entry.state : null;
}
function standaloneActivityWorktree(worktreeId) {
	const displayName = worktreeId === "global-floating-terminal" ? "Floating terminal" : "Standalone terminal";
	return {
		id: worktreeId,
		repoId: STANDALONE_ACTIVITY_WORKTREE_REPO_ID,
		path: "",
		head: "",
		branch: displayName,
		isBare: false,
		isMainWorktree: false,
		displayName,
		comment: "",
		linkedIssue: null,
		linkedPR: null,
		linkedLinearIssue: null,
		isArchived: false,
		isUnread: false,
		isPinned: false,
		sortOrder: 0,
		lastActivityAt: 0
	};
}
var EVENTS_PER_PANE_CAP = 5;
function historyEntrySnapshot(entry, history) {
	return {
		...entry,
		state: history.state,
		prompt: history.prompt,
		updatedAt: history.startedAt,
		stateStartedAt: history.startedAt,
		stateHistory: [],
		toolName: void 0,
		toolInput: void 0,
		lastAssistantMessage: void 0,
		interrupted: history.interrupted
	};
}
function appendActivityEvent(args) {
	const id = `agent:${args.entry.paneKey}:${args.state}:${args.timestamp}`;
	if (args.seenEventIds.has(id)) return;
	args.seenEventIds.add(id);
	args.events.push({
		id,
		state: args.state,
		timestamp: args.timestamp,
		worktree: args.worktree,
		repo: args.repo,
		entry: args.entry,
		tab: args.tab,
		agentType: args.agentType,
		agentAlive: args.agentAlive,
		migrationUnsupportedPtyId: args.migrationUnsupportedPtyId,
		unread: args.acknowledgedAt < args.timestamp
	});
}
function appendActivityEventsForEntry(args) {
	for (const history of args.entry.stateHistory) {
		if (!isActivityEventState(history.state)) continue;
		appendActivityEvent({
			...args,
			state: history.state,
			timestamp: history.startedAt,
			entry: historyEntrySnapshot(args.entry, history)
		});
	}
	if (!isActivityEventState(args.entry.state) || args.entry.sessionBoundary === true) return;
	appendActivityEvent({
		...args,
		state: args.entry.state,
		timestamp: args.entry.stateStartedAt
	});
}
function buildActivityEvents(args) {
	const events = [];
	const seenEventIds = /* @__PURE__ */ new Set();
	const tabContext = /* @__PURE__ */ new Map();
	const liveAgentByPaneKey = {};
	for (const [worktreeId, tabs] of Object.entries(args.tabsByWorktree)) {
		const worktree = args.worktreeMap.get(worktreeId) ?? standaloneActivityWorktree(worktreeId);
		for (const tab of tabs) tabContext.set(tab.id, {
			worktree,
			tab
		});
	}
	for (const [paneKey, entry] of Object.entries(args.agentStatusByPaneKey)) {
		const parsed = parsePaneKey(paneKey);
		if (!parsed) continue;
		const context = tabContext.get(parsed.tabId);
		if (!context) continue;
		const ackAt = args.acknowledgedAgentsByPaneKey[paneKey] ?? 0;
		const liveState = freshActivityLiveAgentState(entry, args.now);
		if (liveState) liveAgentByPaneKey[paneKey] = {
			state: liveState,
			timestamp: entry.stateStartedAt,
			worktree: context.worktree,
			repo: args.repoMap.get(context.worktree.repoId) ?? null,
			entry,
			tab: context.tab,
			agentType: entry.agentType ?? "unknown"
		};
		appendActivityEventsForEntry({
			events,
			seenEventIds,
			worktree: context.worktree,
			repo: args.repoMap.get(context.worktree.repoId) ?? null,
			entry,
			tab: context.tab,
			agentType: entry.agentType ?? "unknown",
			agentAlive: true,
			acknowledgedAt: ackAt
		});
	}
	for (const unsupported of Object.values(args.migrationUnsupportedByPtyId ?? {})) {
		const entry = migrationUnsupportedToAgentStatusEntry(unsupported);
		if (!entry) continue;
		const parsed = parsePaneKey(entry.paneKey);
		if (!parsed) continue;
		const context = tabContext.get(parsed.tabId);
		if (!context) continue;
		const ackAt = args.acknowledgedAgentsByPaneKey[entry.paneKey] ?? 0;
		liveAgentByPaneKey[entry.paneKey] = {
			state: "blocked",
			timestamp: entry.stateStartedAt,
			worktree: context.worktree,
			repo: args.repoMap.get(context.worktree.repoId) ?? null,
			entry,
			tab: context.tab,
			agentType: entry.agentType ?? "unknown"
		};
		appendActivityEventsForEntry({
			events,
			seenEventIds,
			worktree: context.worktree,
			repo: args.repoMap.get(context.worktree.repoId) ?? null,
			entry,
			tab: context.tab,
			agentType: entry.agentType ?? "unknown",
			agentAlive: false,
			acknowledgedAt: ackAt,
			migrationUnsupportedPtyId: unsupported.ptyId
		});
	}
	for (const [paneKey, retained] of Object.entries(args.retainedAgentsByPaneKey)) {
		if (!parsePaneKey(paneKey)) continue;
		const worktree = args.worktreeMap.get(retained.worktreeId) ?? (args.tabsByWorktree[retained.worktreeId] ? standaloneActivityWorktree(retained.worktreeId) : null);
		if (!worktree) continue;
		const ackAt = args.acknowledgedAgentsByPaneKey[paneKey] ?? 0;
		appendActivityEventsForEntry({
			events,
			seenEventIds,
			worktree,
			repo: args.repoMap.get(worktree.repoId) ?? null,
			entry: retained.entry,
			tab: retained.tab,
			agentType: retained.agentType,
			agentAlive: false,
			acknowledgedAt: ackAt
		});
	}
	const sorted = events.sort((a, b) => b.timestamp - a.timestamp);
	const perPaneCount = /* @__PURE__ */ new Map();
	const includedEventIds = /* @__PURE__ */ new Set();
	const capped = [];
	for (const event of sorted) {
		const paneKey = event.entry.paneKey;
		if (perPaneCount.has(paneKey)) continue;
		if (capped.length >= 80) break;
		perPaneCount.set(paneKey, 1);
		includedEventIds.add(event.id);
		capped.push(event);
	}
	for (const event of sorted) {
		if (includedEventIds.has(event.id)) continue;
		if (capped.length >= 80) break;
		const paneKey = event.entry.paneKey;
		const count = perPaneCount.get(paneKey) ?? 0;
		if (count >= EVENTS_PER_PANE_CAP) continue;
		perPaneCount.set(paneKey, count + 1);
		includedEventIds.add(event.id);
		capped.push(event);
	}
	return {
		events: capped.sort((a, b) => b.timestamp - a.timestamp),
		liveAgentByPaneKey
	};
}
function buildAgentPaneThreads(args) {
	const generatedTitlesEnabled = args.generatedTitlesEnabled === true;
	const byPaneKey = /* @__PURE__ */ new Map();
	for (const event of args.events) {
		const paneKey = event.entry.paneKey;
		const existing = byPaneKey.get(paneKey);
		if (!existing) {
			byPaneKey.set(paneKey, {
				paneKey,
				paneTitle: paneTitleForEvent(event, generatedTitlesEnabled),
				worktree: event.worktree,
				repo: event.repo,
				tab: event.tab,
				agentType: event.agentType,
				currentAgentState: null,
				currentAgentEntry: null,
				responsePreview: statusPreviewForEntry(event.entry, event.state),
				latestTimestamp: event.timestamp,
				latestEvent: event,
				events: [event],
				migrationUnsupportedPtyId: event.migrationUnsupportedPtyId,
				unread: event.unread
			});
			continue;
		}
		existing.events.push(event);
		existing.unread = existing.unread || event.unread;
		existing.migrationUnsupportedPtyId = existing.migrationUnsupportedPtyId ?? event.migrationUnsupportedPtyId;
		if (!existing.latestEvent || event.timestamp > existing.latestEvent.timestamp) {
			existing.latestEvent = event;
			existing.paneTitle = paneTitleForEvent(event, generatedTitlesEnabled);
			existing.agentType = event.agentType;
			existing.tab = event.tab;
			existing.responsePreview = statusPreviewForEntry(event.entry, event.state, existing.responsePreview);
			existing.latestTimestamp = event.timestamp;
		}
	}
	for (const [paneKey, liveAgent] of Object.entries(args.liveAgentByPaneKey)) {
		const existing = byPaneKey.get(paneKey);
		if (!existing) {
			byPaneKey.set(paneKey, {
				paneKey,
				paneTitle: paneTitleForEntry(liveAgent.entry, liveAgent.tab, generatedTitlesEnabled),
				worktree: liveAgent.worktree,
				repo: liveAgent.repo,
				tab: liveAgent.tab,
				agentType: liveAgent.agentType,
				currentAgentState: liveAgent.state,
				currentAgentEntry: liveAgent.entry,
				responsePreview: statusPreviewForEntry(liveAgent.entry, liveAgent.state),
				latestTimestamp: liveAgent.timestamp,
				latestEvent: null,
				events: [],
				unread: false
			});
			continue;
		}
		existing.paneTitle = paneTitleForEntry(liveAgent.entry, liveAgent.tab, generatedTitlesEnabled);
		existing.worktree = liveAgent.worktree;
		existing.repo = liveAgent.repo;
		existing.tab = liveAgent.tab;
		existing.agentType = liveAgent.agentType;
		existing.currentAgentState = liveAgent.state;
		existing.currentAgentEntry = liveAgent.entry;
		existing.responsePreview = statusPreviewForEntry(liveAgent.entry, liveAgent.state, existing.responsePreview);
		existing.latestTimestamp = liveAgent.timestamp;
	}
	return Array.from(byPaneKey.values()).map((thread) => ({
		...thread,
		events: [...thread.events].sort((a, b) => b.timestamp - a.timestamp)
	})).sort((a, b) => b.latestTimestamp - a.latestTimestamp);
}
function EventTime({ timestamp }) {
	const absolute = formatAbsoluteDate(timestamp);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
			type: "button",
			className: "rounded px-1 py-0.5 text-xs text-muted-foreground hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none",
			"aria-label": absolute,
			onClick: (event) => event.stopPropagation(),
			children: formatRelativeTime(timestamp)
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "right",
		sideOffset: 6,
		children: absolute
	})] });
}
function ActivityThreadOptionsMenu({ compactMode, hasUnreadThreads, onCompactModeChange, onMarkAllThreadsRead }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "inline-flex shrink-0",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "outline",
					size: "sm",
					className: "size-8 shrink-0 border-input bg-transparent p-0 text-muted-foreground shadow-xs hover:bg-accent hover:text-accent-foreground dark:bg-transparent dark:hover:bg-accent dark:hover:text-accent-foreground",
					"aria-label": translate("auto.components.activity.ActivityPrototypePage.db8a1878b5", "Thread list options"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EllipsisVertical, { className: "size-3.5" })
				})
			})
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "bottom",
		children: translate("auto.components.activity.ActivityPrototypePage.a472a14700", "More options")
	})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuContent, {
		align: "end",
		sideOffset: 6,
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuCheckboxItem, {
				checked: compactMode,
				onCheckedChange: (checked) => onCompactModeChange(checked === true),
				onSelect: (event) => event.preventDefault(),
				children: translate("auto.components.activity.ActivityPrototypePage.f70e4bec47", "Compact mode")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuItem, {
				onSelect: onMarkAllThreadsRead,
				disabled: !hasUnreadThreads,
				children: translate("auto.components.activity.ActivityPrototypePage.023ff75afe", "Mark all read")
			})
		]
	})] });
}
function ActivityProjectLabel({ repo }) {
	const label = repo?.displayName?.trim() || translate("auto.components.activity.ActivityPrototypePage.5651b216c6", "Unknown project");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-w-0 items-center gap-1.5",
		children: [repo ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RepoBadgeMark, { color: repo.badgeColor }) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "min-w-0 truncate text-[11px] font-semibold uppercase tracking-[0.04em] text-muted-foreground",
			title: label,
			children: label
		})]
	});
}
function EventRepoBadge({ repo }) {
	if (!repo) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-w-0 shrink-0 items-center gap-1.5 rounded-[4px] border border-border bg-accent px-1.5 py-0.5 dark:border-border/60 dark:bg-accent/50",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RepoBadgeMark, { color: repo.badgeColor }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "max-w-[6rem] truncate text-[10px] font-semibold leading-none text-foreground lowercase",
			children: repo.displayName
		})]
	});
}
function threadAgentState(thread) {
	return thread.currentAgentState ?? thread.latestEvent?.state ?? "done";
}
function threadAgentStateLabel(thread) {
	const state = threadAgentState(thread);
	if (!thread.currentAgentState && state === "done" && thread.latestEvent?.entry.interrupted) return "Interrupted";
	return agentStateLabel(state);
}
function getActivityThreadGroup(thread, groupBy) {
	if (groupBy === "status") {
		const state = threadAgentState(thread);
		if (!thread.currentAgentState && state === "done" && thread.latestEvent?.entry.interrupted) return {
			key: "done:interrupted",
			label: threadAgentStateLabel(thread)
		};
		return {
			key: state,
			label: threadAgentStateLabel(thread)
		};
	}
	if (groupBy === "project") return thread.repo ? {
		key: `project:${thread.repo.id}`,
		label: thread.repo.displayName
	} : {
		key: "project:unknown",
		label: translate("auto.components.activity.ActivityPrototypePage.5651b216c6", "Unknown project")
	};
	if (groupBy === "worktree") return {
		key: `worktree:${thread.worktree.id}`,
		label: thread.worktree.displayName
	};
	return {
		key: `agent:${thread.agentType}`,
		label: formatAgentTypeLabel(thread.agentType)
	};
}
function buildActivityThreadGroups(threads, groupBy) {
	const groups = [];
	const groupIndexByKey = /* @__PURE__ */ new Map();
	for (const thread of threads) {
		const group = getActivityThreadGroup(thread, groupBy);
		const existingIndex = groupIndexByKey.get(group.key);
		if (existingIndex === void 0) {
			groups.push({
				key: group.key,
				label: group.label,
				threads: [thread]
			});
			groupIndexByKey.set(group.key, groups.length - 1);
			continue;
		}
		groups[existingIndex].threads.push(thread);
	}
	return groups;
}
function threadStatusGroupId(thread) {
	const state = threadAgentState(thread);
	if (!thread.currentAgentState && state === "done" && thread.latestEvent?.entry.interrupted) return "interrupted";
	return state === "working" || state === "blocked" || state === "waiting" ? state : "done";
}
function threadStatusGroupState(id) {
	return id === "interrupted" ? "done" : id;
}
function threadStatusGroupLabel(id) {
	if (id === "interrupted") return "Interrupted";
	return agentStateLabel(threadStatusGroupState(id));
}
function groupActivityThreadsByStatus(threads) {
	const groups = /* @__PURE__ */ new Map();
	for (const thread of threads) {
		const groupId = threadStatusGroupId(thread);
		groups.set(groupId, [...groups.get(groupId) ?? [], thread]);
	}
	return ACTIVITY_STATUS_GROUP_ORDER.flatMap((id) => {
		const groupThreads = groups.get(id) ?? [];
		if (groupThreads.length === 0) return [];
		return [{
			key: id,
			id,
			label: threadStatusGroupLabel(id),
			state: threadStatusGroupState(id),
			threads: groupThreads
		}];
	});
}
function threadSearchText(thread) {
	const latest = thread.latestEvent;
	const stateLabel = threadAgentStateLabel(thread);
	const currentPrompt = thread.currentAgentEntry ? getAgentRowPrimaryText(thread.currentAgentEntry) : "";
	const rawCurrentPrompt = thread.currentAgentEntry?.prompt.trim() ?? "";
	const currentSummary = thread.currentAgentEntry?.lastAssistantMessage?.trim() ?? "";
	const latestEventText = latest ? `${agentTitle(latest)} ${agentSummary(latest)} ${agentMeta(latest)}` : "";
	return `${thread.paneTitle} ${getActivityThreadWorkspaceTitle(thread.worktree)} ${thread.worktree.branch ?? ""} ${thread.repo?.displayName ?? ""} ${formatAgentTypeLabel(thread.agentType)} ${stateLabel} ${currentPrompt} ${rawCurrentPrompt} ${currentSummary} ${thread.responsePreview} ${latestEventText}`.toLowerCase();
}
const ACTIVITY_SEARCH_QUERY_MAX_BYTES = 2 * 1024;
function isActivitySearchQueryTooLarge(query, maxBytes = ACTIVITY_SEARCH_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function activityThreadMatchesSearchQuery({ thread, searchQuery }) {
	if (isActivitySearchQueryTooLarge(searchQuery)) return false;
	const trimmedQuery = searchQuery.trim();
	if (!trimmedQuery) return true;
	return threadSearchText(thread).includes(trimmedQuery.toLowerCase());
}
function isActivityFilterFocusShortcut(event, isMac = navigator.userAgent.includes("Mac")) {
	if (event.key.toLowerCase() !== "f" || event.shiftKey || event.altKey) return false;
	return isMac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
}
function shouldIgnoreActivityFilterFocusShortcutTarget(target, terminalPortalTargets) {
	if (!target) return false;
	return terminalPortalTargets.some((portalTarget) => portalTarget?.contains(target) ?? false);
}
function handleActivityFilterFocusShortcut({ activeElement, event, input, isMac, terminalPortalTargets }) {
	if (shouldIgnoreActivityFilterFocusShortcutTarget(activeElement, terminalPortalTargets)) return false;
	if (!isActivityFilterFocusShortcut(event, isMac)) return false;
	if (!input) return false;
	event.preventDefault();
	event.stopPropagation();
	event.stopImmediatePropagation();
	input.focus();
	input.select();
	return true;
}
function ThreadAgentStateIndicator({ thread }) {
	const state = threadAgentState(thread);
	const label = threadAgentStateLabel(thread);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "inline-flex size-4 shrink-0 items-center justify-center",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentStateDot, {
				state,
				size: "md"
			})
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "top",
		sideOffset: 4,
		children: label
	})] });
}
function ActivityStatusGroupHeader({ group }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "sticky top-0 z-10 flex items-center gap-2 border-b border-border bg-background/95 px-3 py-1.5 backdrop-blur supports-[backdrop-filter]:bg-background/80",
		children: [
			group.state ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "inline-flex size-4 shrink-0 items-center justify-center",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentStateDot, {
					state: group.state,
					size: "sm"
				})
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-0 flex-1 truncate text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground",
				children: group.label
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "rounded-full border border-border bg-accent px-1.5 py-0.5 text-[10px] font-semibold leading-none text-muted-foreground",
				children: group.threads.length
			})
		]
	});
}
function isEventFromNestedInteractiveElement(target, currentTarget) {
	if (!(target instanceof HTMLElement)) return false;
	const interactiveTarget = target.closest("a, button, input, select, textarea, [role=\"button\"], [role=\"link\"], [tabindex]:not([tabindex=\"-1\"])");
	return interactiveTarget instanceof HTMLElement && interactiveTarget !== currentTarget && currentTarget.contains(interactiveTarget);
}
function ThreadRow({ thread, selected, onSelect, onJump, onMarkUnread, canJump, compactMode }) {
	const renderedResponsePreview = activityThreadResponseRenderPreview({ responsePreview: thread.responsePreview });
	const workspaceTitle = getActivityThreadWorkspaceTitle(thread.worktree);
	const taskTitle = thread.paneTitle;
	const agentLabel = formatAgentTypeLabel(thread.agentType);
	const showStatusPreview = !compactMode && renderedResponsePreview.length > 0 && renderedResponsePreview !== taskTitle && renderedResponsePreview !== workspaceTitle;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		"data-current": selected ? "true" : void 0,
		onClick: onSelect,
		role: "button",
		tabIndex: 0,
		onKeyDown: (event) => {
			if (isEventFromNestedInteractiveElement(event.target, event.currentTarget)) return;
			if (event.key === "Enter" || event.key === " ") {
				event.preventDefault();
				onSelect();
			}
		},
		className: cn("group relative flex w-full cursor-pointer flex-col gap-1 border-b border-border px-3 pt-2.5 pb-3 text-left transition-colors", selected ? "bg-black/[0.08] shadow-[0_1px_2px_rgba(0,0,0,0.04)] dark:bg-white/[0.10] dark:shadow-[0_1px_2px_rgba(0,0,0,0.03)]" : "hover:bg-accent/40"),
		children: [thread.unread ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "absolute left-0 top-1.5 bottom-1.5 w-0.5 rounded-r-full bg-primary" }) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex min-w-0 items-start gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
				className: "inline-flex shrink-0 items-start gap-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ThreadAgentStateIndicator, { thread }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "inline-flex shrink-0 pt-px",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
						agent: agentTypeToIconAgent(thread.agentType),
						size: 14
					})
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "min-w-0 flex-1",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex min-w-0 items-start gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0 flex-1 space-y-0.5",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ActivityProjectLabel, { repo: thread.repo }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: cn("min-w-0 text-[13px] leading-snug", compactMode ? "truncate" : "line-clamp-2 break-words", thread.unread ? "font-semibold text-foreground" : "font-medium text-foreground"),
								title: workspaceTitle,
								children: workspaceTitle
							}),
							taskTitle !== workspaceTitle ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: cn("min-w-0 text-[12px] leading-snug text-muted-foreground", compactMode ? "truncate" : "line-clamp-2 break-words"),
								title: taskTitle,
								children: taskTitle
							}) : null,
							showStatusPreview ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommentMarkdown_default, {
								content: renderedResponsePreview,
								className: cn("h-[1lh] min-w-0 overflow-hidden truncate whitespace-nowrap text-[11px] font-normal leading-snug text-muted-foreground/80", "[&_*]:inline [&_*]:!m-0 [&_*]:!p-0 [&_*]:!whitespace-nowrap [&_br]:hidden [&_ol]:list-none [&_ul]:list-none"),
								title: thread.responsePreview
							}) : null,
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex min-w-0 items-center gap-1.5 pt-0.5",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "shrink-0 text-[10px] text-muted-foreground/80",
									children: agentLabel
								}), canJump ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: cn("ml-auto inline-flex shrink-0 items-center transition-opacity", "can-hover:pointer-events-none can-hover:invisible can-hover:opacity-0", "group-hover:pointer-events-auto group-hover:visible group-hover:opacity-100"),
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
										asChild: true,
										children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
											type: "button",
											variant: "outline",
											size: "icon-xs",
											"aria-label": translate("auto.components.activity.ActivityPrototypePage.4616ea39fd", "Jump to workspace"),
											onClick: (event) => {
												event.stopPropagation();
												onJump();
											},
											onMouseDown: (event) => event.stopPropagation(),
											children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3" })
										})
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
										side: "left",
										children: translate("auto.components.activity.ActivityPrototypePage.4616ea39fd", "Jump to workspace")
									})] })
								}) : null]
							})
						]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "inline-flex shrink-0 items-center gap-1.5 pt-px",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "inline-flex size-4 shrink-0 items-center justify-center",
							children: thread.unread ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FilledBellIcon, {
								className: "size-[13px] shrink-0 text-amber-500 drop-shadow-sm",
								"aria-label": translate("auto.components.activity.ActivityPrototypePage.beb2c19173", "Unread")
							}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
								asChild: true,
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
									type: "button",
									onClick: (event) => {
										event.stopPropagation();
										onMarkUnread();
									},
									onMouseDown: (event) => event.stopPropagation(),
									className: cn("group/unread flex size-4 shrink-0 cursor-pointer items-center justify-center rounded transition-all", "hover:bg-accent/80 active:scale-95", "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"),
									"aria-label": translate("auto.components.activity.ActivityPrototypePage.59b131fbd9", "Mark thread unread"),
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Bell, { className: "size-3 text-muted-foreground/40 can-hover:opacity-0 transition-opacity group-hover:opacity-100 group-hover/unread:opacity-100" })
								})
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
								side: "left",
								children: translate("auto.components.activity.ActivityPrototypePage.59b131fbd9", "Mark thread unread")
							})] })
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EventTime, { timestamp: thread.latestTimestamp })]
					})]
				})
			})]
		})]
	});
}
function ActivityPrototypePage() {
	const [readFilter, setReadFilter] = (0, import_react.useState)("all");
	const [groupBy, setGroupBy] = (0, import_react.useState)("status");
	const [query, setQuery] = (0, import_react.useState)("");
	const activityFilterInputRef = (0, import_react.useRef)(null);
	const [compactMode, setCompactMode] = (0, import_react.useState)(false);
	const [selectedPaneKey, setSelectedPaneKey] = (0, import_react.useState)(null);
	const [displayedPaneKey, setDisplayedPaneKey] = (0, import_react.useState)(null);
	const [activePortalSlotId, setActivePortalSlotId] = (0, import_react.useState)("primary");
	const [primaryPortalTargetEl, setPrimaryPortalTargetEl] = (0, import_react.useState)(null);
	const [secondaryPortalTargetEl, setSecondaryPortalTargetEl] = (0, import_react.useState)(null);
	const [threadListWidth, setThreadListWidth] = (0, import_react.useState)(480);
	const { containerRef: threadListRef, isResizing: isThreadListResizing, onResizeStart } = useSidebarResize({
		isOpen: true,
		width: threadListWidth,
		minWidth: 320,
		maxWidth: 720,
		deltaSign: 1,
		setWidth: setThreadListWidth
	});
	const storeData = useAppStore(useShallow((s) => ({
		agentStatusByPaneKey: s.agentStatusByPaneKey,
		migrationUnsupportedByPtyId: s.migrationUnsupportedByPtyId,
		retainedAgentsByPaneKey: s.retainedAgentsByPaneKey,
		tabsByWorktree: s.tabsByWorktree,
		worktreeMap: getWorktreeMapFromState(s),
		repoMap: getRepoMapFromState(s),
		acknowledgedAgentsByPaneKey: s.acknowledgedAgentsByPaneKey,
		acknowledgeAgents: s.acknowledgeAgents,
		unacknowledgeAgents: s.unacknowledgeAgents,
		generatedTitlesEnabled: s.settings?.tabAutoGenerateTitle === true
	})));
	const { events: allEvents, liveAgentByPaneKey } = (0, import_react.useMemo)(() => buildActivityEvents({
		agentStatusByPaneKey: storeData.agentStatusByPaneKey,
		migrationUnsupportedByPtyId: storeData.migrationUnsupportedByPtyId,
		retainedAgentsByPaneKey: storeData.retainedAgentsByPaneKey,
		tabsByWorktree: storeData.tabsByWorktree,
		worktreeMap: storeData.worktreeMap,
		repoMap: storeData.repoMap,
		acknowledgedAgentsByPaneKey: storeData.acknowledgedAgentsByPaneKey,
		now: Date.now()
	}), [storeData, useAppStore((s) => s.agentStatusEpoch)]);
	const allThreads = (0, import_react.useMemo)(() => buildAgentPaneThreads({
		events: allEvents,
		liveAgentByPaneKey,
		generatedTitlesEnabled: storeData.generatedTitlesEnabled
	}), [
		allEvents,
		liveAgentByPaneKey,
		storeData.generatedTitlesEnabled
	]);
	const selectedPaneKeyIsLive = selectedPaneKey === null || allThreads.some((thread) => thread.paneKey === selectedPaneKey);
	const effectiveSelectedPaneKey = selectedPaneKeyIsLive ? selectedPaneKey : null;
	if (!selectedPaneKeyIsLive) setSelectedPaneKey(null);
	const visibleThreads = (0, import_react.useMemo)(() => {
		const normalizedQuery = isActivitySearchQueryTooLarge(query) ? null : query.trim().toLowerCase();
		return allThreads.filter((thread) => {
			if (readFilter === "unread" && !thread.unread && thread.paneKey !== effectiveSelectedPaneKey) return false;
			if (normalizedQuery === null) return false;
			return activityThreadMatchesSearchQuery({
				thread,
				searchQuery: normalizedQuery
			});
		});
	}, [
		allThreads,
		readFilter,
		query,
		effectiveSelectedPaneKey
	]);
	const visibleThreadGroups = (0, import_react.useMemo)(() => buildActivityThreadGroups(visibleThreads, groupBy), [visibleThreads, groupBy]);
	const selectedThread = effectiveSelectedPaneKey ? allThreads.find((thread) => thread.paneKey === effectiveSelectedPaneKey) ?? null : null;
	const selectedTabId = selectedThread?.tab.id ?? null;
	const selectedHasLiveTab = selectedThread && selectedTabId && storeData.worktreeMap.has(selectedThread.worktree.id) ? (storeData.tabsByWorktree[selectedThread.worktree.id] ?? []).some((tab) => tab.id === selectedTabId) : false;
	const displayedThread = displayedPaneKey ? allThreads.find((thread) => thread.paneKey === displayedPaneKey) ?? null : null;
	const displayedTabId = displayedThread?.tab.id ?? null;
	const displayedHasLiveTab = displayedThread && displayedTabId && storeData.worktreeMap.has(displayedThread.worktree.id) ? (storeData.tabsByWorktree[displayedThread.worktree.id] ?? []).some((tab) => tab.id === displayedTabId) : false;
	const { visibleThread, stagedThread } = reconcileActivityPortalThreads({
		selectedThread,
		displayedThread,
		selectedHasLiveTab: Boolean(selectedHasLiveTab),
		displayedHasLiveTab: Boolean(displayedHasLiveTab)
	});
	const inactivePortalSlotId = otherActivityTerminalSlot(activePortalSlotId);
	const portalTargetBySlot = {
		primary: primaryPortalTargetEl,
		secondary: secondaryPortalTargetEl
	};
	const activePortalTargetEl = portalTargetBySlot[activePortalSlotId];
	const inactivePortalTargetEl = portalTargetBySlot[inactivePortalSlotId];
	const visiblePortalStatus = useActivityTerminalPortalStatus(activePortalTargetEl, visibleThread?.paneKey ?? null, visibleThread?.migrationUnsupportedPtyId !== void 0);
	const stagedPortalStatus = useActivityTerminalPortalStatus(inactivePortalTargetEl, stagedThread?.paneKey ?? null, stagedThread?.migrationUnsupportedPtyId !== void 0);
	const visiblePortalReady = visiblePortalStatus === "ready";
	const visiblePortalUnavailable = visiblePortalStatus === "unavailable";
	const stagedPortalReady = stagedPortalStatus === "ready";
	const stagedPortalUnavailable = stagedPortalStatus === "unavailable";
	const showTerminalLoadingLabel = useActivityTerminalLoadingLabel(Boolean(visibleThread && !stagedThread && !visiblePortalReady));
	const setPrimaryPortalTarget = (0, import_react.useCallback)((target) => {
		setPrimaryPortalTargetEl(target);
	}, []);
	const setSecondaryPortalTarget = (0, import_react.useCallback)((target) => {
		setSecondaryPortalTargetEl(target);
	}, []);
	const portalDescriptors = (0, import_react.useMemo)(() => {
		const descriptors = [];
		if (visibleThread && activePortalTargetEl) descriptors.push({
			slotId: activePortalSlotId,
			requestToken: `${activePortalSlotId}:${visibleThread.paneKey}`,
			target: activePortalTargetEl,
			worktreeId: visibleThread.worktree.id,
			tabId: visibleThread.tab.id,
			paneKey: visibleThread.paneKey,
			forceUnavailable: visibleThread.migrationUnsupportedPtyId !== void 0,
			active: true
		});
		if (stagedThread && inactivePortalTargetEl) descriptors.push({
			slotId: inactivePortalSlotId,
			requestToken: `${inactivePortalSlotId}:${stagedThread.paneKey}`,
			target: inactivePortalTargetEl,
			worktreeId: stagedThread.worktree.id,
			tabId: stagedThread.tab.id,
			paneKey: stagedThread.paneKey,
			forceUnavailable: stagedThread.migrationUnsupportedPtyId !== void 0,
			active: false
		});
		return descriptors;
	}, [
		activePortalSlotId,
		activePortalTargetEl,
		inactivePortalSlotId,
		inactivePortalTargetEl,
		stagedThread,
		visibleThread
	]);
	(0, import_react.useLayoutEffect)(() => {
		const swap = resolveActivityPortalSwap({
			selectedThread,
			selectedHasLiveTab: Boolean(selectedHasLiveTab),
			visibleThread,
			stagedThread,
			visiblePortalReady,
			stagedPortalReady,
			stagedPortalUnavailable
		});
		if (swap?.kind === "clear") {
			setDisplayedPaneKey(null);
			return;
		}
		if (swap?.kind === "swap-staged") {
			setActivePortalSlotId(inactivePortalSlotId);
			setDisplayedPaneKey(swap.paneKey);
			return;
		}
		if (swap?.kind === "settle-visible") setDisplayedPaneKey(swap.paneKey);
	}, [
		inactivePortalSlotId,
		selectedHasLiveTab,
		selectedThread,
		stagedPortalUnavailable,
		stagedPortalReady,
		stagedThread,
		visiblePortalReady,
		visibleThread
	]);
	(0, import_react.useLayoutEffect)(() => {
		setActivityTerminalPortals(portalDescriptors);
	}, [portalDescriptors]);
	const setActivityPageRef = (0, import_react.useCallback)((node) => {
		if (!node) setActivityTerminalPortals([]);
	}, []);
	(0, import_react.useEffect)(() => {
		const focusActivityFilter = (event) => {
			handleActivityFilterFocusShortcut({
				activeElement: document.activeElement,
				event,
				input: activityFilterInputRef.current,
				terminalPortalTargets: [activePortalTargetEl, inactivePortalTargetEl]
			});
		};
		window.addEventListener("keydown", focusActivityFilter, { capture: true });
		return () => window.removeEventListener("keydown", focusActivityFilter, { capture: true });
	}, [activePortalTargetEl, inactivePortalTargetEl]);
	const markThreadRead = (thread) => {
		storeData.acknowledgeAgents([thread.paneKey]);
	};
	const markThreadUnread = (thread) => {
		storeData.unacknowledgeAgents([thread.paneKey]);
	};
	const activateThreadTerminal = (thread) => {
		const state = useAppStore.getState();
		const worktree = getWorktreeMapFromState(state).get(thread.worktree.id);
		if (!worktree) return;
		if (!(state.tabsByWorktree[worktree.id] ?? []).some((t) => t.id === thread.tab.id)) return;
		if (state.activeRepoId !== worktree.repoId) state.setActiveRepo(worktree.repoId);
		if (state.activeWorktreeId !== worktree.id) state.setActiveWorktree(worktree.id);
		state.setActiveTabType("terminal");
		const parsed = parsePaneKey(thread.paneKey);
		activateTabAndFocusPane(thread.tab.id, parsed && parsed.tabId === thread.tab.id ? parsed.leafId : null, { scrollToBottomIfOutputSinceLastView: true });
	};
	const selectThread = (thread) => {
		setSelectedPaneKey(thread.paneKey);
		activateThreadTerminal(thread);
	};
	(0, import_react.useEffect)(() => {
		if (!selectedThread || !selectedThread.unread || stagedThread || selectedThread.paneKey !== effectiveSelectedPaneKey) return;
		const selectedThreadHasDetailOnlyView = !selectedHasLiveTab || selectedThread.migrationUnsupportedPtyId !== void 0;
		const selectedThreadIsVisibleTerminal = visibleThread?.paneKey === effectiveSelectedPaneKey && visiblePortalReady;
		if (selectedThreadHasDetailOnlyView || selectedThreadIsVisibleTerminal) storeData.acknowledgeAgents([selectedThread.paneKey]);
	}, [
		selectedHasLiveTab,
		effectiveSelectedPaneKey,
		selectedThread,
		stagedThread,
		storeData,
		visiblePortalReady,
		visibleThread
	]);
	const jumpToWorkspace = (thread) => {
		if (!getWorktreeMapFromState(useAppStore.getState()).has(thread.worktree.id)) return;
		markThreadRead(thread);
		activateAndRevealWorktree(thread.worktree.id);
	};
	const hasUnreadThreads = allThreads.some((thread) => thread.unread);
	const markAllThreadsRead = () => {
		const unreadKeys = allThreads.filter((t) => t.unread).map((t) => t.paneKey);
		if (unreadKeys.length === 0) return;
		storeData.acknowledgeAgents(unreadKeys);
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		ref: setActivityPageRef,
		className: "flex h-full min-h-0 flex-col bg-background pb-3",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("main", {
			className: "flex min-h-0 flex-1 overflow-hidden",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("aside", {
				ref: threadListRef,
				className: "relative flex min-h-0 shrink-0 flex-col border-r border-border",
				style: { width: threadListWidth },
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "shrink-0 border-b border-border px-2 pt-2 pb-2",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center gap-2",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: "relative min-w-0 flex-1",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Search, { className: "pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
										ref: activityFilterInputRef,
										value: query,
										onChange: (event) => setQuery(event.target.value),
										placeholder: translate("auto.components.activity.ActivityPrototypePage.795cbf26e2", "Filter..."),
										className: "h-8 w-full pl-7 text-xs"
									})]
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Select, {
									value: groupBy,
									onValueChange: (value) => setGroupBy(value),
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectTrigger, {
										size: "sm",
										className: "h-8 w-[128px] shrink-0 px-2 text-xs",
										"aria-label": translate("auto.components.activity.ActivityPrototypePage.770d458144", "Group agent activity by"),
										children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectValue, {})
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(SelectContent, {
										align: "end",
										children: [
											/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
												value: "status",
												children: translate("auto.components.activity.ActivityPrototypePage.4a3986b200", "Status")
											}),
											/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
												value: "project",
												children: translate("auto.components.activity.ActivityPrototypePage.8c3b621ddf", "Project")
											}),
											/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
												value: "worktree",
												children: translate("auto.components.activity.ActivityPrototypePage.b29191b3e0", "Worktree")
											}),
											/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
												value: "agent",
												children: translate("auto.components.activity.ActivityPrototypePage.f6396e1f85", "Agent")
											})
										]
									})]
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
									asChild: true,
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Toggle, {
										pressed: readFilter === "unread",
										onPressedChange: (pressed) => setReadFilter(pressed ? "unread" : "all"),
										variant: "outline",
										size: "sm",
										className: cn("size-8 shrink-0 p-0", readFilter === "unread" ? "!border-primary !bg-primary !text-primary-foreground shadow-xs ring-2 ring-primary/35 hover:!bg-primary/90 hover:!text-primary-foreground" : "text-muted-foreground hover:text-foreground"),
										"aria-label": translate("auto.components.activity.ActivityPrototypePage.d1a88df9a8", "Show unread threads only"),
										children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(BellDot, { className: "size-3.5" })
									})
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
									side: "bottom",
									children: translate("auto.components.activity.ActivityPrototypePage.d1a88df9a8", "Show unread threads only")
								})] }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ActivityThreadOptionsMenu, {
									compactMode,
									hasUnreadThreads,
									onCompactModeChange: setCompactMode,
									onMarkAllThreadsRead: markAllThreadsRead
								})
							]
						})
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-h-0 flex-1 overflow-auto scrollbar-sleek",
						children: [visibleThreadGroups.map((group) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
							"aria-label": translate("auto.components.activity.ActivityPrototypePage.a2b4437bfb", "{{value0}} activity", { value0: group.label }),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ActivityStatusGroupHeader, { group }), group.threads.map((thread) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ThreadRow, {
								thread,
								selected: thread.paneKey === selectedThread?.paneKey,
								onSelect: () => selectThread(thread),
								onJump: () => jumpToWorkspace(thread),
								onMarkUnread: () => markThreadUnread(thread),
								canJump: storeData.worktreeMap.has(thread.worktree.id),
								compactMode
							}, thread.paneKey))]
						}, group.key)), visibleThreads.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "px-3 py-8 text-sm text-muted-foreground",
							children: translate("auto.components.activity.ActivityPrototypePage.7cd632006b", "No agent activity matches these filters.")
						}) : null]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						"aria-label": translate("auto.components.activity.ActivityPrototypePage.443690186e", "Resize activity thread list"),
						title: translate("auto.components.activity.ActivityPrototypePage.866083500b", "Drag to resize"),
						className: cn("group absolute -right-1.5 top-0 z-20 flex h-full w-3 cursor-col-resize items-stretch justify-center", isThreadListResizing && "bg-ring/10"),
						onMouseDown: onResizeStart,
						role: "separator",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: cn("h-full w-px bg-border transition-colors group-hover:bg-ring/50", isThreadListResizing && "bg-ring") })
					})
				]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("section", {
				className: "min-w-0 flex-1 overflow-hidden",
				children: selectedThread ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex h-full min-h-0 flex-col",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex shrink-0 items-start gap-4 border-b border-border px-4 pt-2 pb-3",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex min-w-0 items-start gap-2",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
									className: "inline-flex shrink-0 items-start gap-1",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ThreadAgentStateIndicator, { thread: selectedThread }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "inline-flex shrink-0 pt-[3px]",
										children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
											agent: agentTypeToIconAgent(selectedThread.agentType),
											size: 16
										})
									})]
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
									className: "line-clamp-3 break-words text-sm font-semibold leading-snug",
									children: selectedThread.paneTitle
								})]
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mt-1 flex min-w-0 items-center gap-1.5 pl-11",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(EventRepoBadge, { repo: selectedThread.repo }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "truncate text-xs text-muted-foreground",
									children: selectedThread.worktree.displayName
								})]
							})]
						})
					}), (() => {
						if (!selectedHasLiveTab) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex min-h-0 flex-1 flex-col items-center justify-center gap-2 p-4 text-sm text-muted-foreground",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SquareTerminal, { className: "size-7" }), storeData.worktreeMap.has(selectedThread.worktree.id) ? translate("auto.components.activity.ActivityPrototypePage.afdc2139a8", "Agent terminal closed. Open a new terminal in this workspace to continue.") : translate("auto.components.activity.ActivityPrototypePage.22b22034bc", "Standalone terminal unavailable in Activity.")]
						});
						return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "relative min-h-0 flex-1 overflow-hidden bg-editor-surface",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									ref: setPrimaryPortalTarget,
									className: cn("absolute inset-0 min-h-0 min-w-0", activePortalSlotId === "primary" ? "z-10 opacity-100" : "pointer-events-none z-0 opacity-0"),
									"aria-hidden": activePortalSlotId !== "primary",
									"data-activity-terminal-slot-id": "primary"
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									ref: setSecondaryPortalTarget,
									className: cn("absolute inset-0 min-h-0 min-w-0", activePortalSlotId === "secondary" ? "z-10 opacity-100" : "pointer-events-none z-0 opacity-0"),
									"aria-hidden": activePortalSlotId !== "secondary",
									"data-activity-terminal-slot-id": "secondary"
								}),
								visibleThread && !stagedThread && !visiblePortalReady ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "pointer-events-none absolute inset-0 z-20 bg-editor-surface",
									"aria-hidden": "true",
									children: visiblePortalUnavailable ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
										className: "ml-3 mt-3 inline-flex items-center gap-2 rounded-md border border-border bg-background/85 px-2 py-1 text-xs text-muted-foreground shadow-xs",
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "h-3 w-1.5 rounded-sm bg-muted-foreground/70" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.activity.ActivityPrototypePage.8de7c5beaa", "Terminal unavailable") })]
									}) : showTerminalLoadingLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
										className: "ml-3 mt-3 inline-flex items-center gap-2 rounded-md border border-border bg-background/85 px-2 py-1 text-xs text-muted-foreground shadow-xs",
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "h-3 w-1.5 animate-pulse rounded-sm bg-muted-foreground/70" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.activity.ActivityPrototypePage.1b633f5c1e", "Connecting terminal...") })]
									}) : null
								}) : null
							]
						});
					})()]
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex h-full min-h-[240px] flex-col items-center justify-center gap-2 text-sm text-muted-foreground",
					children: visibleThreads.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MessageSquareText, { className: "size-7" }), translate("auto.components.activity.ActivityPrototypePage.e3db9892f6", "No activity yet.")] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SquareTerminal, { className: "size-7" }), translate("auto.components.activity.ActivityPrototypePage.cf780197a1", "Select an agent to view its activity")] })
				})
			})]
		})
	});
}
export { ACTIVITY_SEARCH_QUERY_MAX_BYTES, ActivityThreadOptionsMenu, activityThreadMatchesSearchQuery, activityThreadResponseRenderPreview, buildActivityEvents, buildActivityThreadGroups, buildAgentPaneThreads, ActivityPrototypePage as default, getActivityThreadGroup, groupActivityThreadsByStatus, handleActivityFilterFocusShortcut, isActivityFilterFocusShortcut, isActivitySearchQueryTooLarge, shouldIgnoreActivityFilterFocusShortcutTarget, useActivityTerminalPortalStatus };
