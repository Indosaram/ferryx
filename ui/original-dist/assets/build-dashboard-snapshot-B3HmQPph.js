import { $h as filterEnabledTuiAgents, Eu as getExecutionHostIdForWorktree, Io as getRemoteRuntimePtyEnvironmentId, Jc as parsePaneKey, Kg as isFolderRepo, Qh as TUI_AGENT_AUTO_PICK_ORDER, Uc as branchName, Ut as getConnectionIdFromState, Wl as getHostedReviewCacheKey, dd as parseAppSshPtyId, j as getHostDisplayLabelOverrides, lr as hostedReviewInfoFromGitHubPRInfo, vd as parseWorkspaceKey, wg as DEFAULT_WORKSPACE_STATUSES, xc as folderWorkspaceToWorktree, yg as getWorkspaceStatus, zl as isPositiveHostedReviewNumber } from "./store-CgXrfmaH.js";
import { ct as toRuntimeExecutionHostId, lt as toSshExecutionHostId, mt as isTuiAgent, st as parseExecutionHostId, tt as getWorktreeExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { r as isLocalNativeWindowsConpty } from "./windows-pty-compatibility-XujC9UTf.js";
import { t as migrationUnsupportedToAgentStatusEntry } from "./migration-unsupported-agent-entry-BJ_0rXR-.js";
import { a as lastEnteredDoneAt, c as selectMigrationUnsupportedEntriesForWorktree, d as selectTerminalLayoutsForWorktree, l as selectRetainedAgentEntriesForWorktree, n as applyAgentRowLineage, r as dashboardCardParentPaneKey, s as selectLiveAgentStatusEntriesForWorktree, t as buildWorktreeAgentRows, u as selectRuntimeAgentOrchestrationForWorktree } from "./worktree-agent-rows-C1pW_DbE.js";
import { n as selectRuntimePaneTitlesForWorktree, t as selectLivePtyIdsForWorktree } from "./worktree-card-status-inputs-DozvjAa5.js";
import { t as getAgentRowConversationName } from "./agent-row-conversation-name-DXwI1NP0.js";
import { n as canUseParentPrChecksGitHubPRCacheEntry, r as getParentPrChecksGitHubPRCacheEntry, t as canUseParentPrChecksHostedReviewCacheEntry } from "./parent-pr-checks-hosted-review-cache-Dw8LESfl.js";
import { i as DASHBOARD_MAX_MAP_WORKSPACES, n as DASHBOARD_MAX_LABEL_LENGTH, o as dashboardCardDisplayState, r as DASHBOARD_MAX_LAUNCH_WORKTREES } from "./dashboard-snapshot-B9IiTV8p.js";
import { t as dashboardBucketForDotState } from "./dashboard-card-bucket-DVJn5pXm.js";
import { a as resolveWindowsShiftEnterEncodingForPane, i as hasCtrlEnterCsiUAuthorityForPane, n as shouldDisableKittyKeyboardForTerminal, r as resolveTerminalInputHostPlatform } from "./terminal-keyboard-protocol-De0UZ6qG.js";
var EMPTY_RECORD = {};
function withHydratedSlices(state) {
	return {
		repos: state.repos ?? [],
		worktreesByRepo: state.worktreesByRepo ?? EMPTY_RECORD,
		detectedWorktreesByRepo: state.detectedWorktreesByRepo ?? EMPTY_RECORD,
		folderWorkspaces: state.folderWorkspaces ?? [],
		projectGroups: state.projectGroups ?? [],
		settings: state.settings ?? null,
		sshConnectionStates: state.sshConnectionStates ?? /* @__PURE__ */ new Map(),
		sshStateByEnvironment: state.sshStateByEnvironment ?? /* @__PURE__ */ new Map(),
		runtimeStatusByEnvironmentId: state.runtimeStatusByEnvironmentId ?? /* @__PURE__ */ new Map(),
		restoredRuntimeHostIdByWorkspaceSessionKey: state.restoredRuntimeHostIdByWorkspaceSessionKey ?? EMPTY_RECORD,
		runtimeEnvironments: state.runtimeEnvironments ?? [],
		runtimeEnvironmentCatalogHydrated: state.runtimeEnvironmentCatalogHydrated ?? false,
		removedRuntimeEnvironmentIds: state.removedRuntimeEnvironmentIds ?? /* @__PURE__ */ new Set(),
		paneForegroundAgentByPaneKey: state.paneForegroundAgentByPaneKey ?? EMPTY_RECORD,
		agentLaunchConfigByPaneKey: state.agentLaunchConfigByPaneKey ?? EMPTY_RECORD
	};
}
function resolveDashboardCardTerminalInput(partialState, args) {
	const state = withHydratedSlices(partialState);
	const sshPty = parseAppSshPtyId(args.ptyId);
	const runtimeEnvironmentId = getRemoteRuntimePtyEnvironmentId(args.ptyId);
	const connectionId = sshPty?.connectionId ?? (runtimeEnvironmentId ? null : getConnectionIdFromState(state, args.worktreeId));
	const executionHostId = sshPty ? toSshExecutionHostId(sshPty.connectionId) : runtimeEnvironmentId ? toRuntimeExecutionHostId(runtimeEnvironmentId) : getExecutionHostIdForWorktree(state, args.worktreeId);
	const windowsPtyContext = {
		userAgent: args.userAgent,
		osRelease: args.osRelease,
		connectionId,
		cwd: args.cwd,
		shellOverride: args.shellOverride,
		executionHostId
	};
	return {
		hostPlatform: resolveTerminalInputHostPlatform({
			clientPlatform: args.clientPlatform,
			state,
			worktreeId: args.worktreeId,
			transport: {
				getConnectionId: () => connectionId,
				getPtyId: () => args.ptyId,
				getExecutionHostId: () => executionHostId,
				getLocalSessionMetadata: () => connectionId ? null : {
					...args.cwd ? { cwd: args.cwd } : {},
					...args.shellOverride ? { shellOverride: args.shellOverride } : {}
				}
			}
		}),
		localWindowsConpty: isLocalNativeWindowsConpty(windowsPtyContext),
		...args.osRelease === void 0 ? {} : { osRelease: args.osRelease },
		windowsShiftEnterEncoding: resolveWindowsShiftEnterEncodingForPane(state, args.paneKey),
		ctrlEnterCsiU: hasCtrlEnterCsiUAuthorityForPane(state, args.paneKey),
		kittyKeyboardAdvertised: !shouldDisableKittyKeyboardForTerminal({
			...windowsPtyContext,
			tuiAgent: args.launchAgent ?? null
		})
	};
}
function readDashboardClientHost() {
	const userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent;
	return {
		platform: userAgent.includes("Mac") ? "darwin" : userAgent.includes("Windows") ? "win32" : "linux",
		userAgent,
		osRelease: readClientOsRelease()
	};
}
function readClientOsRelease() {
	try {
		return window.api?.platform?.get?.()?.osRelease;
	} catch {
		return;
	}
}
var EMPTY_RUNTIME_ORCHESTRATION = {};
var EMPTY_TABS_BY_WORKTREE = {};
var EMPTY_AGENT_STATUS = {};
var EMPTY_RETAINED_AGENTS = {};
var EMPTY_BATCH = /* @__PURE__ */ new Map();
const EMPTY_WORKTREE_AGENT_ORCHESTRATION = Object.freeze({});
function createRecord() {
	return Object.create(null);
}
var runtimeDomainCache = null;
var requestedTabMembershipCache = null;
var runtimeBatchCache = null;
function releaseRuntimeAgentOrchestrationBatchCache() {
	runtimeDomainCache = null;
	requestedTabMembershipCache = null;
	runtimeBatchCache = null;
}
function getOrderedRuntimeEntries(runtimeAgentOrchestrationByPaneKey) {
	if (runtimeDomainCache?.source === runtimeAgentOrchestrationByPaneKey) return runtimeDomainCache.orderedEntries;
	const orderedEntries = Object.entries(runtimeAgentOrchestrationByPaneKey);
	runtimeDomainCache = {
		source: runtimeAgentOrchestrationByPaneKey,
		orderedEntries
	};
	return orderedEntries;
}
function uniqueWorktreeIds(worktreeIds) {
	const uniqueIds = [];
	const seen = /* @__PURE__ */ new Set();
	for (const worktreeId of worktreeIds) if (!seen.has(worktreeId)) {
		seen.add(worktreeId);
		uniqueIds.push(worktreeId);
	}
	return uniqueIds;
}
function hasSameWorktreeIds(previous, next) {
	if (previous.length !== next.length) return false;
	return previous.every((worktreeId, index) => worktreeId === next[index]);
}
function getRequestedTabMembership(tabsByWorktree, requestedWorktreeIds) {
	if (requestedTabMembershipCache?.tabsSource === tabsByWorktree && hasSameWorktreeIds(requestedTabMembershipCache.requestedWorktreeIds, requestedWorktreeIds)) return requestedTabMembershipCache;
	const requestedIds = new Set(requestedWorktreeIds);
	const worktreeIdsByTabId = /* @__PURE__ */ new Map();
	for (const worktreeId of requestedWorktreeIds) for (const tab of tabsByWorktree[worktreeId] ?? []) {
		const tabId = tab.id;
		const existing = worktreeIdsByTabId.get(tabId);
		if (existing) existing.add(worktreeId);
		else worktreeIdsByTabId.set(tabId, new Set([worktreeId]));
	}
	requestedTabMembershipCache = {
		tabsSource: tabsByWorktree,
		requestedWorktreeIds,
		requestedIds,
		worktreeIdsByTabId
	};
	return requestedTabMembershipCache;
}
function reuseRecordIfOrderedEqual(previous, next) {
	if (!previous) return next;
	const previousEntries = Object.entries(previous);
	const nextEntries = Object.entries(next);
	if (previousEntries.length !== nextEntries.length) return next;
	for (let index = 0; index < nextEntries.length; index += 1) if (previousEntries[index]?.[0] !== nextEntries[index]?.[0] || previousEntries[index]?.[1] !== nextEntries[index]?.[1]) return next;
	return previous;
}
function buildRuntimeBatch(requestedWorktreeIds, orderedRuntimeEntries, tabsByWorktree, agentStatusByPaneKey, retainedAgentsByPaneKey) {
	const { requestedIds, worktreeIdsByTabId } = getRequestedTabMembership(tabsByWorktree, requestedWorktreeIds);
	const recordsByWorktree = /* @__PURE__ */ new Map();
	for (const [paneKey, orchestration] of orderedRuntimeEntries) {
		const targets = /* @__PURE__ */ new Set();
		const parsed = parsePaneKey(paneKey);
		const parsedParent = orchestration.parentPaneKey ? parsePaneKey(orchestration.parentPaneKey) : null;
		if (parsed) for (const worktreeId of worktreeIdsByTabId.get(parsed.tabId) ?? []) targets.add(worktreeId);
		if (parsedParent) for (const worktreeId of worktreeIdsByTabId.get(parsedParent.tabId) ?? []) targets.add(worktreeId);
		const liveWorktreeId = agentStatusByPaneKey[paneKey]?.worktreeId;
		const retainedWorktreeId = retainedAgentsByPaneKey[paneKey]?.worktreeId;
		if (typeof liveWorktreeId === "string" && requestedIds.has(liveWorktreeId)) targets.add(liveWorktreeId);
		if (typeof retainedWorktreeId === "string" && requestedIds.has(retainedWorktreeId)) targets.add(retainedWorktreeId);
		for (const worktreeId of targets) {
			let record = recordsByWorktree.get(worktreeId);
			if (!record) {
				record = createRecord();
				recordsByWorktree.set(worktreeId, record);
			}
			record[paneKey] = orchestration;
		}
	}
	const previousRecords = runtimeBatchCache?.recordsByWorktree;
	for (const [worktreeId, record] of recordsByWorktree) recordsByWorktree.set(worktreeId, reuseRecordIfOrderedEqual(previousRecords?.get(worktreeId), record));
	return recordsByWorktree;
}
function selectRuntimeAgentOrchestrationBatch(state, worktreeIds) {
	const requestedWorktreeIds = uniqueWorktreeIds(worktreeIds);
	if (requestedWorktreeIds.length === 0) {
		releaseRuntimeAgentOrchestrationBatchCache();
		return EMPTY_BATCH;
	}
	const runtimeAgentOrchestrationByPaneKey = state.runtimeAgentOrchestrationByPaneKey ?? EMPTY_RUNTIME_ORCHESTRATION;
	const orderedRuntimeEntries = getOrderedRuntimeEntries(runtimeAgentOrchestrationByPaneKey);
	if (orderedRuntimeEntries.length === 0) {
		releaseRuntimeAgentOrchestrationBatchCache();
		return EMPTY_BATCH;
	}
	const tabsByWorktree = state.tabsByWorktree ?? EMPTY_TABS_BY_WORKTREE;
	const agentStatusByPaneKey = state.agentStatusByPaneKey ?? EMPTY_AGENT_STATUS;
	const retainedAgentsByPaneKey = state.retainedAgentsByPaneKey ?? EMPTY_RETAINED_AGENTS;
	if (runtimeBatchCache?.runtimeSource === runtimeAgentOrchestrationByPaneKey && runtimeBatchCache.tabsSource === tabsByWorktree && runtimeBatchCache.liveSource === agentStatusByPaneKey && runtimeBatchCache.retainedSource === retainedAgentsByPaneKey && hasSameWorktreeIds(runtimeBatchCache.requestedWorktreeIds, requestedWorktreeIds)) return runtimeBatchCache.recordsByWorktree;
	runtimeBatchCache = {
		runtimeSource: runtimeAgentOrchestrationByPaneKey,
		tabsSource: tabsByWorktree,
		liveSource: agentStatusByPaneKey,
		retainedSource: retainedAgentsByPaneKey,
		requestedWorktreeIds,
		recordsByWorktree: buildRuntimeBatch(requestedWorktreeIds, orderedRuntimeEntries, tabsByWorktree, agentStatusByPaneKey, retainedAgentsByPaneKey)
	};
	return runtimeBatchCache.recordsByWorktree;
}
function hasLinkedReview(worktree) {
	return [
		worktree.linkedPR,
		worktree.linkedGitLabMR,
		worktree.linkedBitbucketPR,
		worktree.linkedAzureDevOpsPR,
		worktree.linkedGiteaPR
	].some(isPositiveHostedReviewNumber);
}
function resolveReview(state, repo, worktree) {
	if (!repo || !state.hostedReviewCache || !state.prCache || repo.kind === "folder") return;
	const branch = branchName(worktree.branch);
	const hostedReviewEntry = state.hostedReviewCache[getHostedReviewCacheKey(repo.path, branch, state.settings, repo.id, repo.connectionId, repo.executionHostId, true)];
	const hostedReview = hostedReviewEntry?.data;
	if (hostedReview && canUseParentPrChecksHostedReviewCacheEntry(worktree, hostedReview, hostedReviewEntry)) return {
		number: hostedReview.number,
		state: hostedReview.state
	};
	const prEntry = getParentPrChecksGitHubPRCacheEntry({
		prCache: state.prCache,
		repo,
		branch,
		settings: state.settings ?? null
	});
	const review = canUseParentPrChecksGitHubPRCacheEntry(worktree, prEntry, hostedReviewEntry) ? hostedReviewInfoFromGitHubPRInfo(prEntry.data) : void 0;
	return review ? {
		number: review.number,
		state: review.state
	} : void 0;
}
function resolveDashboardCardContext(state, repo, worktree) {
	const statuses = state.workspaceStatuses && state.workspaceStatuses.length > 0 ? state.workspaceStatuses : DEFAULT_WORKSPACE_STATUSES;
	const workspaceStatusId = getWorkspaceStatus(worktree, statuses);
	return {
		workspaceStatus: statuses.find((status) => status.id === workspaceStatusId) ?? DEFAULT_WORKSPACE_STATUSES[0],
		review: resolveReview(state, repo, worktree),
		hasReview: hasLinkedReview(worktree)
	};
}
function buildHostLabelLookup(state) {
	const labels = /* @__PURE__ */ new Map();
	for (const [targetId, label] of state.sshTargetLabels ?? []) labels.set(toSshExecutionHostId(targetId), label);
	for (const environment of state.runtimeEnvironments ?? []) labels.set(toRuntimeExecutionHostId(environment.id), environment.name);
	for (const [hostId, label] of getHostDisplayLabelOverrides(state.settings)) labels.set(hostId, label);
	return labels;
}
function remoteHostKind(connectionId, executionHostId) {
	if (connectionId || executionHostId?.startsWith("ssh:")) return "ssh";
	return executionHostId && executionHostId !== "local" ? "remote" : null;
}
function collectActiveDashboardWorkspaces(state, includeMapMetadata = true) {
	const workspaces = [];
	const seenWorkspaceIds = /* @__PURE__ */ new Set();
	let hostLabels = null;
	const resolveHostLabel = (executionHostId) => {
		const parsed = includeMapMetadata ? parseExecutionHostId(executionHostId) : null;
		if (parsed?.kind !== "ssh" && parsed?.kind !== "runtime") return;
		hostLabels ?? (hostLabels = buildHostLabelLookup(state));
		const label = hostLabels.get(executionHostId) ?? (parsed.kind === "ssh" ? parsed.targetId : parsed.environmentId);
		return label.length > 1024 ? label.slice(0, DASHBOARD_MAX_LABEL_LENGTH) : label;
	};
	for (const repo of state.repos ?? []) for (const worktree of state.worktreesByRepo?.[repo.id] ?? []) {
		if (worktree.isArchived) continue;
		seenWorkspaceIds.add(worktree.id);
		const workspaceHostLabel = includeMapMetadata ? resolveHostLabel(getWorktreeExecutionHostId(worktree, repo)) : void 0;
		workspaces.push({
			projectId: repo.id,
			projectName: repo.displayName,
			repo,
			repoIcon: repo.repoIcon ?? null,
			worktree,
			workspaceKind: includeMapMetadata && isFolderRepo(repo) ? "folder" : "worktree",
			remoteHostKind: includeMapMetadata ? remoteHostKind(repo.connectionId, worktree.hostId ?? repo.executionHostId) : null,
			...workspaceHostLabel ? { hostLabel: workspaceHostLabel } : {}
		});
	}
	const projectGroupsById = new Map((state.projectGroups ?? []).map((projectGroup) => [projectGroup.id, projectGroup]));
	for (const folderWorkspace of state.folderWorkspaces ?? []) {
		const worktree = folderWorkspaceToWorktree(folderWorkspace);
		if (folderWorkspace.isArchived || seenWorkspaceIds.has(worktree.id)) continue;
		const projectGroup = projectGroupsById.get(folderWorkspace.projectGroupId);
		const workspaceHostLabel = includeMapMetadata ? resolveHostLabel(getWorktreeExecutionHostId(worktree, void 0)) : void 0;
		workspaces.push({
			projectId: `folder-workspace:${folderWorkspace.projectGroupId}`,
			projectName: projectGroup?.name ?? folderWorkspace.name,
			repo: null,
			repoIcon: null,
			worktree,
			workspaceKind: "folder",
			remoteHostKind: includeMapMetadata ? remoteHostKind(folderWorkspace.connectionId ?? projectGroup?.connectionId, worktree.hostId ?? projectGroup?.executionHostId) : null,
			...workspaceHostLabel ? { hostLabel: workspaceHostLabel } : {}
		});
	}
	return workspaces;
}
function dashboardCardHostKind(workspace, ptyId, terminalInput, clientPlatform) {
	if (workspace.remoteHostKind) return workspace.remoteHostKind;
	if (ptyId && parseAppSshPtyId(ptyId)) return "ssh";
	if (ptyId && getRemoteRuntimePtyEnvironmentId(ptyId)) return "remote";
	return clientPlatform === "win32" && terminalInput?.hostPlatform === "linux" ? "wsl" : "local";
}
function dashboardCardMapWorkspaceMetadata(workspace, ptyId, terminalInput, clientPlatform) {
	return {
		hostKind: dashboardCardHostKind(workspace, ptyId, terminalInput, clientPlatform),
		executionHostId: getWorktreeExecutionHostId(workspace.worktree, workspace.repo ?? void 0),
		workspaceKind: workspace.workspaceKind,
		...workspace.hostLabel ? { hostLabel: workspace.hostLabel } : {}
	};
}
function rowTask(row) {
	return (row.entry.orchestration?.taskTitle ?? "").trim() || (row.entry.prompt ?? "").trim();
}
function nonEmpty(value) {
	const trimmed = (value ?? "").trim();
	return trimmed.length > 0 ? trimmed : void 0;
}
function boundedLabel(value) {
	return value.length > 1024 ? value.slice(0, DASHBOARD_MAX_LABEL_LENGTH) : value;
}
function boundedLabelOrUndefined(value) {
	return value === void 0 ? void 0 : boundedLabel(value);
}
function rowConversationName(row, generatedTitlesEnabled) {
	const parentPaneKey = row.entry.orchestration?.parentPaneKey;
	if (row.lineage?.depth === 1 && parentPaneKey !== void 0 && parsePaneKey(parentPaneKey)?.tabId === row.tab.id) return;
	return getAgentRowConversationName(row.tab, row.agentType, generatedTitlesEnabled) ?? void 0;
}
function buildDashboardLaunchCatalog(state) {
	return {
		foldersById: new Map((state.folderWorkspaces ?? []).map((folder) => [folder.id, folder])),
		groupsById: new Map((state.projectGroups ?? []).map((group) => [group.id, group])),
		reposById: new Map((state.repos ?? []).map((repo) => [repo.id, repo])),
		worktreesByRepoAndId: new Map(Object.entries(state.worktreesByRepo ?? {}).map(([repoId, worktrees]) => [repoId, new Map(worktrees.map((worktree) => [worktree.id, worktree]))]))
	};
}
function detectedAgentsForWorktree(state, worktreeId, repoId, catalog) {
	const workspaceScope = parseWorkspaceKey(worktreeId);
	if (workspaceScope?.type === "folder") {
		const folder = catalog.foldersById.get(workspaceScope.folderWorkspaceId);
		const group = folder ? catalog.groupsById.get(folder.projectGroupId) : void 0;
		const host$1 = parseExecutionHostId(group?.executionHostId);
		if (host$1?.kind === "runtime") return state.runtimeDetectedAgentIds?.[host$1.environmentId] ?? [];
		const connectionId$1 = folder?.connectionId ?? group?.connectionId;
		return connectionId$1 ? state.remoteDetectedAgentIds?.[connectionId$1] ?? [] : [];
	}
	const worktree = catalog.worktreesByRepoAndId.get(repoId)?.get(worktreeId);
	const repo = catalog.reposById.get(worktree?.repoId ?? repoId);
	const host = parseExecutionHostId(worktree?.hostId ?? repo?.executionHostId);
	if (host?.kind === "runtime") return state.runtimeDetectedAgentIds?.[host.environmentId] ?? [];
	const connectionId = host?.kind === "ssh" ? host.targetId : repo?.connectionId;
	return connectionId ? state.remoteDetectedAgentIds?.[connectionId] ?? [] : state.detectedAgentIds ?? [];
}
function buildDashboardWorktreeLaunchOptions(state, cards, workspaces = []) {
	const catalog = buildDashboardLaunchCatalog(state);
	const cardsByWorktreeId = /* @__PURE__ */ new Map();
	const repoIdByWorktreeId = new Map(workspaces.map((workspace) => [workspace.worktreeId, workspace.repoId]));
	for (const card of cards) {
		repoIdByWorktreeId.set(card.worktreeId, card.repoId);
		const existing = cardsByWorktreeId.get(card.worktreeId);
		if (existing) existing.push(card);
		else cardsByWorktreeId.set(card.worktreeId, [card]);
	}
	const result = {};
	for (const [worktreeId, repoId] of repoIdByWorktreeId) {
		if (Object.keys(result).length >= 500) break;
		const worktreeCards = cardsByWorktreeId.get(worktreeId) ?? [];
		const available = new Set(detectedAgentsForWorktree(state, worktreeId, repoId, catalog));
		for (const card of worktreeCards) if (isTuiAgent(card.agentType)) available.add(card.agentType);
		const enabled = filterEnabledTuiAgents(TUI_AGENT_AUTO_PICK_ORDER.filter((agent) => available.has(agent)), state.settings?.disabledTuiAgents);
		const preferred = state.settings?.defaultTuiAgent;
		result[worktreeId] = preferred && preferred !== "blank" && enabled.includes(preferred) ? [preferred, ...enabled.filter((agent) => agent !== preferred)] : enabled;
	}
	return result;
}
function buildDashboardSnapshotFilterOptions(state, activeWorkspaces) {
	return {
		projects: [...new Map(activeWorkspaces.map((workspace) => [workspace.projectId, workspace])).values()].map((workspace) => ({
			id: workspace.projectId,
			label: boundedLabel(workspace.projectName)
		})),
		workspaceStatuses: (state.workspaceStatuses && state.workspaceStatuses.length > 0 ? state.workspaceStatuses : DEFAULT_WORKSPACE_STATUSES).map((status) => ({
			id: status.id,
			label: status.label,
			color: status.color
		}))
	};
}
function buildDashboardSnapshot(state, now, options = {}) {
	const cards = [];
	const workspaces = options.includeCardDetails === false ? void 0 : [];
	const clientHost = readDashboardClientHost();
	const repoIconsByRepoId = {};
	const includeCardDetails = options.includeCardDetails !== false;
	const generatedTitlesEnabled = state.settings?.tabAutoGenerateTitle === true;
	const showIdle = state.settings?.experimentalAgentDashboardShowIdle === true;
	const activeWorktrees = collectActiveDashboardWorkspaces(state, includeCardDetails);
	const filterOptions = options.includeFilterOptions === false ? void 0 : buildDashboardSnapshotFilterOptions(state, activeWorktrees);
	let singletonOrchestration = null;
	let orchestrationByWorktree = null;
	if (activeWorktrees.length >= 2) orchestrationByWorktree = selectRuntimeAgentOrchestrationBatch(state, activeWorktrees.map(({ worktree }) => worktree.id));
	else {
		releaseRuntimeAgentOrchestrationBatchCache();
		if (activeWorktrees.length === 1) singletonOrchestration = selectRuntimeAgentOrchestrationForWorktree(state, activeWorktrees[0].worktree.id);
	}
	for (const workspace of activeWorktrees) {
		const { repo, worktree } = workspace;
		const worktreeId = worktree.id;
		const parentWorktreeId = worktree.parentWorktreeId;
		const liveEntries = selectLiveAgentStatusEntriesForWorktree(state, worktreeId);
		const migrationUnsupported = selectMigrationUnsupportedEntriesForWorktree(state, worktreeId);
		const entries = migrationUnsupported.length > 0 ? [...liveEntries, ...migrationUnsupported.flatMap((unsupported) => {
			const entry = migrationUnsupportedToAgentStatusEntry(unsupported);
			return entry ? [entry] : [];
		})] : liveEntries;
		const terminalLayoutsByTabId = selectTerminalLayoutsForWorktree(state, worktreeId);
		const rows = applyAgentRowLineage(buildWorktreeAgentRows({
			tabs: state.tabsByWorktree[worktreeId] ?? [],
			entries,
			retained: selectRetainedAgentEntriesForWorktree(state, worktreeId),
			runtimePaneTitlesByTabId: selectRuntimePaneTitlesForWorktree(state, worktreeId),
			ptyIdsByTabId: selectLivePtyIdsForWorktree(state, worktreeId),
			terminalLayoutsByTabId,
			runtimeAgentOrchestrationByPaneKey: singletonOrchestration ?? orchestrationByWorktree?.get(worktreeId) ?? EMPTY_WORKTREE_AGENT_ORCHESTRATION,
			now
		}));
		const subagentsByParentPaneKey = includeCardDetails ? /* @__PURE__ */ new Map() : void 0;
		if (subagentsByParentPaneKey) for (const row of rows) {
			if (row.rowSource !== "subagent") continue;
			const parentPaneKey = row.entry.orchestration?.parentPaneKey;
			if (!parentPaneKey) continue;
			const subagent = {
				id: row.paneKey,
				name: nonEmpty(row.entry.orchestration?.displayName) ?? nonEmpty(row.entry.prompt) ?? row.agentType,
				dotState: row.state
			};
			const existing = subagentsByParentPaneKey.get(parentPaneKey);
			if (existing) existing.push(subagent);
			else subagentsByParentPaneKey.set(parentPaneKey, [subagent]);
		}
		const context = includeCardDetails ? resolveDashboardCardContext(state, repo, worktree) : void 0;
		if (workspaces && workspaces.length < 2e3) {
			const hostMetadata = dashboardCardMapWorkspaceMetadata(workspace, null, void 0, clientHost.platform);
			workspaces.push({
				repoId: workspace.projectId,
				worktreeId,
				repoName: boundedLabel(workspace.projectName),
				worktreeName: boundedLabel(worktree.displayName),
				...parentWorktreeId ? { parentWorktreeId } : {},
				...hostMetadata,
				workspaceStatusId: context?.workspaceStatus.id,
				workspaceStatusLabel: context?.workspaceStatus.label,
				workspaceStatusColor: context?.workspaceStatus.color,
				hasReview: context?.hasReview,
				review: context?.review
			});
		}
		for (const row of rows) {
			if (row.rowSource === "subagent") continue;
			const isTitleDerived = row.startedAt === 0;
			const routingPaneKey = row.activationPaneKey ?? row.paneKey;
			const parsed = parsePaneKey(routingPaneKey);
			const tabId = parsed?.tabId ?? row.tab.id;
			const leafId = parsed?.leafId ?? null;
			const layoutPtyId = (leafId ? terminalLayoutsByTabId[tabId]?.ptyIdsByLeafId?.[leafId] : void 0) ?? null;
			const ptyId = layoutPtyId && (state.ptyIdsByTabId?.[tabId] ?? []).includes(layoutPtyId) ? layoutPtyId : null;
			const dotState = row.state;
			const unseen = !isTitleDerived && (state.acknowledgedAgentsByPaneKey?.[row.paneKey] ?? 0) < row.entry.stateStartedAt;
			const bucket = dashboardBucketForDotState(dashboardCardDisplayState({
				dotState,
				unseen
			}));
			const terminalInput = ptyId && includeCardDetails ? resolveDashboardCardTerminalInput(state, {
				ptyId,
				worktreeId,
				paneKey: routingPaneKey,
				cwd: row.tab.startupCwd ?? worktree.path,
				shellOverride: row.tab.shellOverride,
				launchAgent: row.tab.launchAgent,
				clientPlatform: clientHost.platform,
				userAgent: clientHost.userAgent,
				osRelease: clientHost.osRelease
			}) : null;
			const finishedAt = lastEnteredDoneAt(row);
			const hostMetadata = includeCardDetails ? dashboardCardMapWorkspaceMetadata(workspace, ptyId, terminalInput ?? void 0, clientHost.platform) : void 0;
			repoIconsByRepoId[workspace.projectId] = workspace.repoIcon;
			cards.push({
				paneKey: row.paneKey,
				ptyId,
				agentType: row.agentType,
				bucket,
				dotState,
				task: isTitleDerived ? "" : rowTask(row),
				repoId: workspace.projectId,
				worktreeId,
				tabId,
				leafId,
				repoName: boundedLabel(workspace.projectName),
				worktreeName: boundedLabel(worktree.displayName),
				...includeCardDetails ? {
					parentPaneKey: dashboardCardParentPaneKey(row),
					...parentWorktreeId ? { parentWorktreeId } : {},
					...hostMetadata
				} : {},
				workspaceStatusId: context?.workspaceStatus.id,
				workspaceStatusLabel: context?.workspaceStatus.label,
				workspaceStatusColor: context?.workspaceStatus.color,
				hasReview: context ? context.hasReview || context.review !== void 0 : void 0,
				review: context?.review,
				subagents: subagentsByParentPaneKey?.get(row.paneKey),
				lastUserMessage: isTitleDerived ? void 0 : nonEmpty(row.entry.prompt),
				lastAgentMessage: isTitleDerived ? void 0 : nonEmpty(row.entry.lastAssistantMessage),
				startedAt: row.startedAt,
				finishedAt,
				stateChangedAt: row.entry.stateStartedAt || row.startedAt,
				statusUpdatedAt: row.entry.updatedAt,
				unseen,
				askSummary: bucket === "attention" ? row.entry.interactivePrompt ?? void 0 : void 0,
				conversationName: boundedLabelOrUndefined(rowConversationName(row, generatedTitlesEnabled)),
				...terminalInput ? { terminalInput } : {}
			});
		}
	}
	return {
		generatedAt: now,
		cards,
		...workspaces ? { workspaces } : {},
		showIdle,
		filterOptions,
		...includeCardDetails ? { launchableAgentsByWorktreeId: buildDashboardWorktreeLaunchOptions(state, cards, workspaces) } : {},
		repoIconsByRepoId
	};
}
export { buildDashboardSnapshot as t };
