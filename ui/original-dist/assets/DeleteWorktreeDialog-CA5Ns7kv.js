import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import "./worktree-activation-BDsaiyMf.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { Gf as getRuntimeGitStatus, Kg as isFolderRepo, ku as getSettingsForWorktreeRuntimeOwner, m_ as Trash2, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import { t as Workflow } from "./workflow-DL6naYZy.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { t as ScrollArea } from "./scroll-area-DifvZO0h.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import { i as getWorktreeMapFromState, u as useAllWorktrees } from "./selectors-XOBeaOSb.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./web-session-tabs-sync-CYKZbAxS.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import { t as getConnectionId } from "./connection-context-BUPsamzR.js";
import "./localized-catalog-DubKHKUR.js";
import { a as prepareActiveWorktreeFocusAfterDelete, c as getWorkspaceDeleteLineage, i as runWorktreeDeletesInParallel, l as showWorkspaceListChangedToast, o as readWorktreeDeleteIdentities, s as resolveWorktreeBatchDeleteTargets } from "./delete-worktree-flow-RxB6NScm.js";
import "./preserved-branch-batch-toast-DHxeGO1o.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function DeleteWorktreeDirtyChangeHint({ changeCount }) {
	if (changeCount === void 0) return null;
	const label = changeCount > 0 ? `${changeCount} uncommitted or untracked ${changeCount === 1 ? "change" : "changes"}` : "Uncommitted or untracked changes";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "mt-1 flex w-fit max-w-full items-center gap-1.5 text-destructive",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-3 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-0 truncate font-medium",
				children: label
			})]
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "top",
		sideOffset: 4,
		children: translate("auto.components.sidebar.DeleteWorktreeDirtyChangeHint.8e2994ce28", "Deleting this workspace permanently removes these changes from disk.")
	})] });
}
function DeleteWorktreeLineageNotice({ descendants, dirtyChangeCountsByWorktreeId }) {
	const childWorkspaceCount = descendants.length;
	if (childWorkspaceCount === 0) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "min-w-0 max-w-full overflow-hidden rounded-md border border-border/70 bg-muted/35 px-3 py-2 text-xs",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-start gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Workflow, { className: "mt-0.5 size-3.5 shrink-0 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 flex-1",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "font-medium text-foreground",
						children: translate("auto.components.sidebar.DeleteWorktreeLineageNotice.a940f3c96e", "Child workspaces will be deleted")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mt-1 text-muted-foreground",
						children: childWorkspaceCount === 1 ? translate("auto.components.sidebar.DeleteWorktreeLineageNotice.66798cc6a2", "Deleting this workspace also deletes 1 child workspace.") : translate("auto.components.sidebar.DeleteWorktreeLineageNotice.29b98bf9cd", "Deleting this workspace also deletes {{value0}} child workspaces.", { value0: childWorkspaceCount })
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mt-2 min-w-0 max-w-full space-y-1 overflow-hidden rounded-sm border border-border/60 bg-background/60 px-2 py-1.5",
						children: [descendants.slice(0, 4).map((child) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 overflow-hidden",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "truncate font-medium text-foreground",
									children: child.displayName
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "truncate text-muted-foreground",
									children: child.path
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DeleteWorktreeDirtyChangeHint, { changeCount: dirtyChangeCountsByWorktreeId.get(child.id) })
							]
						}, child.id)), descendants.length > 4 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "text-muted-foreground",
							children: [
								"+",
								descendants.length - 4,
								" ",
								translate("auto.components.sidebar.DeleteWorktreeLineageNotice.ad407c2d55", "more")
							]
						}) : null]
					})
				]
			})]
		})
	});
}
function DeleteWorktreeSkipConfirmOption({ showDontAskAgain, dontAskAgain, onToggleDontAskAgain }) {
	if (!showDontAskAgain) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
		type: "button",
		role: "checkbox",
		"aria-checked": dontAskAgain,
		onClick: onToggleDontAskAgain,
		className: "flex items-center gap-2 rounded-sm px-1 py-1 text-xs text-foreground/80 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: `flex size-4 items-center justify-center rounded-sm border transition-colors ${dontAskAgain ? "border-foreground bg-foreground text-background" : "border-muted-foreground bg-transparent"}`,
			children: dontAskAgain ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, {
				className: "size-3",
				strokeWidth: 3
			}) : null
		}), translate("auto.components.sidebar.DeleteWorktreeSkipConfirmOption.29aefb7e52", "Don't ask again")]
	});
}
function DeleteWorktreeDialogFooter({ isMainWorktree, isDeleting, canForceDelete, isBatchDelete, worktreeCount, canDeleteAllLineage, lineageDeleteTargetCount, onCancel, onForceDelete, onDelete, confirmButtonRef }) {
	const label = isDeleting ? canForceDelete ? "Force Deleting..." : "Deleting..." : isBatchDelete ? `Delete ${worktreeCount} Workspaces` : canDeleteAllLineage ? `Delete ${lineageDeleteTargetCount} Workspaces` : canForceDelete ? "Force Delete" : "Delete Workspace";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
		variant: "outline",
		onClick: onCancel,
		disabled: isDeleting,
		children: isMainWorktree ? translate("auto.components.sidebar.DeleteWorktreeDialogFooter.cf95e3b5bb", "Close") : translate("auto.components.sidebar.DeleteWorktreeDialogFooter.c0e972d726", "Cancel")
	}), !isMainWorktree && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
		ref: confirmButtonRef,
		variant: "destructive",
		onClick: canForceDelete ? onForceDelete : onDelete,
		disabled: isDeleting,
		children: [isDeleting ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, {}), label]
	})] });
}
function DeleteWorktreeDialogDescription({ targetClassName, targetLabel, canDeleteAllLineage, childTargetLabel, descriptionSuffix }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogDescription, {
		className: "text-xs",
		children: [
			translate("auto.components.sidebar.DeleteWorktreeDialog.91492c9ad6", "Remove"),
			" ",
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: targetClassName,
				children: targetLabel
			}),
			canDeleteAllLineage ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
				" ",
				translate("auto.components.sidebar.DeleteWorktreeDialog.ff2a74ac0e", "and"),
				" ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "font-medium text-foreground",
					children: childTargetLabel
				}),
				" ",
				descriptionSuffix
			] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [" ", descriptionSuffix] })
		]
	});
}
function DeleteWorktreeTargetPreview({ isBatchDelete, worktree, worktrees, deleteStateByWorktreeId, dirtyChangeCountsByWorktreeId }) {
	if (isBatchDelete) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ScrollArea, {
		className: "max-h-48 rounded-md border border-border/70 bg-muted/35 text-xs",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "space-y-1 px-3 py-2",
			children: worktrees.map((item) => {
				const itemDeleteState = deleteStateByWorktreeId[item.id];
				return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "min-w-0 border-b border-border/50 py-1 last:border-0",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex min-w-0 items-start gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 flex-1",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "break-all font-medium text-foreground",
									children: item.displayName
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "mt-0.5 break-all text-muted-foreground",
									children: item.path
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DeleteWorktreeDirtyChangeHint, { changeCount: dirtyChangeCountsByWorktreeId.get(item.id) }),
								itemDeleteState?.error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "mt-1 whitespace-pre-wrap break-all text-destructive",
									children: itemDeleteState.error
								}) : null
							]
						}), itemDeleteState?.isDeleting ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "mt-0.5 size-3.5 shrink-0 animate-spin text-muted-foreground" }) : null]
					})
				}, item.id);
			})
		})
	});
	return worktree ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "rounded-md border border-border/70 bg-muted/35 px-3 py-2 text-xs",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "break-all font-medium text-foreground",
				children: worktree.displayName
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mt-1 break-all text-muted-foreground",
				children: worktree.path
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DeleteWorktreeDirtyChangeHint, { changeCount: dirtyChangeCountsByWorktreeId.get(worktree.id) })
		]
	}) : null;
}
function DeleteWorktreeWarningPanels({ isMainWorktree, mainWorktreeBlocker, deleteError }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [isMainWorktree && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "rounded-md border border-border/70 bg-muted/35 px-3 py-2 text-xs text-muted-foreground",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-start gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-3.5 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 flex-1",
				children: [
					translate("auto.components.sidebar.DeleteWorktreeWarningPanels.e3be9eba15", "This is the"),
					" ",
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "font-semibold text-foreground",
						children: translate("auto.components.sidebar.DeleteWorktreeWarningPanels.c4f96a6e18", "main worktree")
					}),
					" ",
					translate("auto.components.sidebar.DeleteWorktreeWarningPanels.026738155a", "(the original clone directory)."),
					mainWorktreeBlocker ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [" ", mainWorktreeBlocker] }) : null
				]
			})]
		})
	}), deleteError && !isMainWorktree && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "rounded-md border border-destructive/40 bg-destructive/8 px-3 py-2 text-xs text-destructive",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-start gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-3.5 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "min-w-0 flex-1 whitespace-pre-wrap break-all",
				children: deleteError
			})]
		})
	})] });
}
function persistDeleteWorktreeConfirmSkipPreference({ updateSettings, openSettingsPage, openSettingsTarget }) {
	updateSettings({ skipDeleteWorktreeConfirm: true });
	toast.success(translate("auto.components.sidebar.DeleteWorktreeDialog.dd3a45bbbd", "We'll skip this confirmation next time."), {
		description: translate("auto.components.sidebar.DeleteWorktreeDialog.2b56b35f53", "You can change this in Settings."),
		duration: 8e3,
		action: {
			label: translate("auto.components.sidebar.DeleteWorktreeDialog.5cc1a6701c", "Open Settings"),
			onClick: () => {
				openSettingsPage();
				openSettingsTarget({
					pane: "general",
					repoId: null,
					sectionId: "general-skip-delete-worktree-confirm"
				});
			}
		}
	});
}
function isFolderWorkspaceDelete(repoMap, worktree) {
	if (!worktree) return false;
	const repo = repoMap.get(worktree.repoId);
	return repo ? isFolderRepo(repo) : false;
}
function countFolderWorkspaceDeletes(repoMap, worktrees) {
	return worktrees.filter((item) => isFolderWorkspaceDelete(repoMap, item)).length;
}
function getDeleteWorktreeDialogCopy(args) {
	const allFolderWorkspaceDeletes = args.isBatchDelete && args.worktreeCount > 0 && args.folderWorkspaceDeleteCount === args.worktreeCount;
	const mixedFolderWorkspaceDeletes = args.isBatchDelete && args.folderWorkspaceDeleteCount > 0 && args.folderWorkspaceDeleteCount < args.worktreeCount;
	return {
		targetLabel: args.isBatchDelete ? `${args.worktreeCount} workspaces` : args.worktree?.displayName,
		targetClassName: args.isBatchDelete ? "font-medium text-foreground" : "break-all font-medium text-foreground",
		descriptionSuffix: args.isBatchDelete ? allFolderWorkspaceDeletes ? "from Orca. Project folders on disk will not be deleted." : mixedFolderWorkspaceDeletes ? "from Orca. Git worktrees will also be removed from git and disk; folder workspaces will only remove the Orca workspace entry." : "from git and delete their workspace folders." : args.isFolderWorkspaceDelete ? "from Orca. The project folder on disk will not be deleted." : "from git and delete its workspace folder.",
		mainWorktreeBlocker: args.isFolderWorkspaceDelete ? "Remove the folder project instead of deleting this workspace." : "Git does not allow removing the main worktree."
	};
}
function getDeleteWorktreeLineageDialogCopy(args) {
	const allFolderWorkspaceDeletes = args.deleteTargetCount > 0 && args.folderWorkspaceDeleteCount === args.deleteTargetCount;
	const mixedFolderWorkspaceDeletes = args.folderWorkspaceDeleteCount > 0 && args.folderWorkspaceDeleteCount < args.deleteTargetCount;
	return {
		childTargetLabel: args.childWorkspaceCount === 1 ? "1 child workspace" : `${args.childWorkspaceCount} child workspaces`,
		descriptionSuffix: allFolderWorkspaceDeletes ? "from Orca. Project folders on disk will not be deleted." : mixedFolderWorkspaceDeletes ? "from Orca. Git worktrees will also be removed from git and disk; folder workspaces will only remove the Orca workspace entry." : "from git and delete their workspace folders."
	};
}
function getDeleteWorktreeDirtyChangeCounts({ deleteTargets, deleteStateByWorktreeId, gitStatusByWorktree, repoMap }) {
	const result = /* @__PURE__ */ new Map();
	for (const item of deleteTargets) {
		if (item.isMainWorktree || isFolderWorkspaceDelete(repoMap, item)) continue;
		const forceDeleteReason = deleteStateByWorktreeId[item.id]?.forceDeleteReason;
		const changeCount = gitStatusByWorktree[item.id]?.length;
		if ((changeCount ?? 0) > 0) result.set(item.id, changeCount ?? 0);
		else if (forceDeleteReason === "dirty") result.set(item.id, 0);
	}
	return result;
}
function useDeleteWorktreeStatusHydration({ isOpen, deleteTargets, repoMap }) {
	const repos = useAppStore((state) => state.repos);
	const settings = useAppStore((state) => state.settings);
	const setGitStatus = useAppStore((state) => state.setGitStatus);
	(0, import_react.useEffect)(() => {
		if (!isOpen) return;
		const gitStatusByWorktree = useAppStore.getState().gitStatusByWorktree;
		const targets = deleteTargets.filter((target) => !target.isMainWorktree && !isFolderWorkspaceDelete(repoMap, target) && gitStatusByWorktree[target.id] === void 0);
		const controller = new AbortController();
		for (const target of targets) getRuntimeGitStatus({
			settings: getSettingsForWorktreeRuntimeOwner({
				repos,
				settings,
				worktreesByRepo: useAppStore.getState().worktreesByRepo
			}, target.id),
			worktreeId: target.id,
			worktreePath: target.path,
			connectionId: getConnectionId(target.id) ?? void 0
		}, { signal: controller.signal }).then((status) => {
			if (!controller.signal.aborted) setGitStatus(target.id, status);
		}).catch(() => {});
		return () => {
			controller.abort();
		};
	}, [
		deleteTargets,
		isOpen,
		repoMap,
		repos,
		setGitStatus,
		settings
	]);
}
function useConfirmedWorktreeDeleteTargets({ worktreeIdentityData, lineageIdentityData, closeModal }) {
	return {
		worktreeDeleteIdentities: (0, import_react.useMemo)(() => readWorktreeDeleteIdentities(worktreeIdentityData), [worktreeIdentityData]),
		lineageDeleteIdentities: (0, import_react.useMemo)(() => readWorktreeDeleteIdentities(lineageIdentityData), [lineageIdentityData]),
		resolveConfirmedTargets: (0, import_react.useCallback)((identities, expectedCount) => {
			const targets = resolveWorktreeBatchDeleteTargets(identities, getWorktreeMapFromState(useAppStore.getState()));
			if (!targets || targets.length !== expectedCount) {
				showWorkspaceListChangedToast();
				closeModal();
				return null;
			}
			return targets;
		}, [closeModal])
	};
}
var DeleteWorktreeDialog_default = import_react.memo(function DeleteWorktreeDialog$1() {
	const activeModal = useAppStore((s) => s.activeModal);
	const modalData = useAppStore((s) => s.modalData);
	const closeModal = useAppStore((s) => s.closeModal);
	const removeWorktree = useAppStore((s) => s.removeWorktree);
	const clearWorktreeDeleteState = useAppStore((s) => s.clearWorktreeDeleteState);
	const allWorktrees = useAllWorktrees();
	const repos = useAppStore((s) => s.repos);
	const worktreeLineageById = useAppStore((s) => s.worktreeLineageById);
	const updateSettings = useAppStore((s) => s.updateSettings);
	const openSettingsTarget = useAppStore((s) => s.openSettingsTarget);
	const openSettingsPage = useAppStore((s) => s.openSettingsPage);
	const gitStatusByWorktree = useAppStore((s) => s.gitStatusByWorktree);
	const isOpen = activeModal === "delete-worktree";
	const worktreeId = typeof modalData.worktreeId === "string" ? modalData.worktreeId : "";
	const worktreeIds = (0, import_react.useMemo)(() => Array.isArray(modalData.worktreeIds) ? modalData.worktreeIds.filter((id) => typeof id === "string") : worktreeId ? [worktreeId] : [], [modalData.worktreeIds, worktreeId]);
	const { worktreeDeleteIdentities, lineageDeleteIdentities, resolveConfirmedTargets } = useConfirmedWorktreeDeleteTargets({
		worktreeIdentityData: modalData.worktreeDeleteIdentities,
		lineageIdentityData: modalData.lineageDeleteIdentities,
		closeModal
	});
	const onDeleted = typeof modalData.onDeleted === "function" ? modalData.onDeleted : null;
	const worktree = (0, import_react.useMemo)(() => worktreeId ? allWorktrees.find((item) => item.id === worktreeId) ?? null : null, [allWorktrees, worktreeId]);
	const worktrees = (0, import_react.useMemo)(() => {
		if (worktreeIds.length === 0) return [];
		const selected = new Set(worktreeIds);
		return allWorktrees.filter((item) => selected.has(item.id));
	}, [allWorktrees, worktreeIds]);
	const repoMap = (0, import_react.useMemo)(() => new Map(repos.map((repo) => [repo.id, repo])), [repos]);
	const isBatchDelete = worktreeIds.length > 1;
	const isFolderWorkspaceDelete$1 = !isBatchDelete && isFolderWorkspaceDelete(repoMap, worktree);
	const folderWorkspaceDeleteCount = (0, import_react.useMemo)(() => countFolderWorkspaceDeletes(repoMap, worktrees), [repoMap, worktrees]);
	const deleteCopy = getDeleteWorktreeDialogCopy({
		isBatchDelete,
		worktree,
		worktreeCount: worktrees.length,
		folderWorkspaceDeleteCount,
		isFolderWorkspaceDelete: isFolderWorkspaceDelete$1
	});
	const deleteStateByWorktreeId = useAppStore((s) => s.deleteStateByWorktreeId);
	const lineageDelete = (0, import_react.useMemo)(() => !isBatchDelete && worktree ? getWorkspaceDeleteLineage(worktree, allWorktrees, worktreeLineageById) : {
		descendants: [],
		deleteAllTargets: []
	}, [
		allWorktrees,
		isBatchDelete,
		worktree,
		worktreeLineageById
	]);
	const deleteStateIds = (0, import_react.useMemo)(() => Array.from(new Set([...worktreeIds, ...lineageDelete.deleteAllTargets.map((target) => target.id)])), [lineageDelete.deleteAllTargets, worktreeIds]);
	const deleteStates = (0, import_react.useMemo)(() => deleteStateIds.map((id) => deleteStateByWorktreeId[id]).filter((state) => state != null), [deleteStateByWorktreeId, deleteStateIds]);
	const deleteState = worktreeId ? deleteStateByWorktreeId[worktreeId] : void 0;
	const isDeleting = deleteStates.some((state) => state.isDeleting);
	const deleteError = !isBatchDelete ? deleteState?.error ?? null : null;
	const canForceDelete = !isBatchDelete && (deleteState?.canForceDelete ?? false);
	const confirmButtonRef = (0, import_react.useRef)(null);
	const isMainWorktree = !isBatchDelete && (worktree?.isMainWorktree ?? false);
	const childWorkspaceCount = lineageDelete.descendants.length;
	const hasLineageChildren = childWorkspaceCount > 0;
	const canDeleteAllLineage = !isMainWorktree && !isBatchDelete && lineageDelete.deleteAllTargets.length > 1;
	const lineageFolderWorkspaceDeleteCount = (0, import_react.useMemo)(() => countFolderWorkspaceDeletes(repoMap, lineageDelete.deleteAllTargets), [lineageDelete.deleteAllTargets, repoMap]);
	const lineageDeleteCopy = getDeleteWorktreeLineageDialogCopy({
		childWorkspaceCount,
		deleteTargetCount: lineageDelete.deleteAllTargets.length,
		folderWorkspaceDeleteCount: lineageFolderWorkspaceDeleteCount
	});
	const allowSkipConfirm = !isBatchDelete && modalData.allowSkipConfirm !== false && childWorkspaceCount === 0;
	const [dontAskAgain, setDontAskAgain] = (0, import_react.useState)(false);
	const deleteTargets = (0, import_react.useMemo)(() => canDeleteAllLineage ? lineageDelete.deleteAllTargets : worktrees, [
		canDeleteAllLineage,
		lineageDelete.deleteAllTargets,
		worktrees
	]);
	const dirtyChangeCountsByWorktreeId = (0, import_react.useMemo)(() => {
		return getDeleteWorktreeDirtyChangeCounts({
			deleteTargets,
			deleteStateByWorktreeId,
			gitStatusByWorktree,
			repoMap
		});
	}, [
		deleteStateByWorktreeId,
		deleteTargets,
		gitStatusByWorktree,
		repoMap
	]);
	useDeleteWorktreeStatusHydration({
		isOpen,
		deleteTargets,
		repoMap
	});
	if (!isOpen && dontAskAgain) setDontAskAgain(false);
	(0, import_react.useEffect)(() => {
		if (isOpen && worktreeIds.length > 0 && worktrees.length === 0 && !isDeleting) {
			for (const id of worktreeIds) clearWorktreeDeleteState(id);
			closeModal();
		}
	}, [
		clearWorktreeDeleteState,
		closeModal,
		isDeleting,
		isOpen,
		worktreeIds,
		worktreeIds.length,
		worktrees.length
	]);
	const handleOpenChange = (0, import_react.useCallback)((open) => {
		if (open) return;
		const currentState = worktreeId ? useAppStore.getState().deleteStateByWorktreeId[worktreeId] : void 0;
		if (isBatchDelete) {
			const state = useAppStore.getState().deleteStateByWorktreeId;
			for (const id of worktreeIds) if (!state[id]?.isDeleting) clearWorktreeDeleteState(id);
		} else if (worktreeId && !currentState?.isDeleting) clearWorktreeDeleteState(worktreeId);
		closeModal();
	}, [
		clearWorktreeDeleteState,
		closeModal,
		isBatchDelete,
		worktreeId,
		worktreeIds
	]);
	const persistDontAskAgainPreference = (0, import_react.useCallback)(() => {
		persistDeleteWorktreeConfirmSkipPreference({
			updateSettings,
			openSettingsPage,
			openSettingsTarget
		});
	}, [
		openSettingsPage,
		openSettingsTarget,
		updateSettings
	]);
	const handleForceDeletedFromToast = (0, import_react.useCallback)((deletedId) => {
		onDeleted?.([deletedId]);
	}, [onDeleted]);
	const handleDelete = (0, import_react.useCallback)((force = false) => {
		if (worktreeIds.length === 0) return;
		const currentWorktrees = resolveConfirmedTargets(worktreeDeleteIdentities, worktreeIds.length);
		if (!currentWorktrees) return;
		if (dontAskAgain && allowSkipConfirm && !force) persistDontAskAgainPreference();
		if (force) {
			const commitFocus = prepareActiveWorktreeFocusAfterDelete(worktreeId);
			const deletePromise = removeWorktree(worktreeId, true, { allowUnverifiedPtyStop: true });
			closeModal();
			deletePromise.then((result) => {
				if (!result.ok) {
					toast.error(translate("auto.components.sidebar.DeleteWorktreeDialog.42e610d6cf", "Force delete failed"), { description: result.error });
					return;
				}
				commitFocus();
				onDeleted?.([worktreeId]);
			}).catch((err) => {
				toast.error(translate("auto.components.sidebar.DeleteWorktreeDialog.4f6750ca7b", "Failed to delete workspace"), { description: err instanceof Error ? err.message : String(err) });
			});
		} else {
			const deletePromise = runWorktreeDeletesInParallel(currentWorktrees, {
				force: true,
				onForceDeleted: handleForceDeletedFromToast
			});
			closeModal();
			deletePromise.then((deletedIds) => {
				if (deletedIds.length > 0) onDeleted?.(deletedIds);
			});
		}
	}, [
		closeModal,
		dontAskAgain,
		allowSkipConfirm,
		handleForceDeletedFromToast,
		onDeleted,
		persistDontAskAgainPreference,
		removeWorktree,
		worktreeIds.length,
		worktreeDeleteIdentities,
		worktreeId,
		resolveConfirmedTargets
	]);
	const handleDeleteAll = (0, import_react.useCallback)(() => {
		if (lineageDelete.deleteAllTargets.length <= 1) return;
		const currentTargets = resolveConfirmedTargets(lineageDeleteIdentities, lineageDelete.deleteAllTargets.length);
		if (!currentTargets) return;
		const deletePromise = runWorktreeDeletesInParallel(currentTargets, {
			force: true,
			onForceDeleted: handleForceDeletedFromToast
		});
		closeModal();
		deletePromise.then((deletedIds) => {
			if (deletedIds.length > 0) onDeleted?.(deletedIds);
		});
	}, [
		closeModal,
		handleForceDeletedFromToast,
		lineageDelete.deleteAllTargets.length,
		lineageDeleteIdentities,
		onDeleted,
		resolveConfirmedTargets
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: isOpen,
		onOpenChange: handleOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "max-w-md",
			onOpenAutoFocus: (event) => {
				if (isMainWorktree) return;
				event.preventDefault();
				confirmButtonRef.current?.focus();
			},
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
					className: "text-sm",
					children: isBatchDelete ? translate("auto.components.sidebar.DeleteWorktreeDialog.86f0ae1257", "Delete Workspaces") : translate("auto.components.sidebar.DeleteWorktreeDialog.fc23c4cbdf", "Delete Workspace")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DeleteWorktreeDialogDescription, {
					targetClassName: deleteCopy.targetClassName,
					targetLabel: deleteCopy.targetLabel,
					canDeleteAllLineage,
					childTargetLabel: lineageDeleteCopy.childTargetLabel,
					descriptionSuffix: canDeleteAllLineage ? lineageDeleteCopy.descriptionSuffix : deleteCopy.descriptionSuffix
				})] }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DeleteWorktreeTargetPreview, {
					isBatchDelete,
					worktree,
					worktrees,
					deleteStateByWorktreeId,
					dirtyChangeCountsByWorktreeId
				}),
				hasLineageChildren && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DeleteWorktreeLineageNotice, {
					descendants: lineageDelete.descendants,
					dirtyChangeCountsByWorktreeId
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DeleteWorktreeWarningPanels, {
					isMainWorktree,
					mainWorktreeBlocker: deleteCopy.mainWorktreeBlocker,
					deleteError
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DeleteWorktreeSkipConfirmOption, {
					showDontAskAgain: !isMainWorktree && allowSkipConfirm && !canForceDelete,
					dontAskAgain,
					onToggleDontAskAgain: () => setDontAskAgain((prev) => !prev)
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogFooter, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DeleteWorktreeDialogFooter, {
					isMainWorktree,
					isDeleting,
					canForceDelete,
					isBatchDelete,
					worktreeCount: worktrees.length,
					canDeleteAllLineage,
					lineageDeleteTargetCount: lineageDelete.deleteAllTargets.length,
					onCancel: () => handleOpenChange(false),
					onForceDelete: () => handleDelete(true),
					onDelete: canDeleteAllLineage ? handleDeleteAll : () => handleDelete(false),
					confirmButtonRef
				}) })
			]
		})
	});
});
export { DeleteWorktreeDialog_default as default };
