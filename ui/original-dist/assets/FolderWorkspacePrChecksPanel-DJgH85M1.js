import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { c as PullRequestIcon, n as CHECK_ICON, r as ChecksList, t as CHECK_COLOR, u as prStateColor } from "./checks-panel-content-22yCc4aJ.js";
import { t as ChevronRight } from "./chevron-right-CZtMe6Ev.js";
import "./check-job-log-tail-DmWxGXaB.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as GitMerge } from "./git-merge-CF2lZKL1.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { Jt as openHttpLink, Kg as isFolderRepo, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./checkbox-PAbetBh2.js";
import "./context-menu-D4RKI7hR.js";
import "./dropdown-menu-Dth6LPK-.js";
import "./popover-CgR1mzy7.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import "./selectors-XOBeaOSb.js";
import "./localized-catalog-DubKHKUR.js";
import { t as getWorktreeGitIdentityDisplay } from "./worktree-git-identity-display-B29QxW_l.js";
import "./ShortcutKeyCombo-Ch456Md0.js";
import "./dialog-BbelfMSB.js";
import "./lib-CtirWBBB.js";
import "./lib-D08jHVMa.js";
import "./purify.es-C_rn83UJ.js";
import "./MermaidBlock-gW3wAx0A.js";
import "./CommentMarkdown-bsrexQcY.js";
import { r as getParentPrChecksRefreshIdentity, t as buildParentPrChecksProjection } from "./parent-pr-checks-rows-Dj1wEgdy.js";
import { t as compareWorktreeDisplayName } from "./worktree-display-name-order-AUI-ZkJy.js";
import "./comment-body-submit-state-BHQDrSxB.js";
import { t as getAttachedWorktreesForFolderWorkspace } from "./folder-workspace-attached-worktrees-CE59j94w.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function FolderWorkspacePrChecksRow({ row, expanded, onToggle, onLoadCheckDetails }) {
	const ReviewIcon = row.provider === "gitlab" ? GitMerge : PullRequestIcon;
	const StatusIcon = CHECK_ICON[row.checkTone] ?? CHECK_ICON.neutral;
	const showStatusIcon = row.checkTone !== "neutral";
	const animateStatusIcon = row.checkTone === "pending";
	const reviewProviderLabel = row.provider === "gitlab" ? "MR" : "PR";
	const toggleDetailsLabel = expanded ? translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.hideDetails", "Hide {{value0}} PR check details", { value0: row.worktree.displayName }) : translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.showDetails", "Show {{value0}} PR check details", { value0: row.worktree.displayName });
	const openExternalLabel = translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.openReviewExternally", "Open {{value0}} externally", { value0: reviewProviderLabel });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("group rounded-md border border-transparent", expanded ? "border-border bg-card" : "hover:bg-accent"),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			role: "button",
			tabIndex: 0,
			className: "flex w-full min-w-0 items-start gap-2 rounded-md px-2 py-2 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
			onClick: onToggle,
			onKeyDown: (event) => {
				if (event.key !== "Enter" && event.key !== " ") return;
				event.preventDefault();
				onToggle();
			},
			"aria-expanded": expanded,
			"aria-label": toggleDetailsLabel,
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: cn("mt-0.5 size-3 shrink-0 text-muted-foreground transition-transform", expanded && "rotate-90") }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ReviewIcon, { className: "mt-0.5 size-4 shrink-0 text-muted-foreground" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PrChecksRowHeader, { row }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mt-1 truncate text-[12px] text-foreground/90",
							children: row.title
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mt-1 flex min-w-0 items-center gap-1.5 text-[11px] text-muted-foreground",
							children: [
								showStatusIcon ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatusIcon, { className: cn("size-3 shrink-0", CHECK_COLOR[row.checkTone], animateStatusIcon && "animate-spin") }) : null,
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "truncate",
									children: row.summary
								}),
								row.repo ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
									className: "shrink-0",
									children: ["· ", row.repo.displayName]
								}) : null,
								row.branch ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
									className: "truncate",
									children: ["· ", row.branch]
								}) : null
							]
						}),
						row.detailNames.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mt-1 truncate text-[11px] text-muted-foreground",
							children: row.detailNames.join(", ")
						}) : null
					]
				}),
				row.reviewUrl ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						className: "rounded p-1 text-muted-foreground opacity-80 hover:bg-accent hover:text-foreground group-hover:opacity-100",
						"aria-label": openExternalLabel,
						onClick: (event) => {
							event.stopPropagation();
							openHttpLink(row.reviewUrl);
						},
						onKeyDown: (event) => event.stopPropagation(),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3.5" })
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
					side: "left",
					children: openExternalLabel
				})] }) : null
			]
		}), expanded ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "border-t border-border",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChecksList, {
				checks: row.checks,
				checksLoading: row.isRefreshing,
				checkDetailsContextKey: row.refreshIdentity,
				onLoadCheckDetails,
				githubRepository: row.githubRepository ?? null,
				worktreeId: row.worktree.id,
				detailsStickySurface: "card"
			})
		}) : null]
	});
}
function PrChecksRowHeader({ row }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-w-0 items-center gap-1.5",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "truncate text-[13px] font-medium text-foreground",
				children: row.worktree.displayName
			}),
			row.reviewLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "inline-flex shrink-0 items-center rounded border border-border px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground",
				children: row.reviewLabel
			}) : null,
			row.reviewState ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: cn("shrink-0 rounded border px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wide", prStateColor(row.reviewState)),
				children: row.reviewState
			}) : null
		]
	});
}
function getParentPrChecksRefreshCandidates({ worktrees, repos, knownReviewIdentities = /* @__PURE__ */ new Set() }) {
	const repoById = new Map(repos.map((repo) => [repo.id, repo]));
	return worktrees.map((worktree) => {
		const repo = repoById.get(worktree.repoId);
		const branch = getBranchName(worktree);
		if (!repo || isFolderRepo(repo) || worktree.isBare || !branch) return null;
		const identity = getParentPrChecksRefreshIdentity(worktree, repo, branch);
		return {
			identity,
			worktree,
			repo,
			branch,
			linkedReview: hasLinkedReview(worktree),
			knownReview: knownReviewIdentities.has(identity)
		};
	}).filter((candidate) => candidate !== null).sort(compareRefreshCandidates);
}
async function runLimitedParentPrChecksRefreshes({ candidates, concurrency = 3, force = false, fetchHostedReviewForBranch, fetchPRChecks, onOutcome }) {
	const outcomes = /* @__PURE__ */ new Map();
	const queue = [...candidates].sort(compareRefreshCandidates);
	const workerCount = Math.max(1, Math.min(concurrency, queue.length || 1));
	let cursor = 0;
	const runWorker = async () => {
		while (cursor < queue.length) {
			const candidate = queue[cursor];
			cursor += 1;
			outcomes.set(candidate.identity, { kind: "loading" });
			onOutcome?.(candidate.identity, { kind: "loading" });
			const outcome = await refreshParentPrChecksCandidate(candidate, fetchHostedReviewForBranch, fetchPRChecks, force);
			outcomes.set(candidate.identity, outcome);
			onOutcome?.(candidate.identity, outcome);
		}
	};
	await Promise.all(Array.from({ length: workerCount }, runWorker));
	return outcomes;
}
async function refreshParentPrChecksCandidate(candidate, fetchHostedReviewForBranch, fetchPRChecks, force) {
	try {
		const review = await fetchHostedReviewForBranch(candidate.repo.path, candidate.branch, {
			force,
			repoId: candidate.repo.id,
			linkedGitHubPR: candidate.worktree.linkedPR ?? null,
			linkedGitLabMR: candidate.worktree.linkedGitLabMR ?? null,
			linkedBitbucketPR: candidate.worktree.linkedBitbucketPR ?? null,
			linkedAzureDevOpsPR: candidate.worktree.linkedAzureDevOpsPR ?? null,
			linkedGiteaPR: candidate.worktree.linkedGiteaPR ?? null,
			currentHeadOid: candidate.worktree.head ?? null,
			staleWhileRevalidate: true
		});
		if (!review) return { kind: "unavailable" };
		if (review.provider === "github") await fetchPRChecks?.(candidate.repo.path, review.number, candidate.branch, review.headSha, review.githubRepository ?? null, {
			repoId: candidate.repo.id,
			force
		});
		return {
			kind: "found",
			review
		};
	} catch (error) {
		return {
			kind: "error",
			error
		};
	}
}
function compareRefreshCandidates(left, right) {
	return getRefreshPriority(left) - getRefreshPriority(right) || (right.worktree.lastActivityAt ?? 0) - (left.worktree.lastActivityAt ?? 0) || compareWorktreeDisplayName(left.worktree, right.worktree);
}
function getRefreshPriority(candidate) {
	if (candidate.linkedReview) return 0;
	if (candidate.knownReview) return 1;
	return 2;
}
function getBranchName(worktree) {
	const identity = getWorktreeGitIdentityDisplay(worktree);
	return identity?.kind === "branch" ? identity.branchName : null;
}
function hasLinkedReview(worktree) {
	return Boolean(worktree.linkedPR ?? worktree.linkedGitLabMR ?? worktree.linkedBitbucketPR ?? worktree.linkedAzureDevOpsPR ?? worktree.linkedGiteaPR ?? null);
}
function trackCacheReads(state, cacheName, dependencies) {
	return new Proxy(state[cacheName], { get: (target, property, receiver) => {
		const value = Reflect.get(target, property, receiver);
		if (typeof property === "string") dependencies.push({
			cacheName,
			key: property,
			value
		});
		return value;
	} });
}
function dependenciesAreCurrent(state, previousState, dependencies) {
	return dependencies.every(({ cacheName, key, value }) => state[cacheName] === previousState[cacheName] || Reflect.get(state[cacheName], key) === value);
}
function cacheReferencesAreCurrent(state, previousState) {
	return state.hostedReviewCache === previousState.hostedReviewCache && state.prCache === previousState.prCache && state.checksCache === previousState.checksCache;
}
function createParentPrChecksProjectionSelector(inputs, buildProjection = buildParentPrChecksProjection) {
	let cached = null;
	return (state) => {
		if (cached) {
			if (cacheReferencesAreCurrent(state, cached.cacheReferences)) return cached.projection;
			if (dependenciesAreCurrent(state, cached.cacheReferences, cached.dependencies)) {
				cached.cacheReferences = state;
				return cached.projection;
			}
		}
		const dependencies = [];
		const projection = buildProjection({
			...inputs,
			hostedReviewCache: trackCacheReads(state, "hostedReviewCache", dependencies),
			prCache: trackCacheReads(state, "prCache", dependencies),
			checksCache: trackCacheReads(state, "checksCache", dependencies)
		});
		cached = {
			cacheReferences: state,
			dependencies,
			projection
		};
		return projection;
	};
}
function FolderWorkspacePrChecksPanel({ isVisible = true }) {
	const activeWorktreeId = useAppStore((s) => s.activeWorktreeId);
	const activeWorkspaceKey = useAppStore((s) => s.activeWorkspaceKey);
	const folderWorkspaces = useAppStore((s) => s.folderWorkspaces);
	const workspaceLineageByChildKey = useAppStore((s) => s.workspaceLineageByChildKey);
	const worktreeLineageById = useAppStore((s) => s.worktreeLineageById);
	const worktreesByRepo = useAppStore((s) => s.worktreesByRepo);
	const repos = useAppStore((s) => s.repos);
	const settings = useAppStore((s) => s.settings);
	const fetchHostedReviewForBranch = useAppStore((s) => s.fetchHostedReviewForBranch);
	const fetchPRChecks = useAppStore((s) => s.fetchPRChecks);
	const fetchPRCheckDetails = useAppStore((s) => s.fetchPRCheckDetails);
	const [refreshOutcomes, setRefreshOutcomes] = (0, import_react.useState)(() => /* @__PURE__ */ new Map());
	const [expandedRowIds, setExpandedRowIds] = (0, import_react.useState)(() => /* @__PURE__ */ new Set());
	const [manualRefreshGeneration, setManualRefreshGeneration] = (0, import_react.useState)(0);
	const lastForcedManualRefreshGenerationRef = (0, import_react.useRef)(0);
	const { folderWorkspace, childWorktrees } = (0, import_react.useMemo)(() => getAttachedWorktreesForFolderWorkspace({
		activeWorkspaceKey,
		activeWorktreeId,
		folderWorkspaces,
		workspaceLineageByChildKey,
		worktreeLineageById,
		worktreesByRepo
	}), [
		activeWorkspaceKey,
		activeWorktreeId,
		folderWorkspaces,
		workspaceLineageByChildKey,
		worktreeLineageById,
		worktreesByRepo
	]);
	const projection = useAppStore((0, import_react.useMemo)(() => createParentPrChecksProjectionSelector({
		worktrees: childWorktrees,
		repos,
		settings,
		refreshOutcomes
	}), [
		childWorktrees,
		repos,
		settings,
		refreshOutcomes
	]));
	const folderWorkspaceId = folderWorkspace?.id ?? null;
	const headerSummary = (0, import_react.useMemo)(() => formatReviewChecksHeaderSummary(projection.summary), [projection.summary]);
	const refreshCandidates = (0, import_react.useMemo)(() => getParentPrChecksRefreshCandidates({
		worktrees: childWorktrees,
		repos
	}), [childWorktrees, repos]);
	const refreshCandidateSignature = (0, import_react.useMemo)(() => refreshCandidates.map((candidate) => [
		candidate.identity,
		candidate.repo.path,
		candidate.repo.connectionId ?? "",
		candidate.repo.executionHostId ?? ""
	].join("|")).sort().join(";;"), [refreshCandidates]);
	const refreshCandidatesRef = (0, import_react.useRef)(refreshCandidates);
	(0, import_react.useEffect)(() => {
		refreshCandidatesRef.current = refreshCandidates;
	}, [refreshCandidates]);
	(0, import_react.useEffect)(() => {
		const candidates = refreshCandidatesRef.current;
		if (!isVisible || !folderWorkspaceId || childWorktrees.length === 0) return;
		if (candidates.length === 0) return;
		const forceRefresh = manualRefreshGeneration > lastForcedManualRefreshGenerationRef.current;
		if (forceRefresh) lastForcedManualRefreshGenerationRef.current = manualRefreshGeneration;
		let cancelled = false;
		runLimitedParentPrChecksRefreshes({
			candidates,
			concurrency: 3,
			force: forceRefresh,
			fetchHostedReviewForBranch,
			fetchPRChecks,
			onOutcome: (identity, outcome) => {
				if (cancelled) return;
				setRefreshOutcomes((current) => new Map(current).set(identity, outcome));
			}
		});
		return () => {
			cancelled = true;
		};
	}, [
		isVisible,
		folderWorkspaceId,
		childWorktrees.length,
		fetchHostedReviewForBranch,
		fetchPRChecks,
		refreshCandidateSignature,
		manualRefreshGeneration
	]);
	const currentRefreshIdentities = (0, import_react.useMemo)(() => new Set(refreshCandidates.map((candidate) => candidate.identity)), [refreshCandidates]);
	const isRefreshing = [...refreshOutcomes.entries()].some(([identity, outcome]) => currentRefreshIdentities.has(identity) && outcome.kind === "loading");
	(0, import_react.useEffect)(() => {
		const validRowIds = new Set(projection.rows.map((row) => row.id));
		setExpandedRowIds((current) => {
			const next = new Set([...current].filter((id) => validRowIds.has(id)));
			return next.size === current.size ? current : next;
		});
	}, [projection.rows]);
	const toggleRowExpanded = (0, import_react.useCallback)((rowId) => {
		setExpandedRowIds((current) => {
			const next = new Set(current);
			if (next.has(rowId)) next.delete(rowId);
			else next.add(rowId);
			return next;
		});
	}, []);
	const loadCheckDetails = (0, import_react.useCallback)((row, check) => {
		if (!row.repo) return Promise.resolve(null);
		return fetchPRCheckDetails(row.repo.path, {
			checkRunId: check.checkRunId,
			workflowRunId: check.workflowRunId,
			checkName: check.name,
			url: check.url,
			prRepo: row.githubRepository ?? null
		}, { repoId: row.repo.id });
	}, [fetchPRCheckDetails]);
	if (!folderWorkspace) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "flex min-h-0 flex-1 items-center justify-center p-6 text-center text-sm text-muted-foreground",
		children: translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.unavailable", "PR checks are only shown for folder workspaces.")
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-h-0 flex-1 flex-col overflow-hidden bg-background",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "border-b border-border px-4 py-3",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "truncate text-sm font-medium text-foreground",
						children: translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.reviewChecks", "Review checks")
					}), headerSummary ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mt-1 truncate text-xs text-muted-foreground",
						children: headerSummary
					}) : null]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "ghost",
						size: "icon-xs",
						onClick: () => setManualRefreshGeneration((generation) => generation + 1),
						disabled: childWorktrees.length === 0 || isRefreshing,
						"aria-label": translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.refresh", "Refresh PR checks"),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: cn("size-3.5", isRefreshing && "animate-spin") })
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
					side: "bottom",
					children: translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.refresh", "Refresh PR checks")
				})] })]
			})
		}), childWorktrees.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex flex-1 flex-col items-center justify-center px-6 text-center",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "text-sm font-medium text-foreground",
				children: translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.emptyTitle", "No attached worktrees yet")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mt-2 max-w-[16rem] text-xs leading-5 text-muted-foreground",
				children: translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.emptyCopy", "PR checks will appear here after worktrees are attached to this folder workspace.")
			})]
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "scrollbar-sleek min-h-0 flex-1 overflow-y-auto px-2 py-2",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "space-y-1",
				children: projection.rows.map((row) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderWorkspacePrChecksRow, {
					row,
					expanded: expandedRowIds.has(row.id),
					onToggle: () => toggleRowExpanded(row.id),
					onLoadCheckDetails: (check) => loadCheckDetails(row, check)
				}, row.id))
			})
		})]
	});
}
function formatReviewChecksHeaderSummary(summary) {
	if (summary.attached === 0) return null;
	const worktreeCount = formatWorktreeCount(summary.attached);
	const attentionParts = [summary.failing > 0 ? formatFailingCount(summary.failing) : null, summary.pending > 0 ? formatPendingCount(summary.pending) : null].filter((part) => part !== null);
	if (attentionParts.length > 0) return [...attentionParts, worktreeCount].join(" · ");
	if (summary.passing === summary.attached) return [worktreeCount, translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.allChecksPassing", "all checks passing")].join(" · ");
	return worktreeCount;
}
function formatWorktreeCount(count) {
	return count === 1 ? translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.oneWorktree", "1 worktree") : translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.worktreeCount", "{{value0}} worktrees", { value0: count });
}
function formatFailingCount(count) {
	return count === 1 ? translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.oneFailing", "1 failing") : translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.failingCount", "{{value0}} failing", { value0: count });
}
function formatPendingCount(count) {
	return count === 1 ? translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.onePending", "1 pending") : translate("auto.components.rightSidebar.FolderWorkspacePrChecksPanel.pendingCount", "{{value0}} pending", { value0: count });
}
export { FolderWorkspacePrChecksPanel as default };
