import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { $ as reconcileWebSessionCloseIntents, $f as toRuntimeWorktreeSelector, Au as getRuntimeSessionMirrorEnvironmentIds, Du as getExplicitRuntimeEnvironmentIdForWorktree, Fu as isWebTerminalSurfaceTabId, Gc as isTerminalLeafId, Io as getRemoteRuntimePtyEnvironmentId, Iu as toHostSessionTabId, Jc as parsePaneKey, Kc as makePaneKey, Lu as toWebTerminalSurfaceTabId, Nu as HOST_TERMINAL_SURFACE_SEPARATOR, Pm as FLOATING_TERMINAL_WORKTREE_ID, Pu as WEB_TERMINAL_SURFACE_TAB_PREFIX, Q as isWebSessionCloseIntentPending, Ro as parseRemoteRuntimePtyId, To as sanitizeTerminalLayoutPaneTitlesForLabels, Vo as toRemoteRuntimePtyId, X as clearWebSessionCloseIntentsForOwner, Z as clearWebSessionCloseIntentsForWorktree, _o as resolveTerminalLayoutRoot, as as registerStaleDocumentVisibilityRecovery, cp as getRuntimeEnvironmentRevision, is as isDocumentVisibilityProvenStale, js as normalizeTerminalLayoutPtyOwnership, oa as agentEntryCompletionAt, oo as terminalLayoutEqual, t as useAppStore, vf as agentProviderSessionsEqual } from "./store-CgXrfmaH.js";
import { u as AGENT_STATUS_STALE_AFTER_MS } from "./agent-status-3vUKbY6l.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
import { A as clearWebSessionFocusIntentsForOwner, D as clearWebSessionReorderIntentsForWorktree, E as clearWebSessionReorderIntentsForOwner, M as resolveWebSessionVisibleTabId, O as resolveWebSessionReorderedOrder, _ as suppressE2eWebRuntimeBrowserSnapshot, b as isWebSessionBrowserPlacementGroupReserved, j as peekWebSessionFocusIntent, k as clearWebSessionFocusIntent, u as createWebRuntimeSessionTerminal, v as clearWebSessionBrowserPlacementsForEnvironment, x as peekWebSessionBrowserPlacementGroup, y as clearWebSessionBrowserPlacementsForWorktree } from "./web-runtime-session-CN2syA39.js";
import { n as normalizeCompatibleAgentStatusEntryForOwner, r as normalizeCompatibleAgentTitleForOwner, t as resolvePaneAgentOwner } from "./pane-agent-owner-BPfoVAtS.js";
import { a as isWebAgentSessionHandoffPostCreateSnapshotConfirmed, n as clearWebAgentSessionHandoffsForEnvironment, r as clearWebAgentSessionHandoffsForWorktree, s as resolveWebAgentSessionHandoff, t as clearWebAgentSessionHandoff } from "./web-agent-session-handoff-D4ZdXDx4.js";
import { n as isWindowVisible } from "./window-visibility-interval-CtnbYoau.js";
var panesByPaneKey = /* @__PURE__ */ new Map();
function registerRendererOwnedAgentStatusPane(paneKey, environmentId) {
	const existing = panesByPaneKey.get(paneKey);
	const entry = {
		environmentId,
		hasClientWrite: existing?.environmentId === environmentId && existing.hasClientWrite
	};
	panesByPaneKey.set(paneKey, entry);
	return () => {
		if (panesByPaneKey.get(paneKey) === entry) panesByPaneKey.delete(paneKey);
	};
}
function markRendererOwnedAgentStatusWrite(paneKey) {
	const existing = panesByPaneKey.get(paneKey);
	if (!existing || existing.hasClientWrite) return;
	existing.hasClientWrite = true;
}
function isClientAuthoritativeAgentStatusPane(paneKey) {
	return panesByPaneKey.get(paneKey)?.hasClientWrite === true;
}
var wakeTerminalRespawnInFlightByWorktree = /* @__PURE__ */ new Set();
function shouldSkipWebRuntimeWakeTerminalRespawn(worktreeId) {
	return wakeTerminalRespawnInFlightByWorktree.has(worktreeId);
}
function beginWebRuntimeWakeTerminalRespawn(worktreeId) {
	if (wakeTerminalRespawnInFlightByWorktree.has(worktreeId)) return false;
	wakeTerminalRespawnInFlightByWorktree.add(worktreeId);
	return true;
}
function endWebRuntimeWakeTerminalRespawn(worktreeId) {
	wakeTerminalRespawnInFlightByWorktree.delete(worktreeId);
}
function clearWebRuntimeWakeTerminalRespawnForWorktree(worktreeId) {
	wakeTerminalRespawnInFlightByWorktree.delete(worktreeId);
}
function clearAllWebRuntimeWakeTerminalRespawn() {
	wakeTerminalRespawnInFlightByWorktree.clear();
}
var RUNTIME_SUBSCRIPTION_REPLAY_FLAG = "_replayedAfterReconnect";
function isRuntimeSubscriptionReplayResponse(response) {
	return typeof response === "object" && response !== null && response[RUNTIME_SUBSCRIPTION_REPLAY_FLAG] === true;
}
var subscribersBySession = /* @__PURE__ */ new Map();
var pendingSnapshotBySession = /* @__PURE__ */ new Map();
function sessionKey(environmentId, worktreeId) {
	return `${environmentId}\u0000${worktreeId}`;
}
function resolveSubscriberUpdate(snapshot, subscriber) {
	const surfaces = snapshot.tabs.filter((tab) => tab.type === "terminal" && (tab.parentTabId === subscriber.hostTabId || tab.id === subscriber.hostTabId) && (!subscriber.leafId || tab.leafId === subscriber.leafId));
	if (surfaces.length === 0) return {
		surfacePresent: false,
		terminalHandle: null
	};
	const mirroredSurfaces = surfaces.filter((surface) => surface.parentTabId === subscriber.hostTabId);
	return {
		surfacePresent: true,
		terminalHandle: (mirroredSurfaces.find((surface) => surface.status === "ready" && surface.isActive) ?? mirroredSurfaces.find((surface) => surface.status === "ready"))?.terminal ?? null
	};
}
function subscribeAcceptedWebSessionTerminalHandle(args, listener) {
	const key = sessionKey(args.environmentId, args.worktreeId);
	const subscribers = subscribersBySession.get(key) ?? /* @__PURE__ */ new Set();
	const subscriber = {
		hostTabId: args.hostTabId,
		leafId: args.leafId ?? null,
		listener
	};
	subscribers.add(subscriber);
	subscribersBySession.set(key, subscribers);
	return () => {
		subscribers.delete(subscriber);
		if (subscribers.size === 0) subscribersBySession.delete(key);
	};
}
function queueAcceptedWebSessionTerminalSnapshot(snapshot, environmentId) {
	if (subscribersBySession.size === 0) return;
	const key = sessionKey(environmentId, snapshot.worktree);
	const subscribers = subscribersBySession.get(key);
	if (!subscribers || subscribers.size === 0) return;
	const pendingSnapshot = {
		snapshot,
		eligibleSubscribers: new Set(subscribers)
	};
	pendingSnapshotBySession.set(key, pendingSnapshot);
	queueMicrotask(() => {
		if (pendingSnapshotBySession.get(key) !== pendingSnapshot) return;
		pendingSnapshotBySession.delete(key);
		const currentSubscribers = subscribersBySession.get(key);
		for (const subscriber of pendingSnapshot.eligibleSubscribers) if (currentSubscribers?.has(subscriber)) subscriber.listener(resolveSubscriberUpdate(pendingSnapshot.snapshot, subscriber));
	});
}
function prunePaneLayout(node, retainedLeafIds) {
	if (!node) return null;
	if (node.type === "leaf") return retainedLeafIds.has(node.leafId) ? node : null;
	const first = prunePaneLayout(node.first, retainedLeafIds);
	const second = prunePaneLayout(node.second, retainedLeafIds);
	if (!first) return second;
	if (!second) return first;
	return {
		...node,
		first,
		second
	};
}
function pruneGroupLayout(node, retainedGroupIds) {
	if (!node) return;
	if (node.type === "leaf") return retainedGroupIds.has(node.groupId) ? node : void 0;
	const first = pruneGroupLayout(node.first, retainedGroupIds);
	const second = pruneGroupLayout(node.second, retainedGroupIds);
	if (!first) return second;
	if (!second) return first;
	return {
		...node,
		first,
		second
	};
}
function buildWebTerminalOrphanTopologyProposal(state, worktreeId, candidates, claims) {
	const leafIdsByTabId = /* @__PURE__ */ new Map();
	for (const claim of claims) {
		const leafIds = leafIdsByTabId.get(claim.tabId) ?? /* @__PURE__ */ new Set();
		leafIds.add(claim.leafId);
		leafIdsByTabId.set(claim.tabId, leafIds);
	}
	const hostTabIdByLocalId = new Map(candidates.map((tab) => [tab.id, toHostSessionTabId(tab.id)]));
	const tabs = candidates.flatMap((tab) => {
		const tabId = hostTabIdByLocalId.get(tab.id);
		const retainedLeafIds = leafIdsByTabId.get(tabId);
		const layout = state.terminalLayoutsByTabId[tab.id];
		const root = retainedLeafIds ? prunePaneLayout(layout?.root ?? null, retainedLeafIds) : null;
		if (!layout || !root || !retainedLeafIds || retainedLeafIds.size === 0) return [];
		const fallbackLeafId = [...retainedLeafIds][0];
		return [{
			tabId,
			root,
			activeLeafId: retainedLeafIds.has(layout.activeLeafId ?? "") ? layout.activeLeafId : fallbackLeafId,
			expandedLeafId: layout.expandedLeafId && retainedLeafIds.has(layout.expandedLeafId) ? layout.expandedLeafId : null
		}];
	});
	if (tabs.length !== leafIdsByTabId.size) return;
	const adoptedTabIds = new Set(tabs.map((tab) => tab.tabId));
	const groups = (state.groupsByWorktree?.[worktreeId] ?? []).flatMap((group) => {
		const tabOrder = group.tabOrder.map((tabId) => hostTabIdByLocalId.get(tabId)).filter((tabId) => Boolean(tabId && adoptedTabIds.has(tabId)));
		if (tabOrder.length === 0) return [];
		const requestedActive = group.activeTabId ? hostTabIdByLocalId.get(group.activeTabId) : void 0;
		const recentTabIds = group.recentTabIds?.map((tabId) => hostTabIdByLocalId.get(tabId)).filter((tabId) => Boolean(tabId && tabOrder.includes(tabId)));
		return [{
			id: group.id,
			activeTabId: requestedActive && tabOrder.includes(requestedActive) ? requestedActive : tabOrder[0],
			tabOrder,
			...recentTabIds && recentTabIds.length > 0 ? { recentTabIds } : {}
		}];
	});
	const completeGroups = groups.length > 0 ? groups : [{
		id: state.activeGroupIdByWorktree[worktreeId] ?? "recovered-orphans",
		activeTabId: tabs[0].tabId,
		tabOrder: tabs.map((tab) => tab.tabId)
	}];
	const groupIds = new Set(completeGroups.map((group) => group.id));
	const groupLayout = pruneGroupLayout(state.layoutByWorktree?.[worktreeId], groupIds);
	return {
		tabs,
		groups: completeGroups,
		...groupLayout ? { groupLayout } : {}
	};
}
var inFlightRecoveryByWorktree = /* @__PURE__ */ new Map();
function recoveryKey(environmentId, worktreeId) {
	return `${environmentId}\0${worktreeId}`;
}
function isTerminalListResult(value) {
	return Boolean(value) && typeof value === "object" && Array.isArray(value.terminals);
}
function isAdoptionResult(value) {
	return Boolean(value) && typeof value === "object" && Boolean(value.snapshot) && Array.isArray(value.snapshot?.tabs);
}
async function recoverTerminalOrphans(state, snapshot, environmentId, call) {
	const hostSurfaceKeys = new Set(snapshot.tabs.filter((tab) => tab.type === "terminal").map((tab) => `${tab.parentTabId}\0${tab.leafId}`));
	const candidates = (state.tabsByWorktree[snapshot.worktree] ?? []).filter((tab) => isWebTerminalSurfaceTabId(tab.id) && Object.keys(state.terminalLayoutsByTabId[tab.id]?.ptyIdsByLeafId ?? {}).some((leafId) => !hostSurfaceKeys.has(`${toHostSessionTabId(tab.id)}\0${leafId}`)));
	if (candidates.length === 0) return snapshot;
	const candidateSurfaces = candidates.flatMap((tab) => {
		const layout = state.terminalLayoutsByTabId[tab.id];
		return Object.entries(layout?.ptyIdsByLeafId ?? {}).flatMap(([leafId, remotePtyId]) => {
			const remote = parseRemoteRuntimePtyId(remotePtyId);
			return remote?.environmentId === environmentId && !hostSurfaceKeys.has(`${toHostSessionTabId(tab.id)}\0${leafId}`) ? [{
				tabId: toHostSessionTabId(tab.id),
				leafId,
				handle: remote.handle
			}] : [];
		});
	});
	const candidateHandles = new Set(candidateSurfaces.map((surface) => surface.handle));
	if (candidateHandles.size === 0) return snapshot;
	if (candidateHandles.size > 64) return null;
	const listedResponse = await call({
		selector: environmentId,
		method: "terminal.list",
		params: {
			worktree: toRuntimeWorktreeSelector(snapshot.worktree),
			handles: [...candidateHandles],
			requireFreshPtyLiveness: true,
			includeVisualLayouts: false
		},
		timeoutMs: 15e3
	});
	if (listedResponse.ok === false || !isTerminalListResult(listedResponse.result)) return null;
	const listed = listedResponse.result;
	const orphanByHandle = new Map(listed.terminals.filter((terminal) => terminal.orphaned === true && typeof terminal.ptyId === "string" && typeof terminal.incarnationId === "string").map((terminal) => [terminal.handle, terminal]));
	const claims = candidateSurfaces.flatMap(({ tabId, leafId, handle }) => {
		const orphan = orphanByHandle.get(handle);
		if (!orphan?.ptyId || !orphan.incarnationId) return [];
		return [{
			terminal: orphan.handle,
			ptyId: orphan.ptyId,
			incarnationId: orphan.incarnationId,
			tabId,
			leafId
		}];
	});
	const claimedHandles = new Set(claims.map((claim) => claim.terminal));
	const listedCandidateHandles = new Set(listed.terminals.filter((terminal) => candidateHandles.has(terminal.handle)).map((terminal) => terminal.handle));
	if (listed.truncated && [...candidateHandles].some((handle) => !listedCandidateHandles.has(handle))) return null;
	if (listed.terminals.some((terminal) => candidateHandles.has(terminal.handle) && !claimedHandles.has(terminal.handle))) return null;
	if (claims.length === 0) return snapshot;
	const localActiveTabId = state.activeTabIdByWorktree[snapshot.worktree];
	const activeTabId = localActiveTabId && isWebTerminalSurfaceTabId(localActiveTabId) ? toHostSessionTabId(localActiveTabId) : void 0;
	const activeGroupId = state.activeGroupIdByWorktree[snapshot.worktree] ?? void 0;
	const topology = buildWebTerminalOrphanTopologyProposal(state, snapshot.worktree, candidates, claims);
	const response = await call({
		selector: environmentId,
		method: "terminal.adoptOrphans",
		params: {
			worktree: toRuntimeWorktreeSelector(snapshot.worktree),
			expectedTopologyRevision: listed.topologyRevisions?.[snapshot.worktree] ?? 0,
			claims,
			...activeTabId ? { activeTabId } : {},
			...activeGroupId ? { activeGroupId } : {},
			...topology ? { topology } : {}
		},
		timeoutMs: 15e3
	});
	return response.ok !== false && isAdoptionResult(response.result) && response.result.snapshot.worktree === snapshot.worktree ? response.result.snapshot : null;
}
function recoverWebSessionTerminalOrphansBeforeApply(state, snapshot, environmentId, call = (args) => window.api.runtimeEnvironments.call(args)) {
	const key = recoveryKey(environmentId, snapshot.worktree);
	const recovery = (inFlightRecoveryByWorktree.get(key) ?? Promise.resolve(null)).catch(() => null).then(() => recoverTerminalOrphans(state, snapshot, environmentId, call)).catch(() => null).finally(() => {
		if (inFlightRecoveryByWorktree.get(key) === recovery) inFlightRecoveryByWorktree.delete(key);
	});
	inFlightRecoveryByWorktree.set(key, recovery);
	return recovery;
}
function getReachableRuntimeSessionMirrorTargets(state) {
	const environmentById = new Map((state.runtimeEnvironments ?? []).map((environment) => [environment.id, environment]));
	const targets = [];
	for (const environmentId of getRuntimeSessionMirrorEnvironmentIds(state)) {
		const status = state.runtimeStatusByEnvironmentId?.get(environmentId);
		if (!status?.status) continue;
		const environment = environmentById.get(environmentId);
		if (!environment) continue;
		targets.push({
			environmentId,
			runtimeId: status.status.runtimeId,
			connectionGeneration: status.connectionGeneration ?? 0,
			pairingRevision: environment.pairingRevision ?? environment.createdAt
		});
	}
	return targets;
}
var import_react = /* @__PURE__ */ __toESM(require_react());
function selectRuntimeSessionMirrorTargetInputs(state) {
	return {
		activeRuntimeEnvironmentId: state.settings?.activeRuntimeEnvironmentId ?? null,
		repos: state.repos,
		worktreesByRepo: state.worktreesByRepo,
		detectedWorktreesByRepo: state.detectedWorktreesByRepo,
		projectGroups: state.projectGroups,
		restoredRuntimeHostIdByWorkspaceSessionKey: state.restoredRuntimeHostIdByWorkspaceSessionKey,
		runtimeEnvironments: state.runtimeEnvironments,
		runtimeStatusByEnvironmentId: state.runtimeStatusByEnvironmentId
	};
}
function buildRuntimeSessionMirrorEnvironmentKey(inputs) {
	return getReachableRuntimeSessionMirrorTargets({
		settings: { activeRuntimeEnvironmentId: inputs.activeRuntimeEnvironmentId },
		repos: inputs.repos,
		worktreesByRepo: inputs.worktreesByRepo,
		detectedWorktreesByRepo: inputs.detectedWorktreesByRepo,
		projectGroups: inputs.projectGroups,
		restoredRuntimeHostIdByWorkspaceSessionKey: inputs.restoredRuntimeHostIdByWorkspaceSessionKey,
		runtimeEnvironments: inputs.runtimeEnvironments,
		runtimeStatusByEnvironmentId: inputs.runtimeStatusByEnvironmentId
	}).map(({ environmentId, runtimeId, connectionGeneration, pairingRevision }) => `${environmentId}\u0001${runtimeId}\u0001${connectionGeneration}\u0001${pairingRevision}`).join("\0");
}
function useRuntimeSessionMirrorEnvironmentKey() {
	const { activeRuntimeEnvironmentId, repos, worktreesByRepo, detectedWorktreesByRepo, projectGroups, restoredRuntimeHostIdByWorkspaceSessionKey, runtimeEnvironments, runtimeStatusByEnvironmentId } = useAppStore(useShallow(selectRuntimeSessionMirrorTargetInputs));
	return (0, import_react.useMemo)(() => buildRuntimeSessionMirrorEnvironmentKey({
		activeRuntimeEnvironmentId,
		repos,
		worktreesByRepo,
		detectedWorktreesByRepo,
		projectGroups,
		restoredRuntimeHostIdByWorkspaceSessionKey,
		runtimeEnvironments,
		runtimeStatusByEnvironmentId
	}), [
		activeRuntimeEnvironmentId,
		repos,
		worktreesByRepo,
		detectedWorktreesByRepo,
		projectGroups,
		restoredRuntimeHostIdByWorkspaceSessionKey,
		runtimeEnvironments,
		runtimeStatusByEnvironmentId
	]);
}
function getWindowParkVisible() {
	return isWindowVisible() || isDocumentVisibilityProvenStale();
}
function subscribeWindowParkVisibility(onChange) {
	const unregisterStaleRecovery = registerStaleDocumentVisibilityRecovery(onChange);
	const canListenToVisibility = typeof document !== "undefined" && typeof document.addEventListener === "function";
	if (canListenToVisibility) document.addEventListener("visibilitychange", onChange);
	return () => {
		if (canListenToVisibility) document.removeEventListener("visibilitychange", onChange);
		unregisterStaleRecovery();
	};
}
const WINDOW_HIDE_PARK_GRACE_MS = 500;
const WINDOW_VISIBILITY_SUBSCRIPTION_PARK_DELAY_MS = 500;
const WINDOW_VISIBILITY_SUBSCRIPTION_RETRY_INITIAL_MS = 1e3;
var WINDOW_VISIBILITY_SUBSCRIPTION_RETRY_MAX_MS = 3e4;
var WINDOW_VISIBILITY_SUBSCRIPTION_RETRY_JITTER_MS = 250;
function installWindowVisibilitySubscriptionParking(specs, options = {}) {
	const parkDelayMs = options.parkDelayMs ?? WINDOW_VISIBILITY_SUBSCRIPTION_PARK_DELAY_MS;
	const maxParkDelayMs = parkDelayMs * 8;
	let disposed = false;
	let effectiveVisible = getWindowParkVisible();
	let visibilityGeneration = effectiveVisible ? 0 : 1;
	let parkTimer = null;
	let currentParkDelayMs = parkDelayMs;
	let hiddenSinceMs = null;
	const entries = specs.map(() => ({
		desired: false,
		generation: 0,
		visibilityGeneration: 0,
		pending: null,
		retryAttempt: 0,
		retryTimer: null,
		startTimer: null,
		unsubscribe: null
	}));
	const clearRetry = (entry) => {
		if (entry.retryTimer !== null) {
			clearTimeout(entry.retryTimer);
			entry.retryTimer = null;
		}
	};
	const clearStart = (entry) => {
		if (entry.startTimer !== null) {
			clearTimeout(entry.startTimer);
			entry.startTimer = null;
		}
	};
	const unsubscribeEntry = (entry, spec) => {
		const unsubscribe = entry.unsubscribe;
		entry.unsubscribe = null;
		if (!unsubscribe) return;
		try {
			unsubscribe();
		} catch (error) {
			spec.onUnsubscribeError?.(error);
		}
	};
	function scheduleRetry(entry, spec) {
		if (disposed || !entry.desired || entry.retryTimer !== null) return;
		const exponentialDelay = Math.min(WINDOW_VISIBILITY_SUBSCRIPTION_RETRY_INITIAL_MS * 2 ** Math.min(entry.retryAttempt, 5), WINDOW_VISIBILITY_SUBSCRIPTION_RETRY_MAX_MS);
		const jitter = Math.floor(Math.random() * WINDOW_VISIBILITY_SUBSCRIPTION_RETRY_JITTER_MS);
		entry.retryAttempt += 1;
		entry.retryTimer = setTimeout(() => {
			entry.retryTimer = null;
			startEntry(entry, spec);
		}, exponentialDelay + jitter);
	}
	function startEntry(entry, spec) {
		if (disposed || !entry.desired || entry.pending || entry.retryTimer !== null || entry.startTimer !== null || entry.unsubscribe) return;
		const generation = entry.generation;
		const isCurrent = () => !disposed && entry.desired && entry.generation === generation;
		let subscription;
		try {
			subscription = spec.subscribe(isCurrent, { visibilityGeneration: entry.visibilityGeneration });
		} catch (error) {
			if (isCurrent()) {
				spec.onSubscribeError?.(error);
				scheduleRetry(entry, spec);
			}
			return;
		}
		const pending = Promise.resolve(subscription).then((handle) => {
			if (entry.pending !== pending) {
				try {
					handle.unsubscribe();
				} catch (error) {
					spec.onUnsubscribeError?.(error);
				}
				return;
			}
			entry.pending = null;
			if (isCurrent()) {
				entry.retryAttempt = 0;
				entry.unsubscribe = handle.unsubscribe;
				return;
			}
			try {
				handle.unsubscribe();
			} catch (error) {
				spec.onUnsubscribeError?.(error);
			}
			if (!disposed && entry.desired) startEntry(entry, spec);
		}, (error) => {
			if (entry.pending !== pending) return;
			entry.pending = null;
			if (isCurrent()) {
				spec.onSubscribeError?.(error);
				scheduleRetry(entry, spec);
				return;
			}
			if (!disposed && entry.desired) startEntry(entry, spec);
		});
		entry.pending = pending;
	}
	const startAll = (isVisibilityResume) => {
		const restartingSpecIndexes = entries.flatMap((entry, index) => entry.desired ? [] : [index]);
		if (isVisibilityResume && restartingSpecIndexes.length > 0) options.onVisibilityResume?.({
			visibilityGeneration,
			restartingSpecIndexes
		});
		for (const index of restartingSpecIndexes) {
			const entry = entries[index];
			entry.visibilityGeneration = visibilityGeneration;
			entry.desired = true;
		}
		const startOrder = isVisibilityResume ? [...restartingSpecIndexes].sort((left, right) => {
			const priority = options.getVisibilityResumePriority;
			return (priority?.(left) ?? 0) - (priority?.(right) ?? 0) || left - right;
		}) : restartingSpecIndexes;
		const staggerMs = Math.max(0, options.visibilityResumeStaggerMs ?? 0);
		if (!isVisibilityResume || staggerMs === 0) {
			for (const index of startOrder) startEntry(entries[index], specs[index]);
			return;
		}
		const startNext = (position) => {
			const index = startOrder[position];
			if (index === void 0) return;
			const entry = entries[index];
			entry.startTimer = null;
			if (disposed || !entry.desired) return;
			startEntry(entry, specs[index]);
			const nextIndex = startOrder[position + 1];
			if (nextIndex === void 0) return;
			const nextEntry = entries[nextIndex];
			nextEntry.startTimer = setTimeout(() => startNext(position + 1), staggerMs);
		};
		startNext(0);
	};
	const stopAll = () => {
		entries.forEach((entry, index) => {
			entry.desired = false;
			entry.generation += 1;
			entry.retryAttempt = 0;
			clearStart(entry);
			clearRetry(entry);
			unsubscribeEntry(entry, specs[index]);
		});
	};
	const cancelPark = () => {
		if (parkTimer !== null) {
			clearTimeout(parkTimer);
			parkTimer = null;
		}
	};
	const reconcileVisibility = () => {
		if (getWindowParkVisible()) {
			cancelPark();
			const hiddenMs = hiddenSinceMs === null ? null : Date.now() - hiddenSinceMs;
			hiddenSinceMs = null;
			if (!effectiveVisible) {
				effectiveVisible = true;
				if (hiddenMs !== null) currentParkDelayMs = hiddenMs >= maxParkDelayMs ? parkDelayMs : Math.min(currentParkDelayMs * 2, maxParkDelayMs);
				startAll(visibilityGeneration > 0);
			}
			return;
		}
		if (!effectiveVisible || parkTimer !== null) return;
		hiddenSinceMs = Date.now();
		parkTimer = setTimeout(() => {
			parkTimer = null;
			if (getWindowParkVisible()) {
				reconcileVisibility();
				return;
			}
			effectiveVisible = false;
			visibilityGeneration += 1;
			stopAll();
		}, currentParkDelayMs);
	};
	if (effectiveVisible) startAll(false);
	const unsubscribeVisibility = subscribeWindowParkVisibility(reconcileVisibility);
	return () => {
		disposed = true;
		cancelPark();
		unsubscribeVisibility();
		effectiveVisible = false;
		stopAll();
	};
}
var WEB_SESSION_GROUP_PREFIX = "web-session-tabs:";
const WEB_SESSION_TABS_VISIBILITY_RESUME_STAGGER_MS = 100;
var latestSessionTabsSnapshotByWorktree = /* @__PURE__ */ new Map();
var replayableSessionTabsSnapshotByWorktree = /* @__PURE__ */ new Map();
var latestReceivedSessionTabsSnapshotByWorktree = /* @__PURE__ */ new Map();
var latestSessionTabsRemovalFenceByWorktree = /* @__PURE__ */ new Map();
var sessionTabsRecoveryStateByWorktree = /* @__PURE__ */ new Map();
var trackedSessionTabsWorktreeIdsByEnvironment = /* @__PURE__ */ new Map();
var sessionTabsEnvironmentsByWorktree = /* @__PURE__ */ new Map();
var lastHostTerminalTabCountByWorktree = /* @__PURE__ */ new Map();
var hostSessionTabIdByLocalKey = /* @__PURE__ */ new Map();
var hostSessionTabMappingKeysByEnvironmentAndWorktree = /* @__PURE__ */ new Map();
var receivedSessionTabsFrameSequence = 0;
function isSessionTabsListAllResult(value) {
	return Boolean(value) && typeof value === "object" && Array.isArray(value.snapshots);
}
function sessionTabsFreshnessKey(environmentId, worktreeId) {
	return `${environmentId}:${worktreeId}`;
}
function advancesSessionTabsFreshness(snapshot, baseline) {
	return snapshot.publicationEpoch !== baseline.publicationEpoch || snapshot.snapshotVersion > baseline.snapshotVersion;
}
function getTrackedWebSessionTabsWorktrees(environmentId) {
	return [...trackedSessionTabsWorktreeIdsByEnvironment.get(environmentId) ?? []].flatMap((worktree) => {
		const key = sessionTabsFreshnessKey(environmentId, worktree);
		const freshness = latestSessionTabsSnapshotByWorktree.get(key);
		return freshness ? [{
			worktree,
			freshness
		}] : [];
	});
}
function trackWebSessionTabsWorktree(environmentId, worktreeId) {
	const worktrees = trackedSessionTabsWorktreeIdsByEnvironment.get(environmentId) ?? /* @__PURE__ */ new Set();
	worktrees.add(worktreeId);
	trackedSessionTabsWorktreeIdsByEnvironment.set(environmentId, worktrees);
}
function untrackWebSessionTabsWorktree(environmentId, worktreeId) {
	const worktrees = trackedSessionTabsWorktreeIdsByEnvironment.get(environmentId);
	if (!worktrees) return;
	worktrees.delete(worktreeId);
	if (worktrees.size === 0) trackedSessionTabsWorktreeIdsByEnvironment.delete(environmentId);
}
function recordReceivedWebSessionTabsSnapshot(environmentId, snapshot) {
	const receivedFrame = receivedSessionTabsFrameSequence += 1;
	const key = sessionTabsFreshnessKey(environmentId, snapshot.worktree);
	latestReceivedSessionTabsSnapshotByWorktree.set(key, {
		receivedFrame,
		publicationEpoch: snapshot.publicationEpoch,
		snapshotVersion: snapshot.snapshotVersion
	});
	if (snapshot.removed === true) recordReceivedWebSessionTabsRemoval(environmentId, snapshot.worktree, receivedFrame);
	return receivedFrame;
}
function recordReceivedWebSessionTabsInventory() {
	return receivedSessionTabsFrameSequence += 1;
}
function beginWebSessionTabsSnapshotRecovery(environmentId, worktreeId, receivedFrame) {
	const key = sessionTabsFreshnessKey(environmentId, worktreeId);
	const recoveryState = sessionTabsRecoveryStateByWorktree.get(key) ?? { pendingCount: 0 };
	recoveryState.pendingCount += 1;
	sessionTabsRecoveryStateByWorktree.set(key, recoveryState);
	let settled = false;
	return () => {
		if (settled) return;
		settled = true;
		recoveryState.pendingCount -= 1;
		if (recoveryState.pendingCount === 0 && sessionTabsRecoveryStateByWorktree.get(key) === recoveryState) sessionTabsRecoveryStateByWorktree.delete(key);
		const removalFence = latestSessionTabsRemovalFenceByWorktree.get(key);
		if (removalFence?.recoveryState === recoveryState && receivedFrame < removalFence.receivedFrame) {
			removalFence.pendingCount -= 1;
			if (removalFence.pendingCount === 0) latestSessionTabsRemovalFenceByWorktree.delete(key);
		}
	};
}
function recordReceivedWebSessionTabsRemoval(environmentId, worktreeId, receivedFrame) {
	const key = sessionTabsFreshnessKey(environmentId, worktreeId);
	const current = latestSessionTabsRemovalFenceByWorktree.get(key);
	if (current && current.receivedFrame >= receivedFrame) return;
	const recoveryState = sessionTabsRecoveryStateByWorktree.get(key);
	if (!recoveryState || recoveryState.pendingCount === 0) {
		latestSessionTabsRemovalFenceByWorktree.delete(key);
		return;
	}
	latestSessionTabsRemovalFenceByWorktree.set(key, {
		receivedFrame,
		recoveryState,
		pendingCount: recoveryState.pendingCount
	});
}
function shouldApplyRecoveredWebSessionTabsSnapshot(environmentId, snapshot, receivedFrame) {
	const key = sessionTabsFreshnessKey(environmentId, snapshot.worktree);
	const removalFrame = latestSessionTabsRemovalFenceByWorktree.get(key)?.receivedFrame;
	if (removalFrame !== void 0 && receivedFrame < removalFrame) return false;
	const latest = latestReceivedSessionTabsSnapshotByWorktree.get(key);
	if (!latest || latest.receivedFrame === receivedFrame) return latest !== void 0;
	if (latest.publicationEpoch !== snapshot.publicationEpoch) return receivedFrame > latest.receivedFrame;
	return snapshot.snapshotVersion >= latest.snapshotVersion;
}
function isTrackedWebSessionTabsOmissionCurrent(environmentId, trackedWorktree) {
	const key = sessionTabsFreshnessKey(environmentId, trackedWorktree.worktree);
	const current = latestSessionTabsSnapshotByWorktree.get(key);
	return current?.publicationEpoch === trackedWorktree.freshness.publicationEpoch && current.snapshotVersion === trackedWorktree.freshness.snapshotVersion;
}
function recordAcceptedWebSessionTabsEnvironment(environmentId, snapshot) {
	const environments = new Set(sessionTabsEnvironmentsByWorktree.get(snapshot.worktree) ?? []);
	if (snapshot.tabs.length > 0) environments.add(environmentId);
	else environments.delete(environmentId);
	if (environments.size > 0) sessionTabsEnvironmentsByWorktree.set(snapshot.worktree, environments);
	else sessionTabsEnvironmentsByWorktree.delete(snapshot.worktree);
}
function removeWebSessionTabsEnvironment(environmentId, worktreeId) {
	const environments = new Set(sessionTabsEnvironmentsByWorktree.get(worktreeId) ?? []);
	environments.delete(environmentId);
	if (environments.size > 0) sessionTabsEnvironmentsByWorktree.set(worktreeId, environments);
	else sessionTabsEnvironmentsByWorktree.delete(worktreeId);
}
function buildMissingWebSessionTabsRemovals(environmentId, trackedWorktrees, publishedWorktrees) {
	return trackedWorktrees.filter((trackedWorktree) => !publishedWorktrees.has(trackedWorktree.worktree) && isTrackedWebSessionTabsOmissionCurrent(environmentId, trackedWorktree)).map((trackedWorktree) => ({
		trackedWorktree,
		snapshot: {
			worktree: trackedWorktree.worktree,
			publicationEpoch: "visibility-inventory-removal",
			snapshotVersion: 0,
			removed: true,
			activeGroupId: null,
			activeTabId: null,
			activeTabType: null,
			tabs: []
		}
	}));
}
function rememberHostTerminalTabCount(environmentId, snapshot) {
	const key = sessionTabsFreshnessKey(environmentId, snapshot.worktree);
	const terminalCount = snapshot.tabs.filter((tab) => tab.type === "terminal").length;
	lastHostTerminalTabCountByWorktree.set(key, terminalCount);
}
function getLastKnownHostTerminalTabCount(environmentId, worktreeId) {
	return lastHostTerminalTabCountByWorktree.get(sessionTabsFreshnessKey(environmentId, worktreeId)) ?? 0;
}
function getLatestWebSessionTabsPublicationEpoch(environmentId, worktreeId) {
	return latestSessionTabsSnapshotByWorktree.get(sessionTabsFreshnessKey(environmentId, worktreeId))?.publicationEpoch ?? null;
}
function acceptReplayedWebSessionTabsSnapshot(environmentId, worktreeId) {
	const key = sessionTabsFreshnessKey(environmentId, worktreeId);
	const current = latestSessionTabsSnapshotByWorktree.get(key);
	if (current) replayableSessionTabsSnapshotByWorktree.set(key, current);
}
function shouldApplyWebSessionTabsSnapshot(snapshot, environmentId) {
	const key = sessionTabsFreshnessKey(environmentId, snapshot.worktree);
	if (snapshot.removed === true) {
		clearWebSessionTabsTrackingForWorktree(environmentId, snapshot.worktree);
		queueAcceptedWebSessionTerminalSnapshot(snapshot, environmentId);
		return true;
	}
	if (snapshot.worktree === "global-floating-terminal") return false;
	rememberHostTerminalTabCount(environmentId, snapshot);
	const current = latestSessionTabsSnapshotByWorktree.get(key);
	const replayable = replayableSessionTabsSnapshotByWorktree.get(key);
	const isExactCurrentReplay = Boolean(current && replayable && current.publicationEpoch === replayable.publicationEpoch && current.snapshotVersion === replayable.snapshotVersion && snapshot.publicationEpoch === replayable.publicationEpoch && snapshot.snapshotVersion === replayable.snapshotVersion);
	if (current && current.publicationEpoch === snapshot.publicationEpoch && snapshot.snapshotVersion <= current.snapshotVersion && !isExactCurrentReplay) return false;
	replayableSessionTabsSnapshotByWorktree.delete(key);
	latestSessionTabsSnapshotByWorktree.set(key, {
		publicationEpoch: snapshot.publicationEpoch,
		snapshotVersion: snapshot.snapshotVersion
	});
	trackWebSessionTabsWorktree(environmentId, snapshot.worktree);
	recordAcceptedWebSessionTabsEnvironment(environmentId, snapshot);
	queueAcceptedWebSessionTerminalSnapshot(snapshot, environmentId);
	return true;
}
function shouldBootstrapInitialWebRuntimeTerminal(args) {
	return args.snapshotIsFresh && args.event.type === "snapshot" && args.event.tabs.length === 0 && args.localTerminalCount === 0 && !args.requestedInitialTerminal && args.activeWorktreeId === args.event.worktree;
}
function shouldRespawnWebRuntimeTerminalAfterWake(args) {
	if (!args.snapshotIsFresh || args.requestedRespawnAfterWake || args.skipWakeRespawn === true || args.localTerminalCount === 0 || args.hasLiveLocalPty || args.event.type !== "snapshot" && args.event.type !== "updated") return false;
	if (args.activeWorktreeId !== args.event.worktree) return false;
	return args.event.tabs.filter((tab) => tab.type === "terminal").length === 0;
}
function shouldSyncRuntimeSessionTabs(args) {
	if (!args.activeWorktreeRuntimeEnvironmentId?.trim() || !args.workspaceSessionReady) return false;
	return Boolean(args.activeWorktreeId?.trim());
}
function shouldSyncAllRuntimeSessionTabs(args) {
	const environmentId = args.activeRuntimeEnvironmentId?.trim();
	return Boolean(environmentId && args.workspaceSessionReady);
}
function clearWebSessionTabsTrackingForWorktree(environmentId, worktreeId) {
	const key = sessionTabsFreshnessKey(environmentId, worktreeId);
	latestSessionTabsSnapshotByWorktree.delete(key);
	replayableSessionTabsSnapshotByWorktree.delete(key);
	latestReceivedSessionTabsSnapshotByWorktree.delete(key);
	untrackWebSessionTabsWorktree(environmentId, worktreeId);
	removeWebSessionTabsEnvironment(environmentId, worktreeId);
	lastHostTerminalTabCountByWorktree.delete(key);
	clearWebRuntimeWakeTerminalRespawnForWorktree(worktreeId);
	clearWebSessionReorderIntentsForWorktree({ environmentId }, worktreeId);
	clearWebSessionCloseIntentsForWorktree({ environmentId }, worktreeId);
	clearWebAgentSessionHandoffsForWorktree(environmentId, worktreeId);
	clearHostSessionTabIdMappings(environmentId, worktreeId);
	clearWebSessionBrowserPlacementsForWorktree(environmentId, worktreeId);
}
function clearWebSessionTabsTrackingForEnvironment(environmentId) {
	const trimmedEnvironmentId = environmentId.trim();
	if (!trimmedEnvironmentId) return;
	const keyPrefix = `${trimmedEnvironmentId}:`;
	for (const key of latestSessionTabsSnapshotByWorktree.keys()) if (key.startsWith(keyPrefix)) latestSessionTabsSnapshotByWorktree.delete(key);
	for (const key of replayableSessionTabsSnapshotByWorktree.keys()) if (key.startsWith(keyPrefix)) replayableSessionTabsSnapshotByWorktree.delete(key);
	for (const key of latestReceivedSessionTabsSnapshotByWorktree.keys()) if (key.startsWith(keyPrefix)) latestReceivedSessionTabsSnapshotByWorktree.delete(key);
	for (const key of latestSessionTabsRemovalFenceByWorktree.keys()) if (key.startsWith(keyPrefix)) latestSessionTabsRemovalFenceByWorktree.delete(key);
	for (const key of sessionTabsRecoveryStateByWorktree.keys()) if (key.startsWith(keyPrefix)) sessionTabsRecoveryStateByWorktree.delete(key);
	trackedSessionTabsWorktreeIdsByEnvironment.delete(trimmedEnvironmentId);
	for (const worktreeId of sessionTabsEnvironmentsByWorktree.keys()) removeWebSessionTabsEnvironment(trimmedEnvironmentId, worktreeId);
	for (const key of lastHostTerminalTabCountByWorktree.keys()) if (key.startsWith(keyPrefix)) lastHostTerminalTabCountByWorktree.delete(key);
	const mappingKeysByWorktree = hostSessionTabMappingKeysByEnvironmentAndWorktree.get(trimmedEnvironmentId);
	if (mappingKeysByWorktree) {
		for (const mappingKeys of mappingKeysByWorktree.values()) for (const mappingKey of mappingKeys) hostSessionTabIdByLocalKey.delete(mappingKey);
		hostSessionTabMappingKeysByEnvironmentAndWorktree.delete(trimmedEnvironmentId);
	}
	clearWebAgentSessionHandoffsForEnvironment(trimmedEnvironmentId);
	clearWebSessionBrowserPlacementsForEnvironment(trimmedEnvironmentId);
	clearAllWebRuntimeWakeTerminalRespawn();
}
function hostSessionTabMappingKey(args) {
	return `${args.environmentId}:${args.worktreeId}:${args.tabId}`;
}
function clearHostSessionTabIdMappings(environmentId, worktreeId) {
	const mappingKeysByWorktree = hostSessionTabMappingKeysByEnvironmentAndWorktree.get(environmentId);
	const mappingKeys = mappingKeysByWorktree?.get(worktreeId);
	if (!mappingKeys) return;
	for (const mappingKey of mappingKeys) hostSessionTabIdByLocalKey.delete(mappingKey);
	mappingKeysByWorktree?.delete(worktreeId);
	if (mappingKeysByWorktree?.size === 0) hostSessionTabMappingKeysByEnvironmentAndWorktree.delete(environmentId);
}
function setHostSessionTabIdMapping(args, hostTabId) {
	const mappingKey = hostSessionTabMappingKey(args);
	hostSessionTabIdByLocalKey.set(mappingKey, hostTabId);
	const mappingKeysByWorktree = hostSessionTabMappingKeysByEnvironmentAndWorktree.get(args.environmentId) ?? /* @__PURE__ */ new Map();
	const mappingKeys = mappingKeysByWorktree.get(args.worktreeId) ?? /* @__PURE__ */ new Set();
	mappingKeys.add(mappingKey);
	mappingKeysByWorktree.set(args.worktreeId, mappingKeys);
	hostSessionTabMappingKeysByEnvironmentAndWorktree.set(args.environmentId, mappingKeysByWorktree);
}
function resolveHostSessionTabIdForWebSessionTab(_state, args) {
	return hostSessionTabIdByLocalKey.get(hostSessionTabMappingKey(args)) ?? resolveWebAgentSessionHandoff({
		environmentId: args.environmentId,
		worktreeId: args.worktreeId,
		provisionalTabId: args.tabId
	});
}
function isReadyTerminalTab(tab) {
	return tab.type === "terminal" && tab.status === "ready" && tab.terminal.trim().length > 0;
}
function isTerminalSurfaceTab(tab) {
	return tab.type === "terminal";
}
function isReadyBrowserTab(tab) {
	return tab.type === "browser" && typeof tab.browserPageId === "string" && tab.browserPageId !== "";
}
function isReadyEditorTab(tab) {
	return tab.type === "markdown" || tab.type === "file";
}
function localEditorFileId(tab) {
	if (tab.type === "markdown" && tab.mode === "markdown-preview") return `markdown-preview::${tab.sourceFilePath}`;
	return tab.filePath;
}
function editorSourceFileId(tab) {
	return tab.type === "markdown" && tab.mode === "markdown-preview" ? tab.sourceFilePath : void 0;
}
function isRuntimeTerminalTabForEnvironment(tab, environmentId) {
	if (!tab.ptyId) return false;
	return getRemoteRuntimePtyEnvironmentId(tab.ptyId) === environmentId;
}
function isMirroredTerminalSurfaceId(tabId) {
	return tabId.startsWith("web-terminal-") || tabId.includes("::");
}
function chooseRemoteTerminalLayout(surfaces, ptyIdsByLeafId, existingLayout, requestedActiveLeafId) {
	const leafIds = surfaces.map((surface) => surface.leafId);
	const knownLeafIds = new Set(leafIds);
	const parentLayoutSource = surfaces.find((surface) => surface.parentLayout);
	const parentLayout = parentLayoutSource?.parentLayout ? sanitizeTerminalLayoutPaneTitlesForLabels(parentLayoutSource.parentLayout, [parentLayoutSource.title]) : void 0;
	const activeLeafId = (requestedActiveLeafId && knownLeafIds.has(requestedActiveLeafId) ? requestedActiveLeafId : null) ?? (existingLayout?.activeLeafId && knownLeafIds.has(existingLayout.activeLeafId) ? existingLayout.activeLeafId : null) ?? (parentLayout?.activeLeafId && knownLeafIds.has(parentLayout.activeLeafId) ? parentLayout.activeLeafId : null) ?? surfaces.find((surface) => surface.isActive)?.leafId ?? leafIds[0] ?? null;
	const expandedLeafId = requestedActiveLeafId && (Boolean(existingLayout?.expandedLeafId) || Boolean(parentLayout?.expandedLeafId)) ? requestedActiveLeafId : parentLayout?.expandedLeafId && knownLeafIds.has(parentLayout.expandedLeafId) ? parentLayout.expandedLeafId : null;
	return {
		root: resolveTerminalLayoutRoot({
			authoritativeRoot: parentLayout?.root,
			existingRoot: existingLayout?.root,
			leafIds,
			onSynthesize: (leafCount) => console.warn(`[web-session-tabs-sync] synthesized layout for ${leafCount} leaves; no authoritative or prior tree covered them`)
		}),
		activeLeafId,
		expandedLeafId,
		ptyIdsByLeafId,
		...parentLayout?.titlesByLeafId ? { titlesByLeafId: parentLayout.titlesByLeafId } : {}
	};
}
function shouldReplaceTerminalTab(tab, environmentId, nextRemotePtyIds, nextMirroredTerminalIds, exactProvisionalHandoffs) {
	if (exactProvisionalHandoffs.has(tab.id)) return true;
	if (isMirroredTerminalSurfaceId(tab.id)) return true;
	if (tab.pendingActivationSpawn && tab.ptyId === null && nextRemotePtyIds.size > 0) return true;
	if (!isRuntimeTerminalTabForEnvironment(tab, environmentId)) return false;
	return tab.ptyId !== null && (nextRemotePtyIds.has(tab.ptyId) || nextMirroredTerminalIds.has(toWebTerminalSurfaceTabId(tab.id)));
}
function buildMirroredTerminalTabs(snapshot, environmentId, existingById, existingLayoutsByTabId, sortOffset, now, focusTarget) {
	const groups = /* @__PURE__ */ new Map();
	for (const tab of snapshot.tabs.filter(isTerminalSurfaceTab)) {
		const group = groups.get(tab.parentTabId) ?? [];
		group.push(tab);
		groups.set(tab.parentTabId, group);
	}
	return [...groups.entries()].map(([parentTabId, surfaces], index) => {
		const localTabId = toWebTerminalSurfaceTabId(parentTabId);
		const existingLayout = existingLayoutsByTabId[localTabId];
		const requestedActiveLeafId = focusTarget?.parentTabId === parentTabId ? focusTarget.leafId : void 0;
		const activeSurface = (requestedActiveLeafId ? surfaces.find((surface) => surface.leafId === requestedActiveLeafId) : void 0) ?? (existingLayout?.activeLeafId ? surfaces.find((surface) => surface.leafId === existingLayout.activeLeafId) : void 0) ?? surfaces.find((surface) => surface.isActive) ?? surfaces[0];
		const ptyIdsByLeafId = Object.fromEntries(surfaces.filter((surface) => surface.status === "ready").map((surface) => [surface.leafId, toRemoteRuntimePtyId(surface.terminal, environmentId)]));
		const layout = normalizeTerminalLayoutPtyOwnership(chooseRemoteTerminalLayout(surfaces, ptyIdsByLeafId, existingLayout, requestedActiveLeafId)).snapshot;
		const layoutPtyEntries = Object.entries(layout.ptyIdsByLeafId ?? {});
		const ptyIds = layoutPtyEntries.map(([, ptyId]) => ptyId);
		let retainedSurfaceByPrunedLeafId;
		if (layoutPtyEntries.length < Object.keys(ptyIdsByLeafId).length) {
			const retainedLeafIdByPtyId = new Map(layoutPtyEntries.map(([leafId, ptyId]) => [ptyId, leafId]));
			const surfaceByLeafId = new Map(surfaces.map((surface) => [surface.leafId, surface]));
			retainedSurfaceByPrunedLeafId = /* @__PURE__ */ new Map();
			for (const [leafId, ptyId] of Object.entries(ptyIdsByLeafId)) {
				const retainedLeafId = retainedLeafIdByPtyId.get(ptyId);
				if (retainedLeafId && retainedLeafId !== leafId) {
					const retainedSurface = surfaceByLeafId.get(retainedLeafId);
					if (retainedSurface) retainedSurfaceByPrunedLeafId.set(leafId, retainedSurface);
				}
			}
		}
		const launchAgent = activeSurface.launchAgent ?? surfaces.find((surface) => surface.launchAgent)?.launchAgent;
		const ownerAgent = resolvePaneAgentOwner({
			launchAgent,
			hookAgent: activeSurface.agentStatus?.agentType,
			siblingHookAgent: surfaces.find((surface) => surface.agentStatus?.agentType)?.agentStatus?.agentType
		});
		const title = normalizeCompatibleAgentTitleForOwner(activeSurface.title.trim() || surfaces[0]?.title.trim() || "Terminal", ownerAgent);
		const existing = existingById.get(localTabId) ?? existingById.get(parentTabId) ?? surfaces.map((surface) => existingById.get(toWebTerminalSurfaceTabId(surface.id))).find((tab) => Boolean(tab));
		const quickCommandLabel = activeSurface.quickCommandLabel?.trim() || surfaces.find((surface) => surface.quickCommandLabel?.trim())?.quickCommandLabel?.trim() || existing?.quickCommandLabel?.trim();
		const startupCwd = activeSurface.startupCwd || surfaces.find((surface) => surface.startupCwd)?.startupCwd;
		const hostColorSurface = surfaces.find((surface) => surface.color != null);
		const color = existing ? existing.color ?? null : hostColorSurface?.color ?? null;
		const isPinned = existing ? existing.isPinned === true : surfaces.some((surface) => surface.isPinned);
		const hostViewModeSurface = surfaces.find((surface) => surface.viewMode);
		const viewMode = existing ? existing.viewMode : hostViewModeSurface?.viewMode;
		return {
			tab: {
				id: localTabId,
				ptyId: ptyIdsByLeafId[activeSurface.leafId] ?? null,
				worktreeId: snapshot.worktree,
				title,
				defaultTitle: existing?.defaultTitle ?? title,
				...existing?.generatedTitle ? { generatedTitle: existing.generatedTitle } : {},
				...existing?.aiVaultTitle ? { aiVaultTitle: existing.aiVaultTitle } : {},
				...quickCommandLabel ? { quickCommandLabel } : {},
				...startupCwd ? { startupCwd } : {},
				customTitle: existing?.customTitle ?? null,
				color,
				isPinned,
				...viewMode ? { viewMode } : {},
				sortOrder: sortOffset + index,
				createdAt: existing?.createdAt ?? now + index,
				...launchAgent ? { launchAgent } : {}
			},
			hostTabId: parentTabId,
			ptyIds,
			layout,
			...retainedSurfaceByPrunedLeafId ? { retainedSurfaceByPrunedLeafId } : {}
		};
	});
}
function toMirroredPaneKey(surface, leafId = surface.leafId) {
	if (!isTerminalLeafId(leafId)) return null;
	return makePaneKey(toWebTerminalSurfaceTabId(surface.parentTabId), leafId);
}
function remapHostAgentStatus(surface, retainedSurface) {
	if (!surface.agentStatus) return null;
	const paneKey = toMirroredPaneKey(surface, retainedSurface?.leafId);
	if (!paneKey) return null;
	const ownerAgent = resolvePaneAgentOwner({
		launchAgent: retainedSurface?.launchAgent ?? surface.launchAgent,
		hookAgent: surface.agentStatus.agentType
	});
	return {
		...normalizeCompatibleAgentStatusEntryForOwner(surface.agentStatus, ownerAgent),
		paneKey,
		tabId: toWebTerminalSurfaceTabId(surface.parentTabId)
	};
}
function isMirroredAgentPaneKeyForTabs(paneKey, tabIds) {
	const parsed = parsePaneKey(paneKey);
	return parsed !== null && tabIds.has(parsed.tabId);
}
function hostAgentStatusPiercesClientAuthority(entry) {
	return entry.state === "blocked" || entry.interactivePrompt != null;
}
function isClientOwnedAgentStatus(paneKey, existing) {
	return existing !== void 0 && isClientAuthoritativeAgentStatusPane(paneKey);
}
function isFencedClientAgentStatus(paneKey, existing, now) {
	return isClientOwnedAgentStatus(paneKey, existing) && isAgentStatusFresh(existing, now);
}
function batchAgentPaneKeysForTabs(state, tabIds, batchContext) {
	if (!batchContext) return Object.keys(state.agentStatusByPaneKey);
	if (!batchContext.agentPaneKeysByTabId) {
		batchContext.agentPaneKeysByTabId = /* @__PURE__ */ new Map();
		for (const paneKey of Object.keys(state.agentStatusByPaneKey)) {
			const tabId = parsePaneKey(paneKey)?.tabId;
			if (!tabId) continue;
			const paneKeys = batchContext.agentPaneKeysByTabId.get(tabId) ?? /* @__PURE__ */ new Set();
			paneKeys.add(paneKey);
			batchContext.agentPaneKeysByTabId.set(tabId, paneKeys);
		}
	}
	return [...tabIds].flatMap((tabId) => [...batchContext.agentPaneKeysByTabId?.get(tabId) ?? []]);
}
function updateBatchAgentPaneKey(paneKey, present, batchContext) {
	const tabId = parsePaneKey(paneKey)?.tabId;
	const index = batchContext?.agentPaneKeysByTabId;
	if (!tabId || !index) return;
	if (present) {
		const paneKeys$1 = index.get(tabId) ?? /* @__PURE__ */ new Set();
		paneKeys$1.add(paneKey);
		index.set(tabId, paneKeys$1);
		return;
	}
	const paneKeys = index.get(tabId);
	paneKeys?.delete(paneKey);
	if (paneKeys?.size === 0) index.delete(tabId);
}
function buildMirroredAgentStatusPatch(state, currentTerminalTabs, terminalSurfaceTabs, mirroredTerminalTabs, now, batchContext) {
	const mirroredTabIds = /* @__PURE__ */ new Set();
	for (const tab of currentTerminalTabs) if (isWebTerminalSurfaceTabId(tab.id)) mirroredTabIds.add(tab.id);
	for (const surface of terminalSurfaceTabs) mirroredTabIds.add(toWebTerminalSurfaceTabId(surface.parentTabId));
	if (mirroredTabIds.size === 0) return null;
	let retainedSurfaceByHostTabAndPrunedLeafId;
	for (const entry of mirroredTerminalTabs) if (entry.retainedSurfaceByPrunedLeafId) {
		retainedSurfaceByHostTabAndPrunedLeafId ?? (retainedSurfaceByHostTabAndPrunedLeafId = /* @__PURE__ */ new Map());
		retainedSurfaceByHostTabAndPrunedLeafId.set(entry.hostTabId, entry.retainedSurfaceByPrunedLeafId);
	}
	const nextByPaneKey = /* @__PURE__ */ new Map();
	for (const surface of terminalSurfaceTabs) {
		const retainedSurface = retainedSurfaceByHostTabAndPrunedLeafId?.get(surface.parentTabId)?.get(surface.leafId);
		const entry = remapHostAgentStatus(surface, retainedSurface);
		if (!entry) continue;
		const existing = nextByPaneKey.get(entry.paneKey) ?? state.agentStatusByPaneKey[entry.paneKey];
		const hostIdentityPredatesCurrentTurn = existing !== void 0 && entry.state === "done" && existing.state !== "done" && existing.stateStartedAt > entry.stateStartedAt;
		const clientOwnsEntry = isFencedClientAgentStatus(entry.paneKey, existing, now) && !hostAgentStatusPiercesClientAuthority(entry);
		const nextEntry = existing && (clientOwnsEntry || existing.updatedAt > entry.updatedAt) ? {
			...normalizeCompatibleAgentStatusEntryForOwner(existing, entry.agentType),
			paneKey: entry.paneKey,
			worktreeId: entry.worktreeId ?? existing.worktreeId,
			tabId: entry.tabId,
			providerSession: existing.providerSession ?? (hostIdentityPredatesCurrentTurn ? void 0 : entry.providerSession)
		} : entry;
		nextByPaneKey.set(entry.paneKey, nextEntry);
	}
	let nextAgentStatusByPaneKey = state.agentStatusByPaneKey;
	let changed = false;
	let aggregateRelevantChange = false;
	let sortRelevantChange = false;
	for (const paneKey of batchAgentPaneKeysForTabs(state, mirroredTabIds, batchContext)) {
		if (!isMirroredAgentPaneKeyForTabs(paneKey, mirroredTabIds)) continue;
		if (nextByPaneKey.has(paneKey)) continue;
		if (isClientOwnedAgentStatus(paneKey, state.agentStatusByPaneKey[paneKey])) continue;
		if (nextAgentStatusByPaneKey === state.agentStatusByPaneKey) nextAgentStatusByPaneKey = writableWebSessionTabsRecord(state, "agentStatusByPaneKey", batchContext);
		delete nextAgentStatusByPaneKey[paneKey];
		updateBatchAgentPaneKey(paneKey, false, batchContext);
		changed = true;
		aggregateRelevantChange = true;
		sortRelevantChange = true;
	}
	for (const [paneKey, entry] of nextByPaneKey) {
		const existing = nextAgentStatusByPaneKey[paneKey];
		if (agentStatusEntryEqual(existing, entry)) continue;
		if (nextAgentStatusByPaneKey === state.agentStatusByPaneKey) nextAgentStatusByPaneKey = writableWebSessionTabsRecord(state, "agentStatusByPaneKey", batchContext);
		nextAgentStatusByPaneKey[paneKey] = entry;
		updateBatchAgentPaneKey(paneKey, true, batchContext);
		changed = true;
		const entryAttributionChanged = existing?.worktreeId !== entry.worktreeId || existing?.tabId !== entry.tabId;
		const entryFreshnessChanged = !!existing && isAgentStatusFresh(existing, now) !== isAgentStatusFresh(entry, now);
		const doneAttentionChanged = existing?.state === "done" && entry.state === "done" && agentEntryCompletionAt(existing) !== agentEntryCompletionAt(entry);
		const entrySortRelevantChange = !existing || existing.state !== entry.state || !isAgentStatusFresh(existing, now) || entryFreshnessChanged || entryAttributionChanged || doneAttentionChanged || isMirroredCommandCodeTurnBump(existing, entry);
		aggregateRelevantChange = aggregateRelevantChange || entrySortRelevantChange;
		sortRelevantChange = sortRelevantChange || entrySortRelevantChange;
	}
	if (!changed) return null;
	return {
		agentStatusByPaneKey: nextAgentStatusByPaneKey,
		agentStatusEpoch: aggregateRelevantChange ? state.agentStatusEpoch + 1 : state.agentStatusEpoch,
		sortEpoch: sortRelevantChange ? state.sortEpoch + 1 : state.sortEpoch
	};
}
function buildTerminalUnifiedTab(tab, groupId, viewMode) {
	return {
		id: tab.id,
		entityId: tab.id,
		groupId,
		worktreeId: tab.worktreeId,
		contentType: "terminal",
		label: tab.title,
		...tab.quickCommandLabel?.trim() ? { quickCommandLabel: tab.quickCommandLabel.trim() } : {},
		...tab.generatedTitle?.trim() ? { generatedLabel: tab.generatedTitle.trim() } : {},
		...tab.aiVaultTitle ? { aiVaultTitle: tab.aiVaultTitle } : {},
		customLabel: tab.customTitle,
		color: tab.color,
		sortOrder: tab.sortOrder,
		createdAt: tab.createdAt,
		isPreview: false,
		isPinned: tab.isPinned === true,
		...viewMode ? { viewMode } : {}
	};
}
function buildBrowserUnifiedTab(tab, hostTab, existingUnifiedTab, groupId) {
	return {
		id: existingUnifiedTab?.id ?? hostTab.id,
		entityId: tab.id,
		groupId,
		worktreeId: tab.worktreeId,
		contentType: "browser",
		label: tab.title,
		customLabel: null,
		color: hostTab.color !== void 0 ? hostTab.color : existingUnifiedTab?.color ?? null,
		sortOrder: tab.createdAt,
		createdAt: tab.createdAt,
		isPreview: false,
		isPinned: hostTab.isPinned !== void 0 ? hostTab.isPinned === true : existingUnifiedTab?.isPinned === true
	};
}
function buildEditorUnifiedTab(file, tab, hostTabId, existingUnifiedTab, label, groupId, sortOrder, createdAt) {
	return {
		id: hostTabId,
		entityId: file.id,
		groupId,
		worktreeId: file.worktreeId,
		contentType: "editor",
		label,
		customLabel: null,
		color: tab.color !== void 0 ? tab.color : existingUnifiedTab?.color ?? null,
		sortOrder,
		createdAt,
		isPreview: false,
		isPinned: tab.isPinned !== void 0 ? tab.isPinned === true : existingUnifiedTab?.isPinned === true
	};
}
function findExistingEditorUnifiedTab(state, worktreeId, fileId, hostTabId) {
	return (state.unifiedTabsByWorktree[worktreeId] ?? []).find((tab) => tab.contentType === "editor" && (tab.id === hostTabId || tab.entityId === fileId)) ?? null;
}
function buildMirroredEditorTabs(snapshot, environmentId, state, worktreeOpenFileById, hostGroupIdByTabId, fallbackGroupId, sortOffset, now) {
	return snapshot.tabs.filter(isReadyEditorTab).map((tab, index) => {
		const fileId = localEditorFileId(tab);
		const existingFile = worktreeOpenFileById.get(fileId);
		const existingUnifiedTab = findExistingEditorUnifiedTab(state, snapshot.worktree, fileId, tab.id);
		const sourceFileId = editorSourceFileId(tab);
		const groupId = hostGroupIdByTabId.get(tab.id) ?? fallbackGroupId;
		const file = {
			...existingFile,
			id: fileId,
			filePath: tab.filePath,
			relativePath: tab.relativePath,
			worktreeId: snapshot.worktree,
			language: tab.language,
			isDirty: tab.isDirty,
			runtimeEnvironmentId: environmentId,
			mode: tab.type === "markdown" ? tab.mode : "edit",
			markdownPreviewSourceFileId: sourceFileId,
			mirroredFromRuntimeSession: true
		};
		return {
			file,
			hostTabId: tab.id,
			unifiedTab: buildEditorUnifiedTab(file, tab, tab.id, existingUnifiedTab, tab.title.trim() || tab.relativePath || "File", groupId, sortOffset + index, existingUnifiedTab?.createdAt ?? now + sortOffset + index)
		};
	});
}
function findBrowserWorkspaceForRemotePage(state, worktreeId, environmentId, remotePageId) {
	const workspaces = state.browserTabsByWorktree[worktreeId] ?? [];
	for (const workspace of workspaces) {
		const pages = state.browserPagesByWorkspace[workspace.id] ?? [];
		for (const page of pages) {
			const handle = state.remoteBrowserPageHandlesByPageId[page.id];
			if (handle?.environmentId === environmentId && handle.remotePageId === remotePageId) return {
				workspace,
				page,
				unifiedTab: (state.unifiedTabsByWorktree[worktreeId] ?? []).find((tab) => tab.contentType === "browser" && tab.entityId === workspace.id) ?? null
			};
		}
	}
	return null;
}
function browserWorkspaceHasRemoteEnvironmentPage(state, workspace, environmentId) {
	return (state.browserPagesByWorkspace[workspace.id] ?? []).some((page) => state.remoteBrowserPageHandlesByPageId[page.id]?.environmentId === environmentId);
}
function buildMirroredBrowserTabs(snapshot, environmentId, state, hostGroupIdByTabId, fallbackGroupId, sortOffset, now) {
	const renderedGroupIds = collectLayoutGroupIds(state.layoutByWorktree[snapshot.worktree]);
	const clientGroupIds = new Set((state.groupsByWorktree[snapshot.worktree] ?? []).map((group) => group.id));
	return snapshot.tabs.filter(isReadyBrowserTab).map((tab, index) => {
		const existing = findBrowserWorkspaceForRemotePage(state, snapshot.worktree, environmentId, tab.browserPageId);
		const workspaceId = existing?.workspace.id ?? tab.browserWorkspaceId;
		const pageId = existing?.page.id ?? tab.browserPageId;
		const createdAt = existing?.page.createdAt ?? now + sortOffset + index;
		const recordedClientGroupId = peekWebSessionBrowserPlacementGroup({
			environmentId,
			worktreeId: snapshot.worktree,
			remotePageId: tab.browserPageId
		});
		const hostGroupId = hostGroupIdByTabId.get(tab.id) ?? fallbackGroupId;
		const existingClientGroupId = existing?.unifiedTab?.groupId !== hostGroupId ? existing?.unifiedTab?.groupId : void 0;
		const preferredClientGroupId = recordedClientGroupId ?? existingClientGroupId;
		const clientGroupId = preferredClientGroupId && clientGroupIds.has(preferredClientGroupId) && (renderedGroupIds.size === 0 || renderedGroupIds.has(preferredClientGroupId)) ? preferredClientGroupId : void 0;
		const groupId = clientGroupId ?? hostGroupId;
		const title = tab.title.trim() || "Browser";
		const nextPage = {
			id: pageId,
			workspaceId,
			worktreeId: snapshot.worktree,
			url: tab.url,
			title,
			loading: tab.loading,
			faviconUrl: existing?.page.faviconUrl ?? null,
			canGoBack: tab.canGoBack,
			canGoForward: tab.canGoForward,
			loadError: tab.loadError ?? null,
			createdAt,
			browserRuntimeEnvironmentId: environmentId,
			viewportPresetId: existing?.page.viewportPresetId ?? null
		};
		const page = existing && browserPageEqual(existing.page, nextPage) ? existing.page : nextPage;
		const workspace = {
			id: workspaceId,
			worktreeId: snapshot.worktree,
			label: existing?.workspace.label,
			sessionProfileId: existing?.workspace.sessionProfileId ?? null,
			activePageId: page.id,
			pageIds: [page.id],
			url: page.url,
			title: page.title,
			loading: page.loading,
			faviconUrl: page.faviconUrl,
			canGoBack: page.canGoBack,
			canGoForward: page.canGoForward,
			loadError: page.loadError,
			createdAt
		};
		return {
			workspace,
			page,
			certificateFailure: tab.certificateFailure ?? null,
			remotePageId: tab.browserPageId,
			unifiedTab: buildBrowserUnifiedTab(workspace, tab, existing?.unifiedTab ?? null, groupId),
			hostTabId: tab.id,
			...clientGroupId ? { clientGroupId } : {}
		};
	});
}
function chooseTargetGroupId(state, snapshot) {
	const groups = state.groupsByWorktree[snapshot.worktree] ?? [];
	const layoutGroupIds = collectLayoutGroupIds(state.layoutByWorktree[snapshot.worktree]);
	const inRenderedLayout = (groupId) => Boolean(groupId && (layoutGroupIds.size === 0 || layoutGroupIds.has(groupId)));
	const preferred = groups.find((group) => group.id === snapshot.activeGroupId && inRenderedLayout(group.id)) ?? groups.find((group) => group.id === state.activeGroupIdByWorktree[snapshot.worktree] && inRenderedLayout(group.id)) ?? groups.find((group) => inRenderedLayout(group.id));
	const firstRenderedLayoutGroupId = layoutGroupIds.values().next().value;
	return preferred?.id ?? firstRenderedLayoutGroupId ?? snapshot.activeGroupId ?? `${WEB_SESSION_GROUP_PREFIX}${snapshot.worktree}`;
}
function collectLayoutGroupIds(layout) {
	const result = /* @__PURE__ */ new Set();
	const visit = (node) => {
		if (!node) return;
		if (node.type === "leaf") {
			result.add(node.groupId);
			return;
		}
		visit(node.first);
		visit(node.second);
	};
	visit(layout);
	return result;
}
function buildHostGroupIdByTabId(hostGroups) {
	const result = /* @__PURE__ */ new Map();
	for (const group of hostGroups ?? []) {
		for (const tabId of group.tabOrder) result.set(tabId, group.id);
		if (group.activeTabId) result.set(group.activeTabId, group.id);
	}
	return result;
}
function pruneTabGroupLayout(layout, validGroupIds) {
	if (!layout) return null;
	if (layout.type === "leaf") return validGroupIds.has(layout.groupId) ? layout : null;
	const first = pruneTabGroupLayout(layout.first, validGroupIds);
	const second = pruneTabGroupLayout(layout.second, validGroupIds);
	if (first && second) return {
		...layout,
		first,
		second
	};
	return first ?? second;
}
function appendTabGroupLayout(first, second) {
	if (!first) return second;
	if (!second) return first;
	return {
		type: "split",
		direction: "horizontal",
		first,
		second
	};
}
function tabGroupLayoutEqual(a, b) {
	if (!a || !b) return !a && !b;
	if (a.type !== b.type) return false;
	if (a.type === "leaf") return b.type === "leaf" && a.groupId === b.groupId;
	return b.type === "split" && a.direction === b.direction && a.ratio === b.ratio && tabGroupLayoutEqual(a.first, b.first) && tabGroupLayoutEqual(a.second, b.second);
}
function mapHostRecentTabIds(recentTabIds, hostToLocalTabId, tabOrder) {
	if (!recentTabIds || recentTabIds.length === 0) return [];
	const valid = new Set(tabOrder);
	return sanitizeRecentTabIds(recentTabIds.map((tabId) => hostToLocalTabId.get(tabId) ?? "").filter(Boolean), [...valid]);
}
function buildHostToLocalTabIdMap({ terminalSurfaces, terminalTabs, browserTabs, editorTabs }) {
	const hostToLocal = /* @__PURE__ */ new Map();
	const terminalIds = new Set(terminalTabs.map((tab) => tab.id));
	for (const surface of terminalSurfaces) {
		const localId = toWebTerminalSurfaceTabId(surface.parentTabId);
		if (terminalIds.has(localId)) {
			hostToLocal.set(surface.parentTabId, localId);
			hostToLocal.set(surface.id, localId);
		}
	}
	for (const entry of browserTabs) {
		hostToLocal.set(entry.hostTabId, entry.unifiedTab.id);
		hostToLocal.set(entry.unifiedTab.id, entry.unifiedTab.id);
	}
	for (const entry of editorTabs) hostToLocal.set(entry.hostTabId, entry.unifiedTab.id);
	return hostToLocal;
}
function updateHostSessionTabIdMappings(args) {
	clearHostSessionTabIdMappings(args.environmentId, args.worktreeId);
	const mirroredTerminalIds = new Set(args.terminalTabs.map((tab) => tab.id));
	for (const surface of args.terminalSurfaces) {
		const localId = toWebTerminalSurfaceTabId(surface.parentTabId);
		if (mirroredTerminalIds.has(localId)) setHostSessionTabIdMapping({
			...args,
			tabId: localId
		}, surface.parentTabId);
	}
	for (const entry of args.browserTabs) setHostSessionTabIdMapping({
		...args,
		tabId: entry.unifiedTab.id
	}, entry.hostTabId);
	for (const entry of args.editorTabs) setHostSessionTabIdMapping({
		...args,
		tabId: entry.unifiedTab.id
	}, entry.hostTabId);
}
function retainClientPlacedMirroredTabs(args) {
	return args.groups.map((group) => {
		const retainedTabOrder = group.tabOrder.filter((tabId) => args.validUnifiedTabIds.has(tabId) && (!args.mirroredUnifiedIds.has(tabId) || args.clientGroupIdByLocalTabId.get(tabId) === group.id));
		const placedTabIds = [...args.clientGroupIdByLocalTabId].filter(([tabId, groupId]) => groupId === group.id && args.validUnifiedTabIds.has(tabId) && !retainedTabOrder.includes(tabId)).map(([tabId]) => tabId);
		const tabOrder = [...retainedTabOrder, ...placedTabIds];
		const activeTabId = args.nextActiveUnifiedTabId && tabOrder.includes(args.nextActiveUnifiedTabId) ? args.nextActiveUnifiedTabId : group.activeTabId && tabOrder.includes(group.activeTabId) ? group.activeTabId : tabOrder[0] ?? null;
		return {
			...group,
			tabOrder,
			activeTabId,
			recentTabIds: activeTabId ? pushRecentTabId(sanitizeRecentTabIds(group.recentTabIds, tabOrder), activeTabId) : []
		};
	});
}
function buildMirroredHostGroups({ currentGroups, hostGroups, hostToLocalTabId, mirroredUnifiedIds, nextActiveUnifiedTabId, now, validUnifiedTabIds, environmentId, worktreeId, clientGroupIdByLocalTabId }) {
	const strippedGroups = retainClientPlacedMirroredTabs({
		groups: currentGroups,
		mirroredUnifiedIds,
		validUnifiedTabIds,
		clientGroupIdByLocalTabId,
		nextActiveUnifiedTabId
	});
	const groupsById = new Map(strippedGroups.map((group) => [group.id, group]));
	const orderedGroups = [];
	const seen = /* @__PURE__ */ new Set();
	for (const hostGroup of hostGroups) {
		const existing = groupsById.get(hostGroup.id);
		const localHostOrder = hostGroup.tabOrder.map((tabId) => hostToLocalTabId.get(tabId)).filter((tabId) => tabId !== void 0 && validUnifiedTabIds.has(tabId) && !clientGroupIdByLocalTabId.has(tabId));
		const hostTabOrder = [...existing?.tabOrder.filter((tabId) => !localHostOrder.includes(tabId)) ?? [], ...localHostOrder];
		const tabOrder = resolveWebSessionReorderedOrder({ environmentId }, worktreeId, hostGroup.id, hostTabOrder, now);
		if (tabOrder.length === 0) continue;
		const activeFromHost = hostGroup.activeTabId !== null ? hostToLocalTabId.get(hostGroup.activeTabId) ?? null : null;
		const activeTabId = nextActiveUnifiedTabId && tabOrder.includes(nextActiveUnifiedTabId) ? nextActiveUnifiedTabId : activeFromHost && tabOrder.includes(activeFromHost) ? activeFromHost : existing?.activeTabId && tabOrder.includes(existing.activeTabId) ? existing.activeTabId : tabOrder[0] ?? null;
		orderedGroups.push({
			id: hostGroup.id,
			worktreeId,
			tabOrder,
			activeTabId,
			recentTabIds: activeTabId ? pushRecentTabId(mapHostRecentTabIds(hostGroup.recentTabIds, hostToLocalTabId, tabOrder), activeTabId) : []
		});
		seen.add(hostGroup.id);
	}
	for (const group of strippedGroups) if (!seen.has(group.id) && (group.tabOrder.length > 0 || isWebSessionBrowserPlacementGroupReserved({
		worktreeId,
		groupId: group.id
	}))) orderedGroups.push(group);
	return orderedGroups.length > 0 ? orderedGroups : null;
}
function sameStringArray(a, b) {
	if (a.length !== b.length) return false;
	return a.every((value, index) => value === b[index]);
}
function sameAgentStateHistory(a, b) {
	if (a.length !== b.length) return false;
	return a.every((entry, index) => entry.state === b[index]?.state && entry.prompt === b[index]?.prompt && entry.startedAt === b[index]?.startedAt && entry.interrupted === b[index]?.interrupted);
}
function agentStatusEntryEqual(a, b) {
	if (!a) return false;
	return a.state === b.state && a.prompt === b.prompt && a.updatedAt === b.updatedAt && a.stateStartedAt === b.stateStartedAt && a.agentType === b.agentType && a.paneKey === b.paneKey && a.worktreeId === b.worktreeId && a.tabId === b.tabId && a.terminalTitle === b.terminalTitle && a.toolName === b.toolName && a.toolInput === b.toolInput && a.interactivePrompt === b.interactivePrompt && a.lastAssistantMessage === b.lastAssistantMessage && a.interrupted === b.interrupted && a.promptInteractionKey === b.promptInteractionKey && a.restoredUnconfirmed === b.restoredUnconfirmed && agentProviderSessionsEqual(a.agentType, a.providerSession, b.providerSession) && sameAgentStateHistory(a.stateHistory, b.stateHistory);
}
function isAgentStatusFresh(entry, now) {
	return entry.restoredUnconfirmed !== true && now - entry.updatedAt <= 18e5;
}
function isMirroredCommandCodeTurnBump(existing, entry) {
	return existing?.agentType === "command-code" && entry.agentType === "command-code" && existing.state === "working" && entry.state === "working" && entry.stateStartedAt > existing.stateStartedAt;
}
function sanitizeRecentTabIds(recent, tabOrder) {
	if (!recent || recent.length === 0) return [];
	const valid = new Set(tabOrder);
	const seen = /* @__PURE__ */ new Set();
	const reversed = [];
	for (let i = recent.length - 1; i >= 0; i -= 1) {
		const id = recent[i];
		if (!valid.has(id) || seen.has(id)) continue;
		seen.add(id);
		reversed.push(id);
	}
	return reversed.toReversed();
}
function pushRecentTabId(recent, tabId) {
	const base = recent ?? [];
	if (base.length > 0 && base.at(-1) === tabId) return base;
	return [...base.filter((id) => id !== tabId), tabId];
}
function writableWebSessionTabsRecord(state, recordKey, batchContext) {
	const record = state[recordKey] ?? {};
	if (!batchContext) return { ...record };
	if (batchContext.changedRecords.has(recordKey)) return record;
	const next = { ...record };
	const mutableState = state;
	mutableState[recordKey] = next;
	batchContext.changedRecords.add(recordKey);
	return next;
}
function withWorktreeEntry(state, recordKey, key, value, equal, batchContext, deleteNull = true) {
	const record = state[recordKey] ?? {};
	if (equal(record[key], value)) return record;
	const next = writableWebSessionTabsRecord(state, recordKey, batchContext);
	if (value === null && deleteNull) delete next[key];
	else next[key] = value;
	return next;
}
function terminalTabEqual(a, b) {
	return a.id === b.id && a.ptyId === b.ptyId && a.worktreeId === b.worktreeId && a.title === b.title && a.defaultTitle === b.defaultTitle && a.quickCommandLabel === b.quickCommandLabel && a.startupCwd === b.startupCwd && a.generatedTitle === b.generatedTitle && a.aiVaultTitle?.agent === b.aiVaultTitle?.agent && a.aiVaultTitle?.sessionId === b.aiVaultTitle?.sessionId && a.aiVaultTitle?.title === b.aiVaultTitle?.title && a.customTitle === b.customTitle && a.color === b.color && a.sortOrder === b.sortOrder && a.createdAt === b.createdAt && a.generation === b.generation && a.shellOverride === b.shellOverride && a.launchAgent === b.launchAgent && a.pendingActivationSpawn === b.pendingActivationSpawn;
}
function sameTerminalTabs(a, b) {
	const left = a ?? [];
	const right = b ?? [];
	if (left.length !== right.length) return false;
	return left.every((tab, index) => terminalTabEqual(tab, right[index]));
}
function browserPageEqual(a, b) {
	return a.id === b.id && a.workspaceId === b.workspaceId && a.worktreeId === b.worktreeId && a.url === b.url && a.title === b.title && a.loading === b.loading && a.faviconUrl === b.faviconUrl && a.canGoBack === b.canGoBack && a.canGoForward === b.canGoForward && a.loadError?.code === b.loadError?.code && a.loadError?.description === b.loadError?.description && a.loadError?.validatedUrl === b.loadError?.validatedUrl && a.createdAt === b.createdAt && a.browserRuntimeEnvironmentId === b.browserRuntimeEnvironmentId && a.viewportPresetId === b.viewportPresetId;
}
function browserCertificateFailureEqual(a, b) {
	const left = a ?? null;
	const right = b ?? null;
	if (left === right) return true;
	return Boolean(left && right && left.challengeId === right.challengeId && left.browserPageId === right.browserPageId && left.errorCode === right.errorCode && left.error === right.error && left.origin === right.origin && left.displayHost === right.displayHost && left.canProceed === right.canProceed && left.observedAt === right.observedAt);
}
function sameBrowserPages(a, b) {
	const left = a ?? [];
	const right = b ?? [];
	if (left.length !== right.length) return false;
	return left.every((page, index) => browserPageEqual(page, right[index]));
}
function browserWorkspaceEqual(a, b) {
	return a.id === b.id && a.worktreeId === b.worktreeId && a.label === b.label && a.sessionProfileId === b.sessionProfileId && a.activePageId === b.activePageId && sameStringArray(a.pageIds ?? [], b.pageIds ?? []) && a.url === b.url && a.title === b.title && a.loading === b.loading && a.faviconUrl === b.faviconUrl && a.canGoBack === b.canGoBack && a.canGoForward === b.canGoForward && a.loadError?.code === b.loadError?.code && a.loadError?.description === b.loadError?.description && a.loadError?.validatedUrl === b.loadError?.validatedUrl && a.createdAt === b.createdAt;
}
function sameBrowserTabs(a, b) {
	const left = a ?? [];
	const right = b ?? [];
	if (left.length !== right.length) return false;
	return left.every((tab, index) => browserWorkspaceEqual(tab, right[index]));
}
function openFileEqual(a, b) {
	return a.id === b.id && a.filePath === b.filePath && a.relativePath === b.relativePath && a.worktreeId === b.worktreeId && a.language === b.language && a.isDirty === b.isDirty && a.runtimeEnvironmentId === b.runtimeEnvironmentId && a.markdownPreviewSourceFileId === b.markdownPreviewSourceFileId && a.markdownPreviewAnchor === b.markdownPreviewAnchor && a.isPreview === b.isPreview && a.isUntitled === b.isUntitled && a.deleteUntouchedOnClose === b.deleteUntouchedOnClose && a.externalMutation === b.externalMutation && a.mirroredFromRuntimeSession === b.mirroredFromRuntimeSession && a.mode === b.mode;
}
function sameOpenFiles(a, b) {
	if (a.length !== b.length) return false;
	return a.every((file, index) => openFileEqual(file, b[index]));
}
function webSessionOpenFilesForWorktree(state, worktreeId, batchContext) {
	if (!batchContext) return state.openFiles.filter((file) => file.worktreeId === worktreeId);
	let index = batchContext.openFilesIndex;
	if (!index || index.source !== state.openFiles) {
		const byWorktree = /* @__PURE__ */ new Map();
		for (const file of state.openFiles) {
			const bucket = byWorktree.get(file.worktreeId) ?? [];
			bucket.push(file);
			byWorktree.set(file.worktreeId, bucket);
		}
		index = {
			source: state.openFiles,
			byWorktree
		};
		batchContext.openFilesIndex = index;
	}
	return index.byWorktree.get(worktreeId) ?? [];
}
function advanceWebSessionOpenFilesIndex(batchContext, nextOpenFiles, worktreeId) {
	const index = batchContext?.openFilesIndex;
	if (!index || index.source === nextOpenFiles) return;
	const bucket = [];
	for (const file of nextOpenFiles) if (file.worktreeId === worktreeId) bucket.push(file);
	index.byWorktree.set(worktreeId, bucket);
	index.source = nextOpenFiles;
}
function firstOpenFileByIdForWorktree(files) {
	const byId = /* @__PURE__ */ new Map();
	for (const file of files) if (!byId.has(file.id)) byId.set(file.id, file);
	return byId;
}
function tabEqual(a, b) {
	return a.id === b.id && a.entityId === b.entityId && a.groupId === b.groupId && a.worktreeId === b.worktreeId && a.contentType === b.contentType && a.label === b.label && a.generatedLabel === b.generatedLabel && a.aiVaultTitle?.agent === b.aiVaultTitle?.agent && a.aiVaultTitle?.sessionId === b.aiVaultTitle?.sessionId && a.aiVaultTitle?.title === b.aiVaultTitle?.title && a.customLabel === b.customLabel && a.color === b.color && a.sortOrder === b.sortOrder && a.createdAt === b.createdAt && a.isPreview === b.isPreview && a.isPinned === b.isPinned;
}
function sameUnifiedTabs(a, b) {
	const left = a ?? [];
	const right = b ?? [];
	if (left.length !== right.length) return false;
	return left.every((tab, index) => tabEqual(tab, right[index]));
}
function groupEqual(a, b) {
	return a.id === b.id && a.worktreeId === b.worktreeId && a.activeTabId === b.activeTabId && sameStringArray(a.tabOrder, b.tabOrder) && sameStringArray(a.recentTabIds ?? [], b.recentTabIds ?? []);
}
function sameGroups(a, b) {
	const left = a ?? [];
	const right = b ?? [];
	if (left.length !== right.length) return false;
	return left.every((group, index) => groupEqual(group, right[index]));
}
function toVisibleTabType(tab) {
	if (tab.contentType === "browser" || tab.contentType === "terminal") return tab.contentType;
	return "editor";
}
function applyWebSessionTabsSnapshotWithContext(state, rawSnapshot, environmentId, now = Date.now(), batchContext) {
	if (suppressE2eWebRuntimeBrowserSnapshot(rawSnapshot)) return state;
	const worktreeId = rawSnapshot.worktree;
	if (worktreeId === "global-floating-terminal") return state;
	const snapshotHostTabId = (tab) => tab.type === "terminal" ? tab.parentTabId : tab.id;
	reconcileWebSessionCloseIntents({ environmentId }, worktreeId, new Set(rawSnapshot.tabs.map((tab) => snapshotHostTabId(tab))));
	const snapshot = rawSnapshot.tabs.some((tab) => isWebSessionCloseIntentPending({ environmentId }, worktreeId, snapshotHostTabId(tab), now)) ? {
		...rawSnapshot,
		tabs: rawSnapshot.tabs.filter((tab) => !isWebSessionCloseIntentPending({ environmentId }, worktreeId, snapshotHostTabId(tab), now))
	} : rawSnapshot;
	const focusIntent = peekWebSessionFocusIntent({ environmentId }, worktreeId);
	const focusIntentHostTabId = focusIntent?.hostTabId ?? null;
	const matchingFocusIntentTab = focusIntentHostTabId === null ? null : focusIntent?.leafId ? snapshot.tabs.find((tab) => tab.type === "terminal" && tab.leafId === focusIntent.leafId && (tab.id === focusIntentHostTabId || tab.parentTabId === focusIntentHostTabId)) ?? null : snapshot.tabs.find((tab) => tab.id === focusIntentHostTabId || tab.type === "terminal" && tab.parentTabId === focusIntentHostTabId || tab.type === "browser" && tab.browserPageId === focusIntentHostTabId) ?? null;
	const expectedCurrentLocalTabId = focusIntent?.expectedCurrentLocalTabId;
	const currentVisibleLocalTabId = resolveWebSessionVisibleTabId(state, worktreeId);
	const callerFocusIntentTab = matchingFocusIntentTab && (expectedCurrentLocalTabId === void 0 || expectedCurrentLocalTabId === currentVisibleLocalTabId) ? matchingFocusIntentTab : null;
	const followIntentTab = snapshot.navigationIntent === "follow" ? snapshot.tabs.find((tab) => tab.id === snapshot.activeTabId) ?? null : null;
	const navigationIntentTab = callerFocusIntentTab ?? followIntentTab;
	const honorSnapshotActiveFocus = navigationIntentTab !== null;
	if (matchingFocusIntentTab) clearWebSessionFocusIntent({ environmentId }, worktreeId);
	const currentTerminalTabs = state.tabsByWorktree[worktreeId] ?? [];
	const existingTerminalById = new Map(currentTerminalTabs.map((tab) => [tab.id, tab]));
	const terminalSurfaceTabs = snapshot.tabs.filter(isTerminalSurfaceTab);
	const readyTerminalTabs = terminalSurfaceTabs.filter(isReadyTerminalTab);
	const nextRemotePtyIds = new Set(readyTerminalTabs.map((tab) => toRemoteRuntimePtyId(tab.terminal, environmentId)));
	const nextMirroredTerminalIds = new Set(terminalSurfaceTabs.map((tab) => toWebTerminalSurfaceTabId(tab.parentTabId)));
	const nextHostTerminalTabIds = new Set(terminalSurfaceTabs.map((tab) => tab.parentTabId));
	const exactProvisionalHandoffs = new Set(currentTerminalTabs.filter((tab) => !isMirroredTerminalSurfaceId(tab.id)).filter((tab) => {
		if (nextHostTerminalTabIds.has(tab.id)) return true;
		const handoff = {
			environmentId,
			worktreeId,
			provisionalTabId: tab.id
		};
		const hostTabId = resolveWebAgentSessionHandoff(handoff);
		return hostTabId !== null && (nextHostTerminalTabIds.has(hostTabId) || isWebAgentSessionHandoffPostCreateSnapshotConfirmed(handoff));
	}).map((tab) => tab.id));
	const retainedTerminalTabs = currentTerminalTabs.filter((tab) => !shouldReplaceTerminalTab(tab, environmentId, nextRemotePtyIds, nextMirroredTerminalIds, exactProvisionalHandoffs));
	const mirroredTerminalTabs = buildMirroredTerminalTabs(snapshot, environmentId, existingTerminalById, state.terminalLayoutsByTabId, retainedTerminalTabs.length, now, callerFocusIntentTab?.type === "terminal" ? {
		parentTabId: callerFocusIntentTab.parentTabId,
		leafId: callerFocusIntentTab.leafId
	} : void 0);
	const mirroredTerminalTabEntries = mirroredTerminalTabs.map((entry) => entry.tab);
	const retainedTerminalIds = new Set(retainedTerminalTabs.map((tab) => tab.id));
	const nextTerminalTabs = retainedTerminalTabs.length + mirroredTerminalTabEntries.length > 0 ? [...retainedTerminalTabs, ...mirroredTerminalTabEntries] : null;
	const mirroredTerminalIds = new Set(mirroredTerminalTabEntries.map((tab) => tab.id));
	const removedTerminalIds = new Set(currentTerminalTabs.filter((tab) => !retainedTerminalIds.has(tab.id)).map((tab) => tab.id));
	const removedTerminalResourceIds = [...removedTerminalIds].filter((tabId) => !mirroredTerminalIds.has(tabId));
	for (const provisionalTabId of exactProvisionalHandoffs) clearWebAgentSessionHandoff({
		environmentId,
		worktreeId,
		provisionalTabId
	});
	const targetGroupId = chooseTargetGroupId(state, snapshot);
	const hostGroupIdByTabId = buildHostGroupIdByTabId(snapshot.tabGroups);
	const readyBrowserTabs = snapshot.tabs.filter(isReadyBrowserTab);
	const nextRemoteBrowserPageIds = new Set(readyBrowserTabs.map((tab) => tab.browserPageId));
	const mirroredBrowserTabs = buildMirroredBrowserTabs(snapshot, environmentId, state, hostGroupIdByTabId, targetGroupId, mirroredTerminalTabEntries.length, now);
	const mirroredBrowserWorkspaceIds = new Set(mirroredBrowserTabs.map((entry) => entry.workspace.id));
	const currentBrowserTabs = state.browserTabsByWorktree[worktreeId] ?? [];
	const removedBrowserWorkspaceIds = new Set(currentBrowserTabs.filter((tab) => {
		if (mirroredBrowserWorkspaceIds.has(tab.id)) return true;
		if (!browserWorkspaceHasRemoteEnvironmentPage(state, tab, environmentId)) return false;
		return !(state.browserPagesByWorkspace[tab.id] ?? []).some((page) => {
			const handle = state.remoteBrowserPageHandlesByPageId[page.id];
			return handle?.environmentId === environmentId && nextRemoteBrowserPageIds.has(handle.remotePageId);
		});
	}).map((tab) => tab.id));
	const retainedBrowserTabs = currentBrowserTabs.filter((tab) => !removedBrowserWorkspaceIds.has(tab.id));
	const nextBrowserTabs = retainedBrowserTabs.length + mirroredBrowserTabs.length > 0 ? [...retainedBrowserTabs, ...mirroredBrowserTabs.map((entry) => entry.workspace)] : null;
	const readyEditorTabs = snapshot.tabs.filter(isReadyEditorTab);
	const worktreeOpenFiles = webSessionOpenFilesForWorktree(state, worktreeId, batchContext);
	const mirroredEditorTabs = buildMirroredEditorTabs(snapshot, environmentId, state, firstOpenFileByIdForWorktree(worktreeOpenFiles), hostGroupIdByTabId, targetGroupId, mirroredTerminalTabEntries.length + mirroredBrowserTabs.length, now);
	const mirroredEditorFileIds = new Set(mirroredEditorTabs.map((entry) => entry.file.id));
	const mirroredEditorHostTabIds = new Set(mirroredEditorTabs.map((entry) => entry.hostTabId));
	const removedEditorFileIds = new Set(worktreeOpenFiles.filter((file) => file.runtimeEnvironmentId === environmentId && (file.mode === "edit" || file.mode === "markdown-preview") && file.mirroredFromRuntimeSession === true && !mirroredEditorFileIds.has(file.id)).map((file) => file.id));
	const isReplacedOpenFile = (file) => file.runtimeEnvironmentId === environmentId && (removedEditorFileIds.has(file.id) || mirroredEditorFileIds.has(file.id));
	const replacedOpenFileCount = worktreeOpenFiles.filter(isReplacedOpenFile).length;
	const nextWorktreeOpenFileIds = new Set(worktreeOpenFiles.filter((file) => !isReplacedOpenFile(file)).map((file) => file.id));
	for (const fileId of mirroredEditorFileIds) nextWorktreeOpenFileIds.add(fileId);
	const mirroredOpenFiles = mirroredEditorTabs.map((entry) => entry.file);
	const nextOpenFiles = (() => {
		if (replacedOpenFileCount === 0 && mirroredOpenFiles.length === 0) return state.openFiles;
		const next = [...state.openFiles.filter((file) => !(file.worktreeId === worktreeId && file.runtimeEnvironmentId === environmentId && (removedEditorFileIds.has(file.id) || mirroredEditorFileIds.has(file.id)))), ...mirroredOpenFiles];
		return sameOpenFiles(state.openFiles, next) ? state.openFiles : next;
	})();
	advanceWebSessionOpenFilesIndex(batchContext, nextOpenFiles, worktreeId);
	const currentUnifiedTabs = state.unifiedTabsByWorktree[worktreeId] ?? [];
	const retainedUnifiedTabs = currentUnifiedTabs.filter((tab) => {
		if (tab.contentType === "browser") return !removedBrowserWorkspaceIds.has(tab.entityId) && !mirroredBrowserWorkspaceIds.has(tab.entityId);
		if (tab.contentType === "editor") return !removedEditorFileIds.has(tab.entityId) && !mirroredEditorFileIds.has(tab.entityId) && !mirroredEditorHostTabIds.has(tab.id);
		if (tab.contentType !== "terminal") return true;
		if (removedTerminalIds.has(tab.entityId) || removedTerminalIds.has(tab.id)) return false;
		return !mirroredTerminalIds.has(tab.entityId) && !mirroredTerminalIds.has(tab.id);
	});
	const existingViewModeByTabId = new Map(currentUnifiedTabs.filter((tab) => tab.contentType === "terminal" && tab.viewMode).map((tab) => [tab.id, tab.viewMode]));
	const mirroredTerminalUnifiedTabs = mirroredTerminalTabs.map((entry) => buildTerminalUnifiedTab(entry.tab, hostGroupIdByTabId.get(entry.hostTabId) ?? targetGroupId, entry.tab.viewMode ?? existingViewModeByTabId.get(entry.tab.id)));
	const mirroredBrowserUnifiedTabs = mirroredBrowserTabs.map((entry) => entry.unifiedTab);
	const mirroredEditorUnifiedTabs = mirroredEditorTabs.map((entry) => entry.unifiedTab);
	const mirroredUnifiedTabs = [
		...mirroredTerminalUnifiedTabs,
		...mirroredBrowserUnifiedTabs,
		...mirroredEditorUnifiedTabs
	];
	const nextUnifiedTabs = retainedUnifiedTabs.length + mirroredUnifiedTabs.length > 0 ? [...retainedUnifiedTabs, ...mirroredUnifiedTabs] : null;
	const validUnifiedTabIds = new Set(nextUnifiedTabs?.map((tab) => tab.id) ?? []);
	const activeHostTerminalId = terminalSurfaceTabs.find((tab) => tab.id === snapshot.activeTabId)?.id ?? terminalSurfaceTabs.find((tab) => tab.isActive)?.id ?? null;
	const activeHostTerminalParentId = terminalSurfaceTabs.find((tab) => tab.id === activeHostTerminalId)?.parentTabId ?? terminalSurfaceTabs.find((tab) => tab.isActive)?.parentTabId ?? null;
	const activeMirroredTerminalId = activeHostTerminalId ? toWebTerminalSurfaceTabId(activeHostTerminalParentId ?? activeHostTerminalId) : null;
	const activeHostBrowser = readyBrowserTabs.find((tab) => tab.id === snapshot.activeTabId) ?? readyBrowserTabs.find((tab) => tab.isActive) ?? null;
	const activeMirroredBrowser = activeHostBrowser ? mirroredBrowserTabs.find((entry) => entry.remotePageId === activeHostBrowser.browserPageId) ?? null : null;
	const activeMirroredBrowserTabId = activeMirroredBrowser?.unifiedTab.id ?? null;
	const activeMirroredBrowserWorkspaceId = activeMirroredBrowser?.workspace.id ?? null;
	const activeHostEditor = readyEditorTabs.find((tab) => tab.id === snapshot.activeTabId) ?? readyEditorTabs.find((tab) => tab.isActive) ?? null;
	const activeMirroredEditor = activeHostEditor ? mirroredEditorTabs.find((entry) => entry.hostTabId === activeHostEditor.id) ?? null : null;
	const activeMirroredEditorFileId = activeMirroredEditor?.file.id ?? null;
	const activeMirroredEditorTabId = activeMirroredEditor?.unifiedTab.id ?? null;
	const intentMirroredTerminalId = navigationIntentTab?.type === "terminal" ? toWebTerminalSurfaceTabId(navigationIntentTab.parentTabId) : null;
	const intentMirroredBrowser = navigationIntentTab?.type === "browser" ? mirroredBrowserTabs.find((entry) => entry.hostTabId === navigationIntentTab.id || entry.remotePageId === navigationIntentTab.browserPageId) ?? null : null;
	const intentMirroredEditor = navigationIntentTab?.type === "markdown" || navigationIntentTab?.type === "file" ? mirroredEditorTabs.find((entry) => entry.hostTabId === navigationIntentTab.id) ?? null : null;
	const currentActiveTerminalStillExists = state.activeTabIdByWorktree[worktreeId] && (nextTerminalTabs ?? []).some((tab) => tab.id === state.activeTabIdByWorktree[worktreeId]) ? state.activeTabIdByWorktree[worktreeId] : null;
	const intentTerminalId = honorSnapshotActiveFocus && navigationIntentTab?.type === "terminal" ? intentMirroredTerminalId : null;
	const nextActiveTerminalId = intentTerminalId ?? currentActiveTerminalStillExists ?? (snapshot.activeTabType === "terminal" ? activeMirroredTerminalId ?? mirroredTerminalTabEntries[0]?.id : mirroredTerminalTabEntries[0]?.id) ?? null;
	const currentActiveBrowserStillExists = state.activeBrowserTabIdByWorktree[worktreeId] && (nextBrowserTabs ?? []).some((tab) => tab.id === state.activeBrowserTabIdByWorktree[worktreeId]) ? state.activeBrowserTabIdByWorktree[worktreeId] : null;
	const intentBrowserWorkspaceId = honorSnapshotActiveFocus && navigationIntentTab?.type === "browser" ? intentMirroredBrowser?.workspace.id ?? null : null;
	const nextActiveBrowserWorkspaceId = intentBrowserWorkspaceId ?? currentActiveBrowserStillExists ?? (snapshot.activeTabType === "browser" ? activeMirroredBrowserWorkspaceId ?? mirroredBrowserTabs[0]?.workspace.id : mirroredBrowserTabs[0]?.workspace.id) ?? null;
	const activeEditorFileIdForWorktree = state.activeFileIdByWorktree[worktreeId];
	const currentActiveEditorStillExists = activeEditorFileIdForWorktree && nextWorktreeOpenFileIds.has(activeEditorFileIdForWorktree) ? activeEditorFileIdForWorktree : null;
	const intentEditorFileId = honorSnapshotActiveFocus ? intentMirroredEditor?.file.id ?? null : null;
	const nextActiveEditorFileId = intentEditorFileId ?? currentActiveEditorStillExists ?? (snapshot.activeTabType === "markdown" || snapshot.activeTabType === "file" ? activeMirroredEditorFileId ?? mirroredEditorTabs[0]?.file.id : mirroredEditorTabs[0]?.file.id) ?? null;
	const currentVisibleUnifiedTabId = resolveWebSessionVisibleTabId(state, worktreeId, nextUnifiedTabs ?? []);
	const nextActiveUnifiedTabId = (honorSnapshotActiveFocus ? navigationIntentTab?.type === "browser" ? intentMirroredBrowser?.unifiedTab.id ?? null : navigationIntentTab?.type === "terminal" ? intentTerminalId : navigationIntentTab?.type === "markdown" || navigationIntentTab?.type === "file" ? intentMirroredEditor?.unifiedTab.id ?? null : null : null) ?? currentVisibleUnifiedTabId ?? (snapshot.activeTabType === "browser" ? activeMirroredBrowserTabId ?? mirroredBrowserTabs[0]?.unifiedTab.id ?? state.activeTabIdByWorktree[worktreeId] ?? nextActiveTerminalId : snapshot.activeTabType === "markdown" || snapshot.activeTabType === "file" ? activeMirroredEditorTabId ?? mirroredEditorTabs[0]?.unifiedTab.id ?? state.activeTabIdByWorktree[worktreeId] ?? nextActiveTerminalId : nextActiveTerminalId);
	const mirroredUnifiedIds = new Set(mirroredUnifiedTabs.map((tab) => tab.id));
	const hostToLocalTabId = buildHostToLocalTabIdMap({
		terminalSurfaces: terminalSurfaceTabs,
		terminalTabs: mirroredTerminalTabEntries,
		browserTabs: mirroredBrowserTabs,
		editorTabs: mirroredEditorTabs
	});
	updateHostSessionTabIdMappings({
		environmentId,
		worktreeId,
		terminalSurfaces: terminalSurfaceTabs,
		terminalTabs: mirroredTerminalTabEntries,
		browserTabs: mirroredBrowserTabs,
		editorTabs: mirroredEditorTabs
	});
	const currentGroups = state.groupsByWorktree[worktreeId] ?? [];
	const clientGroupIdByLocalTabId = new Map(mirroredBrowserTabs.flatMap((entry) => entry.clientGroupId ? [[entry.unifiedTab.id, entry.clientGroupId]] : []));
	const nextGroups = (() => {
		if (!nextUnifiedTabs || nextUnifiedTabs.length === 0) return null;
		if (snapshot.tabGroups && snapshot.tabGroups.length > 0) return buildMirroredHostGroups({
			currentGroups,
			hostGroups: snapshot.tabGroups,
			hostToLocalTabId,
			mirroredUnifiedIds,
			nextActiveUnifiedTabId,
			now,
			validUnifiedTabIds,
			environmentId,
			worktreeId,
			clientGroupIdByLocalTabId
		});
		const strippedGroups = retainClientPlacedMirroredTabs({
			groups: currentGroups,
			mirroredUnifiedIds,
			validUnifiedTabIds,
			clientGroupIdByLocalTabId,
			nextActiveUnifiedTabId
		});
		const target = strippedGroups.find((group) => group.id === targetGroupId) ?? {
			id: targetGroupId,
			worktreeId,
			activeTabId: null,
			tabOrder: [],
			recentTabIds: []
		};
		const targetOrder = [...target.tabOrder.filter((tabId) => validUnifiedTabIds.has(tabId)), ...mirroredUnifiedTabs.filter((tab) => !clientGroupIdByLocalTabId.has(tab.id)).map((tab) => tab.id)];
		const targetActiveTabId = nextActiveUnifiedTabId && targetOrder.includes(nextActiveUnifiedTabId) ? nextActiveUnifiedTabId : target.activeTabId && targetOrder.includes(target.activeTabId) ? target.activeTabId : targetOrder[0] ?? null;
		const updatedTarget = {
			...target,
			worktreeId,
			tabOrder: targetOrder,
			activeTabId: targetActiveTabId,
			recentTabIds: targetActiveTabId ? pushRecentTabId(sanitizeRecentTabIds(target.recentTabIds, targetOrder), targetActiveTabId) : []
		};
		return (strippedGroups.some((group) => group.id === targetGroupId) ? strippedGroups.map((group) => group.id === targetGroupId ? updatedTarget : group) : [...strippedGroups, updatedTarget]).filter((group) => group.id === targetGroupId || group.tabOrder.length > 0 || isWebSessionBrowserPlacementGroupReserved({
			worktreeId,
			groupId: group.id
		}));
	})();
	const nextTabBarOrder = (() => {
		const current = state.tabBarOrderByWorktree[worktreeId] ?? [];
		const validTabBarIds = new Set([...retainedUnifiedTabs.map((tab) => tab.id), ...mirroredUnifiedTabs.map((tab) => tab.id)]);
		const hostTabBarOrder = snapshot.tabGroups?.flatMap((group) => group.tabOrder.map((tabId) => hostToLocalTabId.get(tabId)).filter((tabId) => tabId !== void 0 && validTabBarIds.has(tabId))) ?? [];
		const next = [];
		const push = (tabId) => {
			if (validTabBarIds.has(tabId) && !next.includes(tabId)) next.push(tabId);
		};
		for (const tabId of current) push(tabId);
		const hostOrMirroredOrder = hostTabBarOrder.length > 0 ? hostTabBarOrder : mirroredUnifiedTabs.map((tab) => tab.id);
		for (const tabId of hostOrMirroredOrder) push(tabId);
		return next;
	})();
	let nextPtyIdsByTabId = state.ptyIdsByTabId;
	for (const removedId of removedTerminalResourceIds) if (nextPtyIdsByTabId[removedId]) {
		nextPtyIdsByTabId = nextPtyIdsByTabId === state.ptyIdsByTabId ? writableWebSessionTabsRecord(state, "ptyIdsByTabId", batchContext) : nextPtyIdsByTabId;
		delete nextPtyIdsByTabId[removedId];
	}
	for (const { tab, ptyIds } of mirroredTerminalTabs) {
		if (ptyIds.length === 0) {
			if (nextPtyIdsByTabId[tab.id]) {
				nextPtyIdsByTabId = nextPtyIdsByTabId === state.ptyIdsByTabId ? writableWebSessionTabsRecord(state, "ptyIdsByTabId", batchContext) : nextPtyIdsByTabId;
				delete nextPtyIdsByTabId[tab.id];
			}
			continue;
		}
		if (!sameStringArray(nextPtyIdsByTabId[tab.id] ?? [], ptyIds)) {
			nextPtyIdsByTabId = nextPtyIdsByTabId === state.ptyIdsByTabId ? writableWebSessionTabsRecord(state, "ptyIdsByTabId", batchContext) : nextPtyIdsByTabId;
			nextPtyIdsByTabId[tab.id] = ptyIds;
		}
	}
	let nextTerminalLayoutsByTabId = state.terminalLayoutsByTabId;
	for (const removedId of removedTerminalResourceIds) if (nextTerminalLayoutsByTabId[removedId]) {
		nextTerminalLayoutsByTabId = nextTerminalLayoutsByTabId === state.terminalLayoutsByTabId ? writableWebSessionTabsRecord(state, "terminalLayoutsByTabId", batchContext) : nextTerminalLayoutsByTabId;
		delete nextTerminalLayoutsByTabId[removedId];
	}
	for (const { tab, layout } of mirroredTerminalTabs) if (!terminalLayoutEqual(nextTerminalLayoutsByTabId[tab.id], layout)) {
		nextTerminalLayoutsByTabId = nextTerminalLayoutsByTabId === state.terminalLayoutsByTabId ? writableWebSessionTabsRecord(state, "terminalLayoutsByTabId", batchContext) : nextTerminalLayoutsByTabId;
		nextTerminalLayoutsByTabId[tab.id] = layout;
	}
	let nextUnreadTerminalTabs = state.unreadTerminalTabs;
	for (const removedId of removedTerminalIds) if (nextUnreadTerminalTabs[removedId]) {
		nextUnreadTerminalTabs = nextUnreadTerminalTabs === state.unreadTerminalTabs ? writableWebSessionTabsRecord(state, "unreadTerminalTabs", batchContext) : nextUnreadTerminalTabs;
		delete nextUnreadTerminalTabs[removedId];
	}
	const pendingStartupByTabId = state.pendingStartupByTabId ?? {};
	let nextPendingStartupByTabId = pendingStartupByTabId;
	const automaticAgentResumeClaimsByTabId = state.automaticAgentResumeClaimsByTabId ?? {};
	let nextAutomaticAgentResumeClaimsByTabId = automaticAgentResumeClaimsByTabId;
	for (const removedId of exactProvisionalHandoffs) {
		if (nextPendingStartupByTabId[removedId]) {
			nextPendingStartupByTabId = nextPendingStartupByTabId === pendingStartupByTabId ? writableWebSessionTabsRecord(state, "pendingStartupByTabId", batchContext) : nextPendingStartupByTabId;
			delete nextPendingStartupByTabId[removedId];
		}
		if (nextAutomaticAgentResumeClaimsByTabId[removedId]) {
			nextAutomaticAgentResumeClaimsByTabId = nextAutomaticAgentResumeClaimsByTabId === automaticAgentResumeClaimsByTabId ? writableWebSessionTabsRecord(state, "automaticAgentResumeClaimsByTabId", batchContext) : nextAutomaticAgentResumeClaimsByTabId;
			delete nextAutomaticAgentResumeClaimsByTabId[removedId];
		}
	}
	let nextBrowserPagesByWorkspace = state.browserPagesByWorkspace;
	let nextRemoteBrowserPageHandlesByPageId = state.remoteBrowserPageHandlesByPageId;
	let nextBrowserCertificateFailuresByPageId = state.browserCertificateFailuresByPageId;
	if (removedBrowserWorkspaceIds.size > 0) {
		const nextBrowserWorkspaceIds = new Set(nextBrowserTabs?.map((tab) => tab.id) ?? []);
		const nextBrowserPageIds = new Set(mirroredBrowserTabs.map((entry) => entry.page.id));
		for (const workspace of retainedBrowserTabs) for (const page of state.browserPagesByWorkspace[workspace.id] ?? []) nextBrowserPageIds.add(page.id);
		for (const removedWorkspaceId of removedBrowserWorkspaceIds) {
			const pages = nextBrowserPagesByWorkspace[removedWorkspaceId] ?? [];
			if (!nextBrowserWorkspaceIds.has(removedWorkspaceId) && nextBrowserPagesByWorkspace[removedWorkspaceId]) {
				nextBrowserPagesByWorkspace = nextBrowserPagesByWorkspace === state.browserPagesByWorkspace ? writableWebSessionTabsRecord(state, "browserPagesByWorkspace", batchContext) : nextBrowserPagesByWorkspace;
				delete nextBrowserPagesByWorkspace[removedWorkspaceId];
			}
			for (const page of pages) {
				if (nextBrowserPageIds.has(page.id)) continue;
				if (nextBrowserCertificateFailuresByPageId[page.id]) {
					nextBrowserCertificateFailuresByPageId = nextBrowserCertificateFailuresByPageId === state.browserCertificateFailuresByPageId ? writableWebSessionTabsRecord(state, "browserCertificateFailuresByPageId", batchContext) : nextBrowserCertificateFailuresByPageId;
					delete nextBrowserCertificateFailuresByPageId[page.id];
				}
				if (nextRemoteBrowserPageHandlesByPageId[page.id]) {
					nextRemoteBrowserPageHandlesByPageId = nextRemoteBrowserPageHandlesByPageId === state.remoteBrowserPageHandlesByPageId ? writableWebSessionTabsRecord(state, "remoteBrowserPageHandlesByPageId", batchContext) : nextRemoteBrowserPageHandlesByPageId;
					delete nextRemoteBrowserPageHandlesByPageId[page.id];
				}
			}
		}
	}
	for (const { page, certificateFailure, remotePageId } of mirroredBrowserTabs) {
		if (!sameBrowserPages(nextBrowserPagesByWorkspace[page.workspaceId] ?? [], [page])) {
			nextBrowserPagesByWorkspace = nextBrowserPagesByWorkspace === state.browserPagesByWorkspace ? writableWebSessionTabsRecord(state, "browserPagesByWorkspace", batchContext) : nextBrowserPagesByWorkspace;
			nextBrowserPagesByWorkspace[page.workspaceId] = [page];
		}
		const currentHandle = nextRemoteBrowserPageHandlesByPageId[page.id];
		if (currentHandle?.environmentId !== environmentId || currentHandle.remotePageId !== remotePageId) {
			nextRemoteBrowserPageHandlesByPageId = nextRemoteBrowserPageHandlesByPageId === state.remoteBrowserPageHandlesByPageId ? writableWebSessionTabsRecord(state, "remoteBrowserPageHandlesByPageId", batchContext) : nextRemoteBrowserPageHandlesByPageId;
			nextRemoteBrowserPageHandlesByPageId[page.id] = {
				environmentId,
				remotePageId
			};
		}
		if (!browserCertificateFailureEqual(nextBrowserCertificateFailuresByPageId[page.id], certificateFailure)) {
			nextBrowserCertificateFailuresByPageId = nextBrowserCertificateFailuresByPageId === state.browserCertificateFailuresByPageId ? writableWebSessionTabsRecord(state, "browserCertificateFailuresByPageId", batchContext) : nextBrowserCertificateFailuresByPageId;
			if (certificateFailure) nextBrowserCertificateFailuresByPageId[page.id] = certificateFailure;
			else delete nextBrowserCertificateFailuresByPageId[page.id];
		}
	}
	const nextTabsByWorktree = withWorktreeEntry(state, "tabsByWorktree", worktreeId, nextTerminalTabs, sameTerminalTabs, batchContext);
	const nextBrowserTabsByWorktree = withWorktreeEntry(state, "browserTabsByWorktree", worktreeId, nextBrowserTabs, sameBrowserTabs, batchContext);
	const nextUnifiedTabsByWorktree = withWorktreeEntry(state, "unifiedTabsByWorktree", worktreeId, nextUnifiedTabs, sameUnifiedTabs, batchContext);
	const nextGroupsByWorktree = withWorktreeEntry(state, "groupsByWorktree", worktreeId, nextGroups, sameGroups, batchContext);
	const nextActiveGroupId = nextGroups?.find((group) => group.activeTabId === nextActiveUnifiedTabId)?.id ?? nextGroups?.find((group) => group.id === snapshot.activeGroupId)?.id ?? nextGroups?.[0]?.id ?? null;
	const nextActiveGroupIdByWorktree = nextGroups && state.activeGroupIdByWorktree[worktreeId] !== nextActiveGroupId ? withWorktreeEntry(state, "activeGroupIdByWorktree", worktreeId, nextActiveGroupId ?? targetGroupId, (current, next) => current === next, batchContext) : state.activeGroupIdByWorktree;
	const nextLayoutByWorktree = (() => {
		if (!nextGroups) return state.layoutByWorktree;
		const validGroupIds = new Set(nextGroups.map((group) => group.id));
		const hostLayout = pruneTabGroupLayout(snapshot.tabGroupLayout, validGroupIds);
		const defaultLeafLayout = {
			type: "leaf",
			groupId: nextActiveGroupId ?? targetGroupId
		};
		const hostLayoutGroupIds = collectLayoutGroupIds(hostLayout ?? void 0);
		const hostGroupIds = new Set(snapshot.tabGroups?.map((group) => group.id) ?? []);
		const extraGroupIds = new Set(nextGroups.map((group) => group.id).filter((groupId) => hostLayout ? !hostLayoutGroupIds.has(groupId) : snapshot.tabGroups && snapshot.tabGroups.length > 0 ? !hostGroupIds.has(groupId) : false));
		const localExtraLayout = pruneTabGroupLayout(state.layoutByWorktree[worktreeId], extraGroupIds);
		const fallbackLayout = appendTabGroupLayout(hostLayout ?? (snapshot.tabGroups && snapshot.tabGroups.length > 0 ? defaultLeafLayout : null), localExtraLayout) ?? (snapshot.tabGroups && snapshot.tabGroups.length > 0 ? defaultLeafLayout : state.layoutByWorktree[worktreeId] ? null : defaultLeafLayout);
		if (!fallbackLayout) return state.layoutByWorktree;
		if (tabGroupLayoutEqual(state.layoutByWorktree[worktreeId], fallbackLayout)) return state.layoutByWorktree;
		return withWorktreeEntry(state, "layoutByWorktree", worktreeId, fallbackLayout, (current, next) => current === next, batchContext);
	})();
	const nextTabBarOrderByWorktree = withWorktreeEntry(state, "tabBarOrderByWorktree", worktreeId, nextTabBarOrder.length > 0 ? nextTabBarOrder : null, (a, b) => sameStringArray(a ?? [], b ?? []), batchContext);
	const nextActiveTabIdByWorktree = (state.activeTabIdByWorktree[worktreeId] ?? null) !== nextActiveTerminalId ? withWorktreeEntry(state, "activeTabIdByWorktree", worktreeId, nextActiveTerminalId, (current, next) => (current ?? null) === next, batchContext, false) : state.activeTabIdByWorktree;
	const nextActiveBrowserTabIdByWorktree = (state.activeBrowserTabIdByWorktree[worktreeId] ?? null) !== nextActiveBrowserWorkspaceId ? withWorktreeEntry(state, "activeBrowserTabIdByWorktree", worktreeId, nextActiveBrowserWorkspaceId, (current, next) => (current ?? null) === next, batchContext, false) : state.activeBrowserTabIdByWorktree;
	const nextActiveFileIdByWorktree = (state.activeFileIdByWorktree[worktreeId] ?? null) !== nextActiveEditorFileId ? withWorktreeEntry(state, "activeFileIdByWorktree", worktreeId, nextActiveEditorFileId, (current, next) => (current ?? null) === next, batchContext, false) : state.activeFileIdByWorktree;
	const isActiveWorktree = state.activeWorktreeId === worktreeId;
	const focusIntentVisibleTabType = navigationIntentTab?.type === "browser" && intentBrowserWorkspaceId ? "browser" : navigationIntentTab?.type === "terminal" && intentTerminalId ? "terminal" : intentEditorFileId ? "editor" : null;
	const snapshotVisibleTabType = snapshot.activeTabType === "browser" && nextActiveBrowserWorkspaceId ? "browser" : snapshot.activeTabType === "terminal" && nextActiveTerminalId ? "terminal" : (snapshot.activeTabType === "markdown" || snapshot.activeTabType === "file") && nextActiveEditorFileId ? "editor" : null;
	const currentVisibleTabType = state.activeTabTypeByWorktree[worktreeId] ?? (isActiveWorktree ? state.activeTabType : null);
	const currentVisibleTabTypeStillValid = currentVisibleTabType === "browser" && currentActiveBrowserStillExists ? "browser" : currentVisibleTabType === "editor" && currentActiveEditorStillExists ? "editor" : currentVisibleTabType === "terminal" && currentActiveTerminalStillExists ? "terminal" : null;
	const activeUnifiedTab = nextActiveUnifiedTabId && nextUnifiedTabs ? nextUnifiedTabs.find((tab) => tab.id === nextActiveUnifiedTabId) ?? null : null;
	const fallbackVisibleTabType = activeUnifiedTab !== null ? toVisibleTabType(activeUnifiedTab) : nextActiveTerminalId ? "terminal" : nextActiveBrowserWorkspaceId ? "browser" : nextActiveEditorFileId ? "editor" : "terminal";
	const nextVisibleTabType = honorSnapshotActiveFocus ? focusIntentVisibleTabType ?? currentVisibleTabTypeStillValid ?? snapshotVisibleTabType ?? fallbackVisibleTabType : currentVisibleTabTypeStillValid ?? snapshotVisibleTabType ?? fallbackVisibleTabType;
	const currentActiveTerminalStillValid = state.activeTabId && (nextTerminalTabs ?? []).some((tab) => tab.id === state.activeTabId) ? state.activeTabId : null;
	const currentActiveEditorStillValid = state.activeFileId && nextWorktreeOpenFileIds.has(state.activeFileId) ? state.activeFileId : null;
	const nextActiveTabId = isActiveWorktree ? snapshot.activeTabType === "terminal" ? nextActiveTerminalId : currentActiveTerminalStillValid ?? nextActiveTerminalId : state.activeTabId;
	const nextActiveBrowserTabId = isActiveWorktree ? nextActiveBrowserWorkspaceId : state.activeBrowserTabId;
	const nextActiveFileId = isActiveWorktree ? snapshot.activeTabType === "markdown" || snapshot.activeTabType === "file" ? nextActiveEditorFileId : currentActiveEditorStillValid ?? nextActiveEditorFileId : state.activeFileId;
	const nextActiveTabType = isActiveWorktree ? nextVisibleTabType : state.activeTabType;
	const nextActiveTabTypeByWorktree = state.activeTabTypeByWorktree[worktreeId] !== nextVisibleTabType ? withWorktreeEntry(state, "activeTabTypeByWorktree", worktreeId, nextVisibleTabType, (current, next) => current === next, batchContext) : state.activeTabTypeByWorktree;
	const patch = {
		...buildMirroredAgentStatusPatch(state, currentTerminalTabs, terminalSurfaceTabs, mirroredTerminalTabs, now, batchContext),
		...nextOpenFiles !== state.openFiles ? { openFiles: nextOpenFiles } : {},
		...nextTabsByWorktree !== state.tabsByWorktree ? { tabsByWorktree: nextTabsByWorktree } : {},
		...nextBrowserTabsByWorktree !== state.browserTabsByWorktree ? { browserTabsByWorktree: nextBrowserTabsByWorktree } : {},
		...nextUnifiedTabsByWorktree !== state.unifiedTabsByWorktree ? { unifiedTabsByWorktree: nextUnifiedTabsByWorktree } : {},
		...nextGroupsByWorktree !== state.groupsByWorktree ? { groupsByWorktree: nextGroupsByWorktree } : {},
		...nextActiveGroupIdByWorktree !== state.activeGroupIdByWorktree ? { activeGroupIdByWorktree: nextActiveGroupIdByWorktree } : {},
		...nextLayoutByWorktree !== state.layoutByWorktree ? { layoutByWorktree: nextLayoutByWorktree } : {},
		...nextTabBarOrderByWorktree !== state.tabBarOrderByWorktree ? { tabBarOrderByWorktree: nextTabBarOrderByWorktree } : {},
		...nextPtyIdsByTabId !== state.ptyIdsByTabId ? { ptyIdsByTabId: nextPtyIdsByTabId } : {},
		...nextTerminalLayoutsByTabId !== state.terminalLayoutsByTabId ? { terminalLayoutsByTabId: nextTerminalLayoutsByTabId } : {},
		...nextUnreadTerminalTabs !== state.unreadTerminalTabs ? { unreadTerminalTabs: nextUnreadTerminalTabs } : {},
		...nextPendingStartupByTabId !== pendingStartupByTabId ? { pendingStartupByTabId: nextPendingStartupByTabId } : {},
		...nextAutomaticAgentResumeClaimsByTabId !== automaticAgentResumeClaimsByTabId ? { automaticAgentResumeClaimsByTabId: nextAutomaticAgentResumeClaimsByTabId } : {},
		...nextBrowserPagesByWorkspace !== state.browserPagesByWorkspace ? { browserPagesByWorkspace: nextBrowserPagesByWorkspace } : {},
		...nextRemoteBrowserPageHandlesByPageId !== state.remoteBrowserPageHandlesByPageId ? { remoteBrowserPageHandlesByPageId: nextRemoteBrowserPageHandlesByPageId } : {},
		...nextBrowserCertificateFailuresByPageId !== state.browserCertificateFailuresByPageId ? { browserCertificateFailuresByPageId: nextBrowserCertificateFailuresByPageId } : {},
		...nextActiveTabIdByWorktree !== state.activeTabIdByWorktree ? { activeTabIdByWorktree: nextActiveTabIdByWorktree } : {},
		...nextActiveBrowserTabIdByWorktree !== state.activeBrowserTabIdByWorktree ? { activeBrowserTabIdByWorktree: nextActiveBrowserTabIdByWorktree } : {},
		...nextActiveFileIdByWorktree !== state.activeFileIdByWorktree ? { activeFileIdByWorktree: nextActiveFileIdByWorktree } : {},
		...nextActiveTabId !== state.activeTabId ? { activeTabId: nextActiveTabId } : {},
		...nextActiveBrowserTabId !== state.activeBrowserTabId ? { activeBrowserTabId: nextActiveBrowserTabId } : {},
		...nextActiveFileId !== state.activeFileId ? { activeFileId: nextActiveFileId } : {},
		...nextActiveTabType !== state.activeTabType ? { activeTabType: nextActiveTabType } : {},
		...nextActiveTabTypeByWorktree !== state.activeTabTypeByWorktree ? { activeTabTypeByWorktree: nextActiveTabTypeByWorktree } : {}
	};
	return Object.keys(patch).length === 0 ? state : patch;
}
function applyWebSessionTabsSnapshot(state, rawSnapshot, environmentId, now = Date.now()) {
	return applyWebSessionTabsSnapshotWithContext(state, rawSnapshot, environmentId, now);
}
function applyWebSessionTabsSnapshots(state, snapshots, environmentId, now = Date.now()) {
	const nextState = { ...state };
	const batchContext = {
		agentPaneKeysByTabId: null,
		changedRecords: /* @__PURE__ */ new Set(),
		openFilesIndex: null
	};
	let mergedPatch = {};
	for (const snapshot of snapshots) {
		const patch = applyWebSessionTabsSnapshotWithContext(nextState, snapshot, environmentId, now, batchContext);
		if (patch === nextState) continue;
		mergedPatch = {
			...mergedPatch,
			...patch
		};
		Object.assign(nextState, patch);
	}
	const mutableMergedPatch = mergedPatch;
	const mutableNextState = nextState;
	for (const recordKey of batchContext.changedRecords) mutableMergedPatch[recordKey] = mutableNextState[recordKey];
	return Object.keys(mergedPatch).length === 0 ? state : mergedPatch;
}
function applyFreshWebSessionTabsSnapshot(state, snapshot, environmentId, now = Date.now()) {
	if (!shouldApplyWebSessionTabsSnapshot(snapshot, environmentId)) return state;
	return applyWebSessionTabsSnapshot(state, snapshot, environmentId, now);
}
function applyFreshWebSessionTabsSnapshots(state, snapshots, environmentId, now = Date.now()) {
	const freshSnapshots = snapshots.filter((snapshot) => shouldApplyWebSessionTabsSnapshot(snapshot, environmentId));
	return freshSnapshots.length === 0 ? state : applyWebSessionTabsSnapshots(state, freshSnapshots, environmentId, now);
}
function applyWebSessionTabsSnapshotOperations(state, operations) {
	let nextState = state;
	let mergedPatch = {};
	for (const { environmentId, snapshot } of operations) {
		if (!shouldApplyWebSessionTabsSnapshot(snapshot, environmentId)) continue;
		const patch = applyWebSessionTabsSnapshot(nextState, snapshot, environmentId);
		if (patch === nextState) continue;
		mergedPatch = {
			...mergedPatch,
			...patch
		};
		nextState = {
			...nextState,
			...patch
		};
	}
	return Object.keys(mergedPatch).length === 0 ? state : mergedPatch;
}
function applyWebSessionTabsStorePatch(buildPatch) {
	let mirroredAgentStatusChanged = false;
	useAppStore.setState((state) => {
		const patch = buildPatch(state);
		mirroredAgentStatusChanged = patch !== state && Object.hasOwn(patch, "agentStatusByPaneKey");
		return patch;
	});
	if (mirroredAgentStatusChanged) useAppStore.getState().scheduleAgentStatusFreshness();
}
function loadInitialWebSessionTabs(environmentId, expectedEnvironmentPairingRevision, isCurrent) {
	window.api.runtimeEnvironments.call({
		selector: environmentId,
		method: "session.tabs.listAll",
		params: {},
		timeoutMs: 15e3,
		expectedEnvironmentPairingRevision
	}).then(async (response) => {
		if (!isCurrent() || getRuntimeEnvironmentRevision(environmentId) !== expectedEnvironmentPairingRevision) return;
		if (response.ok === false) {
			console.warn("[web-session-tabs-sync] initial listAll failed:", response.error.message);
			return;
		}
		const result = response.result;
		if (!isSessionTabsListAllResult(result)) {
			console.warn("[web-session-tabs-sync] initial listAll returned an invalid payload");
			return;
		}
		const receivedFrames = result.snapshots.map((snapshot) => recordReceivedWebSessionTabsSnapshot(environmentId, snapshot));
		const finishRecoveries = result.snapshots.map((snapshot, index) => beginWebSessionTabsSnapshotRecovery(environmentId, snapshot.worktree, receivedFrames[index]));
		try {
			const recovered = await Promise.all(result.snapshots.map((snapshot) => recoverWebSessionTerminalOrphansBeforeApply(useAppStore.getState(), snapshot, environmentId)));
			if (!isCurrent() || getRuntimeEnvironmentRevision(environmentId) !== expectedEnvironmentPairingRevision) return;
			const applicable = recovered.filter((snapshot, index) => snapshot !== null && shouldApplyRecoveredWebSessionTabsSnapshot(environmentId, snapshot, receivedFrames[index]));
			applyWebSessionTabsStorePatch((state) => applyFreshWebSessionTabsSnapshots(state, applicable, environmentId));
		} finally {
			for (const finishRecovery of finishRecoveries) finishRecovery();
		}
	}).catch((error) => {
		if (isCurrent()) console.warn("[web-session-tabs-sync] failed to load initial session tabs:", error instanceof Error ? error.message : String(error));
	});
}
function useWebSessionTabsSync() {
	const recordVisibilityResumeSnapshotRef = (0, import_react.useRef)(() => {});
	const recordVisibilityResumeSnapshotReceiptRef = (0, import_react.useRef)(() => {});
	const shouldApplyVisibilityResumeSnapshotRef = (0, import_react.useRef)(() => true);
	const visibilityResumeOmissionsByKeyRef = (0, import_react.useRef)(/* @__PURE__ */ new Map());
	const mirroredSessionTabsOwnerRevisionByEnvironmentRef = (0, import_react.useRef)(/* @__PURE__ */ new Map());
	const activeRuntimeEnvironmentIdRef = (0, import_react.useRef)(null);
	const activeRuntimeWorktreeKeyRef = (0, import_react.useRef)(null);
	const activeWorktreeId = useAppStore((state) => state.activeWorktreeId);
	const runtimeSessionMirrorEnvironmentKey = useRuntimeSessionMirrorEnvironmentKey();
	const activeWorktreeRuntimeEnvironmentId = useAppStore((state) => getExplicitRuntimeEnvironmentIdForWorktree(state, state.activeWorktreeId));
	const activeWorktreeRuntimeId = useAppStore((state) => {
		const environmentId = getExplicitRuntimeEnvironmentIdForWorktree(state, state.activeWorktreeId);
		return environmentId ? state.runtimeStatusByEnvironmentId.get(environmentId)?.status?.runtimeId ?? null : null;
	});
	const activeWorktreeRuntimeConnectionGeneration = useAppStore((state) => {
		const environmentId = getExplicitRuntimeEnvironmentIdForWorktree(state, state.activeWorktreeId);
		return environmentId ? state.runtimeStatusByEnvironmentId.get(environmentId)?.connectionGeneration ?? 0 : 0;
	});
	const activeWorktreeRuntimePairingRevision = useAppStore((state) => {
		const environmentId = getExplicitRuntimeEnvironmentIdForWorktree(state, state.activeWorktreeId);
		const environment = state.runtimeEnvironments.find((candidate) => candidate.id === environmentId);
		return environment ? environment.pairingRevision ?? environment.createdAt : void 0;
	});
	const workspaceSessionReady = useAppStore((state) => state.workspaceSessionReady);
	(0, import_react.useLayoutEffect)(() => {
		activeRuntimeEnvironmentIdRef.current = activeWorktreeRuntimeEnvironmentId?.trim() || null;
		activeRuntimeWorktreeKeyRef.current = activeWorktreeRuntimeEnvironmentId && activeWorktreeId ? sessionTabsFreshnessKey(activeWorktreeRuntimeEnvironmentId, activeWorktreeId) : null;
	}, [activeWorktreeId, activeWorktreeRuntimeEnvironmentId]);
	(0, import_react.useEffect)(() => () => {
		for (const environmentId of mirroredSessionTabsOwnerRevisionByEnvironmentRef.current.keys()) clearWebSessionTabsTrackingForEnvironment(environmentId);
		mirroredSessionTabsOwnerRevisionByEnvironmentRef.current.clear();
		visibilityResumeOmissionsByKeyRef.current.clear();
	}, []);
	(0, import_react.useEffect)(() => {
		const environments = runtimeSessionMirrorEnvironmentKey ? runtimeSessionMirrorEnvironmentKey.split("\0").map((entry) => {
			const [environmentId = "", , , rawRevision = ""] = entry.split("");
			return {
				environmentId,
				expectedEnvironmentPairingRevision: rawRevision === "" ? void 0 : Number(rawRevision)
			};
		}).filter(({ environmentId }) => environmentId.trim()) : [];
		const mirroredEnvironmentOwnerRevisions = new Map((workspaceSessionReady ? environments : []).map(({ environmentId, expectedEnvironmentPairingRevision }) => [environmentId, expectedEnvironmentPairingRevision]));
		const previousOwnerRevisions = mirroredSessionTabsOwnerRevisionByEnvironmentRef.current;
		for (const [environmentId, previousRevision] of previousOwnerRevisions) if (!mirroredEnvironmentOwnerRevisions.has(environmentId) || mirroredEnvironmentOwnerRevisions.get(environmentId) !== previousRevision) clearWebSessionTabsTrackingForEnvironment(environmentId);
		mirroredSessionTabsOwnerRevisionByEnvironmentRef.current = mirroredEnvironmentOwnerRevisions;
		for (const [key, omission] of visibilityResumeOmissionsByKeyRef.current) {
			const previousRevision = previousOwnerRevisions.get(omission.environmentId);
			if (!mirroredEnvironmentOwnerRevisions.has(omission.environmentId) || previousOwnerRevisions.has(omission.environmentId) && mirroredEnvironmentOwnerRevisions.get(omission.environmentId) !== previousRevision) visibilityResumeOmissionsByKeyRef.current.delete(key);
		}
		if (!workspaceSessionReady || environments.length === 0) return;
		const subscriptionSpecs = [];
		const environmentIdBySubscriptionSpec = [];
		let visibilityResumeBatch = null;
		const visibilityResumeOmissionsByKey = visibilityResumeOmissionsByKeyRef.current;
		const recordVisibilityResumeSnapshotReceipt = (environmentId, snapshot, receivedFrame) => {
			const omission = visibilityResumeOmissionsByKey.get(sessionTabsFreshnessKey(environmentId, snapshot.worktree));
			if (omission && receivedFrame > omission.inventoryReceivedFrame && (snapshot.removed === true || advancesSessionTabsFreshness(snapshot, omission.baseline))) {
				omission.superseded = true;
				if (visibilityResumeBatch?.pendingMissingByWorktree.has(snapshot.worktree)) reconcileVisibilityResumeWorktrees([snapshot.worktree]);
			}
		};
		const shouldApplyVisibilityResumeSnapshot = (environmentId, snapshot, receivedFrame) => {
			const omission = visibilityResumeOmissionsByKey.get(sessionTabsFreshnessKey(environmentId, snapshot.worktree));
			if (!omission) return true;
			if (receivedFrame < omission.inventoryReceivedFrame) return false;
			return snapshot.removed === true || advancesSessionTabsFreshness(snapshot, omission.baseline);
		};
		recordVisibilityResumeSnapshotReceiptRef.current = recordVisibilityResumeSnapshotReceipt;
		shouldApplyVisibilityResumeSnapshotRef.current = shouldApplyVisibilityResumeSnapshot;
		const isVisibilityResumeMissingCurrent = (missing) => {
			const omission = visibilityResumeOmissionsByKey.get(sessionTabsFreshnessKey(missing.environmentId, missing.snapshot.worktree));
			return omission?.inventoryReceivedFrame === missing.inventoryReceivedFrame && !omission.superseded;
		};
		const getVisibilityResumeSnapshot = (batch, environmentId, worktreeId) => {
			const key = sessionTabsFreshnessKey(environmentId, worktreeId);
			const entry = batch.reapplyableSnapshotsByKey.get(key);
			const freshness = latestSessionTabsSnapshotByWorktree.get(key);
			if (!entry || freshness?.publicationEpoch !== entry.snapshot.publicationEpoch || freshness.snapshotVersion !== entry.snapshot.snapshotVersion || !shouldApplyRecoveredWebSessionTabsSnapshot(environmentId, entry.snapshot, entry.receivedFrame)) return null;
			return entry.snapshot;
		};
		const finishVisibilityResumeBatchIfIdle = (batch) => {
			if (batch.pendingInventoryCount === 0 && batch.pendingMissingByWorktree.size === 0 && visibilityResumeBatch === batch) visibilityResumeBatch = null;
		};
		function reconcileVisibilityResumeWorktrees(worktreeIds) {
			const batch = visibilityResumeBatch;
			if (!batch) return;
			const operations = [];
			for (const worktreeId of new Set(worktreeIds)) {
				const pendingMissing = batch.pendingMissingByWorktree.get(worktreeId);
				if (!pendingMissing) {
					batch.deferredRepairWorktrees.delete(worktreeId);
					continue;
				}
				for (const [environmentId, missing] of pendingMissing) {
					if (isVisibilityResumeMissingCurrent(missing)) continue;
					pendingMissing.delete(environmentId);
					batch.environments.get(environmentId)?.pendingMissingWorktrees.delete(worktreeId);
				}
				if (pendingMissing.size === 0) {
					batch.pendingMissingByWorktree.delete(worktreeId);
					batch.deferredRepairWorktrees.delete(worktreeId);
					continue;
				}
				const missingEnvironmentIds = new Set(pendingMissing.keys());
				const survivingSnapshots = [];
				let canRepairSharedState = true;
				for (const environmentId of sessionTabsEnvironmentsByWorktree.get(worktreeId) ?? []) {
					if (missingEnvironmentIds.has(environmentId)) continue;
					const snapshot = getVisibilityResumeSnapshot(batch, environmentId, worktreeId);
					if (!snapshot) {
						canRepairSharedState = false;
						break;
					}
					survivingSnapshots.push({
						environmentId,
						snapshot
					});
				}
				if (!canRepairSharedState) {
					batch.deferredRepairWorktrees.add(worktreeId);
					continue;
				}
				for (const missing of pendingMissing.values()) operations.push({
					environmentId: missing.environmentId,
					snapshot: missing.snapshot
				});
				for (const { environmentId, snapshot } of survivingSnapshots) {
					acceptReplayedWebSessionTabsSnapshot(environmentId, worktreeId);
					operations.push({
						environmentId,
						snapshot
					});
				}
				for (const environmentId of pendingMissing.keys()) batch.environments.get(environmentId)?.pendingMissingWorktrees.delete(worktreeId);
				batch.pendingMissingByWorktree.delete(worktreeId);
				batch.deferredRepairWorktrees.delete(worktreeId);
			}
			if (operations.length > 0) applyWebSessionTabsStorePatch((state) => applyWebSessionTabsSnapshotOperations(state, operations));
			finishVisibilityResumeBatchIfIdle(batch);
		}
		const recordVisibilityResumeSnapshot = (environmentId, snapshot, receivedFrame) => {
			const batch = visibilityResumeBatch;
			if (!batch || !batch.trackedWorktreeIds.has(snapshot.worktree)) return;
			const key = sessionTabsFreshnessKey(environmentId, snapshot.worktree);
			const existingIsCurrent = getVisibilityResumeSnapshot(batch, environmentId, snapshot.worktree);
			const freshness = latestSessionTabsSnapshotByWorktree.get(key);
			const repairsCrossHostCollision = (sessionTabsEnvironmentsByWorktree.get(snapshot.worktree)?.size ?? 0) > 1 || batch.deferredRepairWorktrees.has(snapshot.worktree);
			if (snapshot.removed === true || snapshot.tabs.length === 0 || !repairsCrossHostCollision || freshness?.publicationEpoch !== snapshot.publicationEpoch || freshness.snapshotVersion !== snapshot.snapshotVersion || !shouldApplyRecoveredWebSessionTabsSnapshot(environmentId, snapshot, receivedFrame)) {
				if (!existingIsCurrent) batch.reapplyableSnapshotsByKey.delete(key);
			} else batch.reapplyableSnapshotsByKey.set(key, {
				snapshot,
				receivedFrame
			});
			if (batch.pendingMissingByWorktree.has(snapshot.worktree)) reconcileVisibilityResumeWorktrees([snapshot.worktree]);
		};
		const recordVisibilityResumeInventory = (environmentId, visibilityGeneration, inventoryReceivedFrame, missingWorktrees) => {
			if (visibilityGeneration === 0 || visibilityResumeBatch?.visibilityGeneration !== visibilityGeneration) return;
			const environment = visibilityResumeBatch.environments.get(environmentId);
			if (!environment || environment.latestInventoryReceivedFrame !== inventoryReceivedFrame) return;
			const batch = visibilityResumeBatch;
			const affectedWorktrees = new Set(environment.pendingMissingWorktrees);
			for (const worktreeId of environment.pendingMissingWorktrees) {
				const pendingMissing = batch.pendingMissingByWorktree.get(worktreeId);
				pendingMissing?.delete(environmentId);
				if (pendingMissing?.size === 0) batch.pendingMissingByWorktree.delete(worktreeId);
			}
			environment.pendingMissingWorktrees.clear();
			for (const missing of missingWorktrees) {
				const worktreeId = missing.snapshot.worktree;
				const pendingMissing = batch.pendingMissingByWorktree.get(worktreeId) ?? /* @__PURE__ */ new Map();
				pendingMissing.set(environmentId, missing);
				batch.pendingMissingByWorktree.set(worktreeId, pendingMissing);
				environment.pendingMissingWorktrees.add(worktreeId);
				affectedWorktrees.add(worktreeId);
			}
			if (!environment.inventoryReceived) {
				environment.inventoryReceived = true;
				batch.pendingInventoryCount -= 1;
			}
			reconcileVisibilityResumeWorktrees(affectedWorktrees);
		};
		recordVisibilityResumeSnapshotRef.current = recordVisibilityResumeSnapshot;
		const recordVisibilityResumeInventoryReceipt = (environmentId, visibilityGeneration, inventoryReceivedFrame, snapshots) => {
			for (const snapshot of snapshots) visibilityResumeOmissionsByKey.delete(sessionTabsFreshnessKey(environmentId, snapshot.worktree));
			if (visibilityResumeBatch?.visibilityGeneration !== visibilityGeneration) return [];
			const environment = visibilityResumeBatch.environments.get(environmentId);
			if (!environment) return [];
			environment.latestInventoryReceivedFrame = Math.max(environment.latestInventoryReceivedFrame, inventoryReceivedFrame);
			if (environment.latestInventoryReceivedFrame !== inventoryReceivedFrame) return [];
			const publishedWorktrees = new Set(snapshots.map((snapshot) => snapshot.worktree));
			return buildMissingWebSessionTabsRemovals(environmentId, environment.trackedWorktrees, publishedWorktrees).map((missing) => {
				const key = sessionTabsFreshnessKey(environmentId, missing.snapshot.worktree);
				visibilityResumeOmissionsByKey.set(key, {
					baseline: missing.trackedWorktree.freshness,
					environmentId,
					inventoryReceivedFrame,
					superseded: false,
					visibilityGeneration
				});
				recordReceivedWebSessionTabsRemoval(environmentId, missing.snapshot.worktree, inventoryReceivedFrame);
				return {
					environmentId,
					inventoryReceivedFrame,
					...missing
				};
			});
		};
		const beginVisibilityResume = ({ visibilityGeneration, restartingSpecIndexes }) => {
			const activeRuntimeWorktreeKey = activeRuntimeWorktreeKeyRef.current;
			for (const [key, omission] of visibilityResumeOmissionsByKey) if (key !== activeRuntimeWorktreeKey || omission.visibilityGeneration < visibilityGeneration - 1) visibilityResumeOmissionsByKey.delete(key);
			const resumedEnvironments = /* @__PURE__ */ new Map();
			const trackedWorktreeIds = /* @__PURE__ */ new Set();
			for (const index of restartingSpecIndexes) {
				const environmentId = environmentIdBySubscriptionSpec[index];
				if (environmentId) {
					const trackedWorktrees = getTrackedWebSessionTabsWorktrees(environmentId);
					if (trackedWorktrees.length === 0) continue;
					for (const { worktree } of trackedWorktrees) trackedWorktreeIds.add(worktree);
					resumedEnvironments.set(environmentId, {
						trackedWorktrees,
						inventoryReceived: false,
						latestInventoryReceivedFrame: 0,
						pendingMissingWorktrees: /* @__PURE__ */ new Set()
					});
				}
			}
			visibilityResumeBatch = resumedEnvironments.size > 0 ? {
				visibilityGeneration,
				environments: resumedEnvironments,
				pendingInventoryCount: resumedEnvironments.size,
				pendingMissingByWorktree: /* @__PURE__ */ new Map(),
				deferredRepairWorktrees: /* @__PURE__ */ new Set(),
				trackedWorktreeIds,
				reapplyableSnapshotsByKey: /* @__PURE__ */ new Map()
			} : null;
		};
		for (const { environmentId, expectedEnvironmentPairingRevision } of environments) {
			if (!shouldSyncAllRuntimeSessionTabs({
				activeRuntimeEnvironmentId: environmentId,
				workspaceSessionReady
			})) continue;
			let requestedInitialLoad = false;
			environmentIdBySubscriptionSpec.push(environmentId);
			subscriptionSpecs.push({
				subscribe: (isCurrent, { visibilityGeneration }) => {
					const isVisibilityRestart = visibilityGeneration > 0;
					let awaitingVisibilityResumeInventory = isVisibilityRestart;
					if (!requestedInitialLoad) {
						requestedInitialLoad = true;
						loadInitialWebSessionTabs(environmentId, expectedEnvironmentPairingRevision, isCurrent);
					}
					return window.api.runtimeEnvironments.subscribe({
						selector: environmentId,
						method: "session.tabs.subscribeAll",
						params: {},
						timeoutMs: 15e3,
						expectedEnvironmentPairingRevision
					}, {
						onResponse: (response) => {
							if (!isCurrent() || getRuntimeEnvironmentRevision(environmentId) !== expectedEnvironmentPairingRevision) return;
							if (response.ok === false) {
								console.warn("[web-session-tabs-sync] global subscription failed:", response.error.message);
								return;
							}
							const event = response.result;
							const replayed = isRuntimeSubscriptionReplayResponse(response);
							if (event.type === "snapshots") {
								const skipUnchangedResumeWork = awaitingVisibilityResumeInventory && !replayed;
								awaitingVisibilityResumeInventory = false;
								const unchangedVisibilityResumeSnapshots = event.snapshots.map((snapshot) => {
									const key = sessionTabsFreshnessKey(environmentId, snapshot.worktree);
									const freshness = latestSessionTabsSnapshotByWorktree.get(key);
									return Boolean(skipUnchangedResumeWork && !replayableSessionTabsSnapshotByWorktree.has(key) && freshness?.publicationEpoch === snapshot.publicationEpoch && freshness.snapshotVersion === snapshot.snapshotVersion);
								});
								const receivedFrames = event.snapshots.map((snapshot) => {
									const receivedFrame$1 = recordReceivedWebSessionTabsSnapshot(environmentId, snapshot);
									recordVisibilityResumeSnapshotReceipt(environmentId, snapshot, receivedFrame$1);
									return receivedFrame$1;
								});
								const inventoryReceivedFrame = recordReceivedWebSessionTabsInventory();
								const missingWorktrees = recordVisibilityResumeInventoryReceipt(environmentId, visibilityGeneration, inventoryReceivedFrame, event.snapshots);
								const finishRecoveries = event.snapshots.map((snapshot, index) => unchangedVisibilityResumeSnapshots[index] ? null : beginWebSessionTabsSnapshotRecovery(environmentId, snapshot.worktree, receivedFrames[index]));
								Promise.all(event.snapshots.map((snapshot, index) => unchangedVisibilityResumeSnapshots[index] ? Promise.resolve(snapshot) : recoverWebSessionTerminalOrphansBeforeApply(useAppStore.getState(), snapshot, environmentId))).then((recovered) => {
									if (isCurrent()) {
										const applicable = recovered.flatMap((snapshot, index) => snapshot !== null && shouldApplyRecoveredWebSessionTabsSnapshot(environmentId, snapshot, receivedFrames[index]) && shouldApplyVisibilityResumeSnapshot(environmentId, snapshot, receivedFrames[index]) ? [{
											index,
											snapshot
										}] : []);
										if (isVisibilityRestart || replayed) for (const { index, snapshot } of applicable) {
											if (unchangedVisibilityResumeSnapshots[index]) continue;
											acceptReplayedWebSessionTabsSnapshot(environmentId, snapshot.worktree);
										}
										const freshSnapshots = applicable.flatMap(({ index, snapshot }) => !unchangedVisibilityResumeSnapshots[index] && shouldApplyWebSessionTabsSnapshot(snapshot, environmentId) ? [snapshot] : []);
										if (freshSnapshots.length > 0) applyWebSessionTabsStorePatch((state) => applyWebSessionTabsSnapshots(state, freshSnapshots, environmentId));
										const freshSnapshotSet = new Set(freshSnapshots);
										for (const { index, snapshot } of applicable) {
											if (unchangedVisibilityResumeSnapshots[index]) queueAcceptedWebSessionTerminalSnapshot(snapshot, environmentId);
											if (unchangedVisibilityResumeSnapshots[index] || freshSnapshotSet.has(snapshot)) recordVisibilityResumeSnapshot(environmentId, snapshot, receivedFrames[index]);
										}
										recordVisibilityResumeInventory(environmentId, visibilityGeneration, inventoryReceivedFrame, missingWorktrees);
									}
								}).catch((error) => {
									if (isCurrent()) console.warn("[web-session-tabs-sync] snapshot recovery failed:", error);
								}).finally(() => {
									for (const finishRecovery$1 of finishRecoveries) finishRecovery$1?.();
								});
								return;
							}
							if (event.type !== "snapshot" && event.type !== "updated") return;
							const receivedFrame = recordReceivedWebSessionTabsSnapshot(environmentId, event);
							recordVisibilityResumeSnapshotReceipt(environmentId, event, receivedFrame);
							const finishRecovery = beginWebSessionTabsSnapshotRecovery(environmentId, event.worktree, receivedFrame);
							recoverWebSessionTerminalOrphansBeforeApply(useAppStore.getState(), event, environmentId).then((recovered) => {
								if (isCurrent() && recovered && shouldApplyRecoveredWebSessionTabsSnapshot(environmentId, recovered, receivedFrame) && shouldApplyVisibilityResumeSnapshot(environmentId, recovered, receivedFrame)) {
									if (replayed) acceptReplayedWebSessionTabsSnapshot(environmentId, recovered.worktree);
									if (shouldApplyWebSessionTabsSnapshot(recovered, environmentId)) {
										applyWebSessionTabsStorePatch((state) => applyWebSessionTabsSnapshot(state, recovered, environmentId));
										recordVisibilityResumeSnapshot(environmentId, recovered, receivedFrame);
									}
								}
							}).catch((error) => {
								if (isCurrent()) console.warn("[web-session-tabs-sync] snapshot recovery failed:", error);
							}).finally(finishRecovery);
						},
						onError: (error) => {
							if (isCurrent()) console.warn("[web-session-tabs-sync] global subscription error:", error.message);
						}
					});
				},
				onSubscribeError: (error) => {
					console.warn("[web-session-tabs-sync] failed to subscribe globally:", error instanceof Error ? error.message : String(error));
				},
				onUnsubscribeError: (error) => {
					console.warn("[web-session-tabs-sync] failed to unsubscribe globally:", error);
				}
			});
		}
		const disposeSubscriptions = installWindowVisibilitySubscriptionParking(subscriptionSpecs, {
			getVisibilityResumePriority: (index) => environmentIdBySubscriptionSpec[index] === activeRuntimeEnvironmentIdRef.current ? 0 : 1,
			visibilityResumeStaggerMs: 100,
			onVisibilityResume: beginVisibilityResume
		});
		return () => {
			recordVisibilityResumeSnapshotRef.current = () => {};
			recordVisibilityResumeSnapshotReceiptRef.current = () => {};
			shouldApplyVisibilityResumeSnapshotRef.current = () => true;
			disposeSubscriptions();
			for (const { environmentId, expectedEnvironmentPairingRevision } of environments) {
				const owner = {
					environmentId,
					pairingRevision: expectedEnvironmentPairingRevision
				};
				clearWebSessionCloseIntentsForOwner(owner);
				clearWebSessionFocusIntentsForOwner(owner);
				clearWebSessionReorderIntentsForOwner(owner);
			}
		};
	}, [runtimeSessionMirrorEnvironmentKey, workspaceSessionReady]);
	(0, import_react.useEffect)(() => {
		const environmentId = activeWorktreeRuntimeEnvironmentId?.trim();
		const expectedEnvironmentPairingRevision = activeWorktreeRuntimePairingRevision;
		if (!shouldSyncRuntimeSessionTabs({
			activeWorktreeId,
			activeWorktreeRuntimeEnvironmentId,
			workspaceSessionReady
		}) || !environmentId || !activeWorktreeId) return;
		let requestedInitialTerminal = false;
		let requestedRespawnAfterWake = false;
		const applyActiveSnapshot = async (event, response, isCurrent, receivedFrame) => {
			const recovered = await recoverWebSessionTerminalOrphansBeforeApply(useAppStore.getState(), event, environmentId);
			if (!isCurrent() || !recovered || !shouldApplyRecoveredWebSessionTabsSnapshot(environmentId, recovered, receivedFrame) || !shouldApplyVisibilityResumeSnapshotRef.current(environmentId, recovered, receivedFrame)) return;
			if (event.type === "snapshot" || isRuntimeSubscriptionReplayResponse(response)) acceptReplayedWebSessionTabsSnapshot(environmentId, recovered.worktree);
			const recoveredEvent = {
				...recovered,
				type: event.type
			};
			const fresh = shouldApplyWebSessionTabsSnapshot(recovered, environmentId);
			const syncState = useAppStore.getState();
			const localWorktreeTabs = syncState.tabsByWorktree[activeWorktreeId] ?? [];
			const localTerminalCount = localWorktreeTabs.length;
			const hasLiveLocalPty = localWorktreeTabs.some((tab) => (syncState.ptyIdsByTabId[tab.id] ?? []).length > 0);
			const shouldBootstrapInitialTerminal = shouldBootstrapInitialWebRuntimeTerminal({
				event: recoveredEvent,
				activeWorktreeId,
				requestedInitialTerminal,
				snapshotIsFresh: fresh,
				localTerminalCount
			});
			const shouldRespawnAfterWake = shouldRespawnWebRuntimeTerminalAfterWake({
				event: recoveredEvent,
				activeWorktreeId,
				requestedRespawnAfterWake,
				snapshotIsFresh: fresh,
				localTerminalCount,
				hasLiveLocalPty,
				skipWakeRespawn: shouldSkipWebRuntimeWakeTerminalRespawn(activeWorktreeId)
			});
			if (fresh) {
				applyWebSessionTabsStorePatch((state) => applyWebSessionTabsSnapshot(state, recovered, environmentId));
				recordVisibilityResumeSnapshotRef.current(environmentId, recovered, receivedFrame);
			}
			if (isCurrent() && shouldBootstrapInitialTerminal) {
				requestedInitialTerminal = true;
				await createWebRuntimeSessionTerminal({
					worktreeId: activeWorktreeId,
					environmentId,
					activate: true
				});
			} else if (isCurrent() && shouldRespawnAfterWake && beginWebRuntimeWakeTerminalRespawn(activeWorktreeId)) {
				requestedRespawnAfterWake = true;
				await createWebRuntimeSessionTerminal({
					worktreeId: activeWorktreeId,
					environmentId,
					activate: true,
					selectWorktree: false
				}).finally(() => endWebRuntimeWakeTerminalRespawn(activeWorktreeId));
			}
		};
		return installWindowVisibilitySubscriptionParking([{
			subscribe: (isCurrent) => window.api.runtimeEnvironments.subscribe({
				selector: environmentId,
				method: "session.tabs.subscribe",
				params: { worktree: toRuntimeWorktreeSelector(activeWorktreeId) },
				timeoutMs: 15e3,
				expectedEnvironmentPairingRevision
			}, {
				onResponse: (response) => {
					if (!isCurrent() || getRuntimeEnvironmentRevision(environmentId) !== expectedEnvironmentPairingRevision) return;
					if (response.ok === false) {
						console.warn("[web-session-tabs-sync] subscription failed:", response.error.message);
						return;
					}
					const event = response.result;
					if (event.type !== "snapshot" && event.type !== "updated") return;
					const receivedFrame = recordReceivedWebSessionTabsSnapshot(environmentId, event);
					recordVisibilityResumeSnapshotReceiptRef.current(environmentId, event, receivedFrame);
					const finishRecovery = beginWebSessionTabsSnapshotRecovery(environmentId, event.worktree, receivedFrame);
					applyActiveSnapshot(event, response, isCurrent, receivedFrame).catch((error) => {
						if (isCurrent()) console.warn("[web-session-tabs-sync] active snapshot recovery failed:", error);
					}).finally(finishRecovery);
				},
				onError: (error) => {
					if (isCurrent()) console.warn("[web-session-tabs-sync] subscription error:", error.message);
				}
			}),
			onSubscribeError: (error) => {
				console.warn("[web-session-tabs-sync] failed to subscribe:", error instanceof Error ? error.message : String(error));
			},
			onUnsubscribeError: (error) => {
				console.warn("[web-session-tabs-sync] failed to unsubscribe:", error);
			}
		}]);
	}, [
		activeWorktreeId,
		activeWorktreeRuntimeEnvironmentId,
		activeWorktreeRuntimeConnectionGeneration,
		activeWorktreeRuntimeId,
		activeWorktreeRuntimePairingRevision,
		workspaceSessionReady
	]);
}
export { beginWebRuntimeWakeTerminalRespawn as C, registerRendererOwnedAgentStatusPane as E, isRuntimeSubscriptionReplayResponse as S, markRendererOwnedAgentStatusWrite as T, useWebSessionTabsSync as _, applyWebSessionTabsSnapshot as a, subscribeWindowParkVisibility as b, clearWebSessionTabsTrackingForEnvironment as c, resolveHostSessionTabIdForWebSessionTab as d, shouldApplyWebSessionTabsSnapshot as f, shouldSyncRuntimeSessionTabs as g, shouldSyncAllRuntimeSessionTabs as h, applyFreshWebSessionTabsSnapshots as i, getLastKnownHostTerminalTabCount as l, shouldRespawnWebRuntimeTerminalAfterWake as m, acceptReplayedWebSessionTabsSnapshot as n, applyWebSessionTabsSnapshots as o, shouldBootstrapInitialWebRuntimeTerminal as p, applyFreshWebSessionTabsSnapshot as r, applyWebSessionTabsStorePatch as s, WEB_SESSION_TABS_VISIBILITY_RESUME_STAGGER_MS as t, getLatestWebSessionTabsPublicationEpoch as u, WINDOW_HIDE_PARK_GRACE_MS as v, endWebRuntimeWakeTerminalRespawn as w, subscribeAcceptedWebSessionTerminalHandle as x, getWindowParkVisible as y };
