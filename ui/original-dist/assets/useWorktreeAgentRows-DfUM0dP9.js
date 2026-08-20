import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { a as isExplicitAgentStatusFresh, u as AGENT_STATUS_STALE_AFTER_MS } from "./agent-status-3vUKbY6l.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
import { t as installWindowVisibilityInterval } from "./window-visibility-interval-CtnbYoau.js";
import { t as migrationUnsupportedToAgentStatusEntry } from "./migration-unsupported-agent-entry-BJ_0rXR-.js";
import { c as selectMigrationUnsupportedEntriesForWorktree, d as selectTerminalLayoutsForWorktree, l as selectRetainedAgentEntriesForWorktree, n as applyAgentRowLineage, s as selectLiveAgentStatusEntriesForWorktree, t as buildWorktreeAgentRows, u as selectRuntimeAgentOrchestrationForWorktree } from "./worktree-agent-rows-C1pW_DbE.js";
import { n as selectRuntimePaneTitlesForWorktree, t as selectLivePtyIdsForWorktree } from "./worktree-card-status-inputs-DozvjAa5.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var nowClocks = /* @__PURE__ */ new Map();
function createSharedNowClock(intervalMs, deps = {
	now: () => Date.now(),
	setInterval: (callback, ms) => setInterval(callback, ms),
	clearInterval: (handle) => clearInterval(handle)
}) {
	let now = deps.now();
	let stopInterval = null;
	const listeners = /* @__PURE__ */ new Set();
	const tick = () => {
		now = deps.now();
		for (const listener of listeners) listener();
	};
	return {
		getSnapshot: () => now,
		subscribe: (listener) => {
			listeners.add(listener);
			if (!stopInterval) stopInterval = installWindowVisibilityInterval({
				run: tick,
				intervalMs,
				setIntervalFn: deps.setInterval,
				clearIntervalFn: deps.clearInterval
			});
			return () => {
				listeners.delete(listener);
				if (listeners.size === 0 && stopInterval) {
					stopInterval();
					stopInterval = null;
				}
			};
		}
	};
}
function getSharedNowClock(intervalMs) {
	let clock = nowClocks.get(intervalMs);
	if (!clock) {
		clock = createSharedNowClock(intervalMs);
		nowClocks.set(intervalMs, clock);
	}
	return clock;
}
function useNow(intervalMs) {
	const clock = getSharedNowClock(intervalMs);
	return (0, import_react.useSyncExternalStore)(clock.subscribe, clock.getSnapshot, clock.getSnapshot);
}
function buildWorktreeAgentFreshnessSignature(state, worktreeId, now) {
	let signature = "";
	for (const entry of selectLiveAgentStatusEntriesForWorktree(state, worktreeId)) {
		if (entry.state !== "working" && entry.state !== "blocked" && entry.state !== "waiting") continue;
		signature += `${entry.paneKey}\0${isExplicitAgentStatusFresh(entry, now, 18e5) ? "1" : "0"}\0`;
	}
	return signature;
}
function createWorktreeAgentFreshnessSelector(worktreeId, readNow = Date.now) {
	let cachedGlobalEpoch = null;
	let cachedSignature = "";
	return (state) => {
		if (cachedGlobalEpoch === state.agentStatusEpoch) return cachedSignature;
		cachedGlobalEpoch = state.agentStatusEpoch;
		cachedSignature = buildWorktreeAgentFreshnessSignature(state, worktreeId, readNow());
		return cachedSignature;
	};
}
function useWorktreeAgentRows(worktreeId, active = true) {
	const selectAgentFreshness = (0, import_react.useMemo)(() => createWorktreeAgentFreshnessSelector(worktreeId), [worktreeId]);
	const tabs = useAppStore((s) => active ? s.tabsByWorktree[worktreeId] : void 0);
	const liveEntries = useAppStore(useShallow((s) => active ? selectLiveAgentStatusEntriesForWorktree(s, worktreeId) : []));
	const migrationUnsupported = useAppStore(useShallow((s) => active ? selectMigrationUnsupportedEntriesForWorktree(s, worktreeId) : []));
	const retained = useAppStore(useShallow((s) => active ? selectRetainedAgentEntriesForWorktree(s, worktreeId) : []));
	const runtimePaneTitlesByTabId = useAppStore(useShallow((s) => active ? selectRuntimePaneTitlesForWorktree(s, worktreeId) : {}));
	const ptyIdsByTabId = useAppStore(useShallow((s) => active ? selectLivePtyIdsForWorktree(s, worktreeId) : {}));
	const terminalLayoutsByTabId = useAppStore(useShallow((s) => active ? selectTerminalLayoutsForWorktree(s, worktreeId) : {}));
	const runtimeAgentOrchestrationByPaneKey = useAppStore(useShallow((s) => active ? selectRuntimeAgentOrchestrationForWorktree(s, worktreeId) : {}));
	return (0, import_react.useMemo)(() => {
		if (!active) return [];
		const now = Date.now();
		const entries = migrationUnsupported.length > 0 ? [...liveEntries, ...migrationUnsupported.flatMap((unsupported) => {
			const entry = migrationUnsupportedToAgentStatusEntry(unsupported);
			return entry ? [entry] : [];
		})] : liveEntries;
		return applyAgentRowLineage(buildWorktreeAgentRows({
			tabs: tabs ?? [],
			entries,
			retained,
			runtimePaneTitlesByTabId,
			ptyIdsByTabId,
			terminalLayoutsByTabId,
			runtimeAgentOrchestrationByPaneKey,
			now
		}));
	}, [
		active,
		tabs,
		liveEntries,
		migrationUnsupported,
		retained,
		runtimePaneTitlesByTabId,
		ptyIdsByTabId,
		terminalLayoutsByTabId,
		runtimeAgentOrchestrationByPaneKey,
		useAppStore((s) => active ? selectAgentFreshness(s) : "")
	]);
}
export { useNow as n, useWorktreeAgentRows as t };
