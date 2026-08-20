import { f as isDefaultBranchWorkspace, r as activateAndRevealWorktree } from "./worktree-activation-BDsaiyMf.js";
import { Pd as markOnboardingProjectAdded, t as useAppStore, zp as relativePathInsideRoot } from "./store-CgXrfmaH.js";
import { st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { a as track } from "./telemetry-ZyUPyKMD.js";
function finalizeImportedRepoAfterSkip(state, importedRepoId) {
	const importedWorktrees = state.worktreesByRepo[importedRepoId] ?? [];
	if (state.activeRepoId !== importedRepoId) state.setActiveRepo(importedRepoId);
	if (state.filterRepoIds.length > 0 && !state.filterRepoIds.includes(importedRepoId)) state.setFilterRepoIds([]);
	if (state.showActiveOnly) state.setShowActiveOnly(false);
	if (importedWorktrees.length > 0 && state.hideDefaultBranchWorkspace && importedWorktrees.every((worktree) => isDefaultBranchWorkspace(worktree))) state.setHideDefaultBranchWorkspace(false);
	if (importedWorktrees.length > 0 && state.alwaysShowDefaultBranchWorkspace === false && !state.showSleepingWorkspaces && importedWorktrees.every((worktree) => worktree.isMainWorktree)) state.setAlwaysShowDefaultBranchWorkspace(true);
}
function getProjectDefaultCheckout(worktrees) {
	return worktrees.find((worktree) => worktree.isMainWorktree) ?? null;
}
function getProjectWorktreesForHost(worktrees, executionHostId) {
	if (!executionHostId) return [...worktrees];
	const parsedHost = parseExecutionHostId(executionHostId);
	return worktrees.filter((worktree) => {
		if (parsedHost?.kind === "runtime") {
			if (worktree.runtimeOwnerEnvironmentId) return worktree.runtimeOwnerEnvironmentId === parsedHost.environmentId;
			return worktree.hostId === executionHostId;
		}
		if (worktree.runtimeOwnerEnvironmentId) return false;
		if (worktree.hostId) return worktree.hostId === executionHostId;
		return executionHostId === "local";
	});
}
function ownerRefreshOptions(executionHostId) {
	return {
		requireAuthoritative: true,
		...executionHostId ? { executionHostId } : {}
	};
}
function getDetectedProjectDefaultCheckout(detected, executionHostId) {
	if (detected?.authoritative !== true) return null;
	return getProjectWorktreesForHost(detected.worktrees, executionHostId).find((worktree) => worktree.isMainWorktree) ?? null;
}
function hasDetectedHiddenLinkedExternalWorktrees(detected, executionHostId) {
	if (detected?.authoritative !== true) return false;
	return getProjectWorktreesForHost(detected.worktrees, executionHostId).some((worktree) => !worktree.isMainWorktree && !worktree.selectedCheckout && !worktree.visible && worktree.ownership !== "orca-managed" && worktree.ownership !== "agent-scratch");
}
async function revealDetectedHiddenLinkedExternalWorktrees(repoId, executionHostId) {
	const state = useAppStore.getState();
	if (!hasDetectedHiddenLinkedExternalWorktrees(state.detectedWorktreesByRepo[repoId], executionHostId)) return null;
	if (!(executionHostId ? await state.updateRepo(repoId, { externalWorktreeVisibility: "show" }, { hostId: executionHostId }) : await state.updateRepo(repoId, { externalWorktreeVisibility: "show" }))) return "show_detected_linked_failed";
	return await useAppStore.getState().fetchWorktrees(repoId, ownerRefreshOptions(executionHostId)) ? null : "linked_external_refresh_failed";
}
async function findDetectedDefaultCheckout(repoId, executionHostId) {
	const state = useAppStore.getState();
	const detected = state.detectedWorktreesByRepo[repoId];
	const detectedDefaultCheckout = getDetectedProjectDefaultCheckout(detected, executionHostId);
	if (!detectedDefaultCheckout) return {
		worktree: null,
		reason: detected?.authoritative === true ? "no_default_checkout" : "no_authoritative_detection"
	};
	if (!detectedDefaultCheckout.visible) {
		if (!(executionHostId ? await state.updateRepo(repoId, { externalWorktreeVisibility: "show" }, { hostId: executionHostId }) : await state.updateRepo(repoId, { externalWorktreeVisibility: "show" }))) return {
			worktree: null,
			reason: "show_detected_default_failed"
		};
	}
	if (!await useAppStore.getState().fetchWorktrees(repoId, ownerRefreshOptions(executionHostId))) return {
		worktree: null,
		reason: "authoritative_refresh_failed"
	};
	const worktree = getProjectDefaultCheckout(getProjectWorktreesForHost(useAppStore.getState().worktreesByRepo[repoId] ?? [], executionHostId));
	return {
		worktree,
		reason: worktree ? "detected_default_checkout" : "refreshed_default_missing"
	};
}
function resolveInitialCwdForDefaultCheckout(defaultCheckout, selectedPath) {
	if (!selectedPath) return;
	const relativePath = relativePathInsideRoot(defaultCheckout.path, selectedPath);
	return relativePath && relativePath.length > 0 ? selectedPath : void 0;
}
async function openProjectDefaultCheckout({ repoId, source, selectedPath, setHideDefaultBranchWorkspace, executionHostId }) {
	let defaultCheckout = getProjectDefaultCheckout(getProjectWorktreesForHost(useAppStore.getState().worktreesByRepo[repoId] ?? [], executionHostId));
	let reason = "loaded_default_checkout";
	if (!defaultCheckout) {
		const detectedDefaultCheckout = await findDetectedDefaultCheckout(repoId, executionHostId);
		defaultCheckout = detectedDefaultCheckout.worktree;
		reason = detectedDefaultCheckout.reason;
	}
	if (defaultCheckout) {
		const revealLinkedFailureReason = await revealDetectedHiddenLinkedExternalWorktrees(repoId, executionHostId);
		if (revealLinkedFailureReason) {
			track("add_repo_default_checkout_handoff", {
				source,
				result: "revealed_project",
				reason: revealLinkedFailureReason
			});
			finalizeImportedRepoAfterSkip(useAppStore.getState(), repoId);
			return;
		}
		if (useAppStore.getState().hideDefaultBranchWorkspace) setHideDefaultBranchWorkspace(false);
		track("add_repo_default_checkout_handoff", {
			source,
			result: "opened_default_checkout",
			reason
		});
		const initialCwd = resolveInitialCwdForDefaultCheckout(defaultCheckout, selectedPath);
		if (initialCwd || executionHostId) activateAndRevealWorktree(defaultCheckout.id, {
			...initialCwd ? { initialCwd } : {},
			...executionHostId ? { executionHostId } : {}
		});
		else activateAndRevealWorktree(defaultCheckout.id);
		return;
	}
	track("add_repo_default_checkout_handoff", {
		source,
		result: "revealed_project",
		reason
	});
	finalizeImportedRepoAfterSkip(useAppStore.getState(), repoId);
}
async function finishProjectAddWithDefaultCheckout({ repoId, source, selectedPath, closeModal, setHideDefaultBranchWorkspace, executionHostId }) {
	await markOnboardingProjectAdded("addedRepo");
	closeModal();
	await openProjectDefaultCheckout({
		repoId,
		source,
		selectedPath,
		executionHostId,
		setHideDefaultBranchWorkspace
	});
}
export { finishProjectAddWithDefaultCheckout as t };
