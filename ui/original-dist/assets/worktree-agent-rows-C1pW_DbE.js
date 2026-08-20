import { Ar as resolveRuntimePaneTitleLeafId, Jc as parsePaneKey, Kc as makePaneKey, oa as agentEntryCompletionAt, qc as parseLegacyNumericPaneKey } from "./store-CgXrfmaH.js";
import { a as isExplicitAgentStatusFresh, u as AGENT_STATUS_STALE_AFTER_MS } from "./agent-status-3vUKbY6l.js";
import { i as resolveCompatibleAgentTypeForOwner } from "./pane-agent-owner-BPfoVAtS.js";
import { n as resolveAgentTypeFromTerminalTitle, t as buildTitleDerivedAgentRows } from "./worktree-title-derived-agent-rows-xbcpjeY8.js";
var liveEntriesFullRebuildCount = 0;
function recordLiveEntriesFullRebuild() {
	liveEntriesFullRebuildCount += 1;
}
function liveEntryWorktreeId(paneKey, entry, tabIdToWorktreeId) {
	const parsed = parsePaneKey(paneKey);
	if (!parsed) return;
	return tabIdToWorktreeId.get(parsed.tabId) ?? (entry.state === "done" ? void 0 : entry.worktreeId);
}
function patchLiveEntriesByWorktree(cache, agentStatusByPaneKey, tabIdToWorktreeId) {
	const previousMap = cache.agentStatusByPaneKey;
	const changed = [];
	let keyCount = 0;
	for (const paneKey in agentStatusByPaneKey) {
		keyCount += 1;
		const entry = agentStatusByPaneKey[paneKey];
		const previous = previousMap[paneKey];
		if (previous === entry) continue;
		if (previous === void 0 || previous.worktreeId !== entry.worktreeId || previous.state === "done" !== (entry.state === "done")) return null;
		changed.push({
			paneKey,
			entry
		});
	}
	if (keyCount !== Object.keys(previousMap).length) return null;
	if (changed.length === 0) return cache.entriesByWorktree;
	const entriesByWorktree = new Map(cache.entriesByWorktree);
	const clonedBuckets = /* @__PURE__ */ new Set();
	for (const { paneKey, entry } of changed) {
		const worktreeId = liveEntryWorktreeId(paneKey, entry, tabIdToWorktreeId);
		if (!worktreeId) continue;
		const bucket = entriesByWorktree.get(worktreeId);
		const index = bucket?.indexOf(previousMap[paneKey]) ?? -1;
		if (!bucket || index < 0) return null;
		const nextBucket = clonedBuckets.has(worktreeId) ? bucket : bucket.slice();
		nextBucket[index] = entry;
		if (!clonedBuckets.has(worktreeId)) {
			clonedBuckets.add(worktreeId);
			entriesByWorktree.set(worktreeId, nextBucket);
		}
	}
	return entriesByWorktree;
}
var EMPTY_SOURCE = {};
const EMPTY_WORKTREE_AGENT_ORCHESTRATION = Object.freeze({});
const EMPTY_WORKTREE_AGENT_ORCHESTRATION_INDEX = /* @__PURE__ */ new Map();
function createRecord() {
	return Object.create(null);
}
var runtimeEntriesCache = null;
var tabMembershipCache = null;
var orchestrationIndexCache = null;
function reuseRecordIfOrderedEqual(previous, next) {
	if (!previous) return next;
	const previousEntries = Object.entries(previous);
	const nextEntries = Object.entries(next);
	if (previousEntries.length !== nextEntries.length) return next;
	for (let index = 0; index < nextEntries.length; index += 1) if (previousEntries[index]?.[0] !== nextEntries[index]?.[0] || previousEntries[index]?.[1] !== nextEntries[index]?.[1]) return next;
	return previous;
}
function getWorktreeIdsByTabId(tabsByWorktree) {
	if (tabMembershipCache?.tabsSource === tabsByWorktree) return tabMembershipCache.worktreeIdsByTabId;
	const worktreeIdsByTabId = /* @__PURE__ */ new Map();
	for (const [worktreeId, tabs] of Object.entries(tabsByWorktree)) for (const tab of tabs ?? []) {
		const tabId = tab.id;
		const existing = worktreeIdsByTabId.get(tabId);
		if (existing) existing.add(worktreeId);
		else worktreeIdsByTabId.set(tabId, new Set([worktreeId]));
	}
	tabMembershipCache = {
		tabsSource: tabsByWorktree,
		worktreeIdsByTabId
	};
	return worktreeIdsByTabId;
}
function buildIndex(runtimeEntries, tabsByWorktree, agentStatusByPaneKey, retainedAgentsByPaneKey) {
	const worktreeIdsByTabId = getWorktreeIdsByTabId(tabsByWorktree);
	const recordsByWorktree = /* @__PURE__ */ new Map();
	for (const [paneKey, orchestration] of runtimeEntries) {
		const parsed = parsePaneKey(paneKey);
		const parsedParent = orchestration.parentPaneKey ? parsePaneKey(orchestration.parentPaneKey) : null;
		const targets = /* @__PURE__ */ new Set();
		if (parsed) for (const worktreeId of worktreeIdsByTabId.get(parsed.tabId) ?? []) targets.add(worktreeId);
		if (parsedParent) for (const worktreeId of worktreeIdsByTabId.get(parsedParent.tabId) ?? []) targets.add(worktreeId);
		const liveWorktreeId = agentStatusByPaneKey[paneKey]?.worktreeId;
		if (typeof liveWorktreeId === "string") targets.add(liveWorktreeId);
		const retainedWorktreeId = retainedAgentsByPaneKey[paneKey]?.worktreeId;
		if (typeof retainedWorktreeId === "string") targets.add(retainedWorktreeId);
		for (const worktreeId of targets) {
			let record = recordsByWorktree.get(worktreeId);
			if (!record) {
				record = createRecord();
				recordsByWorktree.set(worktreeId, record);
			}
			record[paneKey] = orchestration;
		}
	}
	const previousRecords = orchestrationIndexCache?.recordsByWorktree;
	for (const [worktreeId, record] of recordsByWorktree) recordsByWorktree.set(worktreeId, reuseRecordIfOrderedEqual(previousRecords?.get(worktreeId), record));
	return recordsByWorktree;
}
function selectWorktreeAgentOrchestrationIndex(state) {
	const runtimeAgentOrchestrationByPaneKey = state.runtimeAgentOrchestrationByPaneKey ?? EMPTY_SOURCE;
	if (runtimeEntriesCache?.source !== runtimeAgentOrchestrationByPaneKey) runtimeEntriesCache = {
		source: runtimeAgentOrchestrationByPaneKey,
		entries: Object.entries(runtimeAgentOrchestrationByPaneKey)
	};
	const runtimeEntries = runtimeEntriesCache.entries;
	if (runtimeEntries.length === 0) {
		tabMembershipCache = null;
		orchestrationIndexCache = null;
		return EMPTY_WORKTREE_AGENT_ORCHESTRATION_INDEX;
	}
	const tabsByWorktree = state.tabsByWorktree ?? EMPTY_SOURCE;
	const agentStatusByPaneKey = state.agentStatusByPaneKey ?? EMPTY_SOURCE;
	const retainedAgentsByPaneKey = state.retainedAgentsByPaneKey ?? EMPTY_SOURCE;
	if (orchestrationIndexCache?.runtimeSource === runtimeAgentOrchestrationByPaneKey && orchestrationIndexCache.tabsSource === tabsByWorktree && orchestrationIndexCache.liveSource === agentStatusByPaneKey && orchestrationIndexCache.retainedSource === retainedAgentsByPaneKey) return orchestrationIndexCache.recordsByWorktree;
	orchestrationIndexCache = {
		runtimeSource: runtimeAgentOrchestrationByPaneKey,
		tabsSource: tabsByWorktree,
		liveSource: agentStatusByPaneKey,
		retainedSource: retainedAgentsByPaneKey,
		recordsByWorktree: buildIndex(runtimeEntries, tabsByWorktree, agentStatusByPaneKey, retainedAgentsByPaneKey)
	};
	return orchestrationIndexCache.recordsByWorktree;
}
function selectWorktreeAgentOrchestration(state, worktreeId) {
	return selectWorktreeAgentOrchestrationIndex(state).get(worktreeId) ?? EMPTY_WORKTREE_AGENT_ORCHESTRATION;
}
var EMPTY_LIVE_ENTRIES = [];
var EMPTY_MIGRATION_UNSUPPORTED_ENTRIES = [];
var EMPTY_RETAINED = [];
var EMPTY_RECORD = {};
var tabWorktreeIndexCache = null;
var liveEntriesByWorktreeCache = null;
var migrationUnsupportedByWorktreeCache = null;
var retainedEntriesByWorktreeCache = null;
function reuseArrayIfEqual(previous, next) {
	if (!previous || previous.length !== next.length) return next;
	for (let i = 0; i < next.length; i += 1) if (previous[i] !== next[i]) return next;
	return previous;
}
function getTabIdToWorktreeId(tabsByWorktree) {
	if (tabWorktreeIndexCache?.tabsByWorktree === tabsByWorktree) return tabWorktreeIndexCache.tabIdToWorktreeId;
	const tabIdToWorktreeId = /* @__PURE__ */ new Map();
	for (const [worktreeId, tabs] of Object.entries(tabsByWorktree)) for (const tab of tabs) tabIdToWorktreeId.set(tab.id, worktreeId);
	tabWorktreeIndexCache = {
		tabsByWorktree,
		tabIdToWorktreeId
	};
	return tabIdToWorktreeId;
}
function getLiveEntriesByWorktree(state) {
	const agentStatusByPaneKey = state.agentStatusByPaneKey ?? EMPTY_RECORD;
	const tabsByWorktree = state.tabsByWorktree ?? EMPTY_RECORD;
	if (liveEntriesByWorktreeCache?.tabsByWorktree === tabsByWorktree && liveEntriesByWorktreeCache.agentStatusByPaneKey === agentStatusByPaneKey) return liveEntriesByWorktreeCache.entriesByWorktree;
	const tabIdToWorktreeId = getTabIdToWorktreeId(tabsByWorktree);
	if (liveEntriesByWorktreeCache?.tabsByWorktree === tabsByWorktree) {
		const patched = patchLiveEntriesByWorktree(liveEntriesByWorktreeCache, agentStatusByPaneKey, tabIdToWorktreeId);
		if (patched) {
			liveEntriesByWorktreeCache = {
				tabsByWorktree,
				agentStatusByPaneKey,
				entriesByWorktree: patched
			};
			return patched;
		}
	}
	recordLiveEntriesFullRebuild();
	const previous = liveEntriesByWorktreeCache?.entriesByWorktree;
	const entriesByWorktree = /* @__PURE__ */ new Map();
	for (const [paneKey, entry] of Object.entries(agentStatusByPaneKey)) {
		const worktreeId = liveEntryWorktreeId(paneKey, entry, tabIdToWorktreeId);
		if (!worktreeId) continue;
		const bucket = entriesByWorktree.get(worktreeId);
		if (bucket) bucket.push(entry);
		else entriesByWorktree.set(worktreeId, [entry]);
	}
	for (const [worktreeId, entries] of entriesByWorktree) entriesByWorktree.set(worktreeId, reuseArrayIfEqual(previous?.get(worktreeId), entries));
	liveEntriesByWorktreeCache = {
		tabsByWorktree,
		agentStatusByPaneKey,
		entriesByWorktree
	};
	return entriesByWorktree;
}
function getMigrationUnsupportedByWorktree(state) {
	const migrationUnsupportedByPtyId = state.migrationUnsupportedByPtyId ?? EMPTY_RECORD;
	const tabsByWorktree = state.tabsByWorktree ?? EMPTY_RECORD;
	if (migrationUnsupportedByWorktreeCache?.tabsByWorktree === tabsByWorktree && migrationUnsupportedByWorktreeCache.migrationUnsupportedByPtyId === migrationUnsupportedByPtyId) return migrationUnsupportedByWorktreeCache.entriesByWorktree;
	const tabIdToWorktreeId = getTabIdToWorktreeId(tabsByWorktree);
	const previous = migrationUnsupportedByWorktreeCache?.entriesByWorktree;
	const entriesByWorktree = /* @__PURE__ */ new Map();
	for (const unsupported of Object.values(migrationUnsupportedByPtyId)) {
		if (!unsupported.paneKey) continue;
		const parsed = parsePaneKey(unsupported.paneKey);
		const worktreeId = parsed ? tabIdToWorktreeId.get(parsed.tabId) : void 0;
		if (!worktreeId) continue;
		const bucket = entriesByWorktree.get(worktreeId);
		if (bucket) bucket.push(unsupported);
		else entriesByWorktree.set(worktreeId, [unsupported]);
	}
	for (const [worktreeId, entries] of entriesByWorktree) entriesByWorktree.set(worktreeId, reuseArrayIfEqual(previous?.get(worktreeId), entries));
	migrationUnsupportedByWorktreeCache = {
		tabsByWorktree,
		migrationUnsupportedByPtyId,
		entriesByWorktree
	};
	return entriesByWorktree;
}
function getRetainedEntriesByWorktree(state) {
	const retainedAgentsByPaneKey = state.retainedAgentsByPaneKey ?? EMPTY_RECORD;
	if (retainedEntriesByWorktreeCache?.retainedAgentsByPaneKey === retainedAgentsByPaneKey) return retainedEntriesByWorktreeCache.entriesByWorktree;
	const previous = retainedEntriesByWorktreeCache?.entriesByWorktree;
	const entriesByWorktree = /* @__PURE__ */ new Map();
	for (const retained of Object.values(retainedAgentsByPaneKey)) {
		const bucket = entriesByWorktree.get(retained.worktreeId);
		if (bucket) bucket.push(retained);
		else entriesByWorktree.set(retained.worktreeId, [retained]);
	}
	for (const [worktreeId, entries] of entriesByWorktree) entriesByWorktree.set(worktreeId, reuseArrayIfEqual(previous?.get(worktreeId), entries));
	retainedEntriesByWorktreeCache = {
		retainedAgentsByPaneKey,
		entriesByWorktree
	};
	return entriesByWorktree;
}
function selectLiveAgentStatusEntriesForWorktree(state, worktreeId) {
	return getLiveEntriesByWorktree(state).get(worktreeId) ?? EMPTY_LIVE_ENTRIES;
}
function selectMigrationUnsupportedEntriesForWorktree(state, worktreeId) {
	return getMigrationUnsupportedByWorktree(state).get(worktreeId) ?? EMPTY_MIGRATION_UNSUPPORTED_ENTRIES;
}
function selectRetainedAgentEntriesForWorktree(state, worktreeId) {
	return getRetainedEntriesByWorktree(state).get(worktreeId) ?? EMPTY_RETAINED;
}
function selectRuntimeAgentOrchestrationForWorktree(state, worktreeId) {
	return selectWorktreeAgentOrchestration(state, worktreeId);
}
function selectTerminalLayoutsForWorktree(state, worktreeId) {
	const out = {};
	for (const tab of (state.tabsByWorktree ?? EMPTY_RECORD)[worktreeId] ?? []) out[tab.id] = (state.terminalLayoutsByTabId ?? EMPTY_RECORD)[tab.id];
	return out;
}
function lastEnteredDoneAt(agent) {
	if (agent.rowSource === "subagent" && agent.state === "idle") return null;
	const entry = agent.entry;
	const completedAt = agentEntryCompletionAt(entry);
	if (completedAt !== null) return completedAt;
	if (entry.state === "done" && entry.interrupted === true && entry.sessionBoundary !== true) return entry.stateStartedAt;
	for (let i = (entry.stateHistory?.length ?? 0) - 1; i >= 0; i--) if (entry.stateHistory[i].state === "done") return entry.stateHistory[i].startedAt;
	return null;
}
function buildPaneKeyByTerminalHandle(rows) {
	const paneKeyByTerminalHandle = /* @__PURE__ */ new Map();
	for (const row of rows) if (row.entry.terminalHandle && !paneKeyByTerminalHandle.has(row.entry.terminalHandle)) paneKeyByTerminalHandle.set(row.entry.terminalHandle, row.paneKey);
	return paneKeyByTerminalHandle;
}
function resolveAgentRowParentPaneKey(row, rowsByPaneKey, paneKeyByTerminalHandle) {
	const explicitParentPaneKey = row.entry.orchestration?.parentPaneKey;
	if (explicitParentPaneKey && explicitParentPaneKey !== row.paneKey && rowsByPaneKey.has(explicitParentPaneKey)) return explicitParentPaneKey;
	const parentTerminalHandles = [row.entry.orchestration?.parentTerminalHandle, row.entry.orchestration?.coordinatorHandle];
	for (const parentTerminalHandle of parentTerminalHandles) {
		const parentPaneKey = parentTerminalHandle ? paneKeyByTerminalHandle.get(parentTerminalHandle) : void 0;
		if (parentPaneKey && parentPaneKey !== row.paneKey && rowsByPaneKey.has(parentPaneKey)) return parentPaneKey;
	}
}
function buildAgentRowLineageTree(rows) {
	const rowsByPaneKey = /* @__PURE__ */ new Map();
	for (const row of rows) if (!rowsByPaneKey.has(row.paneKey)) rowsByPaneKey.set(row.paneKey, row);
	const paneKeyByTerminalHandle = buildPaneKeyByTerminalHandle(rows);
	const childrenByParentPaneKey = /* @__PURE__ */ new Map();
	const childPaneKeys = /* @__PURE__ */ new Set();
	for (const row of rows) {
		const parentPaneKey = resolveAgentRowParentPaneKey(row, rowsByPaneKey, paneKeyByTerminalHandle);
		if (!parentPaneKey) continue;
		childPaneKeys.add(row.paneKey);
		const siblings = childrenByParentPaneKey.get(parentPaneKey);
		if (siblings) siblings.push(row);
		else childrenByParentPaneKey.set(parentPaneKey, [row]);
	}
	const rootRows = rows.filter((row) => !childPaneKeys.has(row.paneKey));
	if (rootRows.length === 0 && rows.length > 0) return {
		rootRows: [...rows],
		childrenByParentPaneKey: /* @__PURE__ */ new Map(),
		childPaneKeys: /* @__PURE__ */ new Set()
	};
	const reachablePaneKeys = /* @__PURE__ */ new Set();
	const markReachable = (row, ancestorPaneKeys = /* @__PURE__ */ new Set()) => {
		if (reachablePaneKeys.has(row.paneKey) || ancestorPaneKeys.has(row.paneKey)) return;
		reachablePaneKeys.add(row.paneKey);
		const descendantAncestorPaneKeys = new Set(ancestorPaneKeys);
		descendantAncestorPaneKeys.add(row.paneKey);
		for (const childRow of childrenByParentPaneKey.get(row.paneKey) ?? []) markReachable(childRow, descendantAncestorPaneKeys);
	};
	for (const rootRow of rootRows) markReachable(rootRow);
	const unreachableRows = rows.filter((row) => !reachablePaneKeys.has(row.paneKey));
	if (unreachableRows.length === 0) return {
		rootRows,
		childrenByParentPaneKey,
		childPaneKeys
	};
	const normalizedChildrenByParentPaneKey = new Map(childrenByParentPaneKey);
	const normalizedChildPaneKeys = new Set(childPaneKeys);
	for (const row of unreachableRows) {
		if (!rootRows.some((rootRow) => rootRow.paneKey === row.paneKey)) rootRows.push(row);
		normalizedChildPaneKeys.delete(row.paneKey);
		normalizedChildrenByParentPaneKey.delete(row.paneKey);
		for (const [parentPaneKey, siblings] of normalizedChildrenByParentPaneKey) {
			const visibleSiblings = siblings.filter((sibling) => sibling.paneKey !== row.paneKey);
			if (visibleSiblings.length === 0) normalizedChildrenByParentPaneKey.delete(parentPaneKey);
			else if (visibleSiblings.length !== siblings.length) normalizedChildrenByParentPaneKey.set(parentPaneKey, visibleSiblings);
		}
	}
	return {
		rootRows,
		childrenByParentPaneKey: normalizedChildrenByParentPaneKey,
		childPaneKeys: normalizedChildPaneKeys
	};
}
var ROOT_LINEAGE = {
	depth: 0,
	isFirstSibling: true,
	isLastSibling: true,
	childCount: 0
};
function applyAgentRowLineage(rows) {
	if (rows.length <= 1) return rows.map((row) => ({
		...row,
		lineage: ROOT_LINEAGE
	}));
	const { rootRows, childrenByParentPaneKey, childPaneKeys } = buildAgentRowLineageTree(rows);
	if (childPaneKeys.size === 0) return rows.map((row) => ({
		...row,
		lineage: ROOT_LINEAGE
	}));
	const ordered = [];
	const emitted = /* @__PURE__ */ new Set();
	const emitRow = (row, lineage) => {
		if (emitted.has(row.paneKey)) return false;
		emitted.add(row.paneKey);
		ordered.push({
			...row,
			lineage
		});
		return true;
	};
	const emitSubtree = (row, lineage) => {
		const children = childrenByParentPaneKey.get(row.paneKey) ?? [];
		if (!emitRow(row, {
			...lineage,
			childCount: children.length
		})) return;
		children.forEach((child, index) => {
			emitSubtree(child, {
				depth: 1,
				parentPaneKey: row.paneKey,
				isFirstSibling: index === 0,
				isLastSibling: index === children.length - 1,
				childCount: 0
			});
		});
	};
	for (const row of rootRows) emitSubtree(row, ROOT_LINEAGE);
	for (const row of rows) emitRow(row, ROOT_LINEAGE);
	return ordered;
}
function dashboardCardParentPaneKey(row) {
	const directParentPaneKey = row.entry.orchestration?.parentPaneKey;
	return row.lineage.parentPaneKey ?? (directParentPaneKey === row.paneKey ? void 0 : directParentPaneKey);
}
function subagentRowKey(parentPaneKey, subagentId) {
	return `${parentPaneKey}\u0000subagent:${subagentId}`;
}
function buildSubagentChildRows(args) {
	const subagents = args.parentEntry.subagents;
	if (!subagents || subagents.length === 0) return [];
	return subagents.map((subagent) => {
		const activeState = args.parentIsFresh && subagent.state !== "idle" ? subagent.state : void 0;
		const state = activeState ?? "idle";
		const startedAt = subagent.startedAt > 0 ? subagent.startedAt : args.parentEntry.stateStartedAt;
		const paneKey = subagentRowKey(args.parentEntry.paneKey, subagent.id);
		return {
			paneKey,
			entry: {
				state: activeState ?? "done",
				prompt: subagent.description ?? subagent.agentType ?? "",
				updatedAt: args.parentEntry.updatedAt,
				stateStartedAt: startedAt,
				agentType: subagent.agentType,
				model: subagent.model,
				paneKey,
				worktreeId: args.parentEntry.worktreeId,
				tabId: args.parentEntry.tabId,
				stateHistory: [],
				orchestration: {
					taskId: `subagent:${subagent.id}`,
					dispatchId: `subagent:${subagent.id}`,
					displayName: subagent.description,
					parentPaneKey: args.parentEntry.paneKey
				}
			},
			tab: args.tab,
			agentType: subagent.agentType ?? "unknown",
			rowSource: "subagent",
			state,
			activationPaneKey: args.parentEntry.paneKey,
			startedAt
		};
	});
}
function comparableNumber(value, fallback = 0) {
	return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}
function comparePaneKeysOrdinal(a, b) {
	if (a < b) return -1;
	if (a > b) return 1;
	return 0;
}
function compareWorktreeAgentRows(a, b) {
	return comparableNumber(a.startedAt) - comparableNumber(b.startedAt) || comparableNumber(a.tab.sortOrder) - comparableNumber(b.tab.sortOrder) || comparableNumber(a.tab.createdAt) - comparableNumber(b.tab.createdAt) || comparePaneKeysOrdinal(a.paneKey, b.paneKey);
}
function effectiveWorktreeAgentRowStartedAt(entry) {
	return entry.stateHistory[0]?.startedAt ?? entry.stateStartedAt;
}
function tabFromWorktreeAttributedStatusEntry(entry, effectiveStartedAt) {
	const parsed = parsePaneKey(entry.paneKey);
	if (!parsed || !entry.worktreeId) return null;
	return {
		id: parsed.tabId,
		ptyId: null,
		worktreeId: entry.worktreeId,
		title: entry.terminalTitle ?? "Agent",
		customTitle: null,
		color: null,
		sortOrder: Number.MAX_SAFE_INTEGER,
		createdAt: effectiveStartedAt
	};
}
function resolveRowAgentType(entry, tab) {
	const entryAgentType = resolveCompatibleAgentTypeForOwner(entry.agentType, tab?.launchAgent);
	if (entryAgentType && entryAgentType !== "unknown") return entryAgentType;
	return resolveAgentTypeFromTerminalTitle(entry.terminalTitle ?? tab?.title, tab?.launchAgent) ?? tab?.launchAgent ?? entryAgentType ?? "unknown";
}
function orchestrationContextsEqual(a, b) {
	return a.taskId === b.taskId && a.dispatchId === b.dispatchId && a.taskTitle === b.taskTitle && a.displayName === b.displayName && a.parentTerminalHandle === b.parentTerminalHandle && a.parentPaneKey === b.parentPaneKey && a.coordinatorHandle === b.coordinatorHandle && a.orchestrationRunId === b.orchestrationRunId;
}
function entryWithRuntimeOrchestration(entry, runtimeAgentOrchestrationByPaneKey) {
	const runtimeOrchestration = runtimeAgentOrchestrationByPaneKey?.[entry.paneKey];
	const sameDispatch = entry.orchestration && runtimeOrchestration && entry.orchestration.taskId === runtimeOrchestration.taskId && entry.orchestration.dispatchId === runtimeOrchestration.dispatchId;
	if (entry.orchestration && runtimeOrchestration && !sameDispatch) return entry;
	const orchestration = sameDispatch && entry.orchestration && runtimeOrchestration ? {
		...entry.orchestration,
		...runtimeOrchestration
	} : runtimeOrchestration ?? entry.orchestration;
	if (!orchestration || orchestration === entry.orchestration) return entry;
	if (entry.orchestration && orchestrationContextsEqual(entry.orchestration, orchestration)) return entry;
	return {
		...entry,
		orchestration
	};
}
function countTerminalLayoutLeaves(node) {
	if (!node) return 0;
	if (node.type === "leaf") return 1;
	return countTerminalLayoutLeaves(node.first) + countTerminalLayoutLeaves(node.second);
}
function seenStablePaneKeysForTab(seenPaneKeys, tabId) {
	const keys = [];
	for (const paneKey of seenPaneKeys) if (parsePaneKey(paneKey)?.tabId === tabId) keys.push(paneKey);
	return keys;
}
function isRetainedLegacyAliasOfSeenStablePane(args) {
	const legacy = parseLegacyNumericPaneKey(args.paneKey);
	if (!legacy) return false;
	const stablePaneKeys = seenStablePaneKeysForTab(args.seenPaneKeys, legacy.tabId);
	if (stablePaneKeys.length === 0) return false;
	const layout = args.terminalLayoutsByTabId?.[legacy.tabId];
	const leafId = resolveRuntimePaneTitleLeafId(layout, legacy.numericPaneId);
	if (leafId) return args.seenPaneKeys.has(makePaneKey(legacy.tabId, leafId));
	return countTerminalLayoutLeaves(layout?.root) === 1 && stablePaneKeys.length === 1;
}
function markSeenPaneKeyForCurrentTab(args) {
	if (!args.paneKey) return;
	const parsed = parsePaneKey(args.paneKey);
	if (parsed) {
		if (args.currentTabIds.has(parsed.tabId)) args.seenPaneKeys.add(args.paneKey);
		return;
	}
	const legacy = parseLegacyNumericPaneKey(args.paneKey);
	if (!legacy || !args.currentTabIds.has(legacy.tabId)) return;
	args.seenPaneKeys.add(args.paneKey);
	const leafId = resolveRuntimePaneTitleLeafId(args.terminalLayoutsByTabId?.[legacy.tabId], legacy.numericPaneId);
	if (leafId) args.seenPaneKeys.add(makePaneKey(legacy.tabId, leafId));
}
function markCompletedWorkerParentPaneKeysSeen(args) {
	const markEntry = (entry) => {
		const rowEntry = entryWithRuntimeOrchestration(entry, args.runtimeAgentOrchestrationByPaneKey);
		if (rowEntry.state !== "done") return;
		markSeenPaneKeyForCurrentTab({
			paneKey: rowEntry.orchestration?.parentPaneKey,
			currentTabIds: args.currentTabIds,
			terminalLayoutsByTabId: args.terminalLayoutsByTabId,
			seenPaneKeys: args.seenPaneKeys
		});
	};
	for (const entry of args.entries) markEntry(entry);
	for (const retained of args.retained) markEntry(retained.entry);
}
function buildWorktreeAgentRows(args) {
	const rows = [];
	const seenPaneKeys = /* @__PURE__ */ new Set();
	const currentTabIds = new Set(args.tabs.map((tab) => tab.id));
	const entriesByTabId = /* @__PURE__ */ new Map();
	for (const entry of args.entries) {
		const parsed = parsePaneKey(entry.paneKey);
		if (!parsed) continue;
		const bucket = entriesByTabId.get(parsed.tabId);
		if (bucket) bucket.push(entry);
		else entriesByTabId.set(parsed.tabId, [entry]);
	}
	for (const tab of args.tabs) {
		const explicitEntries = entriesByTabId.get(tab.id) ?? [];
		for (const entry of explicitEntries) {
			const rowEntry = entryWithRuntimeOrchestration(entry, args.runtimeAgentOrchestrationByPaneKey);
			const isFresh = isExplicitAgentStatusFresh(rowEntry, args.now, AGENT_STATUS_STALE_AFTER_MS);
			const shouldDecay = !isFresh && (rowEntry.state === "working" || rowEntry.state === "blocked" || rowEntry.state === "waiting");
			const startedAt = effectiveWorktreeAgentRowStartedAt(rowEntry);
			rows.push({
				paneKey: rowEntry.paneKey,
				entry: rowEntry,
				tab,
				agentType: resolveRowAgentType(rowEntry, tab),
				rowSource: "live",
				state: shouldDecay ? "idle" : rowEntry.state,
				startedAt
			});
			rows.push(...buildSubagentChildRows({
				parentEntry: rowEntry,
				tab,
				parentIsFresh: isFresh
			}));
			seenPaneKeys.add(rowEntry.paneKey);
		}
	}
	markCompletedWorkerParentPaneKeysSeen({
		entries: args.entries,
		retained: args.retained,
		runtimeAgentOrchestrationByPaneKey: args.runtimeAgentOrchestrationByPaneKey,
		terminalLayoutsByTabId: args.terminalLayoutsByTabId,
		currentTabIds,
		seenPaneKeys
	});
	rows.push(...buildTitleDerivedAgentRows({
		...args,
		seenPaneKeys
	}));
	for (const entry of args.entries) {
		if (seenPaneKeys.has(entry.paneKey)) continue;
		const rowEntry = entryWithRuntimeOrchestration(entry, args.runtimeAgentOrchestrationByPaneKey);
		const startedAt = effectiveWorktreeAgentRowStartedAt(rowEntry);
		const tab = tabFromWorktreeAttributedStatusEntry(rowEntry, startedAt);
		if (!tab) continue;
		const isFresh = isExplicitAgentStatusFresh(rowEntry, args.now, AGENT_STATUS_STALE_AFTER_MS);
		const shouldDecay = !isFresh && (rowEntry.state === "working" || rowEntry.state === "blocked" || rowEntry.state === "waiting");
		rows.push({
			paneKey: rowEntry.paneKey,
			entry: rowEntry,
			tab,
			agentType: resolveRowAgentType(rowEntry, tab),
			rowSource: "live",
			state: shouldDecay ? "idle" : rowEntry.state,
			startedAt
		});
		rows.push(...buildSubagentChildRows({
			parentEntry: rowEntry,
			tab,
			parentIsFresh: isFresh
		}));
		seenPaneKeys.add(rowEntry.paneKey);
	}
	for (const ra of args.retained) {
		if (seenPaneKeys.has(ra.entry.paneKey)) continue;
		if (isRetainedLegacyAliasOfSeenStablePane({
			paneKey: ra.entry.paneKey,
			terminalLayoutsByTabId: args.terminalLayoutsByTabId,
			seenPaneKeys
		})) continue;
		const rowEntry = entryWithRuntimeOrchestration(ra.entry, args.runtimeAgentOrchestrationByPaneKey);
		rows.push({
			paneKey: rowEntry.paneKey,
			entry: rowEntry,
			tab: ra.tab,
			agentType: resolveRowAgentType(rowEntry, ra.tab),
			rowSource: "retained",
			state: "done",
			startedAt: ra.startedAt
		});
	}
	rows.sort(compareWorktreeAgentRows);
	return rows;
}
export { lastEnteredDoneAt as a, selectMigrationUnsupportedEntriesForWorktree as c, selectTerminalLayoutsForWorktree as d, buildAgentRowLineageTree as i, selectRetainedAgentEntriesForWorktree as l, applyAgentRowLineage as n, reuseArrayIfEqual as o, dashboardCardParentPaneKey as r, selectLiveAgentStatusEntriesForWorktree as s, buildWorktreeAgentRows as t, selectRuntimeAgentOrchestrationForWorktree as u };
