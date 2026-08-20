import { hd as folderWorkspaceKey, vd as parseWorkspaceKey } from "./store-CgXrfmaH.js";
import { i as getProjectedWorktreeLineageChildrenByParentId } from "./worktree-lineage-projection-CS7n_mKq.js";
import { t as compareWorktreeDisplayName } from "./worktree-display-name-order-AUI-ZkJy.js";
function getWorktreeActivityTime(worktree) {
	return Math.max(worktree.lastActivityAt ?? 0, worktree.createdAt ?? 0, worktree.sortOrder ?? 0);
}
function getAttachedWorktreesForFolderWorkspace({ activeWorkspaceKey, activeWorktreeId, folderWorkspaces, workspaceLineageByChildKey, worktreeLineageById, worktreesByRepo }) {
	const activeScope = parseWorkspaceKey(activeWorkspaceKey ?? activeWorktreeId ?? "");
	const folderWorkspace = activeScope?.type === "folder" ? folderWorkspaces.find((workspace) => workspace.id === activeScope.folderWorkspaceId) ?? null : null;
	if (!folderWorkspace) return {
		folderWorkspace: null,
		childWorktrees: [],
		lineageChildrenByParentId: /* @__PURE__ */ new Map(),
		rootChildWorktrees: []
	};
	const folderKey = folderWorkspaceKey(folderWorkspace.id);
	const worktreeById = getWorktreeById(worktreesByRepo);
	const childWorktrees = Object.values(workspaceLineageByChildKey).filter((lineage) => lineage.parentWorkspaceKey === folderKey).map((lineage) => getLineageChildWorktree(lineage, worktreeById)).filter((worktree) => worktree !== null).sort(sortWorktreesByRecentActivity);
	const lineageChildrenByParentId = getLineageChildrenByParentId(worktreeLineageById, worktreeById, new Set(childWorktrees.map((worktree) => worktree.id)));
	const nestedChildIds = /* @__PURE__ */ new Set();
	for (const children of lineageChildrenByParentId.values()) for (const child of children) nestedChildIds.add(child.id);
	const topLevelChildWorktrees = childWorktrees.filter((worktree) => !nestedChildIds.has(worktree.id));
	return {
		folderWorkspace,
		childWorktrees,
		lineageChildrenByParentId,
		rootChildWorktrees: topLevelChildWorktrees.length > 0 ? topLevelChildWorktrees : childWorktrees
	};
}
function getLineageChildrenByParentId(lineageById, worktreeById, rootWorktreeIds) {
	const projectedChildrenByParentId = getProjectedWorktreeLineageChildrenByParentId(lineageById, worktreeById);
	const includedIds = new Set(rootWorktreeIds);
	const queue = [...rootWorktreeIds];
	for (let index = 0; index < queue.length; index += 1) for (const child of projectedChildrenByParentId.get(queue[index]) ?? []) {
		if (child.isArchived || includedIds.has(child.id)) continue;
		includedIds.add(child.id);
		queue.push(child.id);
	}
	const descendantsByParentId = /* @__PURE__ */ new Map();
	for (const parentId of includedIds) {
		const children = (projectedChildrenByParentId.get(parentId) ?? []).filter((child) => includedIds.has(child.id) && !child.isArchived);
		if (children.length > 0) descendantsByParentId.set(parentId, children);
	}
	for (const children of descendantsByParentId.values()) children.sort(sortWorktreesByRecentActivity);
	return descendantsByParentId;
}
function getWorktreeById(worktreesByRepo) {
	return new Map(Object.values(worktreesByRepo).flat().map((worktree) => [worktree.id, worktree]));
}
function getLineageChildWorktree(lineage, worktreeById) {
	const childScope = parseWorkspaceKey(lineage.childWorkspaceKey);
	if (childScope?.type !== "worktree") return null;
	const worktree = worktreeById.get(childScope.worktreeId);
	if (!worktree || worktree.isArchived) return null;
	if (lineage.childInstanceId && lineage.childInstanceId !== worktree.instanceId) return null;
	return worktree;
}
function sortWorktreesByRecentActivity(left, right) {
	return getWorktreeActivityTime(right) - getWorktreeActivityTime(left) || compareWorktreeDisplayName(left, right);
}
export { getAttachedWorktreesForFolderWorkspace as t };
