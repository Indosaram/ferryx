import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as CircleQuestionMark } from "./circle-question-mark-Cytc5uFO.js";
import { t as EyeOff } from "./eye-off-4tMqi6LV.js";
import { t as Eye } from "./eye-BvQpbQVj.js";
import { bp as getRepoHostIdentity, qg as isGitRepoKind, t as useAppStore, yp as findRepoForHost, zp as relativePathInsideRoot } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import { st as parseExecutionHostId, tt as getWorktreeExecutionHostId } from "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { i as PopoverTrigger, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import { t as Switch } from "./switch-NhZdOYtg.js";
import "./useMountedRef-1omUd-IV.js";
import { t as useVirtualizer } from "./esm-DQfOTgcy.js";
import { i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { i as getVisibleNonOrcaWorktrees, l as isLegacyRepoForExternalWorktreeVisibility, n as getHiddenImportableExternalWorktrees, o as effectiveAgentWorktreeVisibility, s as effectiveExternalWorktreeVisibility } from "./worktree-ownership-B1VtdtJF.js";
import { t as importNewExternalWorktreeInboxPaths } from "./new-external-worktrees-inbox-actions-_hRPSCBp.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function WorktreeVisibilityHelpPopover() {
	const [open, setOpen] = (0, import_react.useState)(false);
	const title = translate("auto.components.sidebar.WorktreeVisibilityHelpPopover.c41f2d7e90", "Which worktrees are hidden by default?");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "inline-flex",
		onPointerEnter: () => setOpen(true),
		onPointerLeave: () => setOpen(false),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
			open,
			onOpenChange: setOpen,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "ghost",
					size: "icon-xs",
					className: "size-5 text-muted-foreground hover:bg-transparent hover:text-foreground dark:hover:bg-transparent",
					"aria-label": title,
					"aria-expanded": open,
					title,
					onClick: () => setOpen(true),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleQuestionMark, { className: "size-3.5" })
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(PopoverContent, {
				"aria-label": title,
				align: "start",
				side: "bottom",
				sideOffset: 6,
				className: "w-80 p-3",
				onOpenAutoFocus: (event) => event.preventDefault(),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "text-sm font-medium",
					children: title
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("ul", {
					className: "mt-2 grid list-disc gap-2 pl-4 text-xs leading-5 text-muted-foreground text-pretty",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: translate("auto.components.sidebar.WorktreeVisibilityHelpPopover.8db4e19a26", "This setting never hides worktrees created through Orca.") }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: translate("auto.components.sidebar.WorktreeVisibilityHelpPopover.54a7c2f183", "For newly added repositories, worktrees created manually or by another tool are hidden by default to avoid unexpected sidebar clutter.") }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: translate("auto.components.sidebar.WorktreeVisibilityHelpPopover.b9e0d43675", "Always show includes current and future non-Orca worktrees, including agent worktrees under .claude/worktrees/* and .gsd-workspaces/*. When it is off, use Show below to add one individually.") })
					]
				})]
			})]
		})
	});
}
function resolveWorktreeVisibilityHostTarget(state, repoId, modalHostId) {
	const requestedHostId = typeof modalHostId === "string" ? parseExecutionHostId(modalHostId)?.id : void 0;
	const repo = findRepoForHost(state.repos, repoId, {
		hostId: requestedHostId,
		settings: state.settings
	});
	const detectedForRepo = repoId ? state.detectedWorktreesByRepo[repoId] : void 0;
	return {
		detected: detectedForRepo && repo && requestedHostId ? {
			...detectedForRepo,
			worktrees: detectedForRepo.worktrees.filter((worktree) => getWorktreeExecutionHostId(worktree, repo) === requestedHostId)
		} : detectedForRepo,
		repo,
		requestedHostId,
		scope: repo ? getRepoHostIdentity(repo) : `${requestedHostId ?? ""}\0${repoId}`
	};
}
function useWorktreeVisibilityHostActions(fetchWorktrees, updateRepo, requestedHostId) {
	return {
		refreshTargetRepo: (0, import_react.useCallback)((repoId, options) => fetchWorktrees(repoId, {
			...options,
			...requestedHostId ? { executionHostId: requestedHostId } : {}
		}), [fetchWorktrees, requestedHostId]),
		updateTargetRepo: (0, import_react.useCallback)((repoId, updates) => requestedHostId ? updateRepo(repoId, updates, { hostId: requestedHostId }) : updateRepo(repoId, updates), [requestedHostId, updateRepo])
	};
}
var activeMutations = /* @__PURE__ */ new Map();
var mutationListeners = /* @__PURE__ */ new Map();
function getActiveVisibilityMutation(scope) {
	return activeMutations.get(scope);
}
function startVisibilityMutation(scope, mutation) {
	activeMutations.set(scope, mutation);
}
function subscribeToVisibilityMutation(scope, listener) {
	const listeners = mutationListeners.get(scope) ?? /* @__PURE__ */ new Set();
	listeners.add(listener);
	mutationListeners.set(scope, listeners);
	return () => {
		listeners.delete(listener);
		if (listeners.size === 0) mutationListeners.delete(scope);
	};
}
function finishVisibilityMutation(scope, mutation) {
	if (activeMutations.get(scope) !== mutation) return;
	activeMutations.delete(scope);
	mutationListeners.get(scope)?.forEach((listener) => listener());
}
function useVisibilityMutationFence(args) {
	const { currentScopeRef, refresh, repoId, scope, setActionState, setBusyPath, setIsToggling, setListState } = args;
	(0, import_react.useEffect)(() => {
		const activeMutation = getActiveVisibilityMutation(scope);
		setActionState(null);
		setBusyPath(activeMutation?.kind === "row" ? activeMutation.path : null);
		setIsToggling(activeMutation?.kind === "toggle");
		if (!activeMutation) return;
		let cancelled = false;
		let unsubscribe = () => void 0;
		unsubscribe = subscribeToVisibilityMutation(scope, () => {
			unsubscribe();
			refresh(repoId, { requireAuthoritative: true }).then((refreshed) => {
				if (!cancelled && currentScopeRef.current === scope) {
					setListState(refreshed ? "ready" : "failed");
					setBusyPath(null);
					setIsToggling(false);
				}
			});
		});
		return () => {
			cancelled = true;
			unsubscribe();
		};
	}, [
		currentScopeRef,
		refresh,
		repoId,
		scope,
		setActionState,
		setBusyPath,
		setIsToggling,
		setListState
	]);
}
function WorktreeVisibilityDialog() {
	const activeModal = useAppStore((s) => s.activeModal);
	const modalData = useAppStore((s) => s.modalData);
	const closeModal = useAppStore((s) => s.closeModal);
	const repos = useAppStore((s) => s.repos);
	const updateRepo = useAppStore((s) => s.updateRepo);
	const fetchWorktrees = useAppStore((s) => s.fetchWorktrees);
	const detectedWorktreesByRepo = useAppStore((s) => s.detectedWorktreesByRepo);
	const settings = useAppStore((s) => s.settings);
	const [actionState, setActionState] = (0, import_react.useState)(null);
	const [busyPath, setBusyPath] = (0, import_react.useState)(null);
	const [isToggling, setIsToggling] = (0, import_react.useState)(false);
	const [listState, setListState] = (0, import_react.useState)("checking");
	const alwaysShowSwitchId = (0, import_react.useId)();
	const hiddenListHeadingId = (0, import_react.useId)();
	const hiddenListRef = (0, import_react.useRef)(null);
	const isOpen = activeModal === "worktree-visibility";
	const repoId = typeof modalData.repoId === "string" ? modalData.repoId : "";
	const { detected, repo, requestedHostId, scope: mutationScope } = resolveWorktreeVisibilityHostTarget({
		repos,
		settings,
		detectedWorktreesByRepo
	}, repoId, modalData.hostId);
	const currentMutationScopeRef = (0, import_react.useRef)(mutationScope);
	const activeMutation = getActiveVisibilityMutation(mutationScope);
	const effectiveBusyPath = busyPath ?? (activeMutation?.kind === "row" ? activeMutation.path : null);
	const effectivelyToggling = isToggling || activeMutation?.kind === "toggle";
	const showOther = repo ? effectiveExternalWorktreeVisibility(repo, isLegacyRepoForExternalWorktreeVisibility(repo)) === "show" : false;
	const showAgentScratch = repo ? effectiveAgentWorktreeVisibility(repo) === "show" : false;
	const alwaysShow = showOther && showAgentScratch;
	const hiddenImportable = getHiddenImportableExternalWorktrees(detected);
	const hiddenCount = hiddenImportable.length;
	const otherCount = getVisibleNonOrcaWorktrees(detected).length;
	const hiddenWorktreeLabel = `${hiddenCount} ${hiddenCount === 1 ? "worktree" : "worktrees"}`;
	const shownWorktreeLabel = `${otherCount} ${otherCount === 1 ? "worktree" : "worktrees"}`;
	const hiddenListVirtualizer = useVirtualizer({
		count: hiddenImportable.length,
		getScrollElement: () => hiddenListRef.current,
		estimateSize: () => 56,
		getItemKey: (index) => hiddenImportable[index]?.id ?? index,
		overscan: 3,
		initialRect: {
			width: 480,
			height: 224
		}
	});
	(0, import_react.useLayoutEffect)(() => {
		currentMutationScopeRef.current = mutationScope;
	}, [mutationScope]);
	const { refreshTargetRepo, updateTargetRepo } = useWorktreeVisibilityHostActions(fetchWorktrees, updateRepo, requestedHostId);
	useVisibilityMutationFence({
		scope: mutationScope,
		repoId,
		currentScopeRef: currentMutationScopeRef,
		refresh: refreshTargetRepo,
		setActionState,
		setBusyPath,
		setIsToggling,
		setListState
	});
	(0, import_react.useEffect)(() => {
		if (!isOpen || !repoId) return;
		if (getActiveVisibilityMutation(mutationScope)) return;
		let cancelled = false;
		setListState("checking");
		refreshTargetRepo(repoId, { requireAuthoritative: true }).then((refreshed) => {
			if (!cancelled) setListState(refreshed ? "ready" : "failed");
		});
		return () => {
			cancelled = true;
		};
	}, [
		isOpen,
		mutationScope,
		refreshTargetRepo,
		repoId
	]);
	const handleRetryList = (0, import_react.useCallback)(async () => {
		if (!repoId) return;
		setListState("checking");
		const refreshed = await refreshTargetRepo(repoId, { requireAuthoritative: true });
		if (currentMutationScopeRef.current === mutationScope) setListState(refreshed ? "ready" : "failed");
	}, [
		mutationScope,
		refreshTargetRepo,
		repoId
	]);
	const handleShowWorktree = (0, import_react.useCallback)(async (worktreePath) => {
		if (!repo) return;
		const mutation = {
			kind: "row",
			path: worktreePath
		};
		const targetMutationScope = getRepoHostIdentity(repo);
		startVisibilityMutation(targetMutationScope, mutation);
		setBusyPath(worktreePath);
		try {
			await importNewExternalWorktreeInboxPaths({
				projectId: repo.id,
				repo,
				worktreePaths: [worktreePath],
				updateRepo: updateTargetRepo,
				fetchWorktrees: refreshTargetRepo,
				setInboxState: (_projectId, state) => {
					if (currentMutationScopeRef.current !== targetMutationScope) return;
					setActionState(state);
					if (state === null) setListState("ready");
				}
			});
		} finally {
			finishVisibilityMutation(targetMutationScope, mutation);
			if (currentMutationScopeRef.current === targetMutationScope) setBusyPath(null);
		}
	}, [
		refreshTargetRepo,
		repo,
		updateTargetRepo
	]);
	const handleAlwaysShowChange = (0, import_react.useCallback)(async (checked) => {
		if (!repoId || checked === alwaysShow) return;
		const mutation = { kind: "toggle" };
		startVisibilityMutation(mutationScope, mutation);
		setActionState(null);
		setIsToggling(true);
		try {
			if (!await updateTargetRepo(repoId, {
				externalWorktreeVisibility: checked ? "show" : "hide",
				agentWorktreeVisibility: checked ? "show" : "hide",
				...checked ? { externalWorktreeDiscoverySuppressedAt: null } : {}
			})) {
				if (currentMutationScopeRef.current === mutationScope) setActionState({
					pending: false,
					error: translate("auto.components.sidebar.WorktreeVisibilityDialog.d40d436fc2", "Could not update worktree visibility. Try again.")
				});
				return;
			}
			const refreshed = await refreshTargetRepo(repoId, { requireAuthoritative: true });
			if (currentMutationScopeRef.current === mutationScope) setListState(refreshed ? "ready" : "failed");
		} finally {
			finishVisibilityMutation(mutationScope, mutation);
			if (currentMutationScopeRef.current === mutationScope) setIsToggling(false);
		}
	}, [
		alwaysShow,
		mutationScope,
		refreshTargetRepo,
		repoId,
		updateTargetRepo
	]);
	if (!isOpen || !repo || !isGitRepoKind(repo)) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: true,
		onOpenChange: (open) => !open && closeModal(),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "sm:max-w-lg",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-1.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.WorktreeVisibilityDialog.83a5ba8dd1", "Non-Orca worktrees") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeVisibilityHelpPopover, {})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: repo.displayName })] }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-3 rounded-lg border border-border bg-muted/30 p-3",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "flex size-8 shrink-0 items-center justify-center rounded-md bg-background text-muted-foreground",
							children: alwaysShow ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Eye, { className: "size-4" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EyeOff, { className: "size-4" })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 flex-1",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-sm font-medium",
								children: alwaysShow ? translate("auto.components.sidebar.WorktreeVisibilityDialog.3e045d4cb8", "Shown in sidebar") : translate("auto.components.sidebar.WorktreeVisibilityDialog.5d02a5647f", "Hidden from sidebar")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-xs text-muted-foreground",
								children: alwaysShow ? translate("auto.components.sidebar.WorktreeVisibilityDialog.8372e4bbd9", "{{value0}} currently shown", { value0: shownWorktreeLabel }) : translate("auto.components.sidebar.WorktreeVisibilityDialog.25ddf19920", "{{value0}} currently hidden", { value0: hiddenWorktreeLabel })
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Label, {
							htmlFor: alwaysShowSwitchId,
							className: "shrink-0 gap-2",
							children: [translate("auto.components.sidebar.WorktreeVisibilityDialog.f1f71b9f02", "Always show"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Switch, {
								id: alwaysShowSwitchId,
								checked: alwaysShow,
								disabled: effectiveBusyPath !== null || effectivelyToggling || listState === "checking",
								onCheckedChange: (checked) => void handleAlwaysShowChange(checked)
							})]
						})
					]
				}),
				listState === "checking" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					"aria-live": "polite",
					className: "text-xs text-muted-foreground",
					children: translate("auto.components.sidebar.WorktreeVisibilityDialog.a3f19c07d2", "Checking…")
				}) : null,
				listState === "failed" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex min-w-0 items-center gap-3",
					role: "alert",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "min-w-0 flex-1 text-xs text-destructive",
						children: translate("auto.components.sidebar.WorktreeVisibilityDialog.b8d24e61f5", "Could not list this repo's worktrees.")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "outline",
						size: "sm",
						disabled: effectiveBusyPath !== null || effectivelyToggling,
						onClick: handleRetryList,
						children: translate("auto.components.sidebar.WorktreeVisibilityDialog.c5e70a93b1", "Try again")
					})]
				}) : null,
				hiddenImportable.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "grid min-w-0 gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
						id: hiddenListHeadingId,
						className: "text-sm font-medium",
						children: translate("auto.components.sidebar.WorktreeVisibilityDialog.7d21c5e848", "Hidden worktrees ({{value0}})", { value0: hiddenImportable.length })
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-muted-foreground",
						children: translate("auto.components.sidebar.WorktreeVisibilityDialog.9b53f7a160", "Choose which hidden worktrees to show individually.")
					})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						ref: hiddenListRef,
						"aria-labelledby": hiddenListHeadingId,
						className: "scrollbar-sleek max-h-56 min-w-0 overflow-y-auto",
						tabIndex: 0,
						style: { height: `${Math.min(hiddenListVirtualizer.getTotalSize(), 224)}px` },
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
							className: "relative min-w-0",
							style: { height: `${hiddenListVirtualizer.getTotalSize()}px` },
							children: hiddenListVirtualizer.getVirtualItems().map((virtualRow) => {
								const worktree = hiddenImportable[virtualRow.index];
								if (!worktree) return null;
								const displayPath = relativePathInsideRoot(repo.path, worktree.path) || worktree.path;
								return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", {
									ref: hiddenListVirtualizer.measureElement,
									"data-index": virtualRow.index,
									className: "absolute left-0 top-0 w-full pb-1",
									style: { transform: `translateY(${virtualRow.start}px)` },
									"aria-posinset": virtualRow.index + 1,
									"aria-setsize": hiddenImportable.length,
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
										className: "flex min-w-0 items-center gap-3 rounded-md border border-border px-3 py-2",
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
											className: "min-w-0 flex-1",
											children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
												className: "truncate text-sm",
												children: worktree.displayName
											}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
												className: "truncate text-xs text-muted-foreground",
												children: displayPath
											})]
										}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
											type: "button",
											variant: "outline",
											size: "sm",
											disabled: effectiveBusyPath !== null || effectivelyToggling || listState === "checking",
											onClick: () => void handleShowWorktree(worktree.path),
											children: effectiveBusyPath === worktree.path ? translate("auto.components.sidebar.WorktreeVisibilityDialog.2f80cd4b97", "Showing…") : translate("auto.components.sidebar.WorktreeVisibilityDialog.e64b81d3a9", "Show")
										})]
									})
								}, worktree.id);
							})
						})
					})]
				}) : null,
				actionState?.error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-xs text-destructive",
					role: "alert",
					children: actionState.error
				}) : null
			]
		})
	});
}
export { WorktreeVisibilityDialog as default };
