import { Cm as projectHostSetupProjectionFromRepos, Gt as getIndexedRepoMap, Pm as FLOATING_TERMINAL_WORKTREE_ID, Wt as getIndexedAllWorktrees, qt as getIndexedWorktreeMap, t as useAppStore, ym as getProjectIdentityKey } from "./store-CgXrfmaH.js";
import { $ as getRepoExecutionHostId, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
function normalizeHydratedProjectHostSetupProjection(repos, projects, setups, derived) {
	const repoById = new Map(repos.map((repo) => [repo.id, repo]));
	const derivedProjectIds = new Set(derived.projects.map((project) => project.id));
	const projectIdByHydratedProjectId = /* @__PURE__ */ new Map();
	let changed = false;
	const normalizedSetups = setups.map((setup) => {
		const repo = repoById.get(setup.repoId) ?? repoById.get(setup.id);
		if (!repo) return setup;
		const projectId = getProjectIdentityKey(repo);
		if (projectId === setup.projectId || projectId === `repo:${repo.id}`) return setup;
		changed = true;
		projectIdByHydratedProjectId.set(setup.projectId, projectId);
		return {
			...setup,
			projectId
		};
	});
	return {
		projects: projects.flatMap((project) => {
			const projectId = projectIdByHydratedProjectId.get(project.id);
			if (!projectId || projectId === project.id) return [project];
			if (derivedProjectIds.has(projectId)) {
				changed = true;
				return [];
			}
			changed = true;
			return [{
				...project,
				id: projectId
			}];
		}),
		setups: normalizedSetups,
		changed
	};
}
var projectHostSetupProjectionCache = /* @__PURE__ */ new WeakMap();
var providedProjectHostSetupProjectionCache = /* @__PURE__ */ new WeakMap();
var mergedProjectHostSetupProjectionCache = /* @__PURE__ */ new WeakMap();
var normalizedProjectHostSetupProjectionCache = /* @__PURE__ */ new WeakMap();
function getCachedProjectHostSetupProjection(repos) {
	const cachedProjection = projectHostSetupProjectionCache.get(repos);
	if (cachedProjection) return cachedProjection;
	const projection = projectHostSetupProjectionFromRepos(repos);
	projectHostSetupProjectionCache.set(repos, projection);
	return projection;
}
function getCachedProvidedProjectHostSetupProjection(projects, setups) {
	const cachedBySetups = providedProjectHostSetupProjectionCache.get(projects);
	const cachedProjection = cachedBySetups?.get(setups);
	if (cachedProjection) return cachedProjection;
	const projection = {
		projects,
		setups
	};
	const nextCachedBySetups = cachedBySetups ?? /* @__PURE__ */ new WeakMap();
	nextCachedBySetups.set(setups, projection);
	if (!cachedBySetups) providedProjectHostSetupProjectionCache.set(projects, nextCachedBySetups);
	return projection;
}
function mergeById(base, overlay) {
	const merged = [...base];
	const indexById = new Map(merged.map((entry, index) => [entry.id, index]));
	for (const entry of overlay) {
		const index = indexById.get(entry.id);
		if (index === void 0) {
			indexById.set(entry.id, merged.length);
			merged.push(entry);
		} else merged[index] = entry;
	}
	return merged;
}
function mergeProjectHostSetupProjection(repos, projects, setups) {
	const cachedByProjects = mergedProjectHostSetupProjectionCache.get(repos);
	const cachedBySetups = cachedByProjects?.get(projects);
	const cachedProjection = cachedBySetups?.get(setups);
	if (cachedProjection) return cachedProjection;
	const derived = getCachedProjectHostSetupProjection(repos);
	const normalized = normalizeHydratedProjectHostSetupProjection(repos, projects, setups, derived);
	const projection = {
		projects: mergeById(derived.projects, normalized.projects),
		setups: mergeById(derived.setups, normalized.setups)
	};
	const nextCachedByProjects = cachedByProjects ?? /* @__PURE__ */ new WeakMap();
	const nextCachedBySetups = cachedBySetups ?? /* @__PURE__ */ new WeakMap();
	nextCachedBySetups.set(setups, projection);
	if (!cachedBySetups) nextCachedByProjects.set(projects, nextCachedBySetups);
	if (!cachedByProjects) mergedProjectHostSetupProjectionCache.set(repos, nextCachedByProjects);
	return projection;
}
function getCachedNormalizedProjectHostSetupProjection(repos, projects, setups, derived, normalized) {
	const cachedByProjects = normalizedProjectHostSetupProjectionCache.get(repos);
	const cachedBySetups = cachedByProjects?.get(projects);
	const cachedProjection = cachedBySetups?.get(setups);
	if (cachedProjection) return cachedProjection;
	const projection = {
		projects: mergeById(derived.projects, normalized.projects),
		setups: mergeById(derived.setups, normalized.setups)
	};
	const nextCachedByProjects = cachedByProjects ?? /* @__PURE__ */ new WeakMap();
	const nextCachedBySetups = cachedBySetups ?? /* @__PURE__ */ new WeakMap();
	nextCachedBySetups.set(setups, projection);
	if (!cachedBySetups) nextCachedByProjects.set(projects, nextCachedBySetups);
	if (!cachedByProjects) normalizedProjectHostSetupProjectionCache.set(repos, nextCachedByProjects);
	return projection;
}
function getProjectHostSetupProjectionFromState(state) {
	if (state.projects && state.projectHostSetups) {
		const repoIds = new Set(state.repos.map((repo) => repo.id));
		const coveredRepoIds = /* @__PURE__ */ new Set();
		for (const setup of state.projectHostSetups) {
			const repoId = typeof setup.repoId === "string" ? setup.repoId : "";
			if (repoIds.has(repoId)) coveredRepoIds.add(repoId);
			if (repoIds.has(setup.id)) coveredRepoIds.add(setup.id);
		}
		if (state.repos.length > 0 && coveredRepoIds.size < repoIds.size) return mergeProjectHostSetupProjection(state.repos, state.projects, state.projectHostSetups);
		const derived = getCachedProjectHostSetupProjection(state.repos);
		const normalized = normalizeHydratedProjectHostSetupProjection(state.repos, state.projects, state.projectHostSetups, derived);
		if (normalized.changed) return getCachedNormalizedProjectHostSetupProjection(state.repos, state.projects, state.projectHostSetups, derived, normalized);
		return getCachedProvidedProjectHostSetupProjection(state.projects, state.projectHostSetups);
	}
	return getCachedProjectHostSetupProjection(state.repos);
}
var EMPTY_WORKTREES = [];
var EMPTY_TABS = [];
var EMPTY_BROWSER_TABS = [];
var EMPTY_UNIFIED_TABS = [];
var hasAnyWorktreesCache = /* @__PURE__ */ new WeakMap();
var floatingVisibleTabCountCache = null;
function getCachedHasAnyWorktrees(worktreesByRepo) {
	const cached = hasAnyWorktreesCache.get(worktreesByRepo);
	if (cached !== void 0) return cached;
	const hasWorktrees = Object.values(worktreesByRepo).some((worktrees) => worktrees.length > 0);
	hasAnyWorktreesCache.set(worktreesByRepo, hasWorktrees);
	return hasWorktrees;
}
function selectFloatingVisibleTabCount(state) {
	const terminalTabs = state.tabsByWorktree["global-floating-terminal"] ?? EMPTY_TABS;
	const browserTabs = state.browserTabsByWorktree["global-floating-terminal"] ?? EMPTY_BROWSER_TABS;
	const unifiedTabs = state.unifiedTabsByWorktree["global-floating-terminal"] ?? EMPTY_UNIFIED_TABS;
	const cached = floatingVisibleTabCountCache;
	if (cached && cached.terminalTabs === terminalTabs && cached.browserTabs === browserTabs && cached.openFiles === state.openFiles && cached.unifiedTabs === unifiedTabs) return cached.count;
	const terminalIds = /* @__PURE__ */ new Set();
	for (const tab of terminalTabs) terminalIds.add(tab.id);
	const browserIds = /* @__PURE__ */ new Set();
	for (const tab of browserTabs) browserIds.add(tab.id);
	const editorIds = /* @__PURE__ */ new Set();
	for (const file of state.openFiles) if (file.worktreeId === "global-floating-terminal") editorIds.add(file.id);
	let count = 0;
	for (const tab of unifiedTabs) if (tab.contentType === "terminal") count += terminalIds.has(tab.entityId) ? 1 : 0;
	else if (tab.contentType === "browser") count += browserIds.has(tab.entityId) ? 1 : 0;
	else if (tab.contentType === "simulator") count += 1;
	else count += editorIds.has(tab.entityId) ? 1 : 0;
	floatingVisibleTabCountCache = {
		terminalTabs,
		browserTabs,
		openFiles: state.openFiles,
		unifiedTabs,
		count
	};
	return count;
}
function selectFloatingWorkspaceHasUnread(state) {
	const tabs = state.tabsByWorktree[FLOATING_TERMINAL_WORKTREE_ID];
	if (!tabs || tabs.length === 0) return false;
	const floatingTabIds = /* @__PURE__ */ new Set();
	for (const tab of tabs) {
		if (state.unreadTerminalTabs[tab.id]) return true;
		floatingTabIds.add(tab.id);
	}
	for (const paneKey of Object.keys(state.unreadAgentCompletionPanes)) {
		const separatorIndex = paneKey.indexOf(":");
		const tabId = separatorIndex === -1 ? paneKey : paneKey.slice(0, separatorIndex);
		if (floatingTabIds.has(tabId)) return true;
	}
	return false;
}
function getAllWorktreesFromState(state) {
	return getIndexedAllWorktrees(state.worktreesByRepo);
}
function getWorktreeMapFromState(state) {
	return getIndexedWorktreeMap(state.worktreesByRepo);
}
function getHasAnyWorktreesFromState(state) {
	return getCachedHasAnyWorktrees(state.worktreesByRepo);
}
function getRepoMapFromState(state) {
	return getIndexedRepoMap(state.repos);
}
const useRepos = () => useAppStore((s) => s.repos);
const useActiveRepo = () => useAppStore(useShallow((s) => selectRepoByIdForActiveWorkspace(s, s.activeRepoId)));
const useRepoMap = () => useAppStore((s) => getIndexedRepoMap(s.repos));
function selectRepoByIdForActiveWorkspace(state, repoId) {
	if (!repoId) return null;
	const repo = getIndexedRepoMap(state.repos).get(repoId) ?? null;
	if (repoId === state.activeRepoId && state.activeWorkspaceExecutionHostId) {
		const repoCandidates = state.repos.filter((candidate) => candidate.id === repoId);
		const hostMatch = repoCandidates.find((candidate) => getRepoExecutionHostId(candidate) === state.activeWorkspaceExecutionHostId);
		if (hostMatch) return hostMatch;
		if (parseExecutionHostId(state.activeWorkspaceExecutionHostId)?.kind !== "ssh") return null;
		const pairedHubRepos = repoCandidates.filter((candidate) => parseExecutionHostId(getRepoExecutionHostId(candidate))?.kind === "runtime");
		return pairedHubRepos.length === 1 ? pairedHubRepos[0] : null;
	}
	return repo;
}
const useRepoById = (repoId) => useAppStore((s) => selectRepoByIdForActiveWorkspace(s, repoId));
const useProjectHostSetupProjection = () => useAppStore((s) => getProjectHostSetupProjectionFromState(s));
const useActiveWorktreeId = () => useAppStore((s) => s.activeWorktreeId);
const useWorktreesForRepo = (repoId) => useAppStore((s) => repoId ? s.worktreesByRepo[repoId] ?? EMPTY_WORKTREES : EMPTY_WORKTREES);
const useAllWorktrees = () => useAppStore((s) => getIndexedAllWorktrees(s.worktreesByRepo));
const useWorktreeMap = () => useAppStore((s) => getIndexedWorktreeMap(s.worktreesByRepo));
const useWorktreeById = (worktreeId, executionHostId) => useAppStore((s) => worktreeId ? s.getKnownWorktreeById(worktreeId, executionHostId ?? (worktreeId === s.activeWorktreeId ? s.activeWorkspaceExecutionHostId ?? void 0 : void 0)) ?? null : null);
const useActiveWorktree = () => {
	const activeWorktreeId = useActiveWorktreeId();
	return useAppStore((s) => activeWorktreeId ? s.getKnownWorktreeById(activeWorktreeId, s.activeWorkspaceExecutionHostId ?? void 0) ?? null : null);
};
export { useWorktreesForRepo as _, selectFloatingVisibleTabCount as a, useActiveWorktree as c, useProjectHostSetupProjection as d, useRepoById as f, useWorktreeMap as g, useWorktreeById as h, getWorktreeMapFromState as i, useActiveWorktreeId as l, useRepos as m, getHasAnyWorktreesFromState as n, selectFloatingWorkspaceHasUnread as o, useRepoMap as p, getRepoMapFromState as r, useActiveRepo as s, getAllWorktreesFromState as t, useAllWorktrees as u, getProjectHostSetupProjectionFromState as v };
