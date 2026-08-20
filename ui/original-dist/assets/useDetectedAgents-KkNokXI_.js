import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function normalizeAgentDetectionTarget(target) {
	if (target === void 0) return;
	if (target === null) return { kind: "local" };
	if (typeof target === "string") return {
		kind: "ssh",
		connectionId: target
	};
	return target;
}
function useDetectedAgents(connectionId) {
	const target = normalizeAgentDetectionTarget(connectionId);
	const observedRemoteTargetKeysRef = (0, import_react.useRef)(/* @__PURE__ */ new Set());
	const isUnknown = target === void 0;
	const targetKind = target?.kind;
	const targetId = target?.kind === "ssh" ? target.connectionId : target?.kind === "runtime" ? target.environmentId : null;
	const localWorktreeId = target?.kind === "local" ? target.worktreeId : void 0;
	const localContextKey = target?.kind === "local" ? target.contextKey : void 0;
	const remoteTargetKey = targetKind === "ssh" && targetId ? `ssh:${targetId}` : targetKind === "runtime" && targetId ? `runtime:${targetId}` : null;
	const detectedIds = useAppStore((s) => {
		if (isUnknown) return null;
		if (targetKind === "ssh" && targetId) return s.remoteDetectedAgentIds[targetId] ?? null;
		if (targetKind === "runtime" && targetId) return s.runtimeDetectedAgentIds[targetId] ?? null;
		return localContextKey ? s.localDetectedAgentIdsByContext[localContextKey] ?? null : s.detectedAgentIds;
	});
	const isLoading = useAppStore((s) => {
		if (isUnknown) return true;
		if (targetKind === "ssh" && targetId) return s.isDetectingRemoteAgents[targetId] ?? false;
		if (targetKind === "runtime" && targetId) return s.isDetectingRuntimeAgents[targetId] ?? false;
		return localContextKey ? s.isDetectingLocalAgentsByContext[localContextKey] ?? false : s.isDetectingAgents;
	});
	const isRefreshing = useAppStore((s) => {
		if (targetKind === "runtime" && targetId) return s.isRefreshingRuntimeAgents[targetId] ?? false;
		if (targetKind === "ssh" && targetId) return s.isDetectingRemoteAgents[targetId] ?? false;
		if (targetKind !== "local") return false;
		return localContextKey ? s.isRefreshingLocalAgentsByContext[localContextKey] ?? false : s.isRefreshingAgents;
	});
	const detectionFailed = detectedIds === null && !isLoading && !isRefreshing && remoteTargetKey !== null && observedRemoteTargetKeysRef.current.has(remoteTargetKey);
	const refresh = (0, import_react.useCallback)(() => {
		if (isUnknown) return Promise.resolve([]);
		const state = useAppStore.getState();
		if (targetKind === "runtime" && targetId) return state.refreshRuntimeDetectedAgents(targetId);
		if (targetKind === "ssh" && targetId) return state.refreshRemoteDetectedAgents(targetId);
		return state.refreshDetectedAgents(localWorktreeId);
	}, [
		isUnknown,
		localWorktreeId,
		targetKind,
		targetId
	]);
	(0, import_react.useEffect)(() => {
		if (isUnknown) return;
		const isNewRemoteTarget = remoteTargetKey !== null && !observedRemoteTargetKeysRef.current.has(remoteTargetKey);
		if (remoteTargetKey !== null) observedRemoteTargetKeysRef.current.add(remoteTargetKey);
		const state = useAppStore.getState();
		if (targetKind === "ssh" && targetId) {
			if (detectedIds === null) state.ensureRemoteDetectedAgents(targetId);
			else if (detectedIds.length === 0 && isNewRemoteTarget) state.ensureRemoteDetectedAgents(targetId);
		} else if (targetKind === "runtime" && targetId) {
			if (detectedIds === null) state.ensureRuntimeDetectedAgents(targetId);
			else if (detectedIds.length === 0 && isNewRemoteTarget) state.ensureRuntimeDetectedAgents(targetId);
		} else if (detectedIds === null) state.ensureDetectedAgents(localWorktreeId);
	}, [
		isUnknown,
		targetKind,
		targetId,
		remoteTargetKey,
		detectedIds,
		localWorktreeId,
		localContextKey
	]);
	return {
		detectedIds,
		isLoading,
		detectionFailed,
		isRefreshing,
		refresh
	};
}
export { useDetectedAgents as t };
