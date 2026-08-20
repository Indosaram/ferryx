function sharesResolvedWorktreeLineageBoundary(child, parent) {
	return child.repoId === parent.repoId && (child.hostId === void 0 || parent.hostId === void 0 || child.hostId === parent.hostId) && (child.projectId === void 0 || parent.projectId === void 0 || child.projectId === parent.projectId);
}
function isValidResolvedWorktreeLineageEdge(child, parent, lineage) {
	return child.id !== parent.id && lineage.worktreeId === child.id && lineage.parentWorktreeId === parent.id && sharesResolvedWorktreeLineageBoundary(child, parent) && child.instanceId === lineage.worktreeInstanceId && parent.instanceId === lineage.parentWorktreeInstanceId;
}
function getCyclicWorktreeLineageChildIds(lineageByChildId) {
	const processed = /* @__PURE__ */ new Set();
	const cyclic = /* @__PURE__ */ new Set();
	for (const childId of lineageByChildId.keys()) {
		if (processed.has(childId)) continue;
		const path = [];
		const pathIndexById = /* @__PURE__ */ new Map();
		let currentId = childId;
		while (currentId && lineageByChildId.has(currentId) && !processed.has(currentId)) {
			const cycleStart = pathIndexById.get(currentId);
			if (cycleStart !== void 0) {
				for (let index = cycleStart; index < path.length; index += 1) cyclic.add(path[index]);
				break;
			}
			pathIndexById.set(currentId, path.length);
			path.push(currentId);
			currentId = lineageByChildId.get(currentId)?.parentWorktreeId;
		}
		for (const id of path) processed.add(id);
	}
	return cyclic;
}
function getProjectedWorktreeLineage(worktree, lineageById) {
	if (Object.hasOwn(lineageById, worktree.id)) return lineageById[worktree.id];
	return worktree.lineage;
}
function getCyclicProjectedWorktreeLineageIds(lineageById, worktreeMap) {
	const validLineageByChildId = /* @__PURE__ */ new Map();
	for (const worktree of worktreeMap.values()) {
		const lineage = getProjectedWorktreeLineage(worktree, lineageById);
		if (!lineage) continue;
		const parent = worktreeMap.get(lineage.parentWorktreeId);
		if (parent && isValidResolvedWorktreeLineageEdge(worktree, parent, lineage)) validLineageByChildId.set(worktree.id, lineage);
	}
	return getCyclicWorktreeLineageChildIds(validLineageByChildId);
}
function getLineageRenderInfo(worktree, lineageById, worktreeMap, cyclicLineageIds) {
	const lineage = getProjectedWorktreeLineage(worktree, lineageById);
	if (!lineage) return { state: "none" };
	const parent = worktreeMap.get(lineage.parentWorktreeId);
	if (cyclicLineageIds.has(worktree.id) || !parent || !isValidResolvedWorktreeLineageEdge(worktree, parent, lineage)) return {
		state: "missing",
		lineage
	};
	return {
		state: "valid",
		lineage,
		parent
	};
}
function getProjectedWorktreeLineageChildrenByParentId(lineageById, worktreeMap) {
	const cyclicLineageIds = getCyclicProjectedWorktreeLineageIds(lineageById, worktreeMap);
	const childrenByParentId = /* @__PURE__ */ new Map();
	for (const worktree of worktreeMap.values()) {
		const lineage = getLineageRenderInfo(worktree, lineageById, worktreeMap, cyclicLineageIds);
		if (lineage.state !== "valid") continue;
		const children = childrenByParentId.get(lineage.parent.id) ?? [];
		children.push(worktree);
		childrenByParentId.set(lineage.parent.id, children);
	}
	return childrenByParentId;
}
function getWorktreeLineageAncestors(worktree, lineageById, worktreeMap) {
	const cyclicLineageIds = getCyclicProjectedWorktreeLineageIds(lineageById, worktreeMap);
	const ancestors = [];
	const seen = /* @__PURE__ */ new Set();
	let current = worktree;
	while (current && !seen.has(current.id)) {
		seen.add(current.id);
		const lineage = getLineageRenderInfo(current, lineageById, worktreeMap, cyclicLineageIds);
		if (lineage.state !== "valid") break;
		ancestors.push(lineage.parent);
		current = lineage.parent;
	}
	return ancestors;
}
export { getWorktreeLineageAncestors as a, getProjectedWorktreeLineageChildrenByParentId as i, getLineageRenderInfo as n, getProjectedWorktreeLineage as r, getCyclicProjectedWorktreeLineageIds as t };
