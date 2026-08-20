import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { r as activateAndRevealWorktree } from "./worktree-activation-BDsaiyMf.js";
import { Lp as normalizeRuntimePathForComparison, Pp as isPathInsideOrEqual, Vg as isPairedWebClientWindow, bc as isProvenLivePtyRemovalError, t as useAppStore, vd as parseWorkspaceKey, yc as isLockedWorktreeRemovalError, yp as findRepoForHost } from "./store-CgXrfmaH.js";
import { nt as isRuntimeOwnedSshTargetId, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { i as getWorktreeMapFromState, r as getRepoMapFromState, t as getAllWorktreesFromState } from "./selectors-XOBeaOSb.js";
import { i as getProjectedWorktreeLineageChildrenByParentId } from "./worktree-lineage-projection-CS7n_mKq.js";
import { n as showPreservedBranchBatchToast } from "./preserved-branch-batch-toast-DHxeGO1o.js";
function staleWorkspaceListToast(title) {
	toast.info(title, { description: translate("auto.components.sidebar.delete.worktree.flow.b81b4e40ca", "Refresh Space and try again if the workspace list looks stale.") });
}
function showWorkspaceListChangedToast() {
	staleWorkspaceListToast(translate("auto.components.sidebar.delete.worktree.flow.workspaceListChanged", "Workspace list changed"));
}
function showNoDeletableWorkspacesToast() {
	staleWorkspaceListToast(translate("auto.components.sidebar.delete.worktree.flow.7243145cd6", "No deletable workspaces selected"));
}
function getWorkspaceDeleteLineage(parent, worktrees, lineageById) {
	const childrenByParentId = getProjectedWorktreeLineageChildrenByParentId(lineageById, new Map(worktrees.map((worktree) => [worktree.id, worktree])));
	const descendants = [];
	const childFirstTargets = [];
	const visiting = /* @__PURE__ */ new Set();
	const emitted = new Set([parent.id]);
	const visit = (worktreeId) => {
		if (visiting.has(worktreeId)) return;
		visiting.add(worktreeId);
		const children = childrenByParentId.get(worktreeId) ?? [];
		for (const child of children) {
			if (emitted.has(child.id)) continue;
			emitted.add(child.id);
			descendants.push(child);
			visit(child.id);
			if (!child.isMainWorktree) childFirstTargets.push(child);
		}
		visiting.delete(worktreeId);
	};
	visit(parent.id);
	return {
		descendants,
		deleteAllTargets: [...childFirstTargets, parent]
	};
}
function resolveSshWorkspaceForget(args) {
	const connectionId = args.repo?.connectionId?.trim();
	if (!connectionId || isRuntimeOwnedSshTargetId(connectionId)) return { kind: "not-ssh" };
	const isConfigured = args.sshTargetLabels.has(connectionId);
	const status = args.sshConnectionStates.get(connectionId)?.status;
	if (!isConfigured) return {
		kind: "ghost",
		targetId: connectionId
	};
	if (status === "connected") return {
		kind: "connected",
		targetId: connectionId
	};
	return {
		kind: "disconnected",
		targetId: connectionId,
		status: status ?? "disconnected"
	};
}
function toWorktreeDeleteIdentities(worktrees) {
	return worktrees.map(({ id, instanceId }) => ({
		id,
		instanceId
	}));
}
function resolveWorktreeBatchDeleteTargets(requestedWorktrees, worktreeMap) {
	const uniqueRequests = Array.from(new Map(requestedWorktrees.map((request) => [typeof request === "string" ? request : request.id, request])).values());
	const targets = [];
	for (const request of uniqueRequests) {
		const worktreeId = typeof request === "string" ? request : request.id;
		const target = worktreeMap.get(worktreeId) ?? null;
		if (typeof request !== "string" && (!target || target.instanceId !== request.instanceId)) return null;
		if (target && !target.isMainWorktree) targets.push(target);
	}
	return targets;
}
function readWorktreeDeleteIdentities(value) {
	if (!Array.isArray(value)) return [];
	return value.flatMap((entry) => {
		if (!entry || typeof entry !== "object" || !("id" in entry) || typeof entry.id !== "string") return [];
		const instanceId = "instanceId" in entry ? entry.instanceId : void 0;
		return instanceId === void 0 || typeof instanceId === "string" ? [{
			id: entry.id,
			instanceId
		}] : [];
	});
}
function isHostedOnRuntimeOwnedSshTarget(worktree, repoById) {
	return [
		worktree.hostId,
		repoById.get(worktree.repoId)?.executionHostId,
		repoById.get(worktree.repoId)?.connectionId
	].some((value) => {
		if (!value) return false;
		if (isRuntimeOwnedSshTargetId(value)) return true;
		const parsed = parseExecutionHostId(value);
		return parsed?.kind === "ssh" && isRuntimeOwnedSshTargetId(parsed.targetId);
	});
}
function pickNextWorktreeIdAfterDelete(state, repoId, deletedWorktreeId) {
	const deleteState = state.deleteStateByWorktreeId;
	const repoById = getRepoMapFromState(state);
	const siblings = (state.worktreesByRepo[repoId] ?? []).filter((worktree) => worktree.id !== deletedWorktreeId && !deleteState[worktree.id]?.isDeleting && !isHostedOnRuntimeOwnedSshTarget(worktree, repoById));
	const others = siblings.filter((worktree) => !worktree.isMainWorktree);
	if (others.length > 0) {
		const lastVisited = state.lastVisitedAtByWorktreeId;
		const [mostRecent] = [...others].sort((a, b) => (lastVisited[b.id] ?? 0) - (lastVisited[a.id] ?? 0));
		return mostRecent.id;
	}
	return siblings.find((worktree) => worktree.isMainWorktree)?.id ?? null;
}
function focusNextWorktreeAfterActiveDelete(deletedWorktreeId, repoId, wasViewingBeforeDelete) {
	if (!wasViewingBeforeDelete || !repoId) return;
	const state = useAppStore.getState();
	if (state.activeView !== "terminal" || state.activePendingCreationId !== null || state.activeWorktreeId !== null) return;
	const nextWorktreeId = pickNextWorktreeIdAfterDelete(state, repoId, deletedWorktreeId);
	if (nextWorktreeId) activateAndRevealWorktree(nextWorktreeId);
}
function prepareActiveWorktreeFocusAfterDelete(worktreeId) {
	const state = useAppStore.getState();
	const wasViewing = state.activeView === "terminal" && state.activePendingCreationId === null && state.activeWorktreeId === worktreeId;
	const repoId = getWorktreeMapFromState(state).get(worktreeId)?.repoId ?? null;
	return () => focusNextWorktreeAfterActiveDelete(worktreeId, repoId, wasViewing);
}
function getDeleteWorktreeToastCopy(worktreeName, forceDeleteReason, error, lockReason = null) {
	if (isLockedWorktreeRemovalError(error)) return {
		title: translate("auto.components.sidebar.delete.worktree.toast.1d0fa5c0a5", "Failed to delete workspace {{value0}}", { value0: worktreeName }),
		description: lockReason ? translate("auto.components.sidebar.delete.worktree.toast.lockedReason", "This workspace is locked by Git. Git reported: {{value0}}. Run git worktree unlock <worktree-path> from its repository, then retry deletion.", { value0: lockReason }) : translate("auto.components.sidebar.delete.worktree.toast.locked", "This workspace is locked by Git. Run git worktree unlock <worktree-path> from its repository, then retry deletion."),
		isDestructive: false
	};
	if (forceDeleteReason) {
		if (forceDeleteReason === "orphan-directory") return {
			title: translate("auto.components.sidebar.delete.worktree.toast.1d0fa5c0a5", "Failed to delete workspace {{value0}}", { value0: worktreeName }),
			description: translate("auto.components.sidebar.delete.worktree.toast.0899ebdb28", "Git already forgot this workspace, but its directory is still on disk. Use Force Delete to remove the orphaned directory."),
			isDestructive: false
		};
		if (forceDeleteReason === "unstopped-pty") return {
			title: translate("auto.components.sidebar.delete.worktree.toast.1d0fa5c0a5", "Failed to delete workspace {{value0}}", { value0: worktreeName }),
			description: isProvenLivePtyRemovalError(error) ? translate("auto.components.sidebar.delete.worktree.toast.unstoppedPtyLive", "This workspace still has running terminals, so Orca stopped before deleting any files. Force Delete will kill them and discard any uncommitted work they hold.") : translate("auto.components.sidebar.delete.worktree.toast.unstoppedPty", "Orca could not confirm every terminal in this workspace has exited, so it stopped before deleting any files. Use Force Delete to remove it anyway."),
			isDestructive: false
		};
		if (forceDeleteReason === "missing-registration") return {
			title: translate("auto.components.sidebar.delete.worktree.toast.1d0fa5c0a5", "Failed to delete workspace {{value0}}", { value0: worktreeName }),
			description: translate("auto.components.sidebar.delete.worktree.toast.905fc8efac", "Git already removed this workspace. Use Force Delete to clear it from Orca."),
			isDestructive: false
		};
		return {
			title: translate("auto.components.sidebar.delete.worktree.toast.1d0fa5c0a5", "Failed to delete workspace {{value0}}", { value0: worktreeName }),
			description: translate("auto.components.sidebar.delete.worktree.toast.ead7b8ee15", "It has changed files. Use Force Delete to delete it anyway."),
			isDestructive: false
		};
	}
	return {
		title: translate("auto.components.sidebar.delete.worktree.toast.1d0fa5c0a5", "Failed to delete workspace {{value0}}", { value0: worktreeName }),
		description: error,
		isDestructive: true
	};
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function deleteWorktreeFailureToastId(worktreeId) {
	return `delete-worktree-failure:${worktreeId}`;
}
function DeleteWorktreeFailureToastBody({ description, canForceDelete, showViewChanges, onViewChanges, onForceDelete, toastId }) {
	const viewChanges = () => {
		toast.dismiss(toastId);
		onViewChanges();
	};
	const forceDelete = () => {
		toast.dismiss(toastId);
		onForceDelete();
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex w-full flex-col gap-3",
		children: [description ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
			className: "text-sm leading-5 text-popover-foreground/80",
			children: description
		}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex flex-wrap justify-end gap-2",
			children: [showViewChanges ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				type: "button",
				variant: "outline",
				size: "sm",
				onClick: viewChanges,
				children: translate("auto.components.sidebar.delete.worktree.flow.7488ed8711", "View")
			}) : null, canForceDelete ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				type: "button",
				variant: "destructive",
				size: "sm",
				onClick: forceDelete,
				children: translate("auto.components.sidebar.delete.worktree.flow.2b20ce87b3", "Force Delete")
			}) : null]
		})]
	});
}
function showDeleteWorktreeFailureToast({ error, canForceDelete, forceDeleteReason, lockReason, hasKnownChanges, onViewChanges, onForceDelete, worktreeId, worktreeName }) {
	const toastCopy = getDeleteWorktreeToastCopy(worktreeName, forceDeleteReason, error, lockReason ?? null);
	const showToast = toastCopy.isDestructive ? toast.error : toast.info;
	const id = deleteWorktreeFailureToastId(worktreeId);
	showToast(toastCopy.title, {
		id,
		description: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DeleteWorktreeFailureToastBody, {
			description: toastCopy.description,
			canForceDelete,
			showViewChanges: !isLockedWorktreeRemovalError(error) || hasKnownChanges === true,
			onViewChanges,
			onForceDelete,
			toastId: id
		}),
		duration: canForceDelete ? Infinity : 1e4,
		dismissible: true
	});
}
function viewWorktreeDiff(worktreeId) {
	activateAndRevealWorktree(worktreeId);
	const state = useAppStore.getState();
	state.setRightSidebarTab("source-control");
	state.setRightSidebarOpen(true);
}
function isStrictDescendantPath(parentPath, childPath) {
	return normalizeRuntimePathForComparison(parentPath) !== normalizeRuntimePathForComparison(childPath) && isPathInsideOrEqual(parentPath, childPath);
}
async function runWorktreeDeletesInParallel(targets, options = {}) {
	const uniqueTargets = Array.from(new Map(targets.map((target) => [target.id, target])).values());
	const activeWorktreeIdBefore = useAppStore.getState().activeWorktreeId;
	const commitBatchFocus = activeWorktreeIdBefore ? prepareActiveWorktreeFocusAfterDelete(activeWorktreeIdBefore) : null;
	useAppStore.getState().markWorktreesDeleting(uniqueTargets.map((target) => target.id));
	const groups = /* @__PURE__ */ new Map();
	for (const target of uniqueTargets) {
		const group = groups.get(target.repoId);
		if (group) group.push(target);
		else groups.set(target.repoId, [target]);
	}
	for (const group of groups.values()) group.sort((a, b) => b.path.length - a.path.length);
	const preservedBranches = [];
	const aggregatePreservedBranches = uniqueTargets.length > 1;
	let listChanged = false;
	const groupResults = await Promise.all(Array.from(groups.values()).map(async (group) => {
		const deletedInGroup = [];
		const failedInGroup = [];
		for (const target of group) {
			const currentTarget = getWorktreeMapFromState(useAppStore.getState()).get(target.id);
			if (!currentTarget || currentTarget.instanceId !== target.instanceId) {
				useAppStore.getState().clearWorktreeDeleteState(target.id);
				listChanged = true;
				continue;
			}
			if (failedInGroup.some((failed) => isStrictDescendantPath(target.path, failed.path))) {
				useAppStore.getState().clearWorktreeDeleteState(target.id);
				continue;
			}
			if (await runWorktreeDeleteWithToast(target.id, target.displayName, {
				...options,
				focusSuccessorOnDelete: false,
				suppressPreservedBranchToast: aggregatePreservedBranches,
				onPreservedBranch: (branch) => {
					preservedBranches.push(branch);
					options.onPreservedBranch?.(branch);
				}
			})) deletedInGroup.push(target.id);
			else failedInGroup.push(target);
		}
		return deletedInGroup;
	}));
	if (listChanged) showWorkspaceListChangedToast();
	const deletedSet = new Set(groupResults.flat());
	if (activeWorktreeIdBefore && deletedSet.has(activeWorktreeIdBefore)) commitBatchFocus?.();
	if (aggregatePreservedBranches && preservedBranches.length > 0) {
		const targetOrder = new Map(uniqueTargets.map((target, index) => [target.id, index]));
		preservedBranches.sort((left, right) => (targetOrder.get(left.worktreeId) ?? Number.MAX_SAFE_INTEGER) - (targetOrder.get(right.worktreeId) ?? Number.MAX_SAFE_INTEGER));
		showPreservedBranchBatchToast(deletedSet.size, preservedBranches);
	}
	return uniqueTargets.filter((target) => deletedSet.has(target.id)).map((target) => target.id);
}
function runWorktreeDeleteWithToast(worktreeId, worktreeName, options = {}) {
	const removeWorktree = useAppStore.getState().removeWorktree;
	const commitFocus = prepareActiveWorktreeFocusAfterDelete(worktreeId);
	const focusSuccessor = options.focusSuccessorOnDelete !== false;
	return (options.suppressPreservedBranchToast ? removeWorktree(worktreeId, options.force === true, { suppressPreservedBranchToast: true }) : removeWorktree(worktreeId, options.force === true)).then((result) => {
		if (result.ok) {
			if (result.preservedBranch) options.onPreservedBranch?.({
				worktreeId,
				branchName: result.preservedBranch.branchName,
				expectedHead: result.preservedBranch.head,
				...result.preservedBranch.hostId ? { hostId: result.preservedBranch.hostId } : {},
				...result.preservedBranch.runtimeEnvironmentId ? { runtimeEnvironmentId: result.preservedBranch.runtimeEnvironmentId } : {}
			});
			if (focusSuccessor) commitFocus();
			return true;
		}
		const state = useAppStore.getState().deleteStateByWorktreeId[worktreeId];
		const canForceDelete = state?.canForceDelete ?? false;
		const hasKnownChanges = (useAppStore.getState().gitStatusByWorktree[worktreeId]?.length ?? 0) > 0;
		showDeleteWorktreeFailureToast({
			error: result.error,
			canForceDelete,
			forceDeleteReason: state?.forceDeleteReason ?? null,
			lockReason: state?.lockReason ?? null,
			hasKnownChanges,
			onViewChanges: () => viewWorktreeDiff(worktreeId),
			onForceDelete: () => {
				const commitForceFocus = prepareActiveWorktreeFocusAfterDelete(worktreeId);
				useAppStore.getState().removeWorktree(worktreeId, true, { allowUnverifiedPtyStop: true }).then((forceResult) => {
					if (!forceResult.ok) {
						toast.error(translate("auto.components.sidebar.delete.worktree.flow.4f3876c0f5", "Force delete failed"), {
							description: forceResult.error,
							action: {
								label: translate("auto.components.sidebar.delete.worktree.flow.7488ed8711", "View"),
								onClick: () => viewWorktreeDiff(worktreeId)
							}
						});
						return;
					}
					commitForceFocus();
					options.onForceDeleted?.(worktreeId);
				}).catch((err) => {
					toast.error(translate("auto.components.sidebar.delete.worktree.flow.ae57cbf6e4", "Failed to delete workspace"), {
						description: err instanceof Error ? err.message : String(err),
						action: {
							label: translate("auto.components.sidebar.delete.worktree.flow.7488ed8711", "View"),
							onClick: () => viewWorktreeDiff(worktreeId)
						}
					});
				});
			},
			worktreeId,
			worktreeName
		});
		return false;
	}).catch((err) => {
		toast.error(translate("auto.components.sidebar.delete.worktree.flow.ae57cbf6e4", "Failed to delete workspace"), { description: err instanceof Error ? err.message : String(err) });
		return false;
	});
}
function runWorktreeDelete(worktreeId, options = {}) {
	const state = useAppStore.getState();
	const target = getWorktreeMapFromState(state).get(worktreeId) ?? null;
	const instanceChanged = Object.hasOwn(options, "expectedInstanceId") && target?.instanceId !== options.expectedInstanceId;
	if (!target || instanceChanged) {
		if (parseWorkspaceKey(worktreeId)?.type !== "folder") showWorkspaceListChangedToast();
		return;
	}
	if (target.isMainWorktree) {
		const repo$1 = state.repos.find((entry) => entry.id === target.repoId);
		state.openModal("confirm-remove-folder", {
			repoId: target.repoId,
			displayName: repo$1?.displayName ?? target.displayName
		});
		return;
	}
	state.clearWorktreeDeleteState(worktreeId);
	const matchingRepos = state.repos.filter((entry) => entry.id === target.repoId);
	const repo = target.hostId ? findRepoForHost(matchingRepos, target.repoId, { hostId: target.hostId }) : matchingRepos.length === 1 ? matchingRepos[0] : null;
	const sshResolution = isPairedWebClientWindow() ? { kind: "not-ssh" } : resolveSshWorkspaceForget({
		repo,
		sshConnectionStates: state.sshConnectionStates,
		sshTargetLabels: state.sshTargetLabels
	});
	if (sshResolution.kind === "ghost" || sshResolution.kind === "disconnected") {
		state.openModal("forget-ssh-workspace", {
			worktreeId,
			displayName: target.displayName,
			resolution: sshResolution
		});
		return;
	}
	const deleteLineage = getWorkspaceDeleteLineage(target, getAllWorktreesFromState(state), state.worktreeLineageById);
	const hasLineageChildren = deleteLineage.descendants.length > 0;
	if ((state.settings?.skipDeleteWorktreeConfirm ?? false) && !hasLineageChildren) {
		runWorktreeDeleteWithToast(worktreeId, target.displayName);
		return;
	}
	state.openModal("delete-worktree", {
		worktreeId,
		worktreeDeleteIdentities: toWorktreeDeleteIdentities([target]),
		...hasLineageChildren ? { lineageDeleteIdentities: toWorktreeDeleteIdentities(deleteLineage.deleteAllTargets) } : {},
		...hasLineageChildren ? { allowSkipConfirm: false } : {}
	});
}
function runWorktreeBatchDelete(requestedWorktrees, options = {}) {
	const state = useAppStore.getState();
	const targets = resolveWorktreeBatchDeleteTargets(requestedWorktrees, getWorktreeMapFromState(state));
	if (!targets) {
		showWorkspaceListChangedToast();
		return false;
	}
	if (targets.length === 0) {
		showNoDeletableWorkspacesToast();
		return false;
	}
	for (const target of targets) state.clearWorktreeDeleteState(target.id);
	const singleTargetLineage = targets.length === 1 ? getWorkspaceDeleteLineage(targets[0], getAllWorktreesFromState(state), state.worktreeLineageById) : null;
	const singleTargetHasLineageChildren = (singleTargetLineage?.descendants.length ?? 0) > 0;
	if (!options.forceConfirm && targets.length === 1 && !singleTargetHasLineageChildren && (state.settings?.skipDeleteWorktreeConfirm ?? false)) {
		runWorktreeDeletesInParallel(targets, { onForceDeleted: (deletedId) => options.onDeleted?.([deletedId]) }).then((deletedIds) => {
			if (deletedIds.length > 0) options.onDeleted?.(deletedIds);
		});
		return true;
	}
	if (targets.length === 1) {
		state.openModal("delete-worktree", {
			worktreeId: targets[0].id,
			worktreeDeleteIdentities: toWorktreeDeleteIdentities(targets),
			...singleTargetHasLineageChildren && singleTargetLineage ? { lineageDeleteIdentities: toWorktreeDeleteIdentities(singleTargetLineage.deleteAllTargets) } : {},
			...options.forceConfirm || singleTargetHasLineageChildren ? { allowSkipConfirm: false } : {},
			...options.onDeleted ? { onDeleted: options.onDeleted } : {}
		});
		return true;
	}
	state.openModal("delete-worktree", {
		worktreeIds: targets.map((target) => target.id),
		worktreeDeleteIdentities: toWorktreeDeleteIdentities(targets),
		allowSkipConfirm: false,
		...options.onDeleted ? { onDeleted: options.onDeleted } : {}
	});
	return true;
}
export { prepareActiveWorktreeFocusAfterDelete as a, getWorkspaceDeleteLineage as c, runWorktreeDeletesInParallel as i, showWorkspaceListChangedToast as l, runWorktreeDelete as n, readWorktreeDeleteIdentities as o, runWorktreeDeleteWithToast as r, resolveWorktreeBatchDeleteTargets as s, runWorktreeBatchDelete as t };
