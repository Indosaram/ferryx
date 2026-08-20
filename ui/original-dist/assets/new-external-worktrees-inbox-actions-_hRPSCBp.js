import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { a as mergeExternalWorktreeInboxPaths } from "./worktree-ownership-B1VtdtJF.js";
function newExternalWorktreeInboxKeepHiddenError() {
	return translate("auto.components.sidebar.newExternalWorktreesInboxActions.a11c2f6d89", "Could not keep external worktrees hidden. Try again.");
}
function newExternalWorktreeInboxImportError() {
	return translate("auto.components.sidebar.newExternalWorktreesInboxActions.b7e4d1a062", "Could not import external worktrees. Try again.");
}
function newExternalWorktreeInboxSuppressError() {
	return translate("auto.components.sidebar.newExternalWorktreesInboxActions.c94f0b3a15", "Could not hide external worktrees permanently. Try again.");
}
function rollbackPathList(paths) {
	return [...paths ?? []];
}
async function refreshAfterRepoInboxUpdate(args, updates, rollbackUpdates) {
	args.setInboxState(args.projectId, {
		pending: true,
		error: null
	});
	if (!await args.updateRepo(args.projectId, updates)) {
		args.setInboxState(args.projectId, {
			pending: false,
			error: newExternalWorktreeInboxImportError()
		});
		return false;
	}
	if (!await args.fetchWorktrees(args.projectId, { requireAuthoritative: true })) {
		await args.updateRepo(args.projectId, rollbackUpdates);
		args.setInboxState(args.projectId, {
			pending: false,
			error: newExternalWorktreeInboxImportError()
		});
		return false;
	}
	args.setInboxState(args.projectId, null);
	return true;
}
async function keepNewExternalWorktreeInboxHidden(args) {
	args.setInboxState(args.projectId, {
		pending: true,
		error: null
	});
	const baseline = mergeExternalWorktreeInboxPaths(args.repo.externalWorktreeInboxBaselinePaths, args.worktreePaths);
	if (!await args.updateRepo(args.projectId, { externalWorktreeInboxBaselinePaths: baseline })) {
		args.setInboxState(args.projectId, {
			pending: false,
			error: newExternalWorktreeInboxKeepHiddenError()
		});
		return;
	}
	args.setInboxState(args.projectId, null);
}
async function importNewExternalWorktreeInboxPaths(args) {
	await refreshAfterRepoInboxUpdate(args, {
		importedExternalWorktreePaths: mergeExternalWorktreeInboxPaths(args.repo.importedExternalWorktreePaths, args.worktreePaths),
		externalWorktreeInboxBaselinePaths: mergeExternalWorktreeInboxPaths(args.repo.externalWorktreeInboxBaselinePaths, args.worktreePaths)
	}, {
		importedExternalWorktreePaths: rollbackPathList(args.repo.importedExternalWorktreePaths),
		externalWorktreeInboxBaselinePaths: rollbackPathList(args.repo.externalWorktreeInboxBaselinePaths)
	});
}
async function suppressNewExternalWorktreeInbox(args) {
	args.setInboxState(args.projectId, {
		pending: true,
		error: null
	});
	const externalWorktreeInboxBaselinePaths = mergeExternalWorktreeInboxPaths(args.repo.externalWorktreeInboxBaselinePaths, args.worktreePaths);
	if (!await args.updateRepo(args.projectId, {
		externalWorktreeDiscoverySuppressedAt: Date.now(),
		externalWorktreeInboxBaselinePaths
	})) {
		args.setInboxState(args.projectId, {
			pending: false,
			error: newExternalWorktreeInboxSuppressError()
		});
		return false;
	}
	args.setInboxState(args.projectId, null);
	return true;
}
export { keepNewExternalWorktreeInboxHidden as n, suppressNewExternalWorktreeInbox as r, importNewExternalWorktreeInboxPaths as t };
