import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { s as ORCA_RENDERER_UNLOAD_PREVENTED_EVENT } from "./lazy-with-retry-pSZJrSfN.js";
import { C as cancelPendingSimulatorPaneShutdown, S as rememberPrelaunchedSimulatorSession, T as shutdownManagedSimulatorIfNoPane, b as finishManualSimulatorLaunch, g as beginManualSimulatorLaunch, v as dispatchManualSimulatorLaunchFailed, x as isManualSimulatorLaunchPending, y as dispatchManualSimulatorLaunchStarted } from "./browser-automation-visibility-1UB1fwYb.js";
import { t as Keyboard } from "./keyboard-C3CJE6-G.js";
import { Bu as sanitizeRecentTabIds, E as assertClientCreationActionAvailable, P as normalizeBrowserHistoryEntries, dd as parseAppSshPtyId, rp as callRuntimeRpc, t as useAppStore, vo as getActiveTabNavOrder } from "./store-CgXrfmaH.js";
import { t as BACKGROUND_MOUNT_TERMINAL_WORKTREE_EVENT } from "./terminal-C-BGupDh.js";
import { f as formatKeybindingList, h as getKeybindingDefinition, p as getEffectiveKeybindingsForAction, w as keybindingMatchesAction, x as isKeybindingPotentialTerminalConflict } from "./plugin-manifest-Bs-50M_g.js";
import { nt as isRuntimeOwnedSshTargetId, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { Dt as getNextTabAcrossAllTypes, Et as getActiveEntityIdForTabType, Ot as getNextTabWithinActiveType, mt as pruneLocalTerminalScrollbackBuffers } from "./remote-runtime-pty-recovery-state-CcyktY20.js";
var pendingMounts = /* @__PURE__ */ new Map();
var requestListeners = /* @__PURE__ */ new Set();
var hasRequestedMount = false;
function mergePendingMount(detail) {
	const existing = pendingMounts.get(detail.worktreeId);
	if (!existing) {
		pendingMounts.set(detail.worktreeId, {
			worktreeId: detail.worktreeId,
			...detail.tabIds !== void 0 ? { tabIds: [...new Set(detail.tabIds)] } : {}
		});
		return;
	}
	if (existing.tabIds === void 0 || detail.tabIds === void 0) {
		pendingMounts.set(detail.worktreeId, { worktreeId: detail.worktreeId });
		return;
	}
	pendingMounts.set(detail.worktreeId, {
		worktreeId: detail.worktreeId,
		tabIds: [...new Set([...existing.tabIds, ...detail.tabIds])]
	});
}
function requestBackgroundTerminalWorktreeMount(detail) {
	if (!detail.worktreeId) return;
	mergePendingMount(detail);
	if (!hasRequestedMount) {
		hasRequestedMount = true;
		for (const listener of requestListeners) listener();
	}
	if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent(BACKGROUND_MOUNT_TERMINAL_WORKTREE_EVENT, { detail }));
}
function takePendingBackgroundTerminalWorktreeMount(worktreeId) {
	if (!worktreeId) return null;
	const pending = pendingMounts.get(worktreeId) ?? null;
	pendingMounts.delete(worktreeId);
	return pending;
}
function takeAllPendingBackgroundTerminalWorktreeMounts() {
	const pending = [...pendingMounts.values()];
	pendingMounts.clear();
	return pending;
}
function subscribeBackgroundTerminalWorktreeMountRequests(listener) {
	requestListeners.add(listener);
	return () => requestListeners.delete(listener);
}
function hasRequestedBackgroundTerminalWorktreeMount() {
	return hasRequestedMount;
}
function addBackgroundMountedTerminalWorktree(mountedWorktreeIds, worktreeId, onAdded) {
	if (!worktreeId || mountedWorktreeIds.has(worktreeId)) return false;
	mountedWorktreeIds.add(worktreeId);
	onAdded();
	return true;
}
function applyBackgroundMountTabRestriction(restrictions, mountedWorktreeIds, worktreeId, tabIds) {
	if (!worktreeId) return;
	const existing = restrictions.get(worktreeId);
	if (mountedWorktreeIds.has(worktreeId) && !existing) return;
	if (!tabIds) {
		restrictions.delete(worktreeId);
		return;
	}
	if (existing && tabIds.every((tabId) => existing.has(tabId))) return;
	restrictions.set(worktreeId, new Set([...existing ?? [], ...tabIds]));
}
function shouldMountBackgroundWorktreeTab(restrictedTabIds, tabId) {
	return restrictedTabIds === null || restrictedTabIds.has(tabId);
}
function canMountTerminalWorkspaceForStartup(args) {
	return args.workspaceSessionReady && (args.hydrationSucceeded || args.startupWorktreeRefreshCompleted);
}
function canDeferColdActivationTabsForHost(args) {
	const host = parseExecutionHostId(args.executionHostId);
	if (host?.kind === "local") return true;
	return host?.kind === "runtime" && args.pairedRuntimeParkingEnvironmentIds?.has(host.environmentId) === true;
}
function replaceActivationDeferredMountTabs(deferredMountTabIdsByWorktree, worktreeId, restrictedTabIds, allTabIds) {
	const next = collectDeferredMountTabIds(restrictedTabIds, allTabIds);
	if (next.size === 0) {
		deferredMountTabIdsByWorktree.delete(worktreeId);
		return;
	}
	const current = deferredMountTabIdsByWorktree.get(worktreeId);
	if (current?.size === next.size && Array.from(next).every((tabId) => current.has(tabId))) return;
	deferredMountTabIdsByWorktree.set(worktreeId, next);
}
function planColdActivationTabDeferral(opts) {
	const { restrictions, deferredMountTabIdsByWorktree, worktreeId, allTabIds, isTabLive, isTabDeferrable, immediateTabIds } = opts;
	const previouslyAllowed = restrictions.get(worktreeId);
	const initial = /* @__PURE__ */ new Set();
	for (const tabId of allTabIds) if (isTabLive(tabId) || immediateTabIds.has(tabId) || previouslyAllowed?.has(tabId) || !isTabDeferrable(tabId)) initial.add(tabId);
	if (allTabIds.length - initial.size <= 4) {
		restrictions.delete(worktreeId);
		deferredMountTabIdsByWorktree.delete(worktreeId);
		return false;
	}
	restrictions.set(worktreeId, initial);
	replaceActivationDeferredMountTabs(deferredMountTabIdsByWorktree, worktreeId, initial, allTabIds);
	return true;
}
function revealActivationDeferredTabs(opts) {
	const { restrictions, deferredMountTabIdsByWorktree, worktreeId, allTabIds, immediateTabIds } = opts;
	if (!deferredMountTabIdsByWorktree.has(worktreeId)) return;
	const existing = restrictions.get(worktreeId);
	if (!existing) {
		deferredMountTabIdsByWorktree.delete(worktreeId);
		return;
	}
	let grew = false;
	for (const tabId of immediateTabIds) if (!existing.has(tabId)) {
		grew = true;
		break;
	}
	const next = grew ? new Set([...existing, ...immediateTabIds]) : existing;
	if (allTabIds.length > 0 && allTabIds.every((tabId) => next.has(tabId))) {
		restrictions.delete(worktreeId);
		deferredMountTabIdsByWorktree.delete(worktreeId);
		return;
	}
	if (grew) restrictions.set(worktreeId, next);
	replaceActivationDeferredMountTabs(deferredMountTabIdsByWorktree, worktreeId, next, allTabIds);
}
function collectDeferredMountTabIds(restrictedTabIds, tabIds) {
	const deferred = /* @__PURE__ */ new Set();
	if (restrictedTabIds === null) return deferred;
	for (const tabId of tabIds) if (!restrictedTabIds.has(tabId)) deferred.add(tabId);
	return deferred;
}
function pruneClosedBackgroundMountTabs(restrictions, mountedWorktreeIds, tabsByWorktree, deferredMountTabIdsByWorktree) {
	let changed = false;
	for (const [worktreeId, tabIds] of restrictions) {
		const liveTabIds = new Set((tabsByWorktree[worktreeId] ?? []).map((tab) => tab.id));
		const retained = new Set([...tabIds].filter((tabId) => liveTabIds.has(tabId)));
		const deferred = deferredMountTabIdsByWorktree?.get(worktreeId);
		const retainedDeferred = deferred ? new Set([...deferred].filter((tabId) => liveTabIds.has(tabId))) : null;
		if (deferred !== void 0 && retainedDeferred?.size !== deferred.size) {
			changed = true;
			if (retainedDeferred && retainedDeferred.size > 0) deferredMountTabIdsByWorktree?.set(worktreeId, retainedDeferred);
			else {
				deferredMountTabIdsByWorktree?.delete(worktreeId);
				restrictions.delete(worktreeId);
				continue;
			}
		}
		if (retained.size === tabIds.size) continue;
		changed = true;
		if (retained.size === 0) if (retainedDeferred && retainedDeferred.size > 0) restrictions.set(worktreeId, retained);
		else {
			restrictions.delete(worktreeId);
			mountedWorktreeIds.delete(worktreeId);
		}
		else restrictions.set(worktreeId, retained);
	}
	return changed;
}
function resolveCycleContext() {
	const store = useAppStore.getState();
	const worktreeId = store.activeWorktreeId;
	if (!worktreeId) return null;
	const allTabIds = getActiveTabNavOrder(store, worktreeId);
	if (allTabIds.length <= 1) return null;
	const activeGroupId = store.activeGroupIdByWorktree[worktreeId];
	const group = activeGroupId ? (store.groupsByWorktree[worktreeId] ?? []).find((candidate) => candidate.id === activeGroupId) : void 0;
	return {
		store,
		worktreeId,
		allTabIds,
		groupTabIdInNav: group?.activeTabId && allTabIds.some((entry) => entry.tabId === group.activeTabId) ? group.activeTabId : null
	};
}
function activateCyclableTab(store, next) {
	if (next.type === "terminal") {
		store.setActiveTab(next.id);
		store.setActiveTabType("terminal");
	} else if (next.type === "browser") {
		store.setActiveBrowserTab(next.id);
		if (next.tabId) store.activateTab?.(next.tabId);
		store.setActiveTabType("browser");
	} else if (next.type === "simulator") {
		store.setActiveTab(next.tabId ?? next.id);
		if (next.tabId) store.activateTab?.(next.tabId);
		store.setActiveTabType("simulator");
	} else {
		store.setActiveFile(next.id);
		if (next.tabId) store.activateTab?.(next.tabId);
		store.setActiveTabType("editor");
	}
}
function handleSwitchTab(direction) {
	const ctx = resolveCycleContext();
	if (!ctx) return false;
	const { store, allTabIds, groupTabIdInNav } = ctx;
	const next = getNextTabWithinActiveType({
		tabs: allTabIds,
		activeTabType: store.activeTabType,
		activeTabId: store.activeTabId,
		activeFileId: store.activeFileId,
		activeBrowserTabId: store.activeBrowserTabId,
		activeGroupTabId: groupTabIdInNav,
		direction
	});
	if (!next) return false;
	activateCyclableTab(store, next);
	return true;
}
function handleSwitchTabAcrossAllTypes(direction) {
	const ctx = resolveCycleContext();
	if (!ctx) return false;
	const { store, allTabIds, groupTabIdInNav } = ctx;
	const next = getNextTabAcrossAllTypes({
		tabs: allTabIds,
		activeTabType: store.activeTabType,
		activeTabId: store.activeTabId,
		activeFileId: store.activeFileId,
		activeBrowserTabId: store.activeBrowserTabId,
		activeGroupTabId: groupTabIdInNav,
		direction
	});
	if (!next) return false;
	activateCyclableTab(store, next);
	return true;
}
function handleSwitchRecentTab() {
	const ctx = resolveCycleContext();
	if (!ctx) return false;
	const { store, worktreeId, allTabIds, groupTabIdInNav } = ctx;
	if (!groupTabIdInNav) return false;
	const groupId = store.activeGroupIdByWorktree[worktreeId];
	const group = groupId ? (store.groupsByWorktree[worktreeId] ?? []).find((candidate) => candidate.id === groupId) : void 0;
	if (!group?.recentTabIds) return false;
	const visibleTabIds = allTabIds.flatMap((entry) => entry.tabId ? [entry.tabId] : []);
	const recentTabIds = sanitizeRecentTabIds(group.recentTabIds, visibleTabIds);
	const currentIndex = recentTabIds.lastIndexOf(groupTabIdInNav);
	if (currentIndex <= 0) return false;
	const previousRecentTabId = recentTabIds[currentIndex - 1];
	const next = allTabIds.find((entry) => entry.tabId === previousRecentTabId);
	if (!next) return false;
	activateCyclableTab(store, next);
	return true;
}
function handleSwitchTerminalTab(direction) {
	const store = useAppStore.getState();
	const worktreeId = store.activeWorktreeId;
	if (!worktreeId) return false;
	const terminalTabs = getActiveTabNavOrder(store, worktreeId).filter((entry) => entry.type === "terminal");
	if (terminalTabs.length === 0) return false;
	const currentId = getActiveEntityIdForTabType(store.activeTabType, store.activeTabId, store.activeFileId, store.activeBrowserTabId);
	const idx = terminalTabs.findIndex((t) => t.id === currentId);
	if (terminalTabs.length === 1 && idx === 0) return false;
	const next = terminalTabs[((idx === -1 && direction > 0 ? -1 : idx === -1 ? 0 : idx) + direction + terminalTabs.length) % terminalTabs.length];
	if (next.id === store.activeTabId && store.activeTabType === "terminal") return false;
	store.setActiveTab(next.id);
	store.setActiveTabType("terminal");
	return true;
}
function firstLeafGroupId(node) {
	if (node.type === "leaf") return node.groupId;
	return firstLeafGroupId(node.first);
}
function findReusableRightSplitTarget(node, sourceGroupId) {
	if (node.type === "leaf") return {
		containsSource: node.groupId === sourceGroupId,
		reusableGroupId: null
	};
	const first = findReusableRightSplitTarget(node.first, sourceGroupId);
	if (first.containsSource) return {
		containsSource: true,
		reusableGroupId: first.reusableGroupId ?? (node.direction === "horizontal" ? firstLeafGroupId(node.second) : null)
	};
	const second = findReusableRightSplitTarget(node.second, sourceGroupId);
	return {
		containsSource: second.containsSource,
		reusableGroupId: second.reusableGroupId
	};
}
function findReusableRightSplitGroupId(layout, sourceGroupId) {
	if (!layout) return null;
	return findReusableRightSplitTarget(layout, sourceGroupId).reusableGroupId;
}
function getSimulatorTabForWorktree(worktreeId) {
	return (useAppStore.getState().unifiedTabsByWorktree[worktreeId] ?? []).find((tab) => tab.contentType === "simulator") ?? null;
}
function ensureSimulatorTab(worktreeId, options) {
	const store = useAppStore.getState();
	if (store.settings?.mobileEmulatorEnabled === false) return null;
	const sourceGroupId = options?.targetGroupId ?? store.activeGroupIdByWorktree[worktreeId] ?? store.groupsByWorktree[worktreeId]?.[0]?.id;
	if (!sourceGroupId) return null;
	cancelPendingSimulatorPaneShutdown(worktreeId);
	const existing = getSimulatorTabForWorktree(worktreeId);
	const shouldSurface = options?.surfacePane ?? true;
	if (existing) {
		if (shouldSurface && store.activeWorktreeId === worktreeId) {
			store.activateTab(existing.id);
			store.focusGroup(worktreeId, existing.groupId);
			store.setActiveTabType("simulator");
		}
		return existing.id;
	}
	if (options?.placement === "rightSplit" && shouldSurface) {
		const reusableRightGroupId = findReusableRightSplitGroupId(store.layoutByWorktree[worktreeId], sourceGroupId);
		if (reusableRightGroupId) {
			const tab$1 = store.createUnifiedTab(worktreeId, "simulator", {
				label: translate("auto.lib.ensure.simulator.tab.372d21d428", "Mobile Emulator"),
				targetGroupId: reusableRightGroupId,
				activate: true
			});
			store.activateTab(tab$1.id);
			store.setActiveTabType("simulator");
			store.focusGroup(worktreeId, tab$1.groupId);
			return tab$1.id;
		}
		const splitTab = store.createUnifiedTabInSplit(worktreeId, "simulator", {
			sourceGroupId,
			splitDirection: "right"
		}, {
			label: translate("auto.lib.ensure.simulator.tab.372d21d428", "Mobile Emulator"),
			activate: true
		});
		if (splitTab) return splitTab.id;
	}
	const tab = store.createUnifiedTab(worktreeId, "simulator", {
		label: translate("auto.lib.ensure.simulator.tab.372d21d428", "Mobile Emulator"),
		targetGroupId: sourceGroupId,
		activate: shouldSurface
	});
	if (shouldSurface) {
		store.activateTab(tab.id);
		store.setActiveTabType("simulator");
		store.focusGroup(worktreeId, tab.groupId);
	}
	return tab.id;
}
function dispatchPrelaunchedSession(worktreeId, info) {
	if (typeof window === "undefined") return;
	window.setTimeout(() => {
		window.dispatchEvent(new CustomEvent("orca:emulator-auto-attach", { detail: {
			worktreeId,
			info
		} }));
	}, 0);
}
function getLaunchErrorMessage(error) {
	if (error instanceof Error && error.message) return error.message;
	return translate("auto.lib.open.mobile.emulator.tab.bf4f2a8a72", "Could not start the emulator. Check iOS or Android emulator setup and try another device.");
}
async function openMobileEmulatorTab(worktreeId, options = {}) {
	const store = useAppStore.getState();
	assertClientCreationActionAvailable(store, worktreeId, "mobile-emulator");
	if (store.settings?.mobileEmulatorEnabled === false) return null;
	const existingTab = getSimulatorTabForWorktree(worktreeId);
	if (existingTab) return existingTab.id;
	const targetGroupId = options.targetGroupId ?? store.activeGroupIdByWorktree[worktreeId] ?? store.groupsByWorktree[worktreeId]?.[0]?.id;
	if (!targetGroupId) return null;
	cancelPendingSimulatorPaneShutdown(worktreeId);
	if (isManualSimulatorLaunchPending(worktreeId)) return ensureSimulatorTab(worktreeId, {
		placement: options.placement ?? "rightSplit",
		targetGroupId,
		surfacePane: true
	});
	beginManualSimulatorLaunch(worktreeId);
	try {
		const tabId = ensureSimulatorTab(worktreeId, {
			placement: options.placement ?? "rightSplit",
			targetGroupId,
			surfacePane: true
		});
		if (!tabId) return null;
		dispatchManualSimulatorLaunchStarted(worktreeId);
		try {
			const result = await callRuntimeRpc({ kind: "local" }, "emulator.attach", {
				worktree: worktreeId,
				focus: false
			});
			if (!result.attached || !result.info) throw new Error("Could not start the emulator.");
			if (await shutdownManagedSimulatorIfNoPane(worktreeId, tabId)) return tabId;
			rememberPrelaunchedSimulatorSession(worktreeId, result.info);
			dispatchPrelaunchedSession(worktreeId, result.info);
			return tabId;
		} catch (error) {
			const message = getLaunchErrorMessage(error);
			toast.error(message);
			dispatchManualSimulatorLaunchFailed(worktreeId, message);
			return tabId;
		}
	} finally {
		finishManualSimulatorLaunch(worktreeId);
	}
}
const TOGGLE_QUICK_COMMANDS_MENU_EVENT = "orca:toggleQuickCommandsMenu";
function dedupePersistedTabIds(tabIds) {
	return Array.from(new Set(tabIds));
}
function prunePersistedLayoutForGroups(root, validGroupIds) {
	if (root.type === "leaf") return validGroupIds.has(root.groupId) ? root : null;
	const first = prunePersistedLayoutForGroups(root.first, validGroupIds);
	const second = prunePersistedLayoutForGroups(root.second, validGroupIds);
	if (first === null) return second;
	if (second === null) return first;
	return {
		...root,
		first,
		second
	};
}
function buildPersistedGroupsForWorktree(tabs, groups) {
	const validTabIds = new Set(tabs.map((tab) => tab.id));
	const tabIdsByGroup = /* @__PURE__ */ new Map();
	for (const tab of tabs) {
		const groupTabs = tabIdsByGroup.get(tab.groupId) ?? [];
		groupTabs.push(tab.id);
		tabIdsByGroup.set(tab.groupId, groupTabs);
	}
	return groups.map((group) => {
		const tabOrder = dedupePersistedTabIds([...group.tabOrder.filter((tabId) => validTabIds.has(tabId)), ...tabIdsByGroup.get(group.id) ?? []]);
		const activeTabId = group.activeTabId && tabOrder.includes(group.activeTabId) ? group.activeTabId : null;
		return {
			...group,
			activeTabId,
			tabOrder,
			recentTabIds: group.recentTabIds?.filter((tabId) => tabOrder.includes(tabId))
		};
	}).filter((group) => group.tabOrder.length > 0);
}
function buildPersistedUnifiedTabSessionData(snapshot) {
	const unifiedTabs = {};
	const tabGroups = {};
	const tabGroupLayouts = {};
	const activeGroupIdByWorktree = {};
	const sourceTabs = snapshot.unifiedTabsByWorktree ?? {};
	const sourceGroups = snapshot.groupsByWorktree ?? {};
	const sourceLayouts = snapshot.layoutByWorktree ?? {};
	const sourceActiveGroups = snapshot.activeGroupIdByWorktree ?? {};
	const worktreeIds = new Set([
		...Object.keys(sourceTabs),
		...Object.keys(sourceGroups),
		...Object.keys(sourceLayouts)
	]);
	for (const worktreeId of worktreeIds) {
		const tabs = sourceTabs[worktreeId] ?? [];
		if (tabs.length === 0) continue;
		const groups = buildPersistedGroupsForWorktree(tabs, sourceGroups[worktreeId] ?? []);
		if (groups.length === 0) continue;
		const groupIds = new Set(groups.map((group) => group.id));
		const persistedTabs = tabs.filter((tab) => groupIds.has(tab.groupId));
		if (persistedTabs.length === 0) continue;
		unifiedTabs[worktreeId] = persistedTabs;
		tabGroups[worktreeId] = groups;
		const activeGroupId = sourceActiveGroups[worktreeId];
		activeGroupIdByWorktree[worktreeId] = activeGroupId && groupIds.has(activeGroupId) ? activeGroupId : groups[0].id;
		tabGroupLayouts[worktreeId] = (sourceLayouts[worktreeId] ? prunePersistedLayoutForGroups(sourceLayouts[worktreeId], groupIds) : null) ?? {
			type: "leaf",
			groupId: groups[0].id
		};
	}
	return {
		unifiedTabs,
		tabGroups,
		tabGroupLayouts,
		activeGroupIdByWorktree
	};
}
function buildLastVisitedAtByWorktreeId(snapshot) {
	return snapshot.lastVisitedAtByWorktreeId && Object.keys(snapshot.lastVisitedAtByWorktreeId).length > 0 ? snapshot.lastVisitedAtByWorktreeId : void 0;
}
function buildSleepingAgentSessionData(snapshot) {
	const records = snapshot.sleepingAgentSessionsByPaneKey;
	return records && Object.keys(records).length > 0 ? { sleepingAgentSessionsByPaneKey: records } : {};
}
function buildActiveConnectionIdsAtShutdown(snapshot, remoteSessionIdsByTabId) {
	const targetIds = new Set(Array.from(snapshot.sshConnectionStates.entries()).filter(([targetId, state]) => state.status === "connected" && !isRuntimeOwnedSshTargetId(targetId)).map(([targetId]) => targetId));
	for (const sessionId of Object.values(remoteSessionIdsByTabId ?? {})) {
		const connectionId = parseAppSshPtyId(sessionId)?.connectionId;
		if (!connectionId || isRuntimeOwnedSshTargetId(connectionId)) continue;
		const status = snapshot.sshConnectionStates.get(connectionId)?.status;
		if (status && status !== "disconnected" && status !== "auth-failed") targetIds.add(connectionId);
	}
	return targetIds.size > 0 ? Array.from(targetIds) : void 0;
}
function shouldPersistWorkspaceSession(state) {
	return state.workspaceSessionReady && state.hydrationSucceeded;
}
const SESSION_RELEVANT_FIELDS = [
	"activeRepoId",
	"activeWorkspaceKey",
	"activeWorkspaceExecutionHostId",
	"activeWorktreeId",
	"activeTabId",
	"tabsByWorktree",
	"ptyIdsByTabId",
	"terminalLayoutsByTabId",
	"activeTabIdByWorktree",
	"openFiles",
	"editorDrafts",
	"markdownFrontmatterVisible",
	"activeFileIdByWorktree",
	"activeTabTypeByWorktree",
	"browserTabsByWorktree",
	"browserPagesByWorkspace",
	"activeBrowserTabIdByWorktree",
	"browserUrlHistory",
	"unifiedTabsByWorktree",
	"groupsByWorktree",
	"layoutByWorktree",
	"activeGroupIdByWorktree",
	"sshConnectionStates",
	"repos",
	"worktreesByRepo",
	"lastKnownRelayPtyIdByTabId",
	"lastVisitedAtByWorktreeId",
	"defaultTerminalTabsAppliedByWorktreeId",
	"sleepingAgentSessionsByPaneKey"
];
function buildEditorSessionData(openFiles, editorDrafts, markdownFrontmatterVisible, activeFileIdByWorktree, activeTabTypeByWorktree) {
	const editFiles = openFiles.filter((f) => f.mode === "edit");
	const byWorktree = {};
	const editFileIdsByWorktree = {};
	for (const f of editFiles) {
		const arr = byWorktree[f.worktreeId] ?? (byWorktree[f.worktreeId] = []);
		const dirtyDraftContent = f.isDirty && f.readOnly !== true ? editorDrafts[f.id] : void 0;
		arr.push({
			filePath: f.filePath,
			relativePath: f.relativePath,
			worktreeId: f.worktreeId,
			language: f.language,
			isPreview: f.isPreview || void 0,
			runtimeEnvironmentId: f.runtimeEnvironmentId,
			externalSshTargetId: f.externalSshTargetId,
			...f.readOnly === true ? { readOnly: true } : {},
			...f.readOnly === true && f.liveTail === true ? { liveTail: true } : {},
			...dirtyDraftContent !== void 0 ? { dirtyDraftContent } : {},
			...dirtyDraftContent !== void 0 && f.lastKnownDiskSignature ? { lastKnownDiskSignature: f.lastKnownDiskSignature } : {}
		});
		(editFileIdsByWorktree[f.worktreeId] ?? (editFileIdsByWorktree[f.worktreeId] = /* @__PURE__ */ new Set())).add(f.id);
	}
	const activeFileEntries = [];
	for (const [worktreeId, fileId] of Object.entries(activeFileIdByWorktree)) {
		if (!fileId) continue;
		if (editFileIdsByWorktree[worktreeId]?.has(fileId)) activeFileEntries.push([worktreeId, fileId]);
	}
	const persistedActiveFileIdByWorktree = Object.fromEntries(activeFileEntries);
	const activeTabTypeEntries = [];
	for (const [worktreeId, tabType] of Object.entries(activeTabTypeByWorktree)) {
		if (tabType !== "editor") {
			activeTabTypeEntries.push([worktreeId, tabType]);
			continue;
		}
		if (persistedActiveFileIdByWorktree[worktreeId]) activeTabTypeEntries.push([worktreeId, tabType]);
	}
	const persistedActiveTabTypeByWorktree = Object.fromEntries(activeTabTypeEntries);
	const allEditFileIds = new Set(Object.values(editFileIdsByWorktree).flatMap((ids) => [...ids]));
	return {
		openFilesByWorktree: byWorktree,
		activeFileIdByWorktree: persistedActiveFileIdByWorktree,
		activeTabTypeByWorktree: persistedActiveTabTypeByWorktree,
		markdownFrontmatterVisible: Object.fromEntries(Object.entries(markdownFrontmatterVisible ?? {}).filter(([fileId]) => allEditFileIds.has(fileId)))
	};
}
function buildBrowserSessionData(browserTabsByWorktree, browserPagesByWorkspace, activeBrowserTabIdByWorktree) {
	return {
		browserTabsByWorktree: buildPersistedBrowserTabsByWorktree(browserTabsByWorktree),
		browserPagesByWorkspace: buildPersistedBrowserPagesByWorkspace(browserPagesByWorkspace),
		activeBrowserTabIdByWorktree
	};
}
function buildPersistedBrowserTabsByWorktree(browserTabsByWorktree) {
	return Object.fromEntries(Object.entries(browserTabsByWorktree).map(([worktreeId, tabs]) => [worktreeId, tabs.map((tab) => ({
		...tab,
		loading: false
	}))]));
}
function buildPersistedBrowserPagesByWorkspace(browserPagesByWorkspace) {
	return Object.fromEntries(Object.entries(browserPagesByWorkspace).map(([workspaceId, pages]) => [workspaceId, pages.map((page) => ({
		...page,
		loading: false
	}))]));
}
function buildSanitizedTabsByWorktree(tabsByWorktree) {
	return Object.fromEntries(Object.entries(tabsByWorktree).map(([worktreeId, tabs]) => [worktreeId, tabs.map((tab) => {
		const { pendingActivationSpawn: _unused, ...rest } = tab;
		return rest;
	})]));
}
function buildTerminalSessionData(snapshot) {
	const tabsByWorktree = snapshot.tabsByWorktree;
	const ptyIdsByTabId = snapshot.ptyIdsByTabId;
	const hasLivePty = (tabId) => (ptyIdsByTabId[tabId]?.length ?? 0) > 0;
	const lastKnown = snapshot.lastKnownRelayPtyIdByTabId;
	const hasReconnectableSession = (tab) => hasLivePty(tab.id) || !tab.ptyId && Boolean(lastKnown[tab.id]);
	const activeWorktreeIdsOnShutdown = Object.entries(tabsByWorktree).filter(([, tabs]) => tabs.some(hasReconnectableSession)).map(([worktreeId]) => worktreeId);
	const worktreeById = new Map(Object.values(snapshot.worktreesByRepo).flat().map((worktree) => [worktree.id, worktree]));
	const repoById = new Map(snapshot.repos.map((repo) => [repo.id, repo]));
	const remoteSessionIdsByTabId = {};
	for (const [worktreeId, tabs] of Object.entries(tabsByWorktree)) {
		const worktree = worktreeById.get(worktreeId);
		if (!(worktree ? repoById.get(worktree.repoId) : null)?.connectionId) continue;
		for (const tab of tabs) {
			if (!hasReconnectableSession(tab)) continue;
			const sessionId = tab.ptyId || lastKnown[tab.id];
			if (sessionId) remoteSessionIdsByTabId[tab.id] = sessionId;
		}
	}
	return {
		activeWorktreeIdsOnShutdown,
		remoteSessionIdsByTabId: Object.keys(remoteSessionIdsByTabId).length > 0 ? remoteSessionIdsByTabId : void 0
	};
}
function buildWorkspaceSessionPayload(snapshot) {
	const terminalSessionData = buildTerminalSessionData(snapshot);
	return pruneLocalTerminalScrollbackBuffers({
		activeRepoId: snapshot.activeRepoId,
		activeWorkspaceKey: snapshot.activeWorkspaceKey,
		activeWorkspaceExecutionHostId: snapshot.activeWorkspaceExecutionHostId,
		activeWorktreeId: snapshot.activeWorktreeId,
		activeTabId: snapshot.activeTabId,
		tabsByWorktree: buildSanitizedTabsByWorktree(snapshot.tabsByWorktree),
		terminalLayoutsByTabId: snapshot.terminalLayoutsByTabId,
		activeWorktreeIdsOnShutdown: terminalSessionData.activeWorktreeIdsOnShutdown,
		activeTabIdByWorktree: snapshot.activeTabIdByWorktree,
		...buildEditorSessionData(snapshot.openFiles, snapshot.editorDrafts, snapshot.markdownFrontmatterVisible, snapshot.activeFileIdByWorktree, snapshot.activeTabTypeByWorktree),
		...buildBrowserSessionData(snapshot.browserTabsByWorktree, snapshot.browserPagesByWorkspace, snapshot.activeBrowserTabIdByWorktree),
		browserUrlHistory: normalizeBrowserHistoryEntries(snapshot.browserUrlHistory),
		...buildPersistedUnifiedTabSessionData(snapshot),
		activeConnectionIdsAtShutdown: buildActiveConnectionIdsAtShutdown(snapshot, terminalSessionData.remoteSessionIdsByTabId ?? null),
		remoteSessionIdsByTabId: terminalSessionData.remoteSessionIdsByTabId,
		lastVisitedAtByWorktreeId: buildLastVisitedAtByWorktreeId(snapshot),
		defaultTerminalTabsAppliedByWorktreeId: snapshot.defaultTerminalTabsAppliedByWorktreeId && Object.keys(snapshot.defaultTerminalTabsAppliedByWorktreeId).length > 0 ? snapshot.defaultTerminalTabsAppliedByWorktreeId : void 0,
		...buildSleepingAgentSessionData(snapshot)
	}, snapshot.repos);
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var STORAGE_PREFIX = "orca.terminalShortcutCapturedNotice.";
var NOTICE_DURATION_MS = 2e4;
function hasShownNotice(actionId) {
	try {
		return localStorage.getItem(`${STORAGE_PREFIX}${actionId}`) === "true";
	} catch {
		return false;
	}
}
function markNoticeShown(actionId) {
	try {
		localStorage.setItem(`${STORAGE_PREFIX}${actionId}`, "true");
	} catch {}
}
function openShortcutSettings() {
	const store = useAppStore.getState();
	store.openSettingsPage();
	store.openSettingsTarget({
		pane: "shortcuts",
		repoId: null,
		sectionId: "terminal-shortcut-policy"
	});
}
function showTerminalShortcutCaptureNotification({ actionId, platform, keybindings }) {
	const definition = getKeybindingDefinition(actionId);
	if (!definition || !isKeybindingPotentialTerminalConflict(definition)) return;
	if (hasShownNotice(actionId)) return;
	markNoticeShown(actionId);
	const bindingLabel = formatKeybindingList(getEffectiveKeybindingsForAction(actionId, platform, keybindings), platform);
	toast.message(translate("auto.lib.terminal.shortcut.capture.notification.141ad6c004", "Terminal shortcut handled"), {
		description: `${definition.title} (${bindingLabel})`,
		duration: NOTICE_DURATION_MS,
		dismissible: true,
		className: "!w-[420px] !max-w-[calc(100vw-2rem)] !gap-2 !py-2 !pl-3 !pr-2",
		classNames: {
			content: "min-w-0 flex-1 !gap-0.5",
			title: "truncate !leading-5",
			description: "truncate !leading-4",
			actionButton: "!h-7 !shrink-0 !rounded-md !px-2.5"
		},
		icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Keyboard, { className: "size-4 text-muted-foreground" }),
		action: {
			label: translate("auto.lib.terminal.shortcut.capture.notification.b0536028c9", "Open Shortcuts"),
			onClick: openShortcutSettings
		}
	});
}
function matchesRecentTabSwitcherChord(input, platform, keybindings, options = {}) {
	const control = Boolean(input.control ?? input.ctrlKey);
	const meta = Boolean(input.meta ?? input.metaKey);
	const alt = Boolean(input.alt ?? input.altKey);
	if (input.code !== "Tab" || !control || meta || alt) return false;
	return keybindingMatchesAction("tab.previousRecent", {
		key: input.key,
		code: input.code,
		alt,
		meta,
		control,
		shift: false,
		altKey: alt,
		metaKey: meta,
		ctrlKey: control,
		shiftKey: false
	}, platform, keybindings, options);
}
function isControlKey(input) {
	return input.code === "ControlLeft" || input.code === "ControlRight" || input.code === "Control" || input.key === "Control";
}
function isTabKey(input) {
	return input.code === "Tab" || input.key === "Tab";
}
function isRecentTabSwitcherCommitRelease(input) {
	if (input.type !== "keyUp" && input.type !== "keyup") return false;
	if (isControlKey(input)) return true;
	const control = input.control ?? input.ctrlKey;
	return isTabKey(input) && control === false;
}
function createShutdownCheckpointGuard(persist) {
	let persisted = false;
	return {
		persistOnce() {
			if (persisted) return true;
			try {
				persist();
			} catch {
				return false;
			}
			persisted = true;
			return true;
		},
		reset() {
			persisted = false;
		}
	};
}
function createShutdownCheckpointBeforeUnloadHandler(guard) {
	return (event) => {
		if (!guard.persistOnce()) event.preventDefault();
	};
}
function preventUnloadAndScheduleShutdownCheckpointReset(event, eventTarget) {
	event.preventDefault();
	queueMicrotask(() => {
		eventTarget.dispatchEvent(new Event(ORCA_RENDERER_UNLOAD_PREVENTED_EVENT));
	});
}
export { canDeferColdActivationTabsForHost as A, takePendingBackgroundTerminalWorktreeMount as B, activateCyclableTab as C, handleSwitchTerminalTab as D, handleSwitchTabAcrossAllTypes as E, requestBackgroundTerminalWorktreeMount as F, revealActivationDeferredTabs as I, shouldMountBackgroundWorktreeTab as L, hasRequestedBackgroundTerminalWorktreeMount as M, planColdActivationTabDeferral as N, addBackgroundMountedTerminalWorktree as O, pruneClosedBackgroundMountTabs as P, subscribeBackgroundTerminalWorktreeMountRequests as R, getSimulatorTabForWorktree as S, handleSwitchTab as T, buildLastVisitedAtByWorktreeId as _, matchesRecentTabSwitcherChord as a, openMobileEmulatorTab as b, buildEditorSessionData as c, buildSanitizedTabsByWorktree as d, buildTerminalSessionData as f, buildSleepingAgentSessionData as g, buildActiveConnectionIdsAtShutdown as h, isRecentTabSwitcherCommitRelease as i, canMountTerminalWorkspaceForStartup as j, applyBackgroundMountTabRestriction as k, buildPersistedBrowserPagesByWorkspace as l, shouldPersistWorkspaceSession as m, createShutdownCheckpointGuard as n, showTerminalShortcutCaptureNotification as o, buildWorkspaceSessionPayload as p, preventUnloadAndScheduleShutdownCheckpointReset as r, SESSION_RELEVANT_FIELDS as s, createShutdownCheckpointBeforeUnloadHandler as t, buildPersistedBrowserTabsByWorktree as u, buildPersistedUnifiedTabSessionData as v, handleSwitchRecentTab as w, ensureSimulatorTab as x, TOGGLE_QUICK_COMMANDS_MENU_EVENT as y, takeAllPendingBackgroundTerminalWorktreeMounts as z };
