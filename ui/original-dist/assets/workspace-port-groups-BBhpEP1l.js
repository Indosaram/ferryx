var portsByWorktreeCache = /* @__PURE__ */ new WeakMap();
var workspaceGroupsCache = /* @__PURE__ */ new WeakMap();
var externalPortsCache = /* @__PURE__ */ new WeakMap();
var EMPTY_PORTS_BY_WORKTREE = /* @__PURE__ */ new Map();
var EMPTY_WORKSPACE_PORT_GROUPS = [];
var EMPTY_EXTERNAL_PORTS = [];
function comparePorts(a, b) {
	return a.port - b.port || (a.processName ?? "").localeCompare(b.processName ?? "");
}
function getWorkspacePortsByWorktreeId(scan) {
	if (!scan) return EMPTY_PORTS_BY_WORKTREE;
	const cached = portsByWorktreeCache.get(scan);
	if (cached) return cached;
	const grouped = /* @__PURE__ */ new Map();
	for (const port of scan.ports) {
		if (port.kind !== "workspace") continue;
		const current = grouped.get(port.owner.worktreeId);
		if (current) current.push(port);
		else grouped.set(port.owner.worktreeId, [port]);
	}
	for (const ports of grouped.values()) ports.sort(comparePorts);
	portsByWorktreeCache.set(scan, grouped);
	return grouped;
}
function getWorkspacePortGroups(scan) {
	if (!scan) return EMPTY_WORKSPACE_PORT_GROUPS;
	const cached = workspaceGroupsCache.get(scan);
	if (cached) return cached;
	const groupsByWorktreeId = /* @__PURE__ */ new Map();
	for (const port of scan.ports) {
		if (port.kind !== "workspace") continue;
		const current = groupsByWorktreeId.get(port.owner.worktreeId);
		if (current) current.ports.push(port);
		else groupsByWorktreeId.set(port.owner.worktreeId, {
			worktreeId: port.owner.worktreeId,
			repoId: port.owner.repoId,
			displayName: port.owner.displayName,
			ports: [port]
		});
	}
	const groups = [...groupsByWorktreeId.values()].map((group) => ({
		...group,
		ports: [...group.ports].sort(comparePorts)
	})).sort((a, b) => a.displayName.localeCompare(b.displayName) || (a.ports[0]?.port ?? 0) - (b.ports[0]?.port ?? 0));
	workspaceGroupsCache.set(scan, groups);
	return groups;
}
function getExternalWorkspacePorts(scan) {
	if (!scan) return EMPTY_EXTERNAL_PORTS;
	const cached = externalPortsCache.get(scan);
	if (cached) return cached;
	const ports = scan.ports.filter((port) => port.kind !== "workspace").sort(comparePorts);
	externalPortsCache.set(scan, ports);
	return ports;
}
export { getWorkspacePortGroups as n, getWorkspacePortsByWorktreeId as r, getExternalWorkspacePorts as t };
