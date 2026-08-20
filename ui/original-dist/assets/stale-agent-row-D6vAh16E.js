import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { Kc as makePaneKey, ad as getEnvironmentSshStateGeneration, iu as clampUtf8TextPrefix, ld as sshTargetLabelsEqual, rp as callRuntimeRpc, t as useAppStore, uu as measureUtf8ByteLength } from "./store-CgXrfmaH.js";
import { n as toast } from "./dist-DgqligFk.js";
const SSH_CONNECTION_ERROR_MAX_UTF8_BYTES = 16 * 1024;
var CONNECTION_STATUSES = new Set([
	"disconnected",
	"connecting",
	"auth-failed",
	"deploying-relay",
	"connected",
	"reconnecting",
	"reconnection-failed",
	"error"
]);
function isSshRetainedIdentifier(value) {
	return typeof value === "string" && value.length > 0 && !measureUtf8ByteLength(value, { stopAfterBytes: 1024 }).exceededLimit;
}
function admitSshConnectionState(value, expectedTargetId) {
	if (!value || typeof value !== "object" || !isSshRetainedIdentifier(expectedTargetId)) return null;
	const input = value;
	if (input.targetId !== void 0 && (!isSshRetainedIdentifier(input.targetId) || input.targetId !== expectedTargetId) || typeof input.status !== "string" || !CONNECTION_STATUSES.has(input.status) || !isNonNegativeSafeInteger(input.reconnectAttempt) || input.error !== null && typeof input.error !== "string") return null;
	const error = clampSshConnectionError(input.error);
	const hasProviderEpoch = input.providerEpoch !== void 0 && input.providerEpoch !== null;
	if (hasProviderEpoch !== (input.connectionGeneration !== void 0) || hasProviderEpoch && (!isSshProviderEpoch(input.providerEpoch) || !isNonNegativeSafeInteger(input.connectionGeneration))) return null;
	return {
		targetId: expectedTargetId,
		status: input.status,
		error,
		reconnectAttempt: input.reconnectAttempt,
		providerEpoch: hasProviderEpoch ? input.providerEpoch : null,
		...hasProviderEpoch ? { connectionGeneration: input.connectionGeneration } : {},
		...typeof input.supportsFolderDownload === "boolean" ? { supportsFolderDownload: input.supportsFolderDownload } : {},
		...input.remotePlatform === "linux" || input.remotePlatform === "darwin" || input.remotePlatform === "win32" ? { remotePlatform: input.remotePlatform } : {}
	};
}
function isSshProviderEpoch(value) {
	return typeof value === "string" && value.length > 0 && !measureUtf8ByteLength(value, { stopAfterBytes: 128 }).exceededLimit;
}
function clampSshConnectionError(error) {
	return typeof error === "string" ? clampUtf8TextPrefix(error, SSH_CONNECTION_ERROR_MAX_UTF8_BYTES) : null;
}
function isNonNegativeSafeInteger(value) {
	return Number.isSafeInteger(value) && value >= 0;
}
var SSH_RPC_TIMEOUT_MS = 15e3;
function environmentTarget(environmentId) {
	return {
		kind: "environment",
		environmentId
	};
}
async function fetchEnvironmentSshTargets(environmentId) {
	const { targets } = await callRuntimeRpc(environmentTarget(environmentId), "ssh.listTargetSummaries", void 0, { timeoutMs: SSH_RPC_TIMEOUT_MS });
	if (!Array.isArray(targets)) throw new Error("Remote SSH target metadata is invalid");
	return targets.map((target) => {
		if (typeof target.id !== "string" || typeof target.label !== "string") throw new Error("Remote SSH target metadata is invalid");
		return {
			id: target.id,
			label: target.label
		};
	});
}
async function syncEnvironmentSshTargetMetadata(environmentId, generation) {
	const targets = await fetchEnvironmentSshTargets(environmentId);
	if (generation !== getEnvironmentSshStateGeneration(environmentId)) return [];
	useAppStore.getState().setEnvironmentSshTargetsMetadata(environmentId, targets, generation);
	await syncEnvironmentRemovedSshTargetLabels(environmentId, generation);
	return generation === getEnvironmentSshStateGeneration(environmentId) ? targets : [];
}
async function syncEnvironmentRemovedSshTargetLabels(environmentId, generation) {
	try {
		const { labels } = await callRuntimeRpc(environmentTarget(environmentId), "ssh.listRemovedTargetLabels", void 0, { timeoutMs: SSH_RPC_TIMEOUT_MS });
		useAppStore.getState().setEnvironmentRemovedSshTargetLabels(environmentId, labels, generation);
	} catch {}
}
async function fetchEnvironmentSshConnectionStates(environmentId, targets, generation) {
	for (const target of targets) {
		if (generation !== getEnvironmentSshStateGeneration(environmentId)) return;
		try {
			const { state } = await callRuntimeRpc(environmentTarget(environmentId), "ssh.getState", { targetId: target.id }, { timeoutMs: SSH_RPC_TIMEOUT_MS });
			const admittedState = state ? admitSshConnectionState(state, target.id) : null;
			if (admittedState) useAppStore.getState().setEnvironmentSshConnectionState(environmentId, target.id, admittedState, generation);
		} catch {}
	}
}
var sshRefreshesInFlight = /* @__PURE__ */ new Map();
function mergeSshRefreshKind(current, requested) {
	return current === "full" || requested === "full" ? "full" : "metadata";
}
async function runEnvironmentSshHydration(environmentId) {
	const generation = getEnvironmentSshStateGeneration(environmentId);
	await fetchEnvironmentSshConnectionStates(environmentId, await syncEnvironmentSshTargetMetadata(environmentId, generation), generation);
}
async function runEnvironmentSshTargetMetadataRefresh(environmentId) {
	const generation = getEnvironmentSshStateGeneration(environmentId);
	const bucket = useAppStore.getState().sshStateByEnvironment.get(environmentId);
	const targets = await fetchEnvironmentSshTargets(environmentId);
	if (generation !== getEnvironmentSshStateGeneration(environmentId)) return;
	const metadataChanged = !bucket?.targetsHydrated || !sshTargetLabelsEqual(bucket.targetLabels, targets);
	useAppStore.getState().setEnvironmentSshTargetsMetadata(environmentId, targets, generation);
	const priorTargetIds = new Set(bucket?.targetLabels.keys() ?? []);
	const knownStates = useAppStore.getState().sshStateByEnvironment.get(environmentId)?.connectionStates;
	const needStateRead = targets.filter((target) => !priorTargetIds.has(target.id) || !knownStates?.has(target.id));
	if (!metadataChanged) {
		await fetchEnvironmentSshConnectionStates(environmentId, needStateRead, generation);
		return;
	}
	await syncEnvironmentRemovedSshTargetLabels(environmentId, generation);
	await fetchEnvironmentSshConnectionStates(environmentId, needStateRead, generation);
}
function requestSshRefreshRerun(entry, kind) {
	entry.rerunKind = mergeSshRefreshKind(entry.rerunKind, kind);
}
function startEnvironmentSshRefresh(environmentId, initialKind) {
	const entry = {
		promise: Promise.resolve(),
		generation: getEnvironmentSshStateGeneration(environmentId),
		kind: initialKind,
		rerunKind: null
	};
	entry.promise = (async () => {
		let nextKind = initialKind;
		let lastError = null;
		try {
			while (nextKind) {
				entry.kind = nextKind;
				entry.generation = getEnvironmentSshStateGeneration(environmentId);
				entry.rerunKind = null;
				try {
					await (entry.kind === "full" ? runEnvironmentSshHydration(environmentId) : runEnvironmentSshTargetMetadataRefresh(environmentId));
					lastError = null;
				} catch (error) {
					lastError = error;
					if (entry.rerunKind) entry.rerunKind = mergeSshRefreshKind(entry.rerunKind, entry.kind);
				}
				nextKind = entry.rerunKind;
			}
			if (lastError) throw lastError;
		} finally {
			if (sshRefreshesInFlight.get(environmentId) === entry) sshRefreshesInFlight.delete(environmentId);
		}
	})();
	sshRefreshesInFlight.set(environmentId, entry);
	return entry.promise;
}
async function hydrateRuntimeEnvironmentSshState(environmentId, options = {}) {
	const generation = getEnvironmentSshStateGeneration(environmentId);
	const inFlight = sshRefreshesInFlight.get(environmentId);
	if (inFlight) {
		if (options.force || inFlight.generation !== generation) requestSshRefreshRerun(inFlight, "full");
		return inFlight.promise;
	}
	const bucket = useAppStore.getState().sshStateByEnvironment.get(environmentId);
	if (!options.force && bucket?.targetsHydrated) return;
	return startEnvironmentSshRefresh(environmentId, "full");
}
async function refreshRuntimeEnvironmentSshTargetMetadata(environmentId) {
	const generation = getEnvironmentSshStateGeneration(environmentId);
	const inFlight = sshRefreshesInFlight.get(environmentId);
	if (inFlight) {
		requestSshRefreshRerun(inFlight, inFlight.generation === generation ? "metadata" : "full");
		return inFlight.promise;
	}
	return startEnvironmentSshRefresh(environmentId, useAppStore.getState().sshStateByEnvironment.get(environmentId)?.targetsHydrated ? "metadata" : "full");
}
function applyRuntimeEnvironmentSshStateChanged(environmentId, targetId, state, generation = getEnvironmentSshStateGeneration(environmentId)) {
	if (generation !== getEnvironmentSshStateGeneration(environmentId)) return;
	const admittedState = admitSshConnectionState(state, targetId);
	if (!admittedState) return;
	const store = useAppStore.getState();
	const bucket = store.sshStateByEnvironment.get(environmentId);
	if (bucket?.targetsHydrated && bucket.targetLabels.has(targetId)) {
		store.setEnvironmentSshConnectionState(environmentId, targetId, admittedState, generation);
		return;
	}
	hydrateRuntimeEnvironmentSshState(environmentId, { force: true }).catch(() => {});
}
async function connectRuntimeEnvironmentSshTarget(environmentId, targetId) {
	const generation = getEnvironmentSshStateGeneration(environmentId);
	const { state } = await callRuntimeRpc(environmentTarget(environmentId), "ssh.connect", { targetId }, { timeoutMs: 6e4 });
	const admittedState = state ? admitSshConnectionState(state, targetId) : null;
	if (admittedState) useAppStore.getState().setEnvironmentSshConnectionState(environmentId, targetId, admittedState, generation);
	return admittedState;
}
async function resyncRuntimeEnvironmentSshTargets(environmentId) {
	await hydrateRuntimeEnvironmentSshState(environmentId, { force: true });
}
const SSH_CONNECT_UI_TIMEOUT_MS = 2e4;
const SSH_RECONNECT_UI_TIMEOUT_MS = 18e4;
async function withUiConnectTimeout(promise, timeoutMs = SSH_CONNECT_UI_TIMEOUT_MS) {
	let timer;
	const timeout = new Promise((_, reject) => {
		timer = setTimeout(() => {
			reject(new Error(translate("auto.components.NewWorkspaceComposerCard.connectTimedOut", "Connection timed out. It may still be connecting in the background.")));
		}, timeoutMs);
	});
	try {
		return await Promise.race([promise, timeout]);
	} finally {
		if (timer) clearTimeout(timer);
	}
}
function dismissStaleAgentRowByKey(paneKey) {
	const store = useAppStore.getState();
	const liveExisted = paneKey in store.agentStatusByPaneKey;
	const retainedExisted = paneKey in store.retainedAgentsByPaneKey;
	store.dropAgentStatus(paneKey);
	store.dismissRetainedAgent(paneKey);
	if (liveExisted || retainedExisted) toast.info(translate("auto.components.terminal.pane.stale.agent.row.ad991ece5c", "Agent's pane is no longer available."), { id: translate("auto.components.terminal.pane.stale.agent.row.090d607412", "stale-agent-row-{{value0}}", { value0: paneKey }) });
}
function surfaceStaleAgentRow(tabId, leafId) {
	dismissStaleAgentRowByKey(makePaneKey(tabId, leafId));
}
export { applyRuntimeEnvironmentSshStateChanged as a, refreshRuntimeEnvironmentSshTargetMetadata as c, withUiConnectTimeout as i, resyncRuntimeEnvironmentSshTargets as l, surfaceStaleAgentRow as n, connectRuntimeEnvironmentSshTarget as o, SSH_RECONNECT_UI_TIMEOUT_MS as r, hydrateRuntimeEnvironmentSshState as s, dismissStaleAgentRowByKey as t, admitSshConnectionState as u };
