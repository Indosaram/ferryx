const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./web-session-tabs-sync-DhHVQKaz.js","./button-DszXJEV6.js","./jsx-runtime-Cv_nyRjc.js","./preload-helper-Cgw39-ka.js","./chunk-Dhmk_5SA.js","./react-Da2TLWQy.js","./store-CgXrfmaH.js","./defineProperty-BAtR-r70.js","./dist-DgqligFk.js","./react-dom-Da8MQai-.js","./plugin-manifest-Bs-50M_g.js","./useMountedRef-1omUd-IV.js","./agent-status-3vUKbY6l.js","./agent-kind-Dfx6MnkP.js","./telemetry-ZyUPyKMD.js","./agent-paste-draft-C2PA7vXu.js","./terminal-pty-input-transaction-2UskR-Bm.js","./agent-process-recognition-BB0O3DaN.js","./web-session-tabs-sync-CYKZbAxS.js","./shallow-BpOhx1Gc.js","./window-visibility-interval-CtnbYoau.js","./web-agent-session-handoff-D4ZdXDx4.js","./pane-agent-owner-BPfoVAtS.js","./web-agent-session-handoff-DeCcUmxi.js"])))=>i.map(i=>d[i]);
import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { $f as toRuntimeWorktreeSelector, D as assertRuntimeManagedBrowserCreationAvailable, Fu as isWebTerminalSurfaceTabId, Gd as isNativeChatSupportedAgent, Iu as toHostSessionTabId, Jc as parsePaneKey, Lu as toWebTerminalSurfaceTabId, Ou as getRuntimeEnvironmentIdForWorktree, Ro as parseRemoteRuntimePtyId, Vu as createBrowserUuid, Y as clearWebSessionCloseIntent, Yd as canMirrorLaunchDraftToNativeChat, ap as runtimeEnvironmentSupportsCapability, cp as getRuntimeEnvironmentRevision, dp as unwrapRuntimeRpcResult, et as recordWebSessionCloseIntent, lp as RuntimeRpcCallError, om as AGENT_SESSION_HOST_AUTHORITY_RUNTIME_CAPABILITY, pp as isRuntimeCompatBlockError, sm as AGENT_SESSION_OMP_RESUME_PATH_RUNTIME_CAPABILITY, t as useAppStore, tt as webSessionIntentOwnerKey, up as hasRuntimeRpcErrorCode, vs as e2eConfig } from "./store-CgXrfmaH.js";
import { ct as toRuntimeExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
import { a as pasteDraftWhenAgentReady, d as agentDeliversDraftViaNativePrefill } from "./agent-paste-draft-C2PA7vXu.js";
function randomOperationNonce() {
	const cryptoApi = globalThis.crypto;
	if (!cryptoApi?.getRandomValues) throw new Error("Secure randomness is unavailable for this agent launch.");
	const bytes = new Uint8Array(16);
	cryptoApi.getRandomValues(bytes);
	return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}
function createAgentSessionOperationId(now = Date.now()) {
	return `${now}-${randomOperationNonce()}`;
}
var MAX_AMBIGUOUS_CREATE_ATTEMPTS = 2;
function isAmbiguousCreateFailure(error) {
	return !(error instanceof RuntimeRpcCallError) && !(error instanceof Error && error.name === "AbortError");
}
function createAgentSessionCreateOperation() {
	const clientOperationId = createAgentSessionOperationId();
	return {
		clientOperationId,
		async run(invoke) {
			let lastError;
			for (let attempt = 0; attempt < MAX_AMBIGUOUS_CREATE_ATTEMPTS; attempt += 1) try {
				return await invoke(clientOperationId);
			} catch (error) {
				lastError = error;
				if (!isAmbiguousCreateFailure(error)) throw error;
			}
			throw lastError;
		}
	};
}
function toAgentLaunchPreferences(sessionOptions) {
	if (!sessionOptions) return;
	const readString = (key) => {
		const value = sessionOptions[key];
		return typeof value === "string" && value.trim() ? value.trim() : void 0;
	};
	const model = readString("model");
	const effort = readString("effort");
	const mode = readString("mode");
	const preferences = {
		...model ? { model } : {},
		...effort ? { effort } : {},
		...mode ? { mode } : {}
	};
	return Object.keys(preferences).length > 0 ? preferences : void 0;
}
function withAgentSessionCreateOperationId(request, clientOperationId) {
	return {
		...request,
		clientOperationId
	};
}
var pendingFocusByOwnerAndWorktree = /* @__PURE__ */ new Map();
function resolveWebSessionVisibleTabId(state, worktreeId, tabs = state.unifiedTabsByWorktree?.[worktreeId] ?? []) {
	const currentType = state.activeTabTypeByWorktree?.[worktreeId] ?? (state.activeWorktreeId === worktreeId ? state.activeTabType : null);
	if (currentType === "terminal") {
		const tabId = state.activeTabIdByWorktree?.[worktreeId];
		return tabId && tabs.some((tab) => tab.id === tabId) ? tabId : null;
	}
	const entityId = currentType === "browser" ? state.activeBrowserTabIdByWorktree?.[worktreeId] : currentType === "editor" ? state.activeFileIdByWorktree?.[worktreeId] : null;
	return tabs.find((tab) => tab.contentType === currentType && tab.entityId === entityId)?.id ?? null;
}
function focusIntentPartitionKey(owner, worktreeId) {
	return `${webSessionIntentOwnerKey(owner)}\0${worktreeId}`;
}
function recordWebSessionFocusIntent(owner, worktreeId, hostTabId, leafId, expectedCurrentLocalTabId) {
	const trimmed = hostTabId.trim();
	if (!worktreeId || !trimmed) return;
	const trimmedLeafId = leafId?.trim();
	pendingFocusByOwnerAndWorktree.set(focusIntentPartitionKey(owner, worktreeId), {
		hostTabId: trimmed,
		...trimmedLeafId ? { leafId: trimmedLeafId } : {},
		...expectedCurrentLocalTabId !== void 0 ? { expectedCurrentLocalTabId } : {}
	});
}
function peekWebSessionFocusIntent(owner, worktreeId) {
	return pendingFocusByOwnerAndWorktree.get(focusIntentPartitionKey(owner, worktreeId)) ?? null;
}
function clearWebSessionFocusIntent(owner, worktreeId) {
	pendingFocusByOwnerAndWorktree.delete(focusIntentPartitionKey(owner, worktreeId));
}
function clearWebSessionFocusIntentIfMatches(owner, worktreeId, hostTabId) {
	const key = focusIntentPartitionKey(owner, worktreeId);
	if (pendingFocusByOwnerAndWorktree.get(key)?.hostTabId === hostTabId) pendingFocusByOwnerAndWorktree.delete(key);
}
function clearWebSessionFocusIntentsForOwner(owner) {
	const prefix = `${webSessionIntentOwnerKey(owner)}\0`;
	for (const key of pendingFocusByOwnerAndWorktree.keys()) if (key.startsWith(prefix)) pendingFocusByOwnerAndWorktree.delete(key);
}
var REORDER_INTENT_TTL_MS = 1e4;
var pendingReorderByOwnerAndWorktree = /* @__PURE__ */ new Map();
function reorderIntentPartitionKey(owner, worktreeId) {
	return `${webSessionIntentOwnerKey(owner)}\0${worktreeId}`;
}
function sameMembership(a, b) {
	if (a.length !== b.length) return false;
	const set = new Set(a);
	return b.every((id) => set.has(id));
}
function sameOrder(a, b) {
	return a.length === b.length && a.every((id, index) => id === b[index]);
}
function recordWebSessionReorderIntent(owner, worktreeId, groupId, order, now) {
	if (!worktreeId || !groupId || order.length === 0) return;
	const partitionKey = reorderIntentPartitionKey(owner, worktreeId);
	let byGroup = pendingReorderByOwnerAndWorktree.get(partitionKey);
	if (!byGroup) {
		byGroup = /* @__PURE__ */ new Map();
		pendingReorderByOwnerAndWorktree.set(partitionKey, byGroup);
	}
	byGroup.set(groupId, {
		order: [...order],
		recordedAt: now
	});
}
function resolveWebSessionReorderedOrder(owner, worktreeId, groupId, hostOrder, now) {
	const partitionKey = reorderIntentPartitionKey(owner, worktreeId);
	const byGroup = pendingReorderByOwnerAndWorktree.get(partitionKey);
	const intent = byGroup?.get(groupId);
	if (!intent) return hostOrder;
	const clear = () => {
		byGroup.delete(groupId);
		if (byGroup.size === 0) pendingReorderByOwnerAndWorktree.delete(partitionKey);
	};
	if (now - intent.recordedAt > REORDER_INTENT_TTL_MS) {
		clear();
		return hostOrder;
	}
	if (!sameMembership(intent.order, hostOrder)) {
		clear();
		return hostOrder;
	}
	if (sameOrder(intent.order, hostOrder)) {
		clear();
		return hostOrder;
	}
	return [...intent.order];
}
function clearWebSessionReorderIntentsForWorktree(owner, worktreeId) {
	pendingReorderByOwnerAndWorktree.delete(reorderIntentPartitionKey(owner, worktreeId));
}
function clearWebSessionReorderIntent(owner, worktreeId, groupId) {
	const partitionKey = reorderIntentPartitionKey(owner, worktreeId);
	const byGroup = pendingReorderByOwnerAndWorktree.get(partitionKey);
	byGroup?.delete(groupId);
	if (byGroup?.size === 0) pendingReorderByOwnerAndWorktree.delete(partitionKey);
}
function clearWebSessionReorderIntentsForOwner(owner) {
	const prefix = `${webSessionIntentOwnerKey(owner)}\0`;
	for (const key of pendingReorderByOwnerAndWorktree.keys()) if (key.startsWith(prefix)) pendingReorderByOwnerAndWorktree.delete(key);
}
function seedNativeChatLaunchDraftForAgentTab(args) {
	if (!canMirrorLaunchDraftToNativeChat(args.text) || !isNativeChatSupportedAgent(args.agent)) return;
	useAppStore.getState().seedNativeChatLaunchDraft({
		tabId: args.tabId,
		agent: args.agent,
		text: args.text,
		createdAt: Date.now()
	});
}
function deliverLaunchPromptToAgentTab(args) {
	const { tabId, agent, content, submit, forcePaste, timeoutMs, onTimeout } = args;
	const shouldSeed = submit === true && content.trim().length > 0 && isNativeChatSupportedAgent(agent);
	if (shouldSeed) useAppStore.getState().seedNativeChatLaunchPrompt({
		tabId,
		agent,
		text: content,
		createdAt: Date.now()
	});
	else if (submit !== true) seedNativeChatLaunchDraftForAgentTab({
		tabId,
		agent,
		text: content
	});
	const deliversViaNativePrefill = agentDeliversDraftViaNativePrefill(agent, forcePaste);
	return pasteDraftWhenAgentReady({
		tabId,
		content,
		agent,
		submit,
		forcePaste,
		timeoutMs,
		onTimeout
	}).then((delivered) => {
		if (shouldSeed && !delivered && !deliversViaNativePrefill) useAppStore.getState().markNativeChatLaunchPromptFailed(tabId);
		return delivered || deliversViaNativePrefill;
	});
}
var inFlightBySession = /* @__PURE__ */ new Map();
function remoteRuntimeSessionTabsKey(args) {
	return `${args.environmentId}\u0000${args.worktreeId}`;
}
function listRemoteRuntimeSessionTabsDeduped(args) {
	const key = remoteRuntimeSessionTabsKey(args);
	const existing = inFlightBySession.get(key);
	if (existing) return existing;
	const request = args.load().finally(() => {
		if (inFlightBySession.get(key) === request) inFlightBySession.delete(key);
	});
	inFlightBySession.set(key, request);
	return request;
}
async function listRemoteRuntimeSessionTabsAfterCurrentInFlight(args) {
	const current = inFlightBySession.get(remoteRuntimeSessionTabsKey(args));
	if (current) await current.catch(() => void 0);
	return listRemoteRuntimeSessionTabsDeduped(args);
}
async function runRemoteAgentSessionLaunch(args) {
	if (!args.hostAuthority) return await args.legacy({ skipCompatibilityCheck: false });
	let supported;
	try {
		supported = await runtimeEnvironmentSupportsCapability(args.environmentId, args.hostAuthorityCapability ?? "agent-session.host-authority.v1");
	} catch (error) {
		if (isRuntimeCompatBlockError(error)) throw error;
		return await args.legacy({ skipCompatibilityCheck: true });
	}
	if (!supported) return await args.legacy({ skipCompatibilityCheck: true });
	try {
		return await args.hostAuthority();
	} catch (error) {
		if (error instanceof RuntimeRpcCallError && (error.code === "agent_session_legacy_required" || error.code === "method_not_found")) return await args.legacy({ skipCompatibilityCheck: true });
		throw error;
	}
}
var placementByPendingPage = /* @__PURE__ */ new Map();
var materializedGroupKeys = /* @__PURE__ */ new Set();
var pendingCleanupClaimsByGroup = /* @__PURE__ */ new Map();
var MAX_PENDING_PLACEMENTS = 128;
function pageKey(environmentId, worktreeId, remotePageId) {
	return `${environmentId}\0${worktreeId}\0${remotePageId}`;
}
function worktreePrefix(environmentId, worktreeId) {
	return `${environmentId}\0${worktreeId}\0`;
}
function worktreeGroupKey(worktreeId, groupId) {
	return `${worktreeId}\0${groupId}`;
}
function hasPlacementForGroup(worktreeId, groupId) {
	const worktreeMarker = `\0${worktreeId}\0`;
	for (const [key, placement] of placementByPendingPage) if (key.includes(worktreeMarker) && placement.groupId === groupId) return true;
	return false;
}
function forgetSettledMaterializedGroup(worktreeId, groupId) {
	const key = worktreeGroupKey(worktreeId, groupId);
	if (!hasPlacementForGroup(worktreeId, groupId) && !pendingCleanupClaimsByGroup.has(key)) materializedGroupKeys.delete(key);
}
function recordWebSessionBrowserPlacement(args) {
	const key = pageKey(args.environmentId, args.worktreeId, args.remotePageId);
	const existing = placementByPendingPage.get(key);
	if (!existing && placementByPendingPage.size >= MAX_PENDING_PLACEMENTS) throw new Error("Too many paired browser placements are pending.");
	placementByPendingPage.set(key, {
		groupId: args.groupId,
		ownsGroupCleanup: args.callerCreatedGroup === true || existing?.ownsGroupCleanup === true
	});
}
function moveWebSessionBrowserPlacement(args) {
	const fromKey = pageKey(args.environmentId, args.worktreeId, args.fromRemotePageId);
	const placement = placementByPendingPage.get(fromKey);
	placementByPendingPage.delete(fromKey);
	if (placement) recordWebSessionBrowserPlacement({
		environmentId: args.environmentId,
		worktreeId: args.worktreeId,
		remotePageId: args.toRemotePageId,
		groupId: placement.groupId,
		callerCreatedGroup: placement.ownsGroupCleanup
	});
}
function forgetWebSessionBrowserPlacement(args) {
	const key = pageKey(args.environmentId, args.worktreeId, args.remotePageId);
	const placement = placementByPendingPage.get(key);
	placementByPendingPage.delete(key);
	if (placement) forgetSettledMaterializedGroup(args.worktreeId, placement.groupId);
}
function peekWebSessionBrowserPlacementGroup(args) {
	return placementByPendingPage.get(pageKey(args.environmentId, args.worktreeId, args.remotePageId))?.groupId;
}
function isWebSessionBrowserPlacementGroupReserved(args) {
	const worktreeMarker = `\0${args.worktreeId}\0`;
	for (const [key, placement] of placementByPendingPage) if (key.includes(worktreeMarker) && placement.groupId === args.groupId) return true;
	return false;
}
function releaseWebSessionBrowserPlacementGroup(args) {
	const key = pageKey(args.environmentId, args.worktreeId, args.remotePageId);
	const placement = placementByPendingPage.get(key);
	const groupId = placement?.groupId ?? args.groupId;
	const groupKey = worktreeGroupKey(args.worktreeId, groupId);
	const materialized = materializedGroupKeys.has(groupKey);
	placementByPendingPage.delete(key);
	const ownsCleanup = !materialized && (args.callerCreatedGroup || placement?.ownsGroupCleanup === true);
	if (ownsCleanup) pendingCleanupClaimsByGroup.set(groupKey, (pendingCleanupClaimsByGroup.get(groupKey) ?? 0) + 1);
	forgetSettledMaterializedGroup(args.worktreeId, groupId);
	return ownsCleanup;
}
function markWebSessionBrowserPlacementGroupMaterialized(args) {
	if (hasPlacementForGroup(args.worktreeId, args.groupId)) materializedGroupKeys.add(worktreeGroupKey(args.worktreeId, args.groupId));
}
function claimWebSessionBrowserPlacementGroupCleanup(args) {
	if (!args.ownsGroupCleanup) return false;
	const groupKey = worktreeGroupKey(args.worktreeId, args.groupId);
	const pendingClaims = pendingCleanupClaimsByGroup.get(groupKey) ?? 0;
	if (pendingClaims <= 1) pendingCleanupClaimsByGroup.delete(groupKey);
	else pendingCleanupClaimsByGroup.set(groupKey, pendingClaims - 1);
	if (materializedGroupKeys.has(groupKey)) {
		forgetSettledMaterializedGroup(args.worktreeId, args.groupId);
		return false;
	}
	const worktreeMarker = `\0${args.worktreeId}\0`;
	let transferred = false;
	for (const [key, placement] of placementByPendingPage) if (key.includes(worktreeMarker) && placement.groupId === args.groupId) {
		placementByPendingPage.set(key, {
			...placement,
			ownsGroupCleanup: true
		});
		transferred = true;
	}
	return !transferred;
}
function clearWebSessionBrowserPlacementsForWorktree(environmentId, worktreeId) {
	const prefix = worktreePrefix(environmentId, worktreeId);
	for (const key of placementByPendingPage.keys()) if (key.startsWith(prefix) && !placementByPendingPage.get(key)?.ownsGroupCleanup) placementByPendingPage.delete(key);
}
function clearWebSessionBrowserPlacementsForEnvironment(environmentId) {
	const prefix = `${environmentId}\0`;
	for (const key of placementByPendingPage.keys()) if (key.startsWith(prefix) && !placementByPendingPage.get(key)?.ownsGroupCleanup) placementByPendingPage.delete(key);
}
function hasMaterializedWebRuntimeBrowserPage(state, environmentId, worktreeId, remotePageId, expectedGroupId) {
	return (state.browserTabsByWorktree[worktreeId] ?? []).some((workspace) => {
		if (!(state.browserPagesByWorkspace[workspace.id] ?? []).some((page) => {
			const handle = state.remoteBrowserPageHandlesByPageId[page.id];
			return handle?.environmentId === environmentId && handle.remotePageId === remotePageId;
		})) return false;
		return (state.unifiedTabsByWorktree[worktreeId] ?? []).some((tab) => tab.contentType === "browser" && tab.entityId === workspace.id && (!expectedGroupId || tab.groupId === expectedGroupId));
	});
}
var armed = false;
var capabilityRejectionArmed = false;
var createdPageId = null;
var failNextReconciliation = false;
var releaseCreatedPage = null;
var createdPageBarrier = null;
var suppressedPageIds = /* @__PURE__ */ new Set();
var MAX_SUPPRESSED_PAGE_IDS = 128;
function resetFault() {
	releaseCreatedPage?.();
	armed = false;
	capabilityRejectionArmed = false;
	createdPageId = null;
	failNextReconciliation = false;
	releaseCreatedPage = null;
	createdPageBarrier = null;
	suppressedPageIds.clear();
}
function exposeFaultApi() {
	if (!e2eConfig.exposeStore || typeof window === "undefined") return;
	const target = window;
	target.__webRuntimeBrowserCreationFault ?? (target.__webRuntimeBrowserCreationFault = {
		arm: () => {
			resetFault();
			armed = true;
			createdPageBarrier = new Promise((resolve) => {
				releaseCreatedPage = resolve;
			});
		},
		armCapabilityRejection: () => {
			resetFault();
			capabilityRejectionArmed = true;
		},
		release: () => {
			if (!armed || !createdPageId || !releaseCreatedPage) return false;
			failNextReconciliation = true;
			const release = releaseCreatedPage;
			releaseCreatedPage = null;
			release();
			return true;
		},
		reset: resetFault,
		snapshot: () => ({
			armed,
			capabilityRejectionArmed,
			createdPageId,
			suppressedPageIds: [...suppressedPageIds]
		})
	});
}
exposeFaultApi();
function throwIfE2eWebRuntimeBrowserCapabilityUnavailable() {
	if (!e2eConfig.exposeStore || !capabilityRejectionArmed) return;
	capabilityRejectionArmed = false;
	throw new Error("E2E forced browser capability rejection");
}
async function pauseAfterE2eWebRuntimeBrowserCreate(remotePageId) {
	if (!e2eConfig.exposeStore || !armed || !createdPageBarrier) return;
	createdPageId = remotePageId;
	await createdPageBarrier;
}
function throwIfE2eWebRuntimeBrowserReconciliationFails() {
	if (!e2eConfig.exposeStore || !failNextReconciliation) return;
	failNextReconciliation = false;
	throw new Error("E2E forced session-tabs reconciliation timeout");
}
function suppressE2eWebRuntimeBrowserSnapshot(snapshot) {
	if (!e2eConfig.exposeStore || !armed) return false;
	const pageIds = snapshot.tabs.flatMap((tab) => tab.type === "browser" && tab.browserPageId ? [tab.browserPageId] : []);
	for (const pageId of pageIds) {
		if (suppressedPageIds.size >= MAX_SUPPRESSED_PAGE_IDS) break;
		suppressedPageIds.add(pageId);
	}
	return pageIds.length > 0;
}
function isWebRuntimeSessionActive(activeRuntimeEnvironmentId) {
	return Boolean(activeRuntimeEnvironmentId?.trim());
}
var DEFINITIVE_BROWSER_CREATE_FAILURE_CODES = [
	"browser_error",
	"capability_unsupported",
	"invalid_argument",
	"invalid_params",
	"method_not_found",
	"runtime_rpc_queue_overloaded",
	"selector_ambiguous",
	"selector_not_found",
	"unauthorized"
];
function isDefinitiveBrowserCreateFailure(error) {
	return DEFINITIVE_BROWSER_CREATE_FAILURE_CODES.some((code) => hasRuntimeRpcErrorCode(error, code));
}
var pendingWebRuntimeSplitMirrorTelemetry = /* @__PURE__ */ new Map();
var WEB_RUNTIME_SPLIT_MIRROR_SUPPRESSION_TTL_MS = 3e4;
var pendingWebRuntimeSplitMirrorTelemetryId = 0;
var pendingRuntimeWorktreeRecoveryRefreshes = /* @__PURE__ */ new Map();
var RUNTIME_WORKTREE_RECOVERY_REFRESH_DELAYS_MS = [
	250,
	500,
	1e3,
	2e3,
	4e3
];
function captureRuntimeEnvironmentCall(environmentId, expectedEnvironmentPairingRevision = getRuntimeEnvironmentRevision(environmentId)) {
	return (args) => window.api.runtimeEnvironments.call({
		selector: environmentId,
		...args,
		expectedEnvironmentPairingRevision
	});
}
function captureWebSessionIntentOwner(environmentId) {
	return {
		environmentId,
		pairingRevision: getRuntimeEnvironmentRevision(environmentId)
	};
}
function matchesWebSessionIntentOwner(owner) {
	return getRuntimeEnvironmentRevision(owner.environmentId) === owner.pairingRevision;
}
function createdTerminalLeafId(terminal) {
	const pane = parsePaneKey(terminal.paneKey ?? "");
	return pane && pane.tabId === terminal.tabId ? pane.leafId : void 0;
}
async function createWebRuntimeSessionTerminal(args) {
	return (await createWebRuntimeSessionTerminalResult(args)).outcome;
}
async function createWebRuntimeAgentSessionTerminal(args) {
	const created = await createWebRuntimeSessionTerminalResult(args);
	if (created.outcome.status === "failed" || !created.hostTabId) return {
		outcome: created.outcome,
		promptDelivered: false
	};
	const promptDelivered = await deliverLaunchPromptToAgentTab({
		tabId: toWebTerminalSurfaceTabId(created.hostTabId),
		content: args.promptAfterReady,
		agent: args.agent,
		submit: args.submitPrompt,
		forcePaste: args.forcePromptPaste
	});
	return {
		outcome: created.outcome,
		promptDelivered
	};
}
async function createWebRuntimeAgentSessionTerminalWithLaunchDraft(args) {
	const created = await createWebRuntimeSessionTerminalResult(args);
	if (created.outcome.status !== "failed" && created.hostTabId) seedNativeChatLaunchDraftForAgentTab({
		tabId: toWebTerminalSurfaceTabId(created.hostTabId),
		agent: args.agent,
		text: args.launchDraft
	});
	return created.outcome;
}
async function createWebRuntimeSessionTerminalResult(args) {
	const environmentId = args.environmentId?.trim() ?? useAppStore.getState().settings?.activeRuntimeEnvironmentId?.trim() ?? null;
	if (!environmentId || !isWebRuntimeSessionActive(environmentId)) return { outcome: {
		status: "failed",
		message: translate("auto.runtime.webRuntimeSession.remoteHostDisconnected", "The workspace is not connected to a remote Orca host.")
	} };
	const intentOwner = captureWebSessionIntentOwner(environmentId);
	const callEnvironment = captureRuntimeEnvironmentCall(environmentId, intentOwner.pairingRevision);
	if (args.selectWorktree !== false) selectWebRuntimeSessionWorktree(args.worktreeId, environmentId);
	let hostCreated = false;
	let createdTabId;
	let createdLeafId;
	try {
		const agent = args.launchAgent ?? args.agent;
		const agentArgsOverride = args.agentArgs !== void 0 ? args.agentArgs : args.launchConfig?.agentArgs;
		if (agent) {
			let legacyAlreadyPlacedInGroup = false;
			const hostAuthority = args.afterTabId ? void 0 : args.agentSessionKind === "resume" ? args.providerSession ? async () => unwrapRuntimeRpcResult(await callEnvironment({
				method: "terminal.ensureAgentSession",
				params: {
					kind: "explicit",
					worktree: toRuntimeWorktreeSelector(args.worktreeId),
					agent,
					providerSession: args.providerSession,
					...args.launchConfig?.ompResumeFilePath ? { ompResumeFilePath: args.launchConfig.ompResumeFilePath } : {},
					...agentArgsOverride !== void 0 ? { agentArgs: agentArgsOverride } : {},
					...args.launchPreferences ? { launchPreferences: args.launchPreferences } : {},
					presentation: "background"
				},
				timeoutMs: 15e3
			})) : void 0 : async () => await createAgentSessionCreateOperation().run(async (clientOperationId) => unwrapRuntimeRpcResult(await callEnvironment({
				method: "terminal.createAgentSession",
				params: withAgentSessionCreateOperationId({
					worktree: toRuntimeWorktreeSelector(args.worktreeId),
					agent,
					...args.prompt ? { prompt: args.prompt } : {},
					...args.promptDelivery ? { promptDelivery: args.promptDelivery } : {},
					...agentArgsOverride !== void 0 ? { agentArgs: agentArgsOverride } : {},
					...args.launchPreferences ? { launchPreferences: args.launchPreferences } : {},
					...args.cwd ? { startupCwd: args.cwd } : {},
					...args.viewMode ? { viewMode: args.viewMode } : {},
					presentation: "background"
				}, clientOperationId),
				timeoutMs: 15e3
			})));
			const created = await runRemoteAgentSessionLaunch({
				environmentId,
				...hostAuthority ? { hostAuthority } : {},
				...args.agentSessionKind === "resume" && agent === "omp" ? { hostAuthorityCapability: AGENT_SESSION_OMP_RESUME_PATH_RUNTIME_CAPABILITY } : {},
				legacy: async () => {
					const legacyCreated = unwrapRuntimeRpcResult(await callEnvironment({
						method: "session.tabs.createTerminal",
						params: {
							worktree: toRuntimeWorktreeSelector(args.worktreeId),
							afterTabId: args.afterTabId ? toHostSessionTabId(args.afterTabId) : void 0,
							targetGroupId: args.targetGroupId,
							command: args.command,
							cwd: args.cwd,
							...args.env ? { env: args.env } : {},
							...args.envToDelete ? { envToDelete: args.envToDelete } : {},
							startupCommandDelivery: args.startupCommandDelivery,
							...args.launchConfig ? { launchConfig: args.launchConfig } : {},
							...args.launchToken ? { launchToken: args.launchToken } : {},
							...args.agent ? { agent: args.agent } : {},
							...args.launchAgent ? { launchAgent: args.launchAgent } : {},
							...args.viewMode ? { viewMode: args.viewMode } : {},
							activate: false,
							select: args.activate !== false,
							navigation: "caller"
						},
						timeoutMs: 15e3
					}));
					legacyAlreadyPlacedInGroup = true;
					return { terminal: {
						tabId: legacyCreated.tab.id,
						leafId: legacyCreated.tab.leafId
					} };
				}
			});
			hostCreated = true;
			createdTabId = created.terminal.tabId;
			createdLeafId = legacyAlreadyPlacedInGroup ? created.terminal.leafId : createdTerminalLeafId(created.terminal);
			if (args.targetGroupId && createdTabId && !legacyAlreadyPlacedInGroup) await callEnvironment({
				method: "session.tabs.move",
				params: {
					worktree: toRuntimeWorktreeSelector(args.worktreeId),
					tabId: createdTabId,
					targetGroupId: args.targetGroupId,
					kind: "move-to-group"
				},
				timeoutMs: 15e3
			});
		} else {
			const created = unwrapRuntimeRpcResult(await callEnvironment({
				method: "session.tabs.createTerminal",
				params: {
					worktree: toRuntimeWorktreeSelector(args.worktreeId),
					afterTabId: args.afterTabId ? toHostSessionTabId(args.afterTabId) : void 0,
					targetGroupId: args.targetGroupId,
					command: args.command,
					cwd: args.cwd,
					...args.env ? { env: args.env } : {},
					...args.envToDelete ? { envToDelete: args.envToDelete } : {},
					startupCommandDelivery: args.startupCommandDelivery,
					...args.launchConfig ? { launchConfig: args.launchConfig } : {},
					...args.launchToken ? { launchToken: args.launchToken } : {},
					...args.viewMode ? { viewMode: args.viewMode } : {},
					activate: false,
					select: args.activate !== false,
					navigation: "caller"
				},
				timeoutMs: 15e3
			}));
			hostCreated = true;
			createdTabId = created.tab.id;
			createdLeafId = created.tab.leafId;
		}
		if (args.activate !== false && createdTabId && matchesWebSessionIntentOwner(intentOwner)) recordWebSessionFocusIntent(intentOwner, args.worktreeId, createdTabId, createdLeafId);
		await refreshWebRuntimeSessionTabsSnapshot(environmentId, args.worktreeId, {
			expectedEnvironmentPairingRevision: intentOwner.pairingRevision,
			acceptCurrentSnapshot: args.activate !== false && Boolean(createdTabId)
		});
		return {
			outcome: { status: "created" },
			...createdTabId ? { hostTabId: createdTabId } : {}
		};
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		console.warn(hostCreated ? "[web-runtime-session] terminal created but reconciliation failed:" : "[web-runtime-session] failed to create terminal:", message);
		return {
			outcome: hostCreated ? { status: "created" } : {
				status: "failed",
				message
			},
			...createdTabId ? { hostTabId: createdTabId } : {}
		};
	}
}
async function createWebRuntimeSessionBrowserTab(args) {
	const environmentId = args.environmentId?.trim() ?? useAppStore.getState().settings?.activeRuntimeEnvironmentId?.trim() ?? null;
	if (!environmentId || !isWebRuntimeSessionActive(environmentId)) return false;
	const intentOwner = captureWebSessionIntentOwner(environmentId);
	const callEnvironment = captureRuntimeEnvironmentCall(environmentId, intentOwner.pairingRevision);
	const shouldFocusOnCreate = args.focusOnCreate !== false;
	const shouldSelectWorktree = args.selectWorktree !== false;
	const provisionalPageId = createBrowserUuid();
	let unsubscribeFocusGuard = () => {};
	let guardedPageId = provisionalPageId;
	let createdPageId$1 = null;
	let createAttempted = false;
	try {
		throwIfE2eWebRuntimeBrowserCapabilityUnavailable();
		assertRuntimeManagedBrowserCreationAvailable(useAppStore.getState(), environmentId);
		if (args.clientTargetGroupId) recordWebSessionBrowserPlacement({
			environmentId,
			worktreeId: args.worktreeId,
			remotePageId: provisionalPageId,
			groupId: args.clientTargetGroupId,
			callerCreatedGroup: args.clientTargetGroupCreated
		});
		if (shouldSelectWorktree) selectWebRuntimeSessionBrowserWorktree(args.worktreeId, environmentId);
		const initialFocusState = shouldFocusOnCreate ? useAppStore.getState() : null;
		const expectedActiveWorktreeId = initialFocusState?.activeWorktreeId;
		const expectedActiveWorkspaceExecutionHostId = initialFocusState?.activeWorkspaceExecutionHostId;
		const expectedCurrentLocalTabId = initialFocusState ? resolveWebSessionVisibleTabId(initialFocusState, args.worktreeId) : null;
		if (shouldFocusOnCreate && matchesWebSessionIntentOwner(intentOwner)) {
			recordWebSessionFocusIntent(intentOwner, args.worktreeId, provisionalPageId, void 0, expectedCurrentLocalTabId);
			unsubscribeFocusGuard = useAppStore.subscribe((state, previousState) => {
				if (state.activeBrowserTabIdByWorktree === previousState.activeBrowserTabIdByWorktree && state.activeFileIdByWorktree === previousState.activeFileIdByWorktree && state.activeTabIdByWorktree === previousState.activeTabIdByWorktree && state.activeTabType === previousState.activeTabType && state.activeTabTypeByWorktree === previousState.activeTabTypeByWorktree && state.activeWorktreeId === previousState.activeWorktreeId && state.activeWorkspaceExecutionHostId === previousState.activeWorkspaceExecutionHostId && state.unifiedTabsByWorktree === previousState.unifiedTabsByWorktree) return;
				if (state.activeWorktreeId === expectedActiveWorktreeId && state.activeWorkspaceExecutionHostId === expectedActiveWorkspaceExecutionHostId && resolveWebSessionVisibleTabId(state, args.worktreeId) === expectedCurrentLocalTabId) return;
				clearWebSessionFocusIntentIfMatches(intentOwner, args.worktreeId, guardedPageId);
				unsubscribeFocusGuard();
			});
		}
		createAttempted = true;
		const created = unwrapRuntimeRpcResult(await callEnvironment({
			method: "browser.tabCreate",
			params: {
				worktree: toRuntimeWorktreeSelector(args.worktreeId),
				url: args.url,
				profileId: args.profileId ?? void 0,
				activate: shouldFocusOnCreate,
				...args.targetGroupId ? { targetGroupId: args.targetGroupId } : {},
				waitForRegistration: args.waitForRegistration ?? false
			},
			timeoutMs: 15e3
		}));
		createdPageId$1 = created.browserPageId;
		await pauseAfterE2eWebRuntimeBrowserCreate(created.browserPageId);
		if (created.browserPageId !== provisionalPageId) {
			moveWebSessionBrowserPlacement({
				environmentId,
				worktreeId: args.worktreeId,
				fromRemotePageId: provisionalPageId,
				toRemotePageId: created.browserPageId
			});
			const focusIntent = shouldFocusOnCreate ? peekWebSessionFocusIntent(intentOwner, args.worktreeId) : null;
			if (focusIntent?.hostTabId === provisionalPageId) recordWebSessionFocusIntent(intentOwner, args.worktreeId, created.browserPageId, void 0, focusIntent.expectedCurrentLocalTabId);
			guardedPageId = created.browserPageId;
		}
		try {
			await refreshWebRuntimeSessionTabsSnapshot(environmentId, args.worktreeId, {
				expectedEnvironmentPairingRevision: intentOwner.pairingRevision,
				acceptCurrentSnapshot: true,
				afterCurrentInFlight: true,
				errorMode: "throw"
			});
		} catch (error) {
			if (!hasMaterializedWebRuntimeBrowserPage(useAppStore.getState(), environmentId, args.worktreeId, created.browserPageId, args.clientTargetGroupId ?? args.targetGroupId)) throw error;
		}
		if (!hasMaterializedWebRuntimeBrowserPage(useAppStore.getState(), environmentId, args.worktreeId, created.browserPageId, args.clientTargetGroupId ?? args.targetGroupId)) throw new Error("The created browser tab did not materialize in the client.");
		const remainingFocusIntent = shouldFocusOnCreate ? peekWebSessionFocusIntent(intentOwner, args.worktreeId) : null;
		if (remainingFocusIntent?.hostTabId === guardedPageId && remainingFocusIntent.expectedCurrentLocalTabId === expectedCurrentLocalTabId) clearWebSessionFocusIntentIfMatches(intentOwner, args.worktreeId, guardedPageId);
		unsubscribeFocusGuard();
		if (args.clientTargetGroupId) markWebSessionBrowserPlacementGroupMaterialized({
			worktreeId: args.worktreeId,
			groupId: args.clientTargetGroupId
		});
		forgetWebSessionBrowserPlacement({
			environmentId,
			worktreeId: args.worktreeId,
			remotePageId: guardedPageId
		});
		return true;
	} catch (error) {
		unsubscribeFocusGuard();
		let recoveryError = null;
		const createOutcomeUnknown = !createdPageId$1 && !isDefinitiveBrowserCreateFailure(error);
		const ownsClientGroupCleanup = args.clientTargetGroupId ? releaseWebSessionBrowserPlacementGroup({
			environmentId,
			worktreeId: args.worktreeId,
			remotePageId: guardedPageId,
			groupId: args.clientTargetGroupId,
			callerCreatedGroup: args.clientTargetGroupCreated === true
		}) : false;
		if (!args.clientTargetGroupId) forgetWebSessionBrowserPlacement({
			environmentId,
			worktreeId: args.worktreeId,
			remotePageId: guardedPageId
		});
		if (createdPageId$1) try {
			if (!unwrapRuntimeRpcResult(await callEnvironment({
				method: "browser.tabClose",
				params: {
					worktree: toRuntimeWorktreeSelector(args.worktreeId),
					page: createdPageId$1
				},
				timeoutMs: 15e3
			})).closed) throw new Error("The paired runtime did not close the unreconciled browser tab.");
			await refreshWebRuntimeSessionTabsSnapshot(environmentId, args.worktreeId, {
				expectedEnvironmentPairingRevision: intentOwner.pairingRevision,
				afterCurrentInFlight: true,
				errorMode: "throw"
			});
			if (hasMaterializedWebRuntimeBrowserPage(useAppStore.getState(), environmentId, args.worktreeId, createdPageId$1)) throw new Error("The closed browser tab remained materialized in the client.");
		} catch (cleanupError) {
			recoveryError = cleanupError;
			console.warn("[web-runtime-session] failed to clean up unreconciled browser tab:", cleanupError instanceof Error ? cleanupError.message : String(cleanupError));
		}
		if (shouldFocusOnCreate) clearWebSessionFocusIntentIfMatches(intentOwner, args.worktreeId, guardedPageId);
		if (args.clientTargetGroupId && claimWebSessionBrowserPlacementGroupCleanup({
			worktreeId: args.worktreeId,
			groupId: args.clientTargetGroupId,
			ownsGroupCleanup: ownsClientGroupCleanup
		})) useAppStore.getState().closeEmptyGroup(args.worktreeId, args.clientTargetGroupId);
		if (args.failureLogMode === "operation-only") console.warn("[web-runtime-session] failed to create browser tab");
		else console.warn("[web-runtime-session] failed to create browser tab:", error instanceof Error ? error.message : String(error));
		if (recoveryError) throw new Error("The paired runtime could not recover the failed browser creation.", { cause: recoveryError });
		if (!createAttempted) throw error;
		if (createOutcomeUnknown) throw new Error("The paired runtime did not confirm whether the browser tab was created.", { cause: error });
		return false;
	}
}
function selectWebRuntimeSessionWorktree(worktreeId, environmentId) {
	useAppStore.getState().setActiveWorktree(worktreeId, toRuntimeExecutionHostId(environmentId));
}
function selectWebRuntimeSessionBrowserWorktree(worktreeId, environmentId) {
	const state = useAppStore.getState();
	if (state.activeWorktreeId !== worktreeId || state.activeWorkspaceExecutionHostId !== toRuntimeExecutionHostId(environmentId)) state.setActiveWorktree(worktreeId, toRuntimeExecutionHostId(environmentId));
}
async function refreshWebRuntimeSessionTabsSnapshot(environmentId, worktreeId, options = {}) {
	const expectedEnvironmentPairingRevision = options.expectedEnvironmentPairingRevision ?? getRuntimeEnvironmentRevision(environmentId);
	const callEnvironment = captureRuntimeEnvironmentCall(environmentId, expectedEnvironmentPairingRevision);
	try {
		if (options.acceptCurrentSnapshot) {
			const { acceptReplayedWebSessionTabsSnapshot } = await __vitePreload(async () => {
				const { acceptReplayedWebSessionTabsSnapshot: acceptReplayedWebSessionTabsSnapshot$1 } = await import("./web-session-tabs-sync-DhHVQKaz.js");
				return { acceptReplayedWebSessionTabsSnapshot: acceptReplayedWebSessionTabsSnapshot$1 };
			}, __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22]), import.meta.url);
			acceptReplayedWebSessionTabsSnapshot(environmentId, worktreeId);
		}
		const listSessionTabs = options.confirmAgentSessionHandoff || options.afterCurrentInFlight ? listRemoteRuntimeSessionTabsAfterCurrentInFlight : listRemoteRuntimeSessionTabsDeduped;
		if (options.afterCurrentInFlight) throwIfE2eWebRuntimeBrowserReconciliationFails();
		const snapshot = await listSessionTabs({
			environmentId,
			worktreeId,
			load: async () => {
				return unwrapRuntimeRpcResult(await callEnvironment({
					method: "session.tabs.list",
					params: { worktree: toRuntimeWorktreeSelector(worktreeId) },
					timeoutMs: 15e3
				}));
			}
		});
		if (options.confirmAgentSessionHandoff) {
			const { confirmWebAgentSessionHandoffAfterCreate } = await __vitePreload(async () => {
				const { confirmWebAgentSessionHandoffAfterCreate: confirmWebAgentSessionHandoffAfterCreate$1 } = await import("./web-agent-session-handoff-DeCcUmxi.js");
				return { confirmWebAgentSessionHandoffAfterCreate: confirmWebAgentSessionHandoffAfterCreate$1 };
			}, __vite__mapDeps([23,21]), import.meta.url);
			confirmWebAgentSessionHandoffAfterCreate({
				environmentId,
				worktreeId,
				...options.confirmAgentSessionHandoff
			});
		}
		const { applyFreshWebSessionTabsSnapshot, applyWebSessionTabsStorePatch } = await __vitePreload(async () => {
			const { applyFreshWebSessionTabsSnapshot: applyFreshWebSessionTabsSnapshot$1, applyWebSessionTabsStorePatch: applyWebSessionTabsStorePatch$1 } = await import("./web-session-tabs-sync-DhHVQKaz.js");
			return {
				applyFreshWebSessionTabsSnapshot: applyFreshWebSessionTabsSnapshot$1,
				applyWebSessionTabsStorePatch: applyWebSessionTabsStorePatch$1
			};
		}, __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22]), import.meta.url);
		if (getRuntimeEnvironmentRevision(environmentId) !== expectedEnvironmentPairingRevision) return;
		applyWebSessionTabsStorePatch((state) => {
			const patch = applyFreshWebSessionTabsSnapshot(state, snapshot, environmentId);
			return patch === state ? state : patch;
		});
	} catch (error) {
		if (options.errorMode === "throw") throw error;
		console.warn("[web-runtime-session] failed to refresh session-tabs snapshot:", error instanceof Error ? error.message : String(error));
	}
}
function scheduleRuntimeWorktreeRecoveryRefresh(environmentId, worktreeId, expectedEnvironmentPairingRevision = getRuntimeEnvironmentRevision(environmentId)) {
	const initialState = useAppStore.getState();
	if (!("tabsByWorktree" in initialState)) return;
	if ((initialState.tabsByWorktree[worktreeId] ?? []).length > 0) return;
	const key = `${environmentId}\0${expectedEnvironmentPairingRevision ?? ""}\0${worktreeId}`;
	const token = Symbol(key);
	pendingRuntimeWorktreeRecoveryRefreshes.set(key, token);
	(async () => {
		try {
			for (const delayMs of RUNTIME_WORKTREE_RECOVERY_REFRESH_DELAYS_MS) {
				await new Promise((resolve) => setTimeout(resolve, delayMs));
				if (pendingRuntimeWorktreeRecoveryRefreshes.get(key) !== token) return;
				if (getRuntimeEnvironmentRevision(environmentId) !== expectedEnvironmentPairingRevision) return;
				await refreshWebRuntimeSessionTabsSnapshot(environmentId, worktreeId, { expectedEnvironmentPairingRevision });
				if ((useAppStore.getState().tabsByWorktree[worktreeId] ?? []).length > 0) return;
			}
		} finally {
			if (pendingRuntimeWorktreeRecoveryRefreshes.get(key) === token) pendingRuntimeWorktreeRecoveryRefreshes.delete(key);
		}
	})();
}
async function activateWebRuntimeSessionWorktree(args) {
	const environmentId = args.environmentId?.trim() ?? useAppStore.getState().settings?.activeRuntimeEnvironmentId?.trim() ?? null;
	if (!environmentId || !isWebRuntimeSessionActive(environmentId)) return false;
	const intentOwner = captureWebSessionIntentOwner(environmentId);
	const callEnvironment = captureRuntimeEnvironmentCall(environmentId, intentOwner.pairingRevision);
	try {
		unwrapRuntimeRpcResult(await callEnvironment({
			method: "worktree.activate",
			params: {
				worktree: toRuntimeWorktreeSelector(args.worktreeId),
				notifyClients: false,
				navigation: "caller"
			},
			timeoutMs: 15e3
		}));
		await refreshWebRuntimeSessionTabsSnapshot(environmentId, args.worktreeId, {
			expectedEnvironmentPairingRevision: intentOwner.pairingRevision,
			acceptCurrentSnapshot: true
		});
		scheduleRuntimeWorktreeRecoveryRefresh(environmentId, args.worktreeId, intentOwner.pairingRevision);
		return true;
	} catch (error) {
		console.warn("[web-runtime-session] failed to activate worktree:", error instanceof Error ? error.message : String(error));
		return false;
	}
}
async function activateWebRuntimeSessionTab(args) {
	return callWebRuntimeSessionTabMethod("session.tabs.activate", args);
}
async function closeWebRuntimeSessionTab(args) {
	return callWebRuntimeSessionTabMethod("session.tabs.close", args);
}
async function moveWebRuntimeSessionTab(args) {
	const environmentId = args.environmentId?.trim() ?? useAppStore.getState().settings?.activeRuntimeEnvironmentId?.trim() ?? null;
	if (!environmentId || !isWebRuntimeSessionActive(environmentId)) return false;
	const intentOwner = captureWebSessionIntentOwner(environmentId);
	const callEnvironment = captureRuntimeEnvironmentCall(environmentId, intentOwner.pairingRevision);
	if (args.kind === "reorder") recordWebSessionReorderIntent(intentOwner, args.worktreeId, args.targetGroupId, args.tabOrder, Date.now());
	try {
		const { resolveHostSessionTabIdForWebSessionTab } = await __vitePreload(async () => {
			const { resolveHostSessionTabIdForWebSessionTab: resolveHostSessionTabIdForWebSessionTab$1 } = await import("./web-session-tabs-sync-DhHVQKaz.js");
			return { resolveHostSessionTabIdForWebSessionTab: resolveHostSessionTabIdForWebSessionTab$1 };
		}, __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22]), import.meta.url);
		const state = useAppStore.getState();
		const resolveHostBackedTabId = (tabId) => resolveHostSessionTabIdForWebSessionTab(state, {
			environmentId,
			worktreeId: args.worktreeId,
			tabId
		}) ?? (isWebTerminalSurfaceTabId(tabId) ? toHostSessionTabId(tabId) : null);
		const toHostTabId = (tabId) => resolveHostBackedTabId(tabId) ?? tabId;
		const movedHostTabId = args.kind === "reorder" ? resolveHostBackedTabId(args.tabId) : toHostTabId(args.tabId);
		if (!movedHostTabId) {
			clearWebSessionReorderIntent(intentOwner, args.worktreeId, args.targetGroupId);
			return false;
		}
		const reorderedHostTabOrder = args.kind === "reorder" ? args.tabOrder.map(resolveHostBackedTabId).filter((tabId) => Boolean(tabId)) : null;
		if (reorderedHostTabOrder && !reorderedHostTabOrder.includes(movedHostTabId)) {
			clearWebSessionReorderIntent(intentOwner, args.worktreeId, args.targetGroupId);
			return false;
		}
		const targetHostIndex = args.kind === "move-to-group" && typeof args.index === "number" ? state.groupsByWorktree?.[args.worktreeId]?.find((group) => group.id === args.targetGroupId)?.tabOrder.slice(0, args.index).map(resolveHostBackedTabId).filter((tabId) => Boolean(tabId)).length ?? args.index : args.kind === "move-to-group" ? args.index : void 0;
		const base = {
			worktree: toRuntimeWorktreeSelector(args.worktreeId),
			tabId: movedHostTabId,
			targetGroupId: args.targetGroupId
		};
		unwrapRuntimeRpcResult(await callEnvironment({
			method: "session.tabs.move",
			params: args.kind === "reorder" ? {
				...base,
				kind: "reorder",
				tabOrder: reorderedHostTabOrder
			} : args.kind === "split" ? {
				...base,
				kind: "split",
				splitDirection: args.splitDirection
			} : {
				...base,
				kind: "move-to-group",
				index: targetHostIndex
			},
			timeoutMs: 15e3
		}));
		return true;
	} catch (error) {
		if (args.kind === "reorder") clearWebSessionReorderIntent(intentOwner, args.worktreeId, args.targetGroupId);
		console.warn("[web-runtime-session] failed to move tab:", error instanceof Error ? error.message : String(error));
		return false;
	}
}
async function callWebRuntimeSessionTabMethod(method, args) {
	const environmentId = args.environmentId?.trim() ?? useAppStore.getState().settings?.activeRuntimeEnvironmentId?.trim() ?? null;
	if (!environmentId || !isWebRuntimeSessionActive(environmentId)) return false;
	const intentOwner = captureWebSessionIntentOwner(environmentId);
	const callEnvironment = captureRuntimeEnvironmentCall(environmentId, intentOwner.pairingRevision);
	const closeIntentTabIds = /* @__PURE__ */ new Set();
	let activationHostTabId = null;
	const isClose = method === "session.tabs.close";
	const isLifecycleClose = isClose && args.reason !== "user";
	if (isLifecycleClose && (!args.publicationEpoch || !args.terminalHandle)) {
		const { acceptReplayedWebSessionTabsSnapshot } = await __vitePreload(async () => {
			const { acceptReplayedWebSessionTabsSnapshot: acceptReplayedWebSessionTabsSnapshot$1 } = await import("./web-session-tabs-sync-DhHVQKaz.js");
			return { acceptReplayedWebSessionTabsSnapshot: acceptReplayedWebSessionTabsSnapshot$1 };
		}, __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22]), import.meta.url);
		acceptReplayedWebSessionTabsSnapshot(environmentId, args.worktreeId);
		await refreshWebRuntimeSessionTabsSnapshot(environmentId, args.worktreeId);
		console.warn("[web-runtime-session] suppressed lifecycle close without incarnation evidence", { closeReason: args.reason });
		return false;
	}
	const immediateHostTabId = toHostSessionTabId(args.tabId);
	if (isClose) {
		closeIntentTabIds.add(immediateHostTabId);
		recordWebSessionCloseIntent(intentOwner, args.worktreeId, immediateHostTabId, Date.now());
	}
	try {
		const { resolveHostSessionTabIdForWebSessionTab } = await __vitePreload(async () => {
			const { resolveHostSessionTabIdForWebSessionTab: resolveHostSessionTabIdForWebSessionTab$1 } = await import("./web-session-tabs-sync-DhHVQKaz.js");
			return { resolveHostSessionTabIdForWebSessionTab: resolveHostSessionTabIdForWebSessionTab$1 };
		}, __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22]), import.meta.url);
		const hostTabId = resolveHostSessionTabIdForWebSessionTab(useAppStore.getState(), {
			environmentId,
			worktreeId: args.worktreeId,
			tabId: args.tabId
		}) ?? toHostSessionTabId(args.tabId);
		if (isClose) {
			closeIntentTabIds.add(hostTabId);
			recordWebSessionCloseIntent(intentOwner, args.worktreeId, hostTabId, Date.now());
		} else {
			activationHostTabId = hostTabId;
			recordWebSessionFocusIntent(intentOwner, args.worktreeId, hostTabId);
		}
		const result = unwrapRuntimeRpcResult(await callEnvironment({
			method: isLifecycleClose ? "session.tabs.closeLifecycle" : method,
			params: {
				worktree: toRuntimeWorktreeSelector(args.worktreeId),
				tabId: hostTabId,
				...method === "session.tabs.activate" ? {
					notifyClients: false,
					navigation: "caller",
					intent: "user"
				} : {},
				...isLifecycleClose ? {
					reason: args.reason,
					publicationEpoch: args.publicationEpoch,
					terminal: args.terminalHandle
				} : isClose ? { reason: args.reason } : {}
			},
			timeoutMs: 15e3
		}));
		if (isClose) {
			if (result?.refused === true && result.snapshotRepublished === true) {
				clearWebSessionCloseIntent(intentOwner, args.worktreeId, immediateHostTabId);
				clearWebSessionCloseIntent(intentOwner, args.worktreeId, hostTabId);
				const { acceptReplayedWebSessionTabsSnapshot } = await __vitePreload(async () => {
					const { acceptReplayedWebSessionTabsSnapshot: acceptReplayedWebSessionTabsSnapshot$1 } = await import("./web-session-tabs-sync-DhHVQKaz.js");
					return { acceptReplayedWebSessionTabsSnapshot: acceptReplayedWebSessionTabsSnapshot$1 };
				}, __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22]), import.meta.url);
				acceptReplayedWebSessionTabsSnapshot(environmentId, args.worktreeId);
			}
			await refreshWebRuntimeSessionTabsSnapshot(environmentId, args.worktreeId, { expectedEnvironmentPairingRevision: intentOwner.pairingRevision });
		}
		return true;
	} catch (error) {
		if (activationHostTabId) clearWebSessionFocusIntentIfMatches(intentOwner, args.worktreeId, activationHostTabId);
		for (const hostTabId of closeIntentTabIds) clearWebSessionCloseIntent(intentOwner, args.worktreeId, hostTabId);
		if (isLifecycleClose) {
			const { acceptReplayedWebSessionTabsSnapshot } = await __vitePreload(async () => {
				const { acceptReplayedWebSessionTabsSnapshot: acceptReplayedWebSessionTabsSnapshot$1 } = await import("./web-session-tabs-sync-DhHVQKaz.js");
				return { acceptReplayedWebSessionTabsSnapshot: acceptReplayedWebSessionTabsSnapshot$1 };
			}, __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22]), import.meta.url);
			acceptReplayedWebSessionTabsSnapshot(environmentId, args.worktreeId);
			await refreshWebRuntimeSessionTabsSnapshot(environmentId, args.worktreeId, { expectedEnvironmentPairingRevision: intentOwner.pairingRevision });
		}
		console.warn(`[web-runtime-session] failed to ${isClose ? "close" : "activate"} tab:`, error instanceof Error ? error.message : String(error));
		return false;
	}
}
function splitWebRuntimeTerminal(ptyId, direction, telemetrySource) {
	if (!ptyId) return false;
	const remote = parseRemoteRuntimePtyId(ptyId);
	const environmentId = remote?.environmentId?.trim();
	if (!remote || !environmentId || !isWebRuntimeSessionActive(environmentId)) return false;
	const releasePendingMirrorSuppression = schedulePendingWebRuntimeSplitMirrorTelemetryRelease(ptyId, direction, reservePendingWebRuntimeSplitMirrorTelemetry(ptyId, direction));
	window.api.runtimeEnvironments.call({
		selector: environmentId,
		method: "terminal.split",
		params: {
			terminal: remote.handle,
			direction,
			telemetrySource
		},
		timeoutMs: 15e3
	}).then((response) => {
		unwrapRuntimeRpcResult(response);
	}).catch((error) => {
		releasePendingMirrorSuppression();
		const message = error instanceof Error ? error.message : String(error);
		toast.error(message);
		console.warn("[web-runtime-session] failed to split terminal:", message);
	});
	return true;
}
function consumePendingWebRuntimeSplitMirrorTelemetry(sourcePtyId, direction) {
	if (!sourcePtyId) return false;
	const key = getPendingWebRuntimeSplitMirrorTelemetryKey(sourcePtyId, direction);
	const ids = pendingWebRuntimeSplitMirrorTelemetry.get(key);
	const id = ids?.values().next().value;
	if (!ids || !id) return false;
	ids.delete(id);
	if (ids.size === 0) pendingWebRuntimeSplitMirrorTelemetry.delete(key);
	return true;
}
function reservePendingWebRuntimeSplitMirrorTelemetry(sourcePtyId, direction) {
	const id = String(++pendingWebRuntimeSplitMirrorTelemetryId);
	const key = getPendingWebRuntimeSplitMirrorTelemetryKey(sourcePtyId, direction);
	const ids = pendingWebRuntimeSplitMirrorTelemetry.get(key) ?? /* @__PURE__ */ new Set();
	ids.add(id);
	pendingWebRuntimeSplitMirrorTelemetry.set(key, ids);
	return id;
}
function schedulePendingWebRuntimeSplitMirrorTelemetryRelease(sourcePtyId, direction, id) {
	let released = false;
	const release = () => {
		if (released) return;
		released = true;
		releasePendingWebRuntimeSplitMirrorTelemetry(sourcePtyId, direction, id);
	};
	const timeout = globalThis.setTimeout(release, WEB_RUNTIME_SPLIT_MIRROR_SUPPRESSION_TTL_MS);
	return () => {
		globalThis.clearTimeout(timeout);
		release();
	};
}
function releasePendingWebRuntimeSplitMirrorTelemetry(sourcePtyId, direction, id) {
	const key = getPendingWebRuntimeSplitMirrorTelemetryKey(sourcePtyId, direction);
	const ids = pendingWebRuntimeSplitMirrorTelemetry.get(key);
	if (!ids) return;
	ids.delete(id);
	if (ids.size === 0) pendingWebRuntimeSplitMirrorTelemetry.delete(key);
}
function getPendingWebRuntimeSplitMirrorTelemetryKey(sourcePtyId, direction) {
	return `${direction}:${sourcePtyId}`;
}
function closeWebRuntimeTerminal(ptyId) {
	if (!ptyId) return false;
	const remote = parseRemoteRuntimePtyId(ptyId);
	const environmentId = remote?.environmentId?.trim();
	if (!remote || !environmentId || !isWebRuntimeSessionActive(environmentId)) return false;
	window.api.runtimeEnvironments.call({
		selector: environmentId,
		method: "terminal.close",
		params: { terminal: remote.handle },
		timeoutMs: 15e3
	}).then((response) => {
		unwrapRuntimeRpcResult(response);
	}).catch((error) => {
		console.warn("[web-runtime-session] failed to close terminal pane:", error instanceof Error ? error.message : String(error));
	});
	return true;
}
async function updateWebRuntimePaneLayout(args) {
	const environmentId = getRuntimeEnvironmentIdForWorktree(useAppStore.getState(), args.worktreeId) ?? null;
	if (!environmentId || !isWebRuntimeSessionActive(environmentId)) return false;
	const callEnvironment = captureRuntimeEnvironmentCall(environmentId);
	const hostTabId = isWebTerminalSurfaceTabId(args.tabId) ? toHostSessionTabId(args.tabId) : args.tabId;
	try {
		unwrapRuntimeRpcResult(await callEnvironment({
			method: "session.tabs.updatePaneLayout",
			params: {
				worktree: toRuntimeWorktreeSelector(args.worktreeId),
				tabId: hostTabId,
				root: args.root,
				expandedLeafId: args.expandedLeafId,
				...args.titlesByLeafId ? { titlesByLeafId: args.titlesByLeafId } : {}
			},
			timeoutMs: 15e3
		}));
		return true;
	} catch (error) {
		console.warn("[web-runtime-session] failed to update pane layout:", error instanceof Error ? error.message : String(error));
		return false;
	}
}
function setWebRuntimeTabProps(args) {
	const environmentId = getRuntimeEnvironmentIdForWorktree(useAppStore.getState(), args.worktreeId) ?? null;
	if (!environmentId || !isWebRuntimeSessionActive(environmentId)) return false;
	const callEnvironment = captureRuntimeEnvironmentCall(environmentId);
	const state = useAppStore.getState();
	__vitePreload(async () => {
		const { resolveHostSessionTabIdForWebSessionTab } = await import("./web-session-tabs-sync-DhHVQKaz.js");
		return { resolveHostSessionTabIdForWebSessionTab };
	}, __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22]), import.meta.url).then(({ resolveHostSessionTabIdForWebSessionTab }) => {
		const hostTabId = resolveHostSessionTabIdForWebSessionTab(state, {
			environmentId,
			worktreeId: args.worktreeId,
			tabId: args.tabId
		}) ?? (isWebTerminalSurfaceTabId(args.tabId) ? toHostSessionTabId(args.tabId) : args.tabId);
		return callEnvironment({
			method: "session.tabs.setTabProps",
			params: {
				worktree: toRuntimeWorktreeSelector(args.worktreeId),
				tabId: hostTabId,
				...args.color !== void 0 ? { color: args.color } : {},
				...args.isPinned !== void 0 ? { isPinned: args.isPinned } : {},
				...args.viewMode !== void 0 ? { viewMode: args.viewMode } : {}
			},
			timeoutMs: 15e3
		});
	}).then((response) => {
		unwrapRuntimeRpcResult(response);
	}).catch((error) => {
		console.warn("[web-runtime-session] failed to set tab props:", error instanceof Error ? error.message : String(error));
	});
	return true;
}
function clearWebRuntimeTerminalBuffer(ptyId) {
	if (!ptyId) return false;
	const remote = parseRemoteRuntimePtyId(ptyId);
	const environmentId = remote?.environmentId?.trim();
	if (!remote || !environmentId || !isWebRuntimeSessionActive(environmentId)) return false;
	window.api.runtimeEnvironments.call({
		selector: environmentId,
		method: "terminal.clearBuffer",
		params: { terminal: remote.handle },
		timeoutMs: 15e3
	}).then((response) => {
		unwrapRuntimeRpcResult(response);
	}).catch((error) => {
		console.warn("[web-runtime-session] failed to clear terminal buffer:", error instanceof Error ? error.message : String(error));
	});
	return true;
}
export { clearWebSessionFocusIntentsForOwner as A, listRemoteRuntimeSessionTabsDeduped as C, clearWebSessionReorderIntentsForWorktree as D, clearWebSessionReorderIntentsForOwner as E, withAgentSessionCreateOperationId as F, resolveWebSessionVisibleTabId as M, createAgentSessionCreateOperation as N, resolveWebSessionReorderedOrder as O, toAgentLaunchPreferences as P, runRemoteAgentSessionLaunch as S, seedNativeChatLaunchDraftForAgentTab as T, suppressE2eWebRuntimeBrowserSnapshot as _, closeWebRuntimeTerminal as a, isWebSessionBrowserPlacementGroupReserved as b, createWebRuntimeAgentSessionTerminalWithLaunchDraft as c, isWebRuntimeSessionActive as d, moveWebRuntimeSessionTab as f, updateWebRuntimePaneLayout as g, splitWebRuntimeTerminal as h, closeWebRuntimeSessionTab as i, peekWebSessionFocusIntent as j, clearWebSessionFocusIntent as k, createWebRuntimeSessionBrowserTab as l, setWebRuntimeTabProps as m, activateWebRuntimeSessionWorktree as n, consumePendingWebRuntimeSplitMirrorTelemetry as o, refreshWebRuntimeSessionTabsSnapshot as p, clearWebRuntimeTerminalBuffer as r, createWebRuntimeAgentSessionTerminal as s, activateWebRuntimeSessionTab as t, createWebRuntimeSessionTerminal as u, clearWebSessionBrowserPlacementsForEnvironment as v, deliverLaunchPromptToAgentTab as w, peekWebSessionBrowserPlacementGroup as x, clearWebSessionBrowserPlacementsForWorktree as y };
