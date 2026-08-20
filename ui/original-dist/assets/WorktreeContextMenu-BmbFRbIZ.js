import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn, t as Button } from "./button-DszXJEV6.js";
import { n as getWorkspaceStatusVisualMeta } from "./workspace-status-wl52y3xd.js";
import { t as Bell } from "./bell-D5DYvUTg.js";
import { t as CircleX } from "./circle-x-Cl9fp3Vy.js";
import { t as CodeXml } from "./code-xml-Q_QLKUSg.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { t as FolderPlus } from "./folder-plus-09lX5Kg7.js";
import { Nt as FolderTree, ft as mergeAgentStatusOrchestration, mt as resolveAgentStatusWorktreeId, pt as parseAgentStatusPaneIdentity, r as activateAndRevealWorktree } from "./worktree-activation-BDsaiyMf.js";
import { t as GitBranch } from "./git-branch-CnBuDEti.js";
import { t as Moon } from "./moon-Cw6GyiDZ.js";
import { t as Pencil } from "./pencil-CLc9a5do.js";
import { t as PinOff } from "./pin-off-CP-untcQ.js";
import { t as Pin } from "./pin-K26SGNXp.js";
import { t as Server } from "./server-DYdwnXME.js";
import { al as VIRTUALIZED_SCROLL_ANCHOR_RECORD_EVENT, hd as folderWorkspaceKey, m_ as Trash2, t as useAppStore, vd as parseWorkspaceKey, yd as worktreeWorkspaceKey, yg as getWorkspaceStatus } from "./store-CgXrfmaH.js";
import { t as Unlink } from "./unlink-B9rPq8CF.js";
import { t as Workflow } from "./workflow-DL6naYZy.js";
import { a as isExplicitAgentStatusFresh, tt as getWorktreeExecutionHostId, u as AGENT_STATUS_STALE_AFTER_MS, v as tabHasLivePty } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { a as DropdownMenuLabel, c as DropdownMenuRadioItem, d as DropdownMenuSub, f as DropdownMenuSubContent, i as DropdownMenuItem, l as DropdownMenuSeparator, m as DropdownMenuTrigger, p as DropdownMenuSubTrigger, r as DropdownMenuContent, s as DropdownMenuRadioGroup, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { n as PopoverAnchor, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
import { f as useRepoById, g as useWorktreeMap, p as useRepoMap, u as useAllWorktrees } from "./selectors-XOBeaOSb.js";
import { t as migrationUnsupportedToAgentStatusEntry } from "./migration-unsupported-agent-entry-BJ_0rXR-.js";
import { n as getLineageRenderInfo, r as getProjectedWorktreeLineage, t as getCyclicProjectedWorktreeLineageIds } from "./worktree-lineage-projection-CS7n_mKq.js";
import { c as getWorkspaceDeleteLineage, n as runWorktreeDelete, t as runWorktreeBatchDelete } from "./delete-worktree-flow-RxB6NScm.js";
import { n as runSleepWorktrees } from "./sleep-worktree-flow-83wDDapJ.js";
import { a as CommandInput, l as Re, s as CommandList, t as Command } from "./command-D8Tw17HJ.js";
import { n as RepoBadgeMark } from "./RepoBadgeLabel-BMcVlWTu.js";
import { t as useVirtualizer } from "./esm-DQfOTgcy.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { n as WorktreeOpenInSubMenu } from "./WorktreeOpenInMenu-DH_mb2Hm.js";
import { t as Input } from "./input-DV5rpysh.js";
import { t as isImeCompositionKeyDown } from "./ime-composition-keyboard-event-HdRxQ6x2.js";
import { n as getWorktreeStatusLabel, r as resolveWorktreeStatus } from "./worktree-status-DR0Zr8Ht.js";
import { n as EMPTY_BROWSER_TABS, o as branchDisplayName, r as EMPTY_TABS } from "./WorktreeCardHelpers-Detnezco.js";
import { n as selectRuntimePaneTitlesForWorktree, r as selectTerminalLayoutRootsForWorktree, t as selectLivePtyIdsForWorktree } from "./worktree-card-status-inputs-DozvjAa5.js";
import { t as StatusIndicator_default } from "./StatusIndicator-CJ9TRLK4.js";
import { n as requestManualTerminalWorktreePark } from "./manual-terminal-worktree-parking-DpGjzl0b.js";
var BellOff = createLucideIcon("bell-off", [
	["path", {
		d: "M10.268 21a2 2 0 0 0 3.464 0",
		key: "vwvbt9"
	}],
	["path", {
		d: "M17 17H4a1 1 0 0 1-.74-1.673C4.59 13.956 6 12.499 6 8a6 6 0 0 1 .258-1.742",
		key: "178tsu"
	}],
	["path", {
		d: "m2 2 20 20",
		key: "1ooewy"
	}],
	["path", {
		d: "M8.668 3.01A6 6 0 0 1 18 8c0 2.687.77 4.653 1.707 6.05",
		key: "1hqiys"
	}]
]);
var FolderInput = createLucideIcon("folder-input", [
	["path", {
		d: "M2 9V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-1",
		key: "fm4g5t"
	}],
	["path", {
		d: "M2 13h10",
		key: "pgb2dq"
	}],
	["path", {
		d: "m9 16 3-3-3-3",
		key: "6m91ic"
	}]
]);
var Kanban = createLucideIcon("kanban", [
	["path", {
		d: "M5 3v14",
		key: "9nsxs2"
	}],
	["path", {
		d: "M12 3v8",
		key: "1h2ygw"
	}],
	["path", {
		d: "M19 3v18",
		key: "1sk56x"
	}]
]);
var SquareParking = createLucideIcon("square-parking", [["rect", {
	width: "18",
	height: "18",
	x: "3",
	y: "3",
	rx: "2",
	key: "afitv7"
}], ["path", {
	d: "M9 17V7h4a3 3 0 0 1 0 6H9",
	key: "1dfk2c"
}]]);
function runWorktreeContextMenuDeleteIntent(intent) {
	if (intent.kind === "batch") {
		runWorktreeBatchDelete(intent.worktrees);
		return;
	}
	if (intent.kind === "worktree") {
		runWorktreeDelete(intent.worktree.id, { expectedInstanceId: intent.worktree.instanceId });
		return;
	}
	useAppStore.getState().deleteFolderWorkspace(intent.folderWorkspaceId).then((deleted) => {
		const current = useAppStore.getState();
		if (deleted && current.activeWorktreeId === folderWorkspaceKey(intent.folderWorkspaceId)) current.setActiveWorktree(null);
	});
}
function deferWorktreeContextMenuDeleteIntent(intent, onDispatched, defer = (callback) => window.setTimeout(callback, 0)) {
	defer(() => {
		runWorktreeContextMenuDeleteIntent(intent);
		onDispatched?.();
	});
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function ProjectGroupNameDialog({ open, title, description, initialName, confirmLabel, onOpenChange, onSubmit }) {
	const inputRef = (0, import_react.useRef)(null);
	const inputId = (0, import_react.useId)();
	const [name, setName] = (0, import_react.useState)(initialName);
	const [submitting, setSubmitting] = (0, import_react.useState)(false);
	const [previousOpenState, setPreviousOpenState] = (0, import_react.useState)({
		open,
		initialName
	});
	const mountedRef = (0, import_react.useRef)(true);
	const trimmedName = name.trim();
	const handleDialogContentRef = (0, import_react.useCallback)((node) => {
		mountedRef.current = node !== null;
	}, []);
	if (open !== previousOpenState.open || initialName !== previousOpenState.initialName) {
		setPreviousOpenState({
			open,
			initialName
		});
		if (open) {
			setName(initialName);
			setSubmitting(false);
		}
	}
	const handleSubmit = (0, import_react.useCallback)(async (event) => {
		event?.preventDefault();
		if (!trimmedName || submitting) return;
		setSubmitting(true);
		try {
			await onSubmit(trimmedName);
			if (mountedRef.current) onOpenChange(false);
		} catch (error) {
			console.error("Failed to save project group name:", error);
			if (mountedRef.current) setSubmitting(false);
		}
	}, [
		onOpenChange,
		onSubmit,
		submitting,
		trimmedName
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			ref: handleDialogContentRef,
			className: "max-w-sm sm:max-w-sm",
			onOpenAutoFocus: (event) => {
				event.preventDefault();
				inputRef.current?.focus();
				inputRef.current?.select();
			},
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
				className: "text-sm",
				children: title
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
				className: "text-xs",
				children: description
			})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
				className: "space-y-4",
				onSubmit: handleSubmit,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
						htmlFor: inputId,
						className: "text-[11px] text-muted-foreground",
						children: translate("auto.components.sidebar.ProjectGroupNameDialog.83dfbc5313", "Group Name")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						id: inputId,
						ref: inputRef,
						value: name,
						onChange: (event) => setName(event.target.value),
						className: "h-8 text-xs"
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "outline",
					size: "sm",
					className: "text-xs",
					onClick: () => onOpenChange(false),
					children: translate("auto.components.sidebar.ProjectGroupNameDialog.d99a034073", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "submit",
					size: "sm",
					className: "text-xs",
					disabled: !trimmedName || submitting,
					children: submitting ? translate("auto.components.sidebar.ProjectGroupNameDialog.4a64e78822", "Saving...") : confirmLabel
				})] })]
			})]
		})
	});
}
var EMPTY_AGENT_STATUS_PANE_IDS_BY_TAB_ID = {};
var EMPTY_SUMMARY = {
	hasPermission: false,
	hasLiveWorking: false,
	hasLiveDone: false,
	hasRetainedDone: false,
	agentStatusPaneIdsByTabId: EMPTY_AGENT_STATUS_PANE_IDS_BY_TAB_ID
};
var agentActivityCache = null;
function selectWorktreeAgentActivitySummary(state, worktreeId) {
	return getWorktreeAgentActivitySummaries(state).get(worktreeId) ?? EMPTY_SUMMARY;
}
function getWorktreeAgentActivitySummaries(state) {
	const runtimeAgentOrchestrationByPaneKey = state.runtimeAgentOrchestrationByPaneKey;
	if (agentActivityCache && agentActivityCache.tabsByWorktree === state.tabsByWorktree && agentActivityCache.agentStatusEpoch === state.agentStatusEpoch && agentActivityCache.migrationUnsupportedByPtyId === state.migrationUnsupportedByPtyId && agentActivityCache.retainedAgentsByPaneKey === state.retainedAgentsByPaneKey && agentActivityCache.runtimeAgentOrchestrationByPaneKey === runtimeAgentOrchestrationByPaneKey) return agentActivityCache.summaries;
	const tabIdToWorktreeId = /* @__PURE__ */ new Map();
	for (const [worktreeId, tabs] of Object.entries(state.tabsByWorktree)) for (const tab of tabs) tabIdToWorktreeId.set(tab.id, worktreeId);
	const summaries = /* @__PURE__ */ new Map();
	const summaryForWorktree = (worktreeId) => {
		let summary = summaries.get(worktreeId);
		if (!summary) {
			summary = { ...EMPTY_SUMMARY };
			summaries.set(worktreeId, summary);
		}
		return summary;
	};
	const now = Date.now();
	for (const [paneKey, entry] of Object.entries(state.agentStatusByPaneKey)) {
		const paneIdentity = parseAgentStatusPaneIdentity(paneKey);
		if (!paneIdentity) continue;
		const orchestration = mergeAgentStatusOrchestration(entry, runtimeAgentOrchestrationByPaneKey?.[paneKey]);
		const worktreeId = resolveAgentStatusWorktreeId(entry, tabIdToWorktreeId, orchestration);
		if (!worktreeId) continue;
		const summary = summaryForWorktree(worktreeId);
		if (entry.restoredUnconfirmed) {
			addAgentStatusPaneId(summary, paneIdentity.tabId, paneIdentity.paneId);
			continue;
		}
		if (!isExplicitAgentStatusFresh(entry, now, 18e5)) continue;
		addAgentStatusPaneId(summary, paneIdentity.tabId, paneIdentity.paneId);
		if (entry.state === "done") addParentPaneId(summary, orchestration, worktreeId, tabIdToWorktreeId);
		applyLiveAgentState(summary, entry);
	}
	for (const unsupported of Object.values(state.migrationUnsupportedByPtyId ?? {})) {
		const entry = migrationUnsupportedToAgentStatusEntry(unsupported);
		const worktreeId = entry ? worktreeIdForPaneKey(entry.paneKey, tabIdToWorktreeId) : null;
		if (worktreeId) summaryForWorktree(worktreeId).hasPermission = true;
	}
	for (const retained of Object.values(state.retainedAgentsByPaneKey ?? {})) {
		const summary = summaryForWorktree(retained.worktreeId);
		summary.hasRetainedDone = true;
		const paneIdentity = parseAgentStatusPaneIdentity(retained.entry?.paneKey);
		if (paneIdentity) addAgentStatusPaneId(summary, paneIdentity.tabId, paneIdentity.paneId);
		addParentPaneId(summary, mergeAgentStatusOrchestration(retained.entry, runtimeAgentOrchestrationByPaneKey?.[retained.entry.paneKey]), retained.worktreeId, tabIdToWorktreeId);
	}
	const previousSummaries = agentActivityCache?.summaries;
	if (previousSummaries) for (const [worktreeId, summary] of summaries) {
		const previous = previousSummaries.get(worktreeId);
		if (previous && summariesEqual(previous, summary)) summaries.set(worktreeId, previous);
	}
	agentActivityCache = {
		tabsByWorktree: state.tabsByWorktree,
		agentStatusEpoch: state.agentStatusEpoch,
		migrationUnsupportedByPtyId: state.migrationUnsupportedByPtyId,
		retainedAgentsByPaneKey: state.retainedAgentsByPaneKey,
		runtimeAgentOrchestrationByPaneKey,
		summaries
	};
	return summaries;
}
function summariesEqual(previous, next) {
	return previous.hasPermission === next.hasPermission && previous.hasLiveWorking === next.hasLiveWorking && previous.hasLiveDone === next.hasLiveDone && previous.hasRetainedDone === next.hasRetainedDone && agentStatusPaneIdsByTabIdEqual(previous.agentStatusPaneIdsByTabId, next.agentStatusPaneIdsByTabId);
}
function agentStatusPaneIdsByTabIdEqual(previous, next) {
	if (previous === next) return true;
	const previousKeys = Object.keys(previous);
	if (previousKeys.length !== Object.keys(next).length) return false;
	for (const tabId of previousKeys) {
		const previousPaneIds = previous[tabId];
		const nextPaneIds = next[tabId];
		if (!nextPaneIds || previousPaneIds.size !== nextPaneIds.size) return false;
		for (const paneId of previousPaneIds) if (!nextPaneIds.has(paneId)) return false;
	}
	return true;
}
function applyLiveAgentState(summary, entry) {
	if (entry.state === "blocked" || entry.state === "waiting") summary.hasPermission = true;
	else if (entry.state === "working") summary.hasLiveWorking = true;
	else if (entry.state === "done") summary.hasLiveDone = true;
}
function addAgentStatusPaneId(summary, tabId, paneId) {
	if (summary.agentStatusPaneIdsByTabId === EMPTY_AGENT_STATUS_PANE_IDS_BY_TAB_ID) summary.agentStatusPaneIdsByTabId = {};
	let paneIds = summary.agentStatusPaneIdsByTabId[tabId];
	if (!paneIds) {
		paneIds = /* @__PURE__ */ new Set();
		summary.agentStatusPaneIdsByTabId[tabId] = paneIds;
	}
	paneIds.add(paneId);
}
function worktreeIdForPaneKey(paneKey, tabIdToWorktreeId) {
	const paneIdentity = parseAgentStatusPaneIdentity(paneKey);
	return paneIdentity ? tabIdToWorktreeId.get(paneIdentity.tabId) ?? null : null;
}
function addParentPaneId(summary, orchestration, worktreeId, tabIdToWorktreeId) {
	const parentPaneIdentity = parseAgentStatusPaneIdentity(orchestration?.parentPaneKey);
	if (!parentPaneIdentity) return;
	if (tabIdToWorktreeId.get(parentPaneIdentity.tabId) !== worktreeId) return;
	addAgentStatusPaneId(summary, parentPaneIdentity.tabId, parentPaneIdentity.paneId);
}
function selectWorktreeActivityStatuses(statusInputs, worktreeIds) {
	const statuses = /* @__PURE__ */ new Map();
	for (const worktreeId of worktreeIds) {
		const { hasPermission, hasLiveWorking, hasLiveDone, hasRetainedDone, agentStatusPaneIdsByTabId } = selectWorktreeAgentActivitySummary(statusInputs, worktreeId);
		statuses.set(worktreeId, resolveWorktreeStatus({
			tabs: statusInputs.tabsByWorktree[worktreeId] ?? EMPTY_TABS,
			browserTabs: statusInputs.browserTabsByWorktree[worktreeId] ?? EMPTY_BROWSER_TABS,
			ptyIdsByTabId: selectLivePtyIdsForWorktree(statusInputs, worktreeId),
			runtimePaneTitlesByTabId: selectRuntimePaneTitlesForWorktree(statusInputs, worktreeId),
			agentStatusPaneIdsByTabId,
			terminalLayoutRootsByTabId: selectTerminalLayoutRootsForWorktree(statusInputs, worktreeId),
			hasPermission,
			hasLiveWorking,
			hasLiveDone,
			hasRetainedDone
		}));
	}
	return statuses;
}
function useWorktreeActivityStatuses(worktreeIds) {
	return useAppStore(useShallow((0, import_react.useCallback)((state) => selectWorktreeActivityStatuses(state, worktreeIds), [worktreeIds])));
}
const WorktreeParentPickerRow = import_react.memo(function WorktreeParentPickerRow$1({ candidate, repo, status, isCurrent }) {
	const branch = branchDisplayName(candidate.branch);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-w-0 flex-1 items-start gap-2",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatusIndicator_default, {
				status,
				"aria-hidden": "true",
				className: "mt-0.5"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "sr-only",
				children: getWorktreeStatusLabel(status)
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 flex-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex min-w-0 items-center gap-1.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "truncate text-[13px] font-medium",
						children: candidate.displayName
					}), isCurrent ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "shrink-0 rounded border border-border bg-muted px-1.5 py-px text-[9px] font-medium leading-none text-muted-foreground",
						children: translate("auto.components.sidebar.WorktreeParentPickerPopover.current", "Current")
					}) : null]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mt-1 flex min-w-0 items-center gap-1.5 text-[11px] leading-none text-muted-foreground",
					children: [
						repo ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
							className: "inline-flex min-w-0 max-w-[8rem] shrink-0 items-center gap-1 rounded border border-border bg-accent px-1.5 py-0.5 text-[10px] font-semibold text-foreground",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RepoBadgeMark, { color: repo.badgeColor }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "truncate lowercase",
								children: repo.displayName
							})]
						}) : null,
						repo?.connectionId ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Server, { className: "size-3 shrink-0" }) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitBranch, { className: "size-3 shrink-0" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "truncate",
							children: branch
						})
					]
				})]
			})
		]
	});
});
function canAssignWorktreeParent({ child, candidateParent, lineageById, worktreeMap, cyclicLineageIds: precomputedCyclicLineageIds }) {
	if (child.id === candidateParent.id) return false;
	const cyclicLineageIds = precomputedCyclicLineageIds ?? getCyclicProjectedWorktreeLineageIds(lineageById, worktreeMap);
	const childLineage = getLineageRenderInfo(child, lineageById, worktreeMap, cyclicLineageIds);
	if (childLineage.state === "valid" && childLineage.parent.id === candidateParent.id) return false;
	let current = candidateParent;
	const visited = /* @__PURE__ */ new Set();
	while (current) {
		if (visited.has(current.id) || cyclicLineageIds.has(current.id)) return false;
		visited.add(current.id);
		if (current.id === child.id) return false;
		const lineageInfo = getLineageRenderInfo(current, lineageById, worktreeMap, cyclicLineageIds);
		current = lineageInfo.state === "valid" ? lineageInfo.parent : void 0;
	}
	return true;
}
function getWorktreeOwnerHostId(worktree, repoMap) {
	const repo = repoMap.get(worktree.repoId);
	return repo ? getWorktreeExecutionHostId(worktree, repo) : worktree.hostId ?? null;
}
function getEligibleWorktreeParents({ child, worktrees, lineageById, worktreeMap, repoMap, cyclicLineageIds: precomputedCyclicLineageIds }) {
	const childHostId = getWorktreeOwnerHostId(child, repoMap);
	const cyclicLineageIds = precomputedCyclicLineageIds ?? getCyclicProjectedWorktreeLineageIds(lineageById, worktreeMap);
	return worktrees.filter((candidate) => isEligibleWorktreeParent({
		child,
		candidateParent: candidate,
		lineageById,
		worktreeMap,
		repoMap,
		cyclicLineageIds,
		childHostId
	}));
}
function isEligibleWorktreeParent({ child, candidateParent, lineageById, worktreeMap, repoMap, cyclicLineageIds, childHostId = getWorktreeOwnerHostId(child, repoMap) }) {
	return candidateParent.repoId === child.repoId && childHostId !== null && getWorktreeOwnerHostId(candidateParent, repoMap) === childHostId && (child.projectId === void 0 || candidateParent.projectId === void 0 || child.projectId === candidateParent.projectId) && !candidateParent.isArchived && canAssignWorktreeParent({
		child,
		candidateParent,
		lineageById,
		worktreeMap,
		cyclicLineageIds
	});
}
function getWorktreeParentPickerItemValue(candidate) {
	return `${candidate.displayName} ${branchDisplayName(candidate.branch)} ${candidate.path}`;
}
function filterWorktreeParentCandidates(candidates, search) {
	const query = search.trim();
	if (!query) return [...candidates];
	return candidates.map((candidate) => ({
		candidate,
		score: Re(getWorktreeParentPickerItemValue(candidate), query, [])
	})).filter((scored) => scored.score > 0).sort((a, b) => b.score - a.score).map((scored) => scored.candidate);
}
function clampWorktreeParentPickerIndex(index, count) {
	if (count <= 0) return 0;
	return Math.min(Math.max(index, 0), count - 1);
}
var PICKER_INPUT_HEIGHT = 49;
var PICKER_HEADER_HEIGHT = 28;
var PICKER_SURFACE_BORDER_HEIGHT = 2;
function estimateWorktreeParentPickerHeight(candidateCount) {
	const listHeight = Math.min(Math.max(candidateCount, 1) * 56, 288);
	return PICKER_SURFACE_BORDER_HEIGHT + PICKER_HEADER_HEIGHT + PICKER_INPUT_HEIGHT + listHeight;
}
function clampWorktreeParentPickerAnchorTop(anchorTop, contentHeight, viewportHeight) {
	const maxTop = viewportHeight - 12 - contentHeight;
	if (maxTop <= 12) return 12;
	return Math.min(Math.max(anchorTop, 12), maxTop);
}
function getAnchorRect(anchorElement) {
	return anchorElement?.getBoundingClientRect() ?? null;
}
var FOCUSABLE_ANCHOR_SELECTOR = "a[href],button,input,select,textarea,[tabindex]:not([tabindex=\"-1\"])";
function getWorktreeParentPickerFocusRestoreTarget(anchorElement) {
	if (!anchorElement?.isConnected) return null;
	return anchorElement.closest(FOCUSABLE_ANCHOR_SELECTOR);
}
function selectWorktreeParent({ childWorktreeId, parentWorktreeId, assignWorktreeParent, close, showError }) {
	if (!childWorktreeId) return;
	close();
	assignWorktreeParent(childWorktreeId, { parentWorktreeId }).catch((error) => {
		console.error("Failed to set parent worktree:", error);
		showError(translate("auto.components.sidebar.WorktreeParentPickerPopover.failedSetParent", "Failed to set parent worktree"));
	});
}
function handleWorktreeParentPickerKeyDown({ event, candidates, activeIndex, moveHighlight, selectParent }) {
	if (isImeCompositionKeyDown(event) || candidates.length === 0) return;
	const navigate = (nextIndex) => {
		event.preventDefault();
		event.stopPropagation();
		moveHighlight(clampWorktreeParentPickerIndex(nextIndex, candidates.length));
	};
	if (event.key === "ArrowDown") navigate(activeIndex + 1);
	else if (event.key === "ArrowUp") navigate(activeIndex - 1);
	else if (event.key === "Home") navigate(0);
	else if (event.key === "End") navigate(candidates.length - 1);
	else if (event.key === "Enter") {
		const candidate = candidates[activeIndex];
		if (candidate) {
			event.preventDefault();
			event.stopPropagation();
			selectParent(candidate.id);
		}
	}
}
function WorktreeParentPickerPopover({ open, childWorktreeId, anchorElement, onOpenChange }) {
	const worktrees = useAllWorktrees();
	const worktreeMap = useWorktreeMap();
	const repoMap = useRepoMap();
	const activeWorktreeId = useAppStore((s) => s.activeWorktreeId);
	const lineageById = useAppStore((s) => s.worktreeLineageById);
	const assignWorktreeParent = useAppStore((s) => s.assignWorktreeParent);
	const suppressInitialOutsideCloseRef = (0, import_react.useRef)(false);
	const listRef = (0, import_react.useRef)(null);
	const inputRef = (0, import_react.useRef)(null);
	const optionIdPrefix = `${(0, import_react.useId)()}option`;
	const [search, setSearch] = (0, import_react.useState)("");
	const [highlightedIndex, setHighlightedIndex] = (0, import_react.useState)(0);
	const [anchorRect, setAnchorRect] = (0, import_react.useState)(() => getAnchorRect(anchorElement));
	const [viewportHeight, setViewportHeight] = (0, import_react.useState)(() => window.innerHeight);
	const child = childWorktreeId ? worktreeMap.get(childWorktreeId) : void 0;
	const candidates = (0, import_react.useMemo)(() => child ? getEligibleWorktreeParents({
		child,
		worktrees,
		lineageById,
		worktreeMap,
		repoMap
	}) : [], [
		child,
		lineageById,
		repoMap,
		worktreeMap,
		worktrees
	]);
	(0, import_react.useLayoutEffect)(() => {
		if (!open) return;
		const updateAnchorRect = () => {
			setAnchorRect(getAnchorRect(anchorElement));
			setViewportHeight(window.innerHeight);
		};
		updateAnchorRect();
		window.addEventListener("resize", updateAnchorRect);
		window.addEventListener("scroll", updateAnchorRect, true);
		return () => {
			window.removeEventListener("resize", updateAnchorRect);
			window.removeEventListener("scroll", updateAnchorRect, true);
		};
	}, [anchorElement, open]);
	(0, import_react.useEffect)(() => {
		if (!open) {
			suppressInitialOutsideCloseRef.current = false;
			return;
		}
		suppressInitialOutsideCloseRef.current = true;
		const timerId = window.setTimeout(() => {
			suppressInitialOutsideCloseRef.current = false;
		}, 150);
		return () => window.clearTimeout(timerId);
	}, [open]);
	const handleSelect = (0, import_react.useCallback)((parentWorktreeId) => {
		selectWorktreeParent({
			childWorktreeId,
			parentWorktreeId,
			assignWorktreeParent,
			close: () => onOpenChange(false),
			showError: toast.error
		});
	}, [
		assignWorktreeParent,
		childWorktreeId,
		onOpenChange
	]);
	const virtualAnchorRef = (0, import_react.useMemo)(() => {
		if (!anchorRect) return;
		const top = clampWorktreeParentPickerAnchorTop(anchorRect.top, estimateWorktreeParentPickerHeight(candidates.length), viewportHeight);
		const rect = new DOMRect(anchorRect.left, top, anchorRect.width, anchorRect.height);
		return { current: { getBoundingClientRect: () => rect } };
	}, [
		anchorRect,
		candidates.length,
		viewportHeight
	]);
	const filtered = (0, import_react.useMemo)(() => filterWorktreeParentCandidates(candidates, search), [candidates, search]);
	const activeIndex = clampWorktreeParentPickerIndex(highlightedIndex, filtered.length);
	const virtualizer = useVirtualizer({
		count: filtered.length,
		getScrollElement: () => listRef.current,
		estimateSize: () => 56,
		overscan: 6,
		getItemKey: (index) => filtered[index]?.id ?? index,
		initialRect: {
			width: 0,
			height: 288
		}
	});
	const handleSearchChange = (0, import_react.useCallback)((nextSearch) => {
		setSearch(nextSearch);
		setHighlightedIndex(0);
		virtualizer.scrollToOffset(0);
	}, [virtualizer]);
	const virtualRows = virtualizer.getVirtualItems();
	const statuses = useWorktreeActivityStatuses((0, import_react.useMemo)(() => virtualRows.map((row) => filtered[row.index]?.id).filter((id) => id !== void 0), [filtered, virtualRows]));
	const moveHighlight = (0, import_react.useCallback)((nextIndex) => {
		setHighlightedIndex(nextIndex);
		virtualizer.scrollToIndex(nextIndex, { align: "auto" });
	}, [virtualizer]);
	const handleKeyDown = (0, import_react.useCallback)((event) => {
		handleWorktreeParentPickerKeyDown({
			event,
			candidates: filtered,
			activeIndex,
			moveHighlight,
			selectParent: handleSelect
		});
	}, [
		activeIndex,
		filtered,
		handleSelect,
		moveHighlight
	]);
	(0, import_react.useEffect)(() => {
		const input = inputRef.current;
		if (!input) return;
		const activeOptionId = filtered.length > 0 ? `${optionIdPrefix}-${activeIndex}` : null;
		if (activeOptionId) input.setAttribute("aria-activedescendant", activeOptionId);
		else input.removeAttribute("aria-activedescendant");
	});
	if (!child || !anchorRect) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
		modal: true,
		open,
		onOpenChange,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverAnchor, { virtualRef: virtualAnchorRef }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(PopoverContent, {
			align: "start",
			side: "right",
			sideOffset: 8,
			collisionPadding: 12,
			className: "flex max-h-(--radix-popover-content-available-height) w-80 flex-col p-0",
			onOpenAutoFocus: (event) => {
				event.preventDefault();
				inputRef.current?.focus();
			},
			onCloseAutoFocus: (event) => {
				event.preventDefault();
				getWorktreeParentPickerFocusRestoreTarget(anchorElement)?.focus();
			},
			onInteractOutside: (event) => {
				if (suppressInitialOutsideCloseRef.current) event.preventDefault();
			},
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 shrink-0 items-center gap-1.5 border-b border-border bg-muted/30 px-3 py-2 text-[11px] leading-none text-muted-foreground",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "shrink-0",
					children: translate("auto.components.sidebar.WorktreeParentPickerPopover.setParentFor", "Set parent for")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "truncate font-medium text-foreground",
					children: child.displayName
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Command, {
				shouldFilter: false,
				className: "min-h-0",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandInput, {
					ref: inputRef,
					value: search,
					onValueChange: handleSearchChange,
					onKeyDown: handleKeyDown,
					wrapperClassName: "shrink-0",
					placeholder: translate("auto.components.sidebar.WorktreeParentPickerPopover.searchPlaceholder", "Search worktrees...")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandList, {
					ref: listRef,
					className: "max-h-72 min-h-0 flex-1",
					children: filtered.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "py-6 text-center text-sm text-muted-foreground",
						children: translate("auto.components.sidebar.WorktreeParentPickerPopover.empty", "No matching eligible worktrees.")
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "relative w-full",
						style: { height: virtualizer.getTotalSize() },
						children: virtualRows.map((virtualRow) => {
							const candidate = filtered[virtualRow.index];
							if (!candidate) return null;
							const isHighlighted = virtualRow.index === activeIndex;
							return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								id: `${optionIdPrefix}-${virtualRow.index}`,
								role: "option",
								"aria-selected": isHighlighted,
								"data-selected": isHighlighted || void 0,
								className: cn("absolute left-0 top-0 flex w-full cursor-default select-none items-start gap-2 overflow-hidden rounded-sm px-2 py-2 text-sm outline-none", isHighlighted && "bg-accent text-accent-foreground"),
								style: {
									height: virtualRow.size,
									transform: `translateY(${virtualRow.start}px)`
								},
								onPointerMove: () => setHighlightedIndex(virtualRow.index),
								onClick: () => handleSelect(candidate.id),
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeParentPickerRow, {
									candidate,
									repo: repoMap.get(candidate.repoId),
									status: statuses.get(candidate.id) ?? "inactive",
									isCurrent: activeWorktreeId === candidate.id
								})
							}, candidate.id);
						})
					})
				})]
			})]
		})]
	});
}
function WorktreeDeveloperMenu({ worktreeId, disabled }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSub, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSubTrigger, {
		disabled,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CodeXml, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeDeveloperMenu.developer", "Developer")]
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSubContent, {
		className: "w-44",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
			onSelect: () => requestManualTerminalWorktreePark(worktreeId),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SquareParking, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeDeveloperMenu.parkTerminal", "Park terminal")]
		})
	})] });
}
function hasSleepableWorkspaceActivity(worktreeId, { tabsByWorktree, ptyIdsByTabId, browserTabsByWorktree }) {
	return (tabsByWorktree[worktreeId] ?? []).some((tab) => tabHasLivePty(ptyIdsByTabId, tab.id)) || (browserTabsByWorktree[worktreeId] ?? []).length > 0;
}
function getWorkspaceLineageMenuActions(args) {
	const { descendants } = getWorkspaceDeleteLineage(args.parent, args.worktrees, args.lineageById);
	const targets = [args.parent, ...descendants];
	return {
		descendants,
		targets,
		sleepableTargets: targets.filter((target) => hasSleepableWorkspaceActivity(target.id, args.activity))
	};
}
var EMPTY_LINEAGE_MENU_ACTIONS = {
	descendants: [],
	targets: [],
	sleepableTargets: []
};
function useWorkspaceLineageMenuActions(args) {
	const { enabled, parent, worktrees, lineageById, activity } = args;
	const { tabsByWorktree, ptyIdsByTabId, browserTabsByWorktree } = activity;
	return (0, import_react.useMemo)(() => enabled ? getWorkspaceLineageMenuActions({
		parent,
		worktrees,
		lineageById,
		activity: {
			tabsByWorktree,
			ptyIdsByTabId,
			browserTabsByWorktree
		}
	}) : EMPTY_LINEAGE_MENU_ACTIONS, [
		browserTabsByWorktree,
		enabled,
		lineageById,
		parent,
		ptyIdsByTabId,
		tabsByWorktree,
		worktrees
	]);
}
function WorkspaceSleepMenuItems({ isMultiContext, sleepLabel, sleepDisabled, descendantCount, subtreeSleepDisabled, onSleep, onSleepSubtree }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
			onSelect: onSleep,
			disabled: sleepDisabled,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Moon, { className: "size-3.5" }), sleepLabel]
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "right",
		sideOffset: 8,
		className: "max-w-[200px] text-pretty",
		children: isMultiContext ? translate("auto.components.sidebar.WorktreeContextMenu.7d190f7d2b", "Close all active panels in the selected workspaces to free up memory and CPU.") : translate("auto.components.sidebar.WorktreeContextMenu.0918b35e4f", "Close all active panels in this workspace to free up memory and CPU.")
	})] }), !isMultiContext && descendantCount > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
			onSelect: onSleepSubtree,
			disabled: subtreeSleepDisabled,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Moon, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeContextMenu.sleepWithDescendants", "Sleep with Descendants ({{value0}})", { value0: descendantCount })]
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "right",
		sideOffset: 8,
		className: "max-w-[220px] text-pretty",
		children: translate("auto.components.sidebar.WorktreeContextMenu.sleepWithDescendantsDescription", "Close active panels in this workspace and every nested descendant to free up memory and CPU.")
	})] }) : null] });
}
function isEventTargetInsideCurrentTarget(currentTarget, target) {
	if (!(currentTarget instanceof Node) || !(target instanceof Node)) return false;
	return currentTarget.contains(target);
}
var CLOSE_ALL_CONTEXT_MENUS_EVENT = "orca-close-all-context-menus";
var WORKTREE_CONTEXT_MENU_SCOPE_ATTR = "data-worktree-context-menu-scope";
var WORKTREE_NATIVE_CONTEXT_MENU_ATTR = "data-worktree-native-context-menu";
var CONTEXT_MENU_CLICK_SUPPRESSION_MS = 500;
var DELETE_POSITION_RESTORE_MAX_FRAMES = 180;
var DELETE_POSITION_RESTORE_STABLE_FRAMES = 6;
var PARENT_PICKER_EXIT_ANIMATION_MS = 200;
var EMPTY_TABS_BY_WORKTREE = {};
var EMPTY_PTY_IDS_BY_TAB_ID = {};
var EMPTY_BROWSER_TABS_BY_WORKTREE = {};
var EMPTY_DELETE_STATE_BY_WORKTREE_ID = {};
var EMPTY_WORKTREE_LINEAGE_BY_ID = {};
var EMPTY_WORKSPACE_LINEAGE_BY_CHILD_KEY = {};
var EMPTY_CYCLIC_LINEAGE_IDS = /* @__PURE__ */ new Set();
function selectMenuScopedMap(menuOpen, live, empty) {
	return menuOpen ? live : empty;
}
function shouldRevealWorktreeDeveloperMenu(args) {
	return args.developerMenuRevealed && !args.isMultiContext;
}
function hasWorktreeParentLink(worktree, lineageById, workspaceLineageByChildKey) {
	return Boolean(getProjectedWorktreeLineage(worktree, lineageById) || workspaceLineageByChildKey[worktreeWorkspaceKey(worktree.id)]);
}
function shouldUseNativeContextMenu(target) {
	const maybeElement = target;
	const nativeContextMenuSelector = `[${WORKTREE_NATIVE_CONTEXT_MENU_ATTR}]`;
	return (maybeElement?.closest?.(nativeContextMenuSelector) ?? maybeElement?.parentElement?.closest?.(nativeContextMenuSelector)) != null;
}
function shouldIgnoreNestedWorktreeContextMenuScope(currentTarget, target) {
	const maybeScopedTarget = target;
	const scopeSelector = `[${WORKTREE_CONTEXT_MENU_SCOPE_ATTR}]`;
	const closestScope = maybeScopedTarget?.closest?.(scopeSelector) ?? maybeScopedTarget?.parentElement?.closest?.(scopeSelector);
	return closestScope != null && closestScope !== currentTarget;
}
function shouldSuppressContextMenuFollowUpClick(contextMenuOpenedAt, now) {
	return now - contextMenuOpenedAt >= 0 && now - contextMenuOpenedAt <= CONTEXT_MENU_CLICK_SUPPRESSION_MS;
}
function getWorktreeParentPickerLabel(validParentWorktreeId) {
	return validParentWorktreeId ? translate("auto.components.sidebar.WorktreeContextMenu.changeParentWorkspace", "Change Parent Worktree...") : translate("auto.components.sidebar.WorktreeContextMenu.setParentWorkspace", "Set Parent Worktree...");
}
function isWorktreeParentPickerDisabled(args) {
	return args.isDeleting || args.eligibleParentCount === 0;
}
function getWorktreeParentPickerAnchor(scope, worktreeId) {
	const dragRow = scope?.closest("[data-worktree-drag-id]");
	if (dragRow?.dataset.worktreeDragId === worktreeId) return dragRow;
	return scope;
}
function shouldRemoveProjectFromContextMenu(repo, worktree) {
	return repo != null && worktree.isMainWorktree;
}
function isContextWorktreeDeletable(worktree, repo) {
	return repo != null && !worktree.isMainWorktree;
}
function findSidebarVirtualRowByKey(sidebar, rowKey) {
	return Array.from(sidebar.querySelectorAll("[data-worktree-virtual-row]")).find((element) => element.getAttribute("data-worktree-virtual-row-key") === rowKey) ?? null;
}
function shouldContinueDeleteSiblingPositionRestore(args) {
	return args.attempts < DELETE_POSITION_RESTORE_MAX_FRAMES && args.stableFrames < DELETE_POSITION_RESTORE_STABLE_FRAMES;
}
function preserveDeleteSiblingPosition(scope) {
	const sidebar = scope?.closest("[data-worktree-sidebar]");
	const row = scope?.closest("[data-worktree-virtual-row]");
	if (!(sidebar instanceof HTMLElement) || !(row instanceof HTMLElement)) return () => {};
	const rows = Array.from(sidebar.querySelectorAll("[data-worktree-virtual-row]")).sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top);
	const rowIndex = rows.indexOf(row);
	const anchorKey = (rows[rowIndex + 1] ?? rows[rowIndex - 1] ?? null)?.getAttribute("data-worktree-virtual-row-key");
	const rowKey = row.getAttribute("data-worktree-virtual-row-key");
	if (!anchorKey || !rowKey) return () => {};
	const previousScrollTop = sidebar.scrollTop;
	const previousScrollHeight = sidebar.scrollHeight;
	const desiredTop = row.getBoundingClientRect().top;
	return () => {
		let attempts = 0;
		let stableFrames = 0;
		const restore = () => {
			const currentSidebar = document.querySelector("[data-worktree-sidebar]");
			if (!(currentSidebar instanceof HTMLElement)) return;
			const currentAnchor = findSidebarVirtualRowByKey(currentSidebar, rowKey) ?? findSidebarVirtualRowByKey(currentSidebar, anchorKey);
			if (currentAnchor) {
				const delta = currentAnchor.getBoundingClientRect().top - desiredTop;
				if (Math.abs(delta) > 1) {
					currentSidebar.scrollTop += delta;
					stableFrames = 0;
				} else stableFrames += 1;
			} else {
				currentSidebar.scrollTop = Math.max(0, previousScrollTop + currentSidebar.scrollHeight - previousScrollHeight);
				stableFrames = 0;
			}
			attempts += 1;
			if (shouldContinueDeleteSiblingPositionRestore({
				attempts,
				stableFrames
			})) window.requestAnimationFrame(restore);
		};
		restore();
	};
}
function planWorkspaceStatusAssignment(worktrees, status, workspaceStatuses, boardSyncEnabled) {
	if (boardSyncEnabled) return {
		kind: "board-sync",
		worktreeIds: worktrees.map((item) => item.id)
	};
	return {
		kind: "local-only",
		localWriteIds: worktrees.filter((item) => getWorkspaceStatus(item, workspaceStatuses) !== status).map((item) => item.id)
	};
}
var WorktreeContextMenu_default = import_react.memo(function WorktreeContextMenu$1({ worktree, children, contentClassName, selectedWorktrees, onContextMenuSelect, onAssignWorkspaceStatus, onOpenChange, onLifecycleComplete }) {
	const defaultSelectedWorktrees = (0, import_react.useMemo)(() => [worktree], [worktree]);
	const effectiveSelectedWorktrees = selectedWorktrees ?? defaultSelectedWorktrees;
	const updateWorktreeMeta = useAppStore((s) => s.updateWorktreeMeta);
	const setWorktreesPinnedAndReveal = useAppStore((s) => s.setWorktreesPinnedAndReveal);
	const workspaceStatuses = useAppStore((s) => s.workspaceStatuses);
	const openModal = useAppStore((s) => s.openModal);
	const projectGroups = useAppStore((s) => s.projectGroups);
	const createProjectGroup = useAppStore((s) => s.createProjectGroup);
	const moveProjectToGroup = useAppStore((s) => s.moveProjectToGroup);
	const repo = useRepoById(worktree.repoId);
	const deleteState = useAppStore((s) => s.deleteStateByWorktreeId[worktree.id]);
	const [menuOpen, setMenuOpen] = (0, import_react.useState)(false);
	const [developerMenuRevealed, setDeveloperMenuRevealed] = (0, import_react.useState)(false);
	const [menuPoint, setMenuPoint] = (0, import_react.useState)({
		x: 0,
		y: 0
	});
	const [contextWorktrees, setContextWorktrees] = (0, import_react.useState)(effectiveSelectedWorktrees);
	const [createGroupDialogOpen, setCreateGroupDialogOpen] = (0, import_react.useState)(false);
	const createGroupDialogActiveRef = (0, import_react.useRef)(false);
	const [parentPicker, setParentPicker] = (0, import_react.useState)(null);
	const [parentPickerOpen, setParentPickerOpen] = (0, import_react.useState)(false);
	const pendingParentPickerRef = (0, import_react.useRef)(null);
	const parentPickerFallbackTimerRef = (0, import_react.useRef)(null);
	const parentPickerUnmountTimerRef = (0, import_react.useRef)(null);
	const lifecycleStartedRef = (0, import_react.useRef)(false);
	const isDeleting = deleteState?.isDeleting ?? false;
	const repoMap = useRepoMap();
	const worktreeMap = useWorktreeMap();
	const allWorktrees = useAllWorktrees();
	const worktreeLineageById = useAppStore((s) => selectMenuScopedMap(menuOpen, s.worktreeLineageById, EMPTY_WORKTREE_LINEAGE_BY_ID));
	const workspaceLineageByChildKey = useAppStore((s) => selectMenuScopedMap(menuOpen, s.workspaceLineageByChildKey, EMPTY_WORKSPACE_LINEAGE_BY_CHILD_KEY));
	const updateWorktreeLineage = useAppStore((s) => s.updateWorktreeLineage);
	const tabsByWorktree = useAppStore((s) => selectMenuScopedMap(menuOpen, s.tabsByWorktree, EMPTY_TABS_BY_WORKTREE));
	const ptyIdsByTabId = useAppStore((s) => selectMenuScopedMap(menuOpen, s.ptyIdsByTabId, EMPTY_PTY_IDS_BY_TAB_ID));
	const browserTabsByWorktree = useAppStore((s) => selectMenuScopedMap(menuOpen, s.browserTabsByWorktree, EMPTY_BROWSER_TABS_BY_WORKTREE));
	const deleteStateByWorktreeId = useAppStore((s) => selectMenuScopedMap(menuOpen, s.deleteStateByWorktreeId, EMPTY_DELETE_STATE_BY_WORKTREE_ID));
	const scopeRef = (0, import_react.useRef)(null);
	const contextMenuOpenedAtRef = (0, import_react.useRef)(null);
	const activeContextWorktrees = menuOpen ? contextWorktrees : effectiveSelectedWorktrees;
	const isMultiContext = activeContextWorktrees.length > 1;
	const workspaceScope = parseWorkspaceKey(worktree.id);
	const folderWorkspaceId = workspaceScope?.type === "folder" ? workspaceScope.folderWorkspaceId : null;
	const sleepableWorktrees = (0, import_react.useMemo)(() => activeContextWorktrees.filter((item) => hasSleepableWorkspaceActivity(item.id, {
		tabsByWorktree,
		ptyIdsByTabId,
		browserTabsByWorktree
	})), [
		activeContextWorktrees,
		browserTabsByWorktree,
		ptyIdsByTabId,
		tabsByWorktree
	]);
	const lineageMenuActions = useWorkspaceLineageMenuActions({
		enabled: !isMultiContext,
		parent: worktree,
		worktrees: allWorktrees,
		lineageById: worktreeLineageById,
		activity: {
			tabsByWorktree,
			ptyIdsByTabId,
			browserTabsByWorktree
		}
	});
	const lineageDescendantCount = lineageMenuActions.descendants.length;
	const subtreeSleepableWorktrees = lineageMenuActions.sleepableTargets;
	const deletingContext = (0, import_react.useMemo)(() => activeContextWorktrees.some((item) => deleteStateByWorktreeId[item.id]?.isDeleting), [activeContextWorktrees, deleteStateByWorktreeId]);
	const deletingSubtree = lineageMenuActions.targets.some((item) => deleteStateByWorktreeId[item.id]?.isDeleting);
	const contextDeletePending = isMultiContext ? deletingContext : deletingSubtree;
	const contextWorkspaceStatus = (0, import_react.useMemo)(() => {
		const [first, ...rest] = activeContextWorktrees;
		if (!first) return "";
		const status = getWorkspaceStatus(first, workspaceStatuses);
		return rest.every((item) => getWorkspaceStatus(item, workspaceStatuses) === status) ? status : "";
	}, [activeContextWorktrees, workspaceStatuses]);
	const batchDeleteWorktrees = (0, import_react.useMemo)(() => activeContextWorktrees.filter((item) => {
		return isContextWorktreeDeletable(item, repoMap.get(item.repoId));
	}), [activeContextWorktrees, repoMap]);
	const removesProject = shouldRemoveProjectFromContextMenu(repo, worktree);
	const sleepLabel = isMultiContext && sleepableWorktrees.length > 0 ? `Sleep ${sleepableWorktrees.length} Workspace${sleepableWorktrees.length === 1 ? "" : "s"}` : "Sleep";
	const deleteLabel = isMultiContext && batchDeleteWorktrees.length > 0 ? `Delete ${batchDeleteWorktrees.length} Workspace${batchDeleteWorktrees.length === 1 ? "" : "s"}` : "Delete Selected";
	const hasParentLink = hasWorktreeParentLink(worktree, worktreeLineageById, workspaceLineageByChildKey);
	const cyclicLineageIds = (0, import_react.useMemo)(() => menuOpen ? getCyclicProjectedWorktreeLineageIds(worktreeLineageById, worktreeMap) : EMPTY_CYCLIC_LINEAGE_IDS, [
		menuOpen,
		worktreeLineageById,
		worktreeMap
	]);
	const lineageInfo = (0, import_react.useMemo)(() => getLineageRenderInfo(worktree, worktreeLineageById, worktreeMap, cyclicLineageIds), [
		cyclicLineageIds,
		worktree,
		worktreeLineageById,
		worktreeMap
	]);
	const validParentWorktreeId = lineageInfo.state === "valid" ? lineageInfo.parent.id : null;
	const hasAnyContextLineage = activeContextWorktrees.some((item) => hasWorktreeParentLink(item, worktreeLineageById, workspaceLineageByChildKey));
	const eligibleParentCount = (0, import_react.useMemo)(() => menuOpen ? getEligibleWorktreeParents({
		child: worktree,
		worktrees: allWorktrees,
		lineageById: worktreeLineageById,
		worktreeMap,
		repoMap,
		cyclicLineageIds
	}).length : 0, [
		allWorktrees,
		cyclicLineageIds,
		menuOpen,
		repoMap,
		worktree,
		worktreeLineageById,
		worktreeMap
	]);
	const setMenuOpenState = (0, import_react.useCallback)((open) => {
		setMenuOpen(open);
		if (!open) setDeveloperMenuRevealed(false);
		onOpenChange?.(open);
	}, [onOpenChange]);
	(0, import_react.useEffect)(() => {
		if (!onLifecycleComplete) return;
		if (menuOpen) lifecycleStartedRef.current = true;
		if (!lifecycleStartedRef.current || menuOpen || createGroupDialogOpen || createGroupDialogActiveRef.current || parentPicker !== null || pendingParentPickerRef.current !== null) return;
		const timer = window.setTimeout(() => {
			if (createGroupDialogActiveRef.current || pendingParentPickerRef.current !== null) return;
			lifecycleStartedRef.current = false;
			onLifecycleComplete?.();
		}, 0);
		return () => window.clearTimeout(timer);
	}, [
		createGroupDialogOpen,
		menuOpen,
		onLifecycleComplete,
		parentPicker
	]);
	(0, import_react.useEffect)(() => {
		const closeMenu = () => setMenuOpenState(false);
		window.addEventListener(CLOSE_ALL_CONTEXT_MENUS_EVENT, closeMenu);
		return () => window.removeEventListener(CLOSE_ALL_CONTEXT_MENUS_EVENT, closeMenu);
	}, [setMenuOpenState]);
	(0, import_react.useEffect)(() => () => {
		if (parentPickerFallbackTimerRef.current != null) window.clearTimeout(parentPickerFallbackTimerRef.current);
		if (parentPickerUnmountTimerRef.current != null) window.clearTimeout(parentPickerUnmountTimerRef.current);
	}, []);
	const handleCopyPath = (0, import_react.useCallback)(() => {
		window.api.ui.writeClipboardText(worktree.path);
	}, [worktree.path]);
	const handleToggleRead = (0, import_react.useCallback)(() => {
		updateWorktreeMeta(worktree.id, { isUnread: !worktree.isUnread });
	}, [
		worktree.id,
		worktree.isUnread,
		updateWorktreeMeta
	]);
	const handleTogglePin = (0, import_react.useCallback)(() => {
		setWorktreesPinnedAndReveal([worktree.id], !worktree.isPinned);
	}, [
		worktree.id,
		worktree.isPinned,
		setWorktreesPinnedAndReveal
	]);
	const handleCreateGroupFromRepo = (0, import_react.useCallback)(() => {
		if (!repo) return;
		createGroupDialogActiveRef.current = true;
		setCreateGroupDialogOpen(true);
	}, [repo]);
	const handleCreateGroupDialogOpenChange = (0, import_react.useCallback)((open) => {
		createGroupDialogActiveRef.current = open;
		setCreateGroupDialogOpen(open);
	}, []);
	const handleSubmitNewProjectGroup = (0, import_react.useCallback)(async (name) => {
		if (!repo) return;
		const group = await createProjectGroup(name);
		if (group) await moveProjectToGroup(repo.id, group.id);
	}, [
		createProjectGroup,
		moveProjectToGroup,
		repo
	]);
	const handleMoveProjectToGroup = (0, import_react.useCallback)((groupId) => {
		if (!repo || repo.projectGroupId === groupId) return;
		moveProjectToGroup(repo.id, groupId);
	}, [moveProjectToGroup, repo]);
	const handleRemoveProjectFromGroup = (0, import_react.useCallback)(() => {
		if (!repo) return;
		moveProjectToGroup(repo.id, null);
	}, [moveProjectToGroup, repo]);
	const handleAssignWorkspaceStatus = (0, import_react.useCallback)((status) => {
		setMenuOpenState(false);
		const plan = planWorkspaceStatusAssignment(activeContextWorktrees, status, workspaceStatuses, Boolean(onAssignWorkspaceStatus));
		if (plan.kind === "board-sync") {
			onAssignWorkspaceStatus?.(plan.worktreeIds, status);
			return;
		}
		Promise.all(plan.localWriteIds.map((id) => updateWorktreeMeta(id, { workspaceStatus: status })));
	}, [
		activeContextWorktrees,
		onAssignWorkspaceStatus,
		setMenuOpenState,
		updateWorktreeMeta,
		workspaceStatuses
	]);
	const handleRename = (0, import_react.useCallback)(() => {
		openModal("edit-meta", {
			worktreeId: worktree.id,
			repoId: worktree.repoId,
			currentDisplayName: worktree.displayName,
			currentIssue: worktree.linkedIssue,
			currentPR: worktree.linkedPR,
			currentComment: worktree.comment,
			focus: "displayName"
		});
	}, [
		worktree.id,
		worktree.repoId,
		worktree.displayName,
		worktree.linkedIssue,
		worktree.linkedPR,
		worktree.comment,
		openModal
	]);
	const sleepWorktreesAfterMenuClose = (0, import_react.useCallback)((worktreeIds) => {
		setMenuOpenState(false);
		window.setTimeout(() => {
			runSleepWorktrees(worktreeIds);
		}, 50);
	}, [setMenuOpenState]);
	const handleCloseTerminals = (0, import_react.useCallback)(() => {
		sleepWorktreesAfterMenuClose(sleepableWorktrees.map((item) => item.id));
	}, [sleepWorktreesAfterMenuClose, sleepableWorktrees]);
	const handleSleepSubtree = (0, import_react.useCallback)(() => {
		sleepWorktreesAfterMenuClose(subtreeSleepableWorktrees.map((item) => item.id));
	}, [sleepWorktreesAfterMenuClose, subtreeSleepableWorktrees]);
	const handleDelete = (0, import_react.useCallback)(() => {
		const restoreSidebarPosition = preserveDeleteSiblingPosition(scopeRef.current);
		scopeRef.current?.closest("[data-worktree-sidebar]")?.dispatchEvent(new Event(VIRTUALIZED_SCROLL_ANCHOR_RECORD_EVENT));
		deferWorktreeContextMenuDeleteIntent(isMultiContext ? {
			kind: "batch",
			worktrees: batchDeleteWorktrees.map(({ id, instanceId }) => ({
				id,
				instanceId
			}))
		} : folderWorkspaceId ? {
			kind: "folder",
			folderWorkspaceId
		} : {
			kind: "worktree",
			worktree: {
				id: worktree.id,
				instanceId: worktree.instanceId
			}
		}, restoreSidebarPosition);
		setMenuOpenState(false);
	}, [
		batchDeleteWorktrees,
		folderWorkspaceId,
		isMultiContext,
		setMenuOpenState,
		worktree.id,
		worktree.instanceId
	]);
	const handleOpenParent = (0, import_react.useCallback)(() => {
		if (validParentWorktreeId) activateAndRevealWorktree(validParentWorktreeId);
	}, [validParentWorktreeId]);
	const openPendingParentPicker = (0, import_react.useCallback)(() => {
		const pendingParentPicker = pendingParentPickerRef.current;
		if (!pendingParentPicker) return;
		pendingParentPickerRef.current = null;
		if (parentPickerFallbackTimerRef.current != null) {
			window.clearTimeout(parentPickerFallbackTimerRef.current);
			parentPickerFallbackTimerRef.current = null;
		}
		if (parentPickerUnmountTimerRef.current != null) {
			window.clearTimeout(parentPickerUnmountTimerRef.current);
			parentPickerUnmountTimerRef.current = null;
		}
		setParentPicker(pendingParentPicker);
		setParentPickerOpen(true);
	}, []);
	const handleParentPickerOpenChange = (0, import_react.useCallback)((open) => {
		if (open) return;
		setParentPickerOpen(false);
		parentPickerUnmountTimerRef.current = window.setTimeout(() => {
			parentPickerUnmountTimerRef.current = null;
			setParentPicker(null);
		}, PARENT_PICKER_EXIT_ANIMATION_MS);
	}, []);
	const handleOpenParentPicker = (0, import_react.useCallback)((event) => {
		event?.preventDefault();
		const anchorElement = getWorktreeParentPickerAnchor(scopeRef.current, worktree.id);
		if (!anchorElement) return;
		pendingParentPickerRef.current = {
			childWorktreeId: worktree.id,
			anchorElement
		};
		setMenuOpenState(false);
		parentPickerFallbackTimerRef.current = window.setTimeout(openPendingParentPicker, 50);
	}, [
		openPendingParentPicker,
		setMenuOpenState,
		worktree.id
	]);
	const handleRemoveParentLink = (0, import_react.useCallback)(() => {
		Promise.all(activeContextWorktrees.map((item) => updateWorktreeLineage(item.id, { noParent: true })));
	}, [activeContextWorktrees, updateWorktreeLineage]);
	const suppressOpeningPointerEvent = (0, import_react.useCallback)((event) => {
		const contextMenuOpenedAt = contextMenuOpenedAtRef.current;
		if (contextMenuOpenedAt == null || !shouldSuppressContextMenuFollowUpClick(contextMenuOpenedAt, Date.now())) {
			if (contextMenuOpenedAt != null) contextMenuOpenedAtRef.current = null;
			return;
		}
		event.preventDefault();
		event.stopPropagation();
		if (event.type === "click") contextMenuOpenedAtRef.current = null;
	}, []);
	const handleCloseAutoFocus = (0, import_react.useCallback)((event) => {
		event.preventDefault();
		if (pendingParentPickerRef.current) {
			window.setTimeout(openPendingParentPicker, 0);
			return;
		}
		const sidebar = scopeRef.current?.closest("[data-worktree-sidebar]");
		if (sidebar instanceof HTMLElement) sidebar.focus({ preventScroll: true });
	}, [openPendingParentPicker]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: scopeRef,
		className: "relative",
		[WORKTREE_CONTEXT_MENU_SCOPE_ATTR]: "worktree",
		onContextMenuCapture: (event) => {
			if (!isEventTargetInsideCurrentTarget(event.currentTarget, event.target)) return;
			if (shouldUseNativeContextMenu(event.target)) return;
			if (shouldIgnoreNestedWorktreeContextMenuScope(event.currentTarget, event.target)) return;
			event.preventDefault();
			contextMenuOpenedAtRef.current = Date.now();
			window.dispatchEvent(new Event(CLOSE_ALL_CONTEXT_MENUS_EVENT));
			setDeveloperMenuRevealed(event.altKey);
			setContextWorktrees(onContextMenuSelect?.(event) ?? effectiveSelectedWorktrees);
			const bounds = event.currentTarget.getBoundingClientRect();
			setMenuPoint({
				x: event.clientX - bounds.left,
				y: event.clientY - bounds.top
			});
			setMenuOpenState(true);
		},
		onClickCapture: (event) => {
			suppressOpeningPointerEvent(event);
		},
		children: [
			children,
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, {
				open: menuOpen,
				onOpenChange: setMenuOpenState,
				modal: false,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						"aria-hidden": true,
						tabIndex: -1,
						className: "pointer-events-none absolute size-px opacity-0",
						style: {
							left: menuPoint.x,
							top: menuPoint.y
						}
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuContent, {
					className: cn(lineageDescendantCount > 0 ? "w-60" : "w-52", contentClassName),
					sideOffset: 0,
					align: "start",
					onPointerUpCapture: suppressOpeningPointerEvent,
					onPointerDownCapture: (event) => {
						if (event.button === 0) contextMenuOpenedAtRef.current = null;
					},
					onMouseUpCapture: suppressOpeningPointerEvent,
					onClickCapture: suppressOpeningPointerEvent,
					onCloseAutoFocus: handleCloseAutoFocus,
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, {
							className: "px-2 py-1 text-[11px] font-medium text-muted-foreground",
							children: translate("auto.components.sidebar.WorktreeContextMenu.workspaceSection", "Workspace")
						}),
						!isMultiContext && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
							onSelect: handleRename,
							disabled: isDeleting,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pencil, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeContextMenu.439fa94d53", "Update")]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSub, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSubTrigger, {
							disabled: deletingContext,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Kanban, { className: "size-3.5" }), isMultiContext ? translate("auto.components.sidebar.WorktreeContextMenu.56cde9e8e6", "Move Statuses To") : translate("auto.components.sidebar.WorktreeContextMenu.84cdbb7e30", "Move to Status")]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSubContent, {
							className: "w-44",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuRadioGroup, {
								value: contextWorkspaceStatus,
								children: workspaceStatuses.map((status) => {
									const meta = getWorkspaceStatusVisualMeta(status);
									return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuRadioItem, {
										value: status.id,
										onSelect: () => handleAssignWorkspaceStatus(status.id),
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(meta.icon, { className: cn("size-3.5", meta.tone) }), status.label]
									}, status.id);
								})
							})
						})] }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
						!isMultiContext && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeOpenInSubMenu, {
								worktreePath: worktree.path,
								connectionId: repo?.connectionId ?? null,
								disabled: isDeleting
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
								onSelect: handleCopyPath,
								disabled: isDeleting,
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeContextMenu.3350101edb", "Copy Path")]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
								onSelect: handleTogglePin,
								disabled: isDeleting,
								children: [worktree.isPinned ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PinOff, { className: "size-3.5" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pin, { className: "size-3.5" }), worktree.isPinned ? translate("auto.components.sidebar.WorktreeContextMenu.697d0f6e1b", "Unpin") : translate("auto.components.sidebar.WorktreeContextMenu.3baa7d6507", "Pin")]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
								onSelect: handleToggleRead,
								disabled: isDeleting,
								children: [worktree.isUnread ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(BellOff, { className: "size-3.5" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Bell, { className: "size-3.5" }), worktree.isUnread ? translate("auto.components.sidebar.WorktreeContextMenu.8dacff1fe0", "Mark Read") : translate("auto.components.sidebar.WorktreeContextMenu.f50603c6b2", "Mark Unread")]
							}),
							repo ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
									onSelect: handleCreateGroupFromRepo,
									disabled: isDeleting,
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderPlus, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeContextMenu.503ec0f8e6", "New group from project")]
								}),
								projectGroups.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSub, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSubTrigger, {
									disabled: isDeleting,
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderInput, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeContextMenu.76865d827f", "Move to group")]
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSubContent, { children: projectGroups.map((group) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuItem, {
									disabled: repo.projectGroupId === group.id,
									onSelect: () => handleMoveProjectToGroup(group.id),
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "max-w-48 truncate",
										children: group.name
									})
								}, group.id)) })] }) : null,
								repo.projectGroupId ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
									onSelect: handleRemoveProjectFromGroup,
									disabled: isDeleting,
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleX, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeContextMenu.d35dfeae58", "Remove from group")]
								}) : null
							] }) : null,
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
								onSelect: handleOpenParentPicker,
								disabled: isWorktreeParentPickerDisabled({
									isDeleting,
									eligibleParentCount
								}),
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderTree, { className: "size-3.5" }), getWorktreeParentPickerLabel(validParentWorktreeId)]
							}),
							(validParentWorktreeId || hasParentLink) && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
								validParentWorktreeId && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
									onSelect: handleOpenParent,
									disabled: isDeleting,
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Workflow, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeContextMenu.8d9cd19d09", "Open Parent Worktree")]
								}),
								hasParentLink && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
									onSelect: handleRemoveParentLink,
									disabled: isDeleting,
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Unlink, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeContextMenu.579b1a8e61", "Remove from Parent")]
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {})
							] })
						] }),
						isMultiContext && hasAnyContextLineage ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
							onSelect: handleRemoveParentLink,
							disabled: deletingContext,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Unlink, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeContextMenu.579b1a8e61", "Remove from Parent")]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {})] }) : null,
						shouldRevealWorktreeDeveloperMenu({
							developerMenuRevealed,
							isMultiContext
						}) ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeDeveloperMenu, {
							worktreeId: worktree.id,
							disabled: isDeleting
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {})] }) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceSleepMenuItems, {
							isMultiContext,
							sleepLabel,
							sleepDisabled: deletingContext || sleepableWorktrees.length === 0,
							descendantCount: lineageDescendantCount,
							subtreeSleepDisabled: deletingSubtree || subtreeSleepableWorktrees.length === 0,
							onSleep: handleCloseTerminals,
							onSleepSubtree: handleSleepSubtree
						}),
						!isMultiContext && removesProject ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
							asChild: true,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
								variant: "destructive",
								disabled: true,
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeContextMenu.deleteWorktree", "Delete Worktree")]
							}) })
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
							side: "right",
							sideOffset: 8,
							className: "max-w-[200px] text-pretty",
							children: translate("auto.components.sidebar.WorktreeContextMenu.primaryDeleteDisabled", "Primary worktree — can't be deleted. Remove the project instead.")
						})] }) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
							variant: "destructive",
							onSelect: handleDelete,
							disabled: contextDeletePending || !isMultiContext && worktree.isMainWorktree && !removesProject || isMultiContext && batchDeleteWorktrees.length === 0,
							title: !isMultiContext && worktree.isMainWorktree && !removesProject ? translate("auto.components.sidebar.WorktreeContextMenu.e091caab15", "The project could not be found") : void 0,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3.5" }), contextDeletePending ? translate("auto.components.sidebar.WorktreeContextMenu.b42391d8bf", "Deleting…") : isMultiContext ? deleteLabel : folderWorkspaceId ? translate("auto.components.sidebar.WorktreeContextMenu.250de158fd", "Remove Workspace") : removesProject ? translate("auto.components.sidebar.WorktreeContextMenu.f5ac91531d", "Remove Project from Orca") : lineageDescendantCount > 0 ? translate("auto.components.sidebar.WorktreeContextMenu.deleteWithDescendants", "Delete with Descendants…") : translate("auto.components.sidebar.WorktreeContextMenu.f4475537d8", "Delete")]
						})
					]
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ProjectGroupNameDialog, {
				open: createGroupDialogOpen,
				title: translate("auto.components.sidebar.WorktreeContextMenu.6664418e98", "New Project Group"),
				description: translate("auto.components.sidebar.WorktreeContextMenu.c39c37676a", "Create a group and move this project into it."),
				initialName: repo ? `${repo.displayName} group` : "",
				confirmLabel: "Create",
				onOpenChange: handleCreateGroupDialogOpenChange,
				onSubmit: handleSubmitNewProjectGroup
			}),
			parentPicker ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeParentPickerPopover, {
				open: parentPickerOpen,
				childWorktreeId: parentPicker.childWorktreeId,
				anchorElement: parentPicker.anchorElement,
				onOpenChange: handleParentPickerOpenChange
			}) : null
		]
	});
});
export { FolderInput as C, Kanban as S, shouldUseNativeContextMenu as _, getWorktreeParentPickerAnchor as a, selectWorktreeAgentActivitySummary as b, isContextWorktreeDeletable as c, selectMenuScopedMap as d, shouldContinueDeleteSiblingPositionRestore as f, shouldSuppressContextMenuFollowUpClick as g, shouldRevealWorktreeDeveloperMenu as h, WorktreeContextMenu_default as i, isWorktreeParentPickerDisabled as l, shouldRemoveProjectFromContextMenu as m, WORKTREE_CONTEXT_MENU_SCOPE_ATTR as n, getWorktreeParentPickerLabel as o, shouldIgnoreNestedWorktreeContextMenuScope as p, WORKTREE_NATIVE_CONTEXT_MENU_ATTR as r, hasWorktreeParentLink as s, CLOSE_ALL_CONTEXT_MENUS_EVENT as t, planWorkspaceStatusAssignment as u, isEventTargetInsideCurrentTarget as v, ProjectGroupNameDialog as x, isEligibleWorktreeParent as y };
