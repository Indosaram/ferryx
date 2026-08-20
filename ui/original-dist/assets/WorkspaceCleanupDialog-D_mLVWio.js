import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as ChevronsUpDown } from "./chevrons-up-down-avw2FWhd.js";
import { t as Clock3 } from "./clock-3-CTP0LtUg.js";
import { t as EyeOff } from "./eye-off-4tMqi6LV.js";
import { t as FileExclamationPoint } from "./file-exclamation-point-DBMojurh.js";
import { r as activateAndRevealWorktree } from "./worktree-activation-BDsaiyMf.js";
import { t as GitBranch } from "./git-branch-CnBuDEti.js";
import { t as GitPullRequest } from "./git-pull-request-BS_5feqf.js";
import { t as Info } from "./info-D3uaWrfJ.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as RefreshCcw } from "./refresh-ccw-CVkvAIoB.js";
import { t as Search } from "./search-DK1nVA6d.js";
import { t as SlidersHorizontal } from "./sliders-horizontal-D-rA728l.js";
import { t as SquareTerminal } from "./square-terminal-rgWG-Apn.js";
import { Lp as normalizeRuntimePathForComparison, Oi as canQueueWorkspaceCleanupCandidate, Pp as isPathInsideOrEqual, Wl as getHostedReviewCacheKey, m_ as Trash2, qg as isGitRepoKind, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import { t as X } from "./x-BrGKE4uz.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { a as DropdownMenuLabel, c as DropdownMenuRadioItem, d as DropdownMenuSub, f as DropdownMenuSubContent, i as DropdownMenuItem, l as DropdownMenuSeparator, m as DropdownMenuTrigger, p as DropdownMenuSubTrigger, r as DropdownMenuContent, s as DropdownMenuRadioGroup, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
import { i as PopoverTrigger, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import { t as Progress } from "./progress-BsVdJvWF.js";
import { t as ScrollArea } from "./scroll-area-DifvZO0h.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
import { i as getWorktreeMapFromState } from "./selectors-XOBeaOSb.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./web-session-tabs-sync-CYKZbAxS.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import "./connection-context-BUPsamzR.js";
import "./localized-catalog-DubKHKUR.js";
import { n as showPreservedBranchBatchToast } from "./preserved-branch-batch-toast-DHxeGO1o.js";
import { a as CommandInput, o as CommandItem, r as CommandEmpty, s as CommandList, t as Command } from "./command-D8Tw17HJ.js";
import { t as RepoBadgeLabel_default } from "./RepoBadgeLabel-BMcVlWTu.js";
import { n as searchRepos } from "./repo-search-CDSD-LSk.js";
import { t as useVirtualizer } from "./esm-DQfOTgcy.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
import { t as countEstimatedInactiveWorkspaces } from "./inactive-workspace-estimate-B_n4w59B.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function renderTriggerLabel(repos, selected) {
	if (repos.length === 0) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: "text-muted-foreground",
		children: translate("auto.components.ui.repo.multi.combobox.65a3dae41d", "No projects")
	});
	if (selected.size === repos.length) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: "inline-flex min-w-0 items-center gap-1.5",
		children: translate("auto.components.ui.repo.multi.combobox.bfd8ce21c6", "All projects")
	});
	const [first, second, ...rest] = repos.filter((r) => selected.has(r.id));
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		className: "inline-flex min-w-0 items-center gap-1.5 truncate",
		children: [
			first ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RepoBadgeLabel_default, {
				name: first.displayName,
				color: first.badgeColor,
				badgeClassName: "size-1.5"
			}) : null,
			second ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
				className: "text-muted-foreground",
				children: [", ", second.displayName]
			}) : null,
			rest.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
				className: "text-muted-foreground",
				children: ["+", rest.length]
			}) : null
		]
	});
}
function getRepoMultiComboboxDetail(repo, hostLabel) {
	const trimmedHostLabel = hostLabel?.trim();
	return trimmedHostLabel ? `${trimmedHostLabel} · ${repo.path}` : repo.path;
}
function RepoMultiCombobox({ repos, selected, onChange, onSelectAll, getRepoHostLabel, triggerClassName }) {
	const [open, setOpen] = (0, import_react.useState)(false);
	const [query, setQuery] = (0, import_react.useState)("");
	const [commandValue, setCommandValue] = (0, import_react.useState)("");
	const filteredRepos = (0, import_react.useMemo)(() => searchRepos(repos, query), [repos, query]);
	const allSelected = selected.size === repos.length && repos.length > 0;
	const handleOpenChange = (0, import_react.useCallback)((nextOpen) => {
		setOpen(nextOpen);
		if (!nextOpen) setQuery("");
	}, []);
	const toggle = (0, import_react.useCallback)((repoId) => {
		const next = new Set(selected);
		if (next.has(repoId)) {
			if (next.size <= 1) return;
			next.delete(repoId);
		} else next.add(repoId);
		onChange(next);
	}, [onChange, selected]);
	const handleSelectAll = (0, import_react.useCallback)(() => {
		if (allSelected) {
			const first = repos[0];
			if (!first) return;
			onChange(new Set([first.id]));
			return;
		}
		onSelectAll();
	}, [
		allSelected,
		onChange,
		onSelectAll,
		repos
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
		open,
		onOpenChange: handleOpenChange,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				type: "button",
				variant: "outline",
				role: "combobox",
				"aria-expanded": open,
				className: cn("h-8 w-full justify-between px-3 text-xs font-normal", triggerClassName),
				children: [renderTriggerLabel(repos, selected), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronsUpDown, { className: "size-3.5 opacity-50" })]
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverContent, {
			align: "start",
			className: "w-[min(320px,calc(100vw-1rem))] min-w-[var(--radix-popover-trigger-width)] p-0",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Command, {
				shouldFilter: false,
				value: commandValue,
				onValueChange: setCommandValue,
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandInput, {
						autoFocus: true,
						placeholder: translate("auto.components.ui.repo.multi.combobox.a58a0cd100", "Search projects..."),
						value: query,
						onValueChange: setQuery,
						className: "text-xs"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "border-b border-border",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							onClick: handleSelectAll,
							onMouseDown: (event) => event.preventDefault(),
							onMouseEnter: () => setCommandValue(""),
							className: cn("flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs text-foreground transition-colors hover:bg-accent hover:text-accent-foreground", allSelected && "opacity-80"),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: cn("size-3 text-muted-foreground", allSelected ? "opacity-70" : "opacity-0") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.ui.repo.multi.combobox.bfd8ce21c6", "All projects") })]
						})
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CommandList, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandEmpty, { children: translate("auto.components.ui.repo.multi.combobox.4471d4a1c0", "No projects match your search.") }), filteredRepos.map((repo) => {
						const isSelected = selected.has(repo.id);
						const isLastSelected = isSelected && selected.size <= 1;
						const detail = getRepoMultiComboboxDetail(repo, getRepoHostLabel?.(repo));
						return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CommandItem, {
							value: repo.id,
							onSelect: () => toggle(repo.id),
							disabled: isLastSelected,
							className: "items-center gap-2 px-3 py-1.5 text-xs",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: cn("size-3 text-muted-foreground", isSelected ? "opacity-70" : "opacity-0") }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "min-w-0 flex-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "inline-flex items-center gap-1.5 text-xs",
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RepoBadgeLabel_default, {
										name: repo.displayName,
										color: repo.badgeColor,
										className: "max-w-full"
									})
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
									className: "mt-0.5 truncate text-[10px] text-muted-foreground",
									children: detail
								})]
							})]
						}, repo.id);
					})] })
				]
			})
		})]
	});
}
var DAY_MS = 1440 * 60 * 1e3;
var EMPTY_REVIEW_INFO$1 = {
	hasReview: false,
	label: null,
	state: null,
	provider: null,
	title: null
};
var EMPTY_REVIEW_INFO_MAP = /* @__PURE__ */ new Map();
function hasWorkspaceCleanupLocalContext(candidate) {
	return candidate.localContext.terminalTabCount > 0 || candidate.localContext.cleanEditorTabCount > 0 || candidate.localContext.browserTabCount > 0 || candidate.localContext.diffCommentCount > 0 || candidate.localContext.retainedDoneAgentCount > 0;
}
function getWorkspaceCleanupSearchText(candidate, reviewInfo = EMPTY_REVIEW_INFO$1) {
	return [
		candidate.displayName,
		candidate.repoName,
		candidate.branch,
		candidate.path,
		reviewInfo.label,
		reviewInfo.title,
		getWorkspaceCleanupGitLabel(candidate),
		hasWorkspaceCleanupLocalContext(candidate) ? "has context" : "no context"
	].filter(Boolean).join(" ").toLowerCase();
}
function filterWorkspaceCleanupCandidates(candidates, filters, reviewInfoByWorktreeId = EMPTY_REVIEW_INFO_MAP, now = Date.now()) {
	const normalizedQuery = filters.query.trim().toLowerCase();
	return candidates.filter((candidate) => {
		const reviewInfo = reviewInfoByWorktreeId.get(candidate.worktreeId) ?? EMPTY_REVIEW_INFO$1;
		if (normalizedQuery && !getWorkspaceCleanupSearchText(candidate, reviewInfo).includes(normalizedQuery)) return false;
		return matchesTimeFilter(candidate, filters.time, now) && matchesReviewFilter(reviewInfo, filters.review) && matchesGitFilter(candidate, filters.git) && matchesContextFilter(candidate, filters.context);
	});
}
function sortWorkspaceCleanupCandidates(candidates, sortKey, direction, reviewInfoByWorktreeId = EMPTY_REVIEW_INFO_MAP) {
	const multiplier = direction === "asc" ? 1 : -1;
	return [...candidates].sort((left, right) => {
		return compareWorkspaceCleanupCandidates(left, right, sortKey, reviewInfoByWorktreeId) * multiplier || left.lastActivityAt - right.lastActivityAt || left.repoName.localeCompare(right.repoName) || left.displayName.localeCompare(right.displayName);
	});
}
function getWorkspaceCleanupGitLabel(candidate) {
	if (hasUnpushedCommits(candidate)) return "Unpushed";
	if (isGitStatusUnknown(candidate)) return "Unknown";
	if (candidate.git.clean === true) return "Clean";
	if (candidate.git.clean === false) return "Dirty";
	return "Unknown";
}
function compareWorkspaceCleanupCandidates(left, right, sortKey, reviewInfoByWorktreeId) {
	switch (sortKey) {
		case "activity": return left.lastActivityAt - right.lastActivityAt;
		case "name": return left.displayName.localeCompare(right.displayName);
		case "repo": return left.repoName.localeCompare(right.repoName) || left.displayName.localeCompare(right.displayName);
		case "review": return getReviewSortRank(reviewInfoByWorktreeId.get(left.worktreeId) ?? EMPTY_REVIEW_INFO$1) - getReviewSortRank(reviewInfoByWorktreeId.get(right.worktreeId) ?? EMPTY_REVIEW_INFO$1) || (reviewInfoByWorktreeId.get(left.worktreeId)?.label ?? "").localeCompare(reviewInfoByWorktreeId.get(right.worktreeId)?.label ?? "");
		case "git": return getGitSortRank(left) - getGitSortRank(right);
	}
}
function matchesTimeFilter(candidate, filter, now) {
	switch (filter) {
		case "all": return true;
		case "30d": return now - candidate.lastActivityAt >= 30 * DAY_MS;
		case "90d": return now - candidate.lastActivityAt >= 90 * DAY_MS;
		case "archived": return candidate.reasons.includes("archived");
	}
}
function matchesReviewFilter(reviewInfo, filter) {
	switch (filter) {
		case "all": return true;
		case "no-review": return !reviewInfo.hasReview;
		case "has-review": return reviewInfo.hasReview;
		case "open-review": return reviewInfo.hasReview && (reviewInfo.state === "open" || reviewInfo.state === "draft");
		case "closed-review": return reviewInfo.hasReview && (reviewInfo.state === "closed" || reviewInfo.state === "merged");
	}
}
function matchesGitFilter(candidate, filter) {
	switch (filter) {
		case "all": return true;
		case "clean": return candidate.git.clean === true && !hasUnpushedCommits(candidate) && !isGitStatusUnknown(candidate);
		case "dirty": return candidate.git.clean === false;
		case "unpushed": return hasUnpushedCommits(candidate);
		case "unknown": return isGitStatusUnknown(candidate);
	}
}
function matchesContextFilter(candidate, filter) {
	switch (filter) {
		case "all": return true;
		case "has-context": return hasWorkspaceCleanupLocalContext(candidate);
		case "no-context": return !hasWorkspaceCleanupLocalContext(candidate);
	}
}
function getReviewSortRank(reviewInfo) {
	if (!reviewInfo.hasReview) return 0;
	if (reviewInfo.state === "open" || reviewInfo.state === "draft") return 3;
	if (reviewInfo.state === "unknown") return 2;
	return 1;
}
function getGitSortRank(candidate) {
	if (hasUnpushedCommits(candidate)) return 4;
	if (candidate.git.clean === false) return 3;
	if (isGitStatusUnknown(candidate)) return 2;
	return 1;
}
function hasUnpushedCommits(candidate) {
	return (candidate.git.upstreamAhead ?? 0) > 0 || candidate.blockers.includes("unpushed-commits");
}
function isGitStatusUnknown(candidate) {
	return candidate.git.clean === null || candidate.blockers.includes("git-status-error") || candidate.blockers.includes("unknown-base");
}
function getWorkspaceCleanupReviewInfo(candidate, state) {
	const worktree = getWorktreeMapFromState(state).get(candidate.worktreeId) ?? null;
	const hostedReview = getCachedHostedReview(candidate, worktree, state.repos.find((entry) => entry.id === candidate.repoId) ?? null, state);
	if (hostedReview) return {
		hasReview: true,
		label: `${getReviewShortLabel(hostedReview.provider)} #${hostedReview.number}`,
		state: hostedReview.state,
		provider: hostedReview.provider,
		title: hostedReview.title
	};
	const linkedReview = getLinkedReviewFallback(worktree);
	if (linkedReview) return {
		hasReview: true,
		label: linkedReview.label,
		state: "unknown",
		provider: linkedReview.provider,
		title: null
	};
	return {
		hasReview: false,
		label: null,
		state: null,
		provider: null,
		title: null
	};
}
function getCachedHostedReview(candidate, worktree, repo, state) {
	if (!repo) return null;
	const cacheKey = getHostedReviewCacheKey(repo.path, getBranchDisplayName(worktree?.branch ?? candidate.branch), state.settings, repo.id, repo.connectionId);
	return state.hostedReviewCache[cacheKey]?.data ?? null;
}
function getLinkedReviewFallback(worktree) {
	if (!worktree) return null;
	if (worktree.linkedGitLabMR != null) return {
		label: translate("components.workspace.cleanup.presentation.gitlabMergeRequestNumber", "MR #{{value0}}", { value0: worktree.linkedGitLabMR }),
		provider: "gitlab"
	};
	if (worktree.linkedPR != null) return {
		label: translate("components.workspace.cleanup.presentation.githubPullRequestNumber", "PR #{{value0}}", { value0: worktree.linkedPR }),
		provider: "github"
	};
	return null;
}
function getReviewShortLabel(provider) {
	return provider === "gitlab" ? "MR" : "PR";
}
function getBranchDisplayName(branch) {
	return branch.replace(/^refs\/heads\//, "") || "HEAD";
}
function getWorkspaceCleanupBlockerLabel(blocker) {
	switch (blocker) {
		case "main-worktree": return translate("auto.components.workspace.cleanup.candidateRow.mainWorkspaceBlocker", "Main workspace");
		case "folder-repo": return translate("auto.components.workspace.cleanup.candidateRow.folderProjectBlocker", "Folder project");
		case "pinned": return translate("auto.components.workspace.cleanup.candidateRow.pinnedBlocker", "Pinned");
		case "active-workspace": return translate("auto.components.workspace.cleanup.candidateRow.activeWorkspaceBlocker", "Active workspace");
		case "running-terminal": return translate("auto.components.workspace.cleanup.candidateRow.runningTerminalBlocker", "Running terminal process");
		case "terminal-liveness-unknown": return translate("auto.components.workspace.cleanup.candidateRow.terminalLivenessUnknownBlocker", "Terminal liveness unknown");
		case "dirty-editor-buffer": return translate("auto.components.workspace.cleanup.candidateRow.dirtyEditorBufferBlocker", "Unsaved editor buffer");
		case "volatile-local-context": return translate("auto.components.workspace.cleanup.candidateRow.volatileLocalContextBlocker", "Volatile local context");
		case "recent-visible-context": return translate("auto.components.workspace.cleanup.candidateRow.recentVisibleContextBlocker", "Recently visited tabs");
		case "live-agent": return translate("auto.components.workspace.cleanup.candidateRow.liveAgentBlocker", "Active agent");
		case "ssh-disconnected": return translate("auto.components.workspace.cleanup.candidateRow.sshDisconnectedBlocker", "Remote unavailable");
		case "git-status-error": return translate("auto.components.workspace.cleanup.candidateRow.gitStatusErrorBlocker", "Git status unavailable");
		case "dirty-files": return translate("auto.components.workspace.cleanup.candidateRow.dirtyFilesBlocker", "Changed files");
		case "unpushed-commits": return getUnpushedCommitsLabel();
		case "unknown-base": return translate("auto.components.workspace.cleanup.candidateRow.unknownBaseBlocker", "Could not verify unpushed commits");
		case "dismissed": return translate("auto.components.workspace.cleanup.candidateRow.dismissedBlocker", "Ignored");
	}
}
function formatWorkspaceCleanupGitStatusLabel(label) {
	switch (label) {
		case "Clean": return translate("auto.components.workspace.cleanup.candidateRow.cleanGit", "Clean git");
		case "Dirty": return translate("auto.components.workspace.cleanup.candidateRow.dirtyGit", "Dirty git");
		case "Unpushed": return getUnpushedCommitsLabel();
		case "Unknown": return translate("auto.components.workspace.cleanup.candidateRow.gitUnknown", "Git unknown");
	}
	return translate("auto.components.workspace.cleanup.candidateRow.gitUnknown", "Git unknown");
}
function getNoUnpushedCommitsLabel() {
	return translate("auto.components.workspace.cleanup.candidateRow.noUnpushedCommits", "No unpushed commits");
}
function getUnpushedCommitsLabel() {
	return translate("auto.components.workspace.cleanup.candidateRow.unpushedCommits", "Unpushed commits");
}
function formatUnpushedCommitCount(count) {
	return translate("auto.components.workspace.cleanup.candidateRow.unpushedCommitsCount", "Unpushed commits: {{value0}}", { value0: count });
}
function getUncommittedChangesLabel() {
	return translate("auto.components.workspace.cleanup.candidateRow.uncommittedChanges", "Uncommitted changes");
}
function getGitStatusUnknownLabel() {
	return translate("auto.components.workspace.cleanup.candidateRow.gitStatusUnknown", "Git status unknown");
}
function formatWorkspaceCleanupContextDetail(kind, count) {
	switch (kind) {
		case "terminal": return translate("auto.components.workspace.cleanup.candidateRow.terminalTabsCount", "Terminal tabs: {{value0}}", { value0: count });
		case "editor": return translate("auto.components.workspace.cleanup.candidateRow.editorTabsCount", "Editor tabs: {{value0}}", { value0: count });
		case "browser": return translate("auto.components.workspace.cleanup.candidateRow.browserTabsCount", "Browser tabs: {{value0}}", { value0: count });
		case "diff": return translate("auto.components.workspace.cleanup.candidateRow.diffNotesCount", "Diff notes: {{value0}}", { value0: count });
		case "agent": return translate("auto.components.workspace.cleanup.candidateRow.completedAgentsCount", "Completed agents: {{value0}}", { value0: count });
	}
}
function formatWorkspaceCleanupContextCount(count) {
	return translate("auto.components.workspace.cleanup.candidateRow.contextCount", "Context: {{value0}}", { value0: count });
}
function getWorkspaceCleanupBlockerLabels(candidate) {
	return candidate.blockers.map((blocker) => getWorkspaceCleanupBlockerLabel(blocker));
}
function getCandidateStatus(candidate) {
	if (candidate.blockers.includes("dismissed")) return {
		label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.e8b3741ff7", "Ignored"),
		tone: "neutral"
	};
	if (candidate.tier === "ready") return {
		label: candidate.reasons.includes("archived") ? translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.archivedStatus", "Archived") : translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.readyStatus", "Ready"),
		tone: "ready"
	};
	if (candidate.blockers.length > 0) return {
		label: getWorkspaceCleanupBlockerLabel(candidate.blockers[0]),
		tone: "neutral"
	};
	if (candidate.git.upstreamAhead && candidate.git.upstreamAhead > 0) return {
		label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.9623a5107d", "Unpushed commits"),
		tone: "review"
	};
	if (candidate.git.clean === false) return {
		label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.e97e4580c7", "Dirty"),
		tone: "review"
	};
	if (candidate.tier === "review") return {
		label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.0a2e3c7cba", "Review"),
		tone: "review"
	};
	return {
		label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.c4f4782c02", "Not suggested"),
		tone: "neutral"
	};
}
function formatGitStatus(candidate) {
	return formatWorkspaceCleanupGitStatusLabel(getWorkspaceCleanupGitLabel(candidate));
}
function formatBranchSafetyDetails(candidate) {
	const details = [];
	if (candidate.git.upstreamAhead !== null) details.push(candidate.git.upstreamAhead === 0 ? getNoUnpushedCommitsLabel() : formatUnpushedCommitCount(candidate.git.upstreamAhead));
	return details;
}
function formatContextDetails(candidate) {
	const parts = [];
	if (candidate.localContext.terminalTabCount > 0) parts.push(formatWorkspaceCleanupContextDetail("terminal", candidate.localContext.terminalTabCount));
	if (candidate.localContext.cleanEditorTabCount > 0) parts.push(formatWorkspaceCleanupContextDetail("editor", candidate.localContext.cleanEditorTabCount));
	if (candidate.localContext.browserTabCount > 0) parts.push(formatWorkspaceCleanupContextDetail("browser", candidate.localContext.browserTabCount));
	if (candidate.localContext.diffCommentCount > 0) parts.push(formatWorkspaceCleanupContextDetail("diff", candidate.localContext.diffCommentCount));
	if (candidate.localContext.retainedDoneAgentCount > 0) parts.push(formatWorkspaceCleanupContextDetail("agent", candidate.localContext.retainedDoneAgentCount));
	return parts.length > 0 ? parts.join(", ") : null;
}
function getDirtyGitLabel(candidate) {
	if (candidate.blockers.includes("unknown-base") || candidate.blockers.includes("git-status-error")) return null;
	if (candidate.blockers.includes("unpushed-commits")) {
		if (candidate.git.upstreamAhead && candidate.git.upstreamAhead > 0) return formatUnpushedCommitCount(candidate.git.upstreamAhead);
		return getUnpushedCommitsLabel();
	}
	if (candidate.git.upstreamAhead && candidate.git.upstreamAhead > 0) return formatUnpushedCommitCount(candidate.git.upstreamAhead);
	if (candidate.git.clean === false) return getUncommittedChangesLabel();
	if (candidate.git.clean == null) return getGitStatusUnknownLabel();
	return null;
}
function shouldShowGitMetadataChip(candidate) {
	return !candidate.blockers.includes("unknown-base") && !candidate.blockers.includes("git-status-error") && !hasGitStatusPill(candidate);
}
function hasGitStatusPill(candidate) {
	if (candidate.blockers.includes("dirty-files") || candidate.blockers.includes("unpushed-commits")) return true;
	if (candidate.blockers.length > 0 || candidate.tier === "ready") return false;
	return (candidate.git.upstreamAhead ?? 0) > 0 || candidate.git.clean === false;
}
function getReviewPillTone(reviewInfo) {
	if (reviewInfo.state === "open" || reviewInfo.state === "draft") return "review";
	return "neutral";
}
function getContextPillLabel(candidate) {
	if (!hasWorkspaceCleanupLocalContext(candidate)) return null;
	return formatWorkspaceCleanupContextCount(getContextCount(candidate));
}
function getContextCount(candidate) {
	return candidate.localContext.terminalTabCount + candidate.localContext.cleanEditorTabCount + candidate.localContext.browserTabCount + candidate.localContext.diffCommentCount + candidate.localContext.retainedDoneAgentCount;
}
function CandidateRowDetails({ blockers, branchSafetyDetails, candidate, contextDetails, expanded }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("grid overflow-hidden transition-[grid-template-rows,margin-top,opacity] duration-200 ease-out motion-reduce:transition-none", expanded ? "mt-2 grid-rows-[1fr] opacity-100" : "mt-0 grid-rows-[0fr] opacity-0"),
		"aria-hidden": !expanded,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "min-h-0 overflow-hidden",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "pl-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "grid gap-x-4 gap-y-1.5 text-xs text-muted-foreground sm:grid-cols-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DetailLine, {
							label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.0b1766738a", "Repo"),
							value: candidate.repoName
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DetailLine, {
							label: translate("auto.components.workspace.cleanup.candidateRow.gitLabel", "Git"),
							value: formatGitStatus(candidate)
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DetailLine, {
							label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.bef0adef9b", "Branch"),
							value: candidate.branch,
							mono: true
						}),
						branchSafetyDetails.slice(0, 1).map((detail) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DetailLine, {
							label: translate("auto.components.workspace.cleanup.candidateRow.commitsLabel", "Commits"),
							value: detail
						}, detail)),
						contextDetails ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DetailLine, {
							label: translate("auto.components.workspace.cleanup.candidateRow.contextLabel", "Context"),
							value: contextDetails
						}) : null,
						blockers.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DetailLine, {
							label: translate("auto.components.workspace.cleanup.candidateRow.flagsLabel", "Flags"),
							value: blockers.slice(0, 2).join(", ")
						}) : null
					]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mt-2 min-w-0 truncate font-mono text-[11px] text-muted-foreground",
					children: candidate.path
				})]
			})
		})
	});
}
function DetailLine({ label, mono = false, value }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-w-0 items-baseline gap-2",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "shrink-0 text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground/80",
			children: label
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: cn("min-w-0 truncate", mono && "font-mono text-[11px]"),
			children: value
		})]
	});
}
function StatusPill({ children, tone = "neutral" }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex h-5 items-center rounded-full border px-2 text-[11px] font-medium", tone === "neutral" && "border-border bg-background text-muted-foreground", tone === "ready" && "border-status-success-border bg-status-success-background text-status-success", tone === "review" && "border-border bg-muted text-foreground", tone === "destructive" && "border-destructive/30 text-destructive"),
		children
	});
}
function MetadataIconChip({ icon: Icon, label, value, tone = "neutral" }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: cn("inline-flex h-5 shrink-0 items-center gap-1 rounded-full border px-1.5 text-[11px] font-medium", "border-border bg-background text-muted-foreground", tone === "ready" && "border-[color:color-mix(in_srgb,var(--git-decoration-added)_45%,transparent)] bg-[color:color-mix(in_srgb,var(--git-decoration-added)_10%,transparent)] text-[var(--git-decoration-added)]", tone === "review" && "bg-muted text-foreground", tone === "destructive" && "border-destructive/30 text-destructive"),
			"aria-label": label,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, {
				className: "size-3",
				"aria-hidden": "true"
			}), value ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: value }) : null]
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "top",
		sideOffset: 4,
		children: label
	})] });
}
const CandidateRow = import_react.memo(function CandidateRow$1({ candidate, deletionPhase, expanded, failure, last, lastActivityLabel, removing = false, reviewInfo, selected, onIgnore, onRemove, onToggleExpanded, onToggleSelected, onView }) {
	const deleting = deletionPhase !== void 0;
	const selectable = canQueueWorkspaceCleanupCandidate(candidate) && !removing && !deleting;
	const ignored = candidate.blockers.includes("dismissed");
	const blockers = getWorkspaceCleanupBlockerLabels(candidate);
	const contextDetails = formatContextDetails(candidate);
	const branchSafetyDetails = formatBranchSafetyDetails(candidate);
	const status = getCandidateStatus(candidate);
	const dirtyLabel = getDirtyGitLabel(candidate);
	const showGitMetadataChip = shouldShowGitMetadataChip(candidate);
	const contextCount = getContextCount(candidate);
	const hasExpandableDetails = blockers.length > 0 || candidate.path.length > 0 || candidate.branch.length > 0 || contextDetails !== null || branchSafetyDetails.length > 0;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("group w-full border-b border-border/60 px-3 py-2.5 text-left text-foreground transition-colors hover:bg-accent/40", selected && "bg-accent/30", deleting && "opacity-70", last && "border-b-0"),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "grid grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-x-2.5 gap-y-1",
			children: [
				selectable ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					role: "checkbox",
					"aria-checked": selected,
					"aria-label": translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.bbb1ab6a6f", "Select {{value0}}", { value0: candidate.displayName }),
					onClick: () => onToggleSelected(candidate.worktreeId),
					className: "mt-0.5 flex size-4 shrink-0 items-center justify-center rounded border border-border bg-background text-primary hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
					children: selected ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, {
						className: "size-3",
						strokeWidth: 3
					}) : null
				}) : deleting ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "mt-0.5 size-4 shrink-0 animate-spin text-muted-foreground" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mt-0.5 size-4 shrink-0",
					"aria-hidden": "true"
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex min-w-0 flex-wrap items-center gap-1.5",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "min-w-0 truncate text-sm font-medium",
									children: candidate.displayName
								}),
								deletionPhase ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatusPill, {
									tone: "destructive",
									children: deletionPhase === "queued" ? translate("auto.components.workspace.cleanup.workspace.cleanup.candidate.row.e1135728e3", "Queued for deletion") : translate("auto.components.workspace.cleanup.workspace.cleanup.candidate.row.b5d2b33e47", "Deleting…")
								}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatusPill, {
									tone: status.tone,
									children: status.label
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MetadataIconChip, {
									icon: Clock3,
									label: `${translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.352f15d6fc", "Last active")} ${lastActivityLabel}`,
									value: formatCompactActivityLabel(lastActivityLabel)
								}),
								dirtyLabel && showGitMetadataChip ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MetadataIconChip, {
									icon: FileExclamationPoint,
									label: dirtyLabel,
									tone: "destructive"
								}) : showGitMetadataChip ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MetadataIconChip, {
									icon: GitBranch,
									label: formatGitStatus(candidate),
									tone: getWorkspaceCleanupGitLabel(candidate) === "Clean" ? "ready" : "review"
								}) : null,
								contextDetails ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MetadataIconChip, {
									icon: SquareTerminal,
									label: contextDetails,
									value: String(contextCount)
								}) : null,
								reviewInfo.label ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MetadataIconChip, {
									icon: GitPullRequest,
									label: getReviewTooltip(reviewInfo),
									value: reviewInfo.label,
									tone: getReviewPillTone(reviewInfo)
								}) : null
							]
						}),
						failure ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mt-2 flex items-center gap-1.5 text-xs text-destructive",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-3.5" }), failure]
						}) : null,
						hasExpandableDetails ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CandidateRowDetails, {
							blockers,
							branchSafetyDetails,
							candidate,
							contextDetails,
							expanded
						}) : null
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex shrink-0 items-center gap-0.5",
					children: [
						hasExpandableDetails ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
							asChild: true,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "ghost",
								size: "icon-xs",
								"aria-label": expanded ? translate("auto.components.workspace.cleanup.candidateRow.collapseDetails", "Collapse details") : translate("auto.components.workspace.cleanup.candidateRow.expandDetails", "Expand details"),
								"aria-expanded": expanded,
								onClick: () => onToggleExpanded(candidate.worktreeId),
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: cn("size-3.5 transition-transform", expanded && "rotate-180") })
							})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
							side: "top",
							sideOffset: 4,
							children: expanded ? translate("auto.components.workspace.cleanup.candidateRow.collapseDetails", "Collapse details") : translate("auto.components.workspace.cleanup.candidateRow.expandDetails", "Expand details")
						})] }) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
							asChild: true,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "ghost",
								size: "icon-xs",
								"aria-label": translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.1bffc07ba7", "View {{value0}}", { value0: candidate.displayName }),
								onClick: () => onView(candidate),
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Search, { className: "size-3.5" })
							})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
							side: "top",
							sideOffset: 4,
							children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.ee81adfcef", "View")
						})] }),
						!ignored ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
							asChild: true,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "ghost",
								size: "icon-xs",
								"aria-label": translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.a9957007eb", "Ignore {{value0}}", { value0: candidate.displayName }),
								onClick: () => onIgnore(candidate),
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EyeOff, { className: "size-3.5" })
							})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
							side: "top",
							sideOffset: 4,
							children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.4d0b72481c", "Ignore")
						})] }) : null,
						selectable ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
							asChild: true,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "ghost",
								size: "icon-xs",
								"aria-label": translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.3828408538", "Remove {{value0}}", { value0: candidate.displayName }),
								className: "text-destructive hover:text-destructive",
								onClick: () => onRemove(candidate),
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3.5" })
							})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
							side: "top",
							sideOffset: 4,
							children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.9cc26c019d", "Remove")
						})] }) : null
					]
				})
			]
		})
	});
});
function formatCompactActivityLabel(label) {
	if (label === "Just now") return "now";
	return label.replace(/ ago$/, "");
}
function getReviewTooltip(reviewInfo) {
	const parts = [reviewInfo.label];
	if (reviewInfo.state) parts.push(reviewInfo.state);
	if (reviewInfo.title) parts.push(reviewInfo.title);
	return parts.filter(Boolean).join(" · ");
}
var WORKSPACE_CLEANUP_ROW_ESTIMATE_PX = 48;
var WORKSPACE_CLEANUP_ROW_OVERSCAN = 8;
function WorkspaceCleanupCandidateList({ rows, renderRow, scrollElement }) {
	const virtualize = rows.length >= 40;
	const virtualizer = useVirtualizer({
		count: rows.length,
		enabled: virtualize && scrollElement !== null,
		getScrollElement: () => scrollElement,
		estimateSize: () => WORKSPACE_CLEANUP_ROW_ESTIMATE_PX,
		overscan: WORKSPACE_CLEANUP_ROW_OVERSCAN,
		getItemKey: (index) => rows[index]?.worktreeId ?? index
	});
	if (!virtualize) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_jsx_runtime.Fragment, { children: rows.map((candidate, index) => renderRow(candidate, index)) });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "relative w-full",
		style: { height: virtualizer.getTotalSize() },
		children: virtualizer.getVirtualItems().map((item) => {
			const candidate = rows[item.index];
			if (candidate === void 0) return null;
			return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				ref: virtualizer.measureElement,
				"data-index": item.index,
				className: "absolute top-0 left-0 w-full",
				style: { transform: `translateY(${item.start}px)` },
				children: renderRow(candidate, item.index)
			}, item.key);
		})
	});
}
function resolveWorkspaceCleanupActiveView({ requestedView, counts, open, loading, hasScan }) {
	if (!open || loading || !hasScan || counts[requestedView] > 0) return requestedView;
	if (counts.ready > 0) return "ready";
	if (counts.review > 0) return "review";
	if (counts.protected > 0) return "protected";
	if (counts.hidden > 0) return "hidden";
	return requestedView;
}
function getSkippedAncestorMessage(provisional) {
	return provisional ? translate("auto.components.workspace.cleanup.backgroundRemoval.skippedPendingAncestor", "Skipped because a nested workspace has not finished removing.") : translate("auto.components.workspace.cleanup.backgroundRemoval.skippedAncestor", "Skipped because a nested workspace could not be removed.");
}
function isStrictWorkspaceCleanupDescendant(parent, child) {
	return parent.connectionId === child.connectionId && isStrictWorkspaceCleanupDescendantPath(parent.path, child.path);
}
function isStrictWorkspaceCleanupDescendantPath(parentPath, childPath) {
	return normalizeRuntimePathForComparison(parentPath) !== normalizeRuntimePathForComparison(childPath) && isPathInsideOrEqual(parentPath, childPath);
}
async function waitForWorkspaceCleanupRemovalWithTimeout(promise, timeoutMs, settlementGraceMs) {
	const settlement = toWorkspaceCleanupRemovalSettlement(promise);
	if (timeoutMs <= 0 || !Number.isFinite(timeoutMs)) return settlement;
	const outcome = await pollWorkspaceCleanupRemoval(settlement, timeoutMs + (settlementGraceMs > 0 && Number.isFinite(settlementGraceMs) ? settlementGraceMs : 0));
	return outcome.status === "unresolved" ? {
		status: "unresolved",
		settlement
	} : outcome;
}
function getWorkspaceCleanupTimeoutFailure(candidate) {
	return {
		worktreeId: candidate.worktreeId,
		displayName: candidate.displayName,
		message: translate("auto.components.workspace.cleanup.backgroundRemoval.timedOut", "Removing {{value0}} is taking longer than expected. It will keep running in the background.", { value0: candidate.displayName })
	};
}
function trackWorkspaceCleanupLateSettlement(settlement, candidate, reconcileBeforeBatchResult) {
	const candidateIdentity = {
		worktreeId: candidate.worktreeId,
		displayName: candidate.displayName
	};
	const state = {
		active: true,
		reconcile: reconcileBeforeBatchResult,
		report: null
	};
	settlement.then((outcome) => {
		const reconcile = state.reconcile;
		const report = state.report;
		state.active = false;
		state.reconcile = null;
		state.report = null;
		const result = toWorkspaceCleanupRemoveResult(candidateIdentity, outcome);
		if (reconcile) {
			reconcile(result);
			return;
		}
		report?.(candidateIdentity, result);
	}).catch((error) => {
		console.error("Workspace cleanup late settlement reporting failed", error);
	});
	return {
		candidate: candidateIdentity,
		detach: (reportAfterBatchResult) => {
			if (!state.active) return;
			state.reconcile = null;
			state.report = reportAfterBatchResult ?? null;
		}
	};
}
function toWorkspaceCleanupRemovalSettlement(promise) {
	return promise.then((result) => ({
		status: "fulfilled",
		result
	}), (error) => ({
		status: "rejected",
		error
	}));
}
function toWorkspaceCleanupRemoveResult(candidate, settlement) {
	if (settlement.status === "fulfilled") return settlement.result;
	return {
		removedIds: [],
		failures: [{
			worktreeId: candidate.worktreeId,
			displayName: candidate.displayName,
			message: settlement.error instanceof Error ? settlement.error.message : String(settlement.error)
		}]
	};
}
async function pollWorkspaceCleanupRemoval(settlement, timeoutMs) {
	let timeout = null;
	try {
		return await Promise.race([settlement, new Promise((resolve) => {
			timeout = setTimeout(() => {
				resolve({ status: "unresolved" });
			}, timeoutMs);
		})]);
	} finally {
		if (timeout) clearTimeout(timeout);
	}
}
function reclassifySkippedWorkspaceCleanupAncestors({ skippedAncestors, findBlockingDescendants, provisionallyBlocked, failedCandidates, failures }) {
	const unblocked = [];
	const updatedFailures = [];
	let changed = true;
	while (changed) {
		changed = false;
		let index = 0;
		while (index < skippedAncestors.length) {
			const entry = skippedAncestors[index];
			const blockers = findBlockingDescendants(entry.candidate);
			if (blockers.length === 0) {
				skippedAncestors.splice(index, 1);
				removeArrayEntry$2(failedCandidates, entry.candidate);
				removeArrayEntry$2(failures, entry.failure);
				provisionallyBlocked.delete(entry.candidate);
				unblocked.push(entry.candidate);
				changed = true;
				continue;
			}
			const provisional = blockers.every((blocker) => provisionallyBlocked.has(blocker));
			if (provisional !== entry.provisional) {
				entry.provisional = provisional;
				entry.failure.message = getSkippedAncestorMessage(provisional);
				if (provisional) provisionallyBlocked.add(entry.candidate);
				else provisionallyBlocked.delete(entry.candidate);
				updatedFailures.push(entry.failure);
				changed = true;
			}
			index += 1;
		}
	}
	return {
		unblocked,
		updatedFailures
	};
}
function removeArrayEntry$2(entries, entry) {
	const index = entries.indexOf(entry);
	if (index !== -1) entries.splice(index, 1);
}
function createPostBatchLateSettlementReporter({ skippedAncestors, failedCandidates, provisionallyBlocked, removeCandidates, removalTimeoutMs, removalSettlementGraceMs, reportResult }) {
	const retainedSkippedAncestors = skippedAncestors.filter((entry) => entry.provisional).map((entry) => ({
		...entry,
		failure: { ...entry.failure }
	}));
	const retainedCandidates = failedCandidates.filter((candidate) => provisionallyBlocked.has(candidate) && retainedSkippedAncestors.some((entry) => entry.candidate.worktreeId === candidate.worktreeId || isStrictWorkspaceCleanupDescendant(entry.candidate, candidate)));
	const state = {
		skippedAncestors: retainedSkippedAncestors,
		failedCandidates: retainedCandidates,
		failures: retainedSkippedAncestors.map((entry) => entry.failure),
		provisionallyBlocked: new Set(retainedCandidates),
		removeCandidates,
		removalTimeoutMs,
		removalSettlementGraceMs
	};
	let reconcileChain = Promise.resolve();
	const reportLateSettlement = (candidate, result) => {
		reconcileChain = reconcileChain.then(async () => {
			const reconciled = await reconcilePostBatchLateSettlement(state, candidate, result, reportLateSettlement);
			if (reconciled.result.removedIds.length > 0 || reconciled.result.failures.length > 0) reportResult(reconciled.result, reconciled.pendingSettlementFailures);
		}).catch((error) => {
			console.error("Workspace cleanup post-batch late settlement failed", error);
		});
		reportResult(result);
	};
	const retainedCandidateIds = new Set(retainedCandidates.map((candidate) => candidate.worktreeId));
	const reportWithoutReconciliation = (_candidate, result) => {
		reportResult(result);
	};
	return (candidate) => retainedCandidateIds.has(candidate.worktreeId) ? reportLateSettlement : reportWithoutReconciliation;
}
async function reconcilePostBatchLateSettlement(state, settledCandidateIdentity, lateResult, reportLateSettlement) {
	const settledCandidate = state.failedCandidates.find((candidate) => candidate.worktreeId === settledCandidateIdentity.worktreeId);
	if (settledCandidate) {
		state.provisionallyBlocked.delete(settledCandidate);
		if (lateResult.failures.length === 0) removeArrayEntry$1(state.failedCandidates, settledCandidate);
	}
	const removedIds = [];
	const lateFailures = [];
	const pendingSettlementFailures = /* @__PURE__ */ new Set();
	const findBlockingDescendants = (candidate) => state.failedCandidates.filter((failedCandidate) => isStrictWorkspaceCleanupDescendant(candidate, failedCandidate));
	const { unblocked, updatedFailures } = reclassifySkippedWorkspaceCleanupAncestors({
		skippedAncestors: state.skippedAncestors,
		findBlockingDescendants,
		provisionallyBlocked: state.provisionallyBlocked,
		failedCandidates: state.failedCandidates,
		failures: state.failures
	});
	lateFailures.push(...updatedFailures);
	unblocked.sort((a, b) => b.path.length - a.path.length);
	for (const ancestor of unblocked) {
		const blockers = findBlockingDescendants(ancestor);
		if (blockers.length > 0) {
			const provisional = blockers.every((blocker) => state.provisionallyBlocked.has(blocker));
			const failure = {
				worktreeId: ancestor.worktreeId,
				displayName: ancestor.displayName,
				message: getSkippedAncestorMessage(provisional)
			};
			if (provisional) state.provisionallyBlocked.add(ancestor);
			state.failedCandidates.push(ancestor);
			state.skippedAncestors.push({
				candidate: ancestor,
				failure,
				provisional
			});
			state.failures.push(failure);
			lateFailures.push(failure);
			continue;
		}
		const removeCandidates = state.removeCandidates;
		if (!removeCandidates) continue;
		let removal;
		try {
			removal = removeCandidates([ancestor.worktreeId], { approvedCandidates: [ancestor] });
		} catch (error) {
			state.failedCandidates.push(ancestor);
			lateFailures.push({
				worktreeId: ancestor.worktreeId,
				displayName: ancestor.displayName,
				message: error instanceof Error ? error.message : String(error)
			});
			continue;
		}
		const outcome = await waitForWorkspaceCleanupRemovalWithTimeout(removal, state.removalTimeoutMs, state.removalSettlementGraceMs);
		if (outcome.status === "unresolved") {
			const timeoutFailure = getWorkspaceCleanupTimeoutFailure(ancestor);
			state.failedCandidates.push(ancestor);
			state.provisionallyBlocked.add(ancestor);
			lateFailures.push(timeoutFailure);
			pendingSettlementFailures.add(timeoutFailure);
			trackWorkspaceCleanupLateSettlement(outcome.settlement, ancestor, () => {}).detach(reportLateSettlement);
			continue;
		}
		const result = outcome.status === "fulfilled" ? outcome.result : {
			removedIds: [],
			failures: [{
				worktreeId: ancestor.worktreeId,
				displayName: ancestor.displayName,
				message: outcome.error instanceof Error ? outcome.error.message : String(outcome.error)
			}]
		};
		removedIds.push(...result.removedIds);
		if (result.failures.length > 0) {
			state.failedCandidates.push(ancestor);
			lateFailures.push(...result.failures);
		}
	}
	releaseSettledPostBatchState(state);
	return {
		result: {
			removedIds,
			failures: lateFailures
		},
		pendingSettlementFailures: pendingSettlementFailures.size > 0 ? pendingSettlementFailures : void 0
	};
}
function releaseSettledPostBatchState(state) {
	if (state.skippedAncestors.some((entry) => entry.provisional)) return;
	state.skippedAncestors.length = 0;
	state.failedCandidates.length = 0;
	state.failures.length = 0;
	state.provisionallyBlocked.clear();
	state.removeCandidates = null;
}
function removeArrayEntry$1(entries, entry) {
	const index = entries.indexOf(entry);
	if (index !== -1) entries.splice(index, 1);
}
function showWorkspaceCleanupRemovalResultToasts(result, pendingSettlementFailures) {
	if (result.preservedBranches && result.preservedBranches.length > 0) showPreservedBranchBatchToast(result.removedIds.length, result.preservedBranches);
	else if (result.removedIds.length > 0) toast.success(translate("auto.components.workspace.cleanup.backgroundRemoval.removed", "Removed workspaces: {{value0}}", { value0: result.removedIds.length }));
	const definitiveFailures = pendingSettlementFailures ? result.failures.filter((failure) => !pendingSettlementFailures.has(failure)) : result.failures;
	if (definitiveFailures.length > 0) toast.error(translate("auto.components.workspace.cleanup.backgroundRemoval.failed", "Workspaces not removed: {{value0}}", { value0: definitiveFailures.length }), { description: definitiveFailures.map((failure) => failure.message).join("; ") });
	const stillRemovingCount = result.failures.length - definitiveFailures.length;
	if (stillRemovingCount > 0) toast.info(translate("auto.components.workspace.cleanup.backgroundRemoval.stillRemoving", "Still removing workspaces: {{value0}}", { value0: stillRemovingCount }));
}
var DEFAULT_WORKSPACE_CLEANUP_REMOVAL_TIMEOUT_MS = 12e4;
var DEFAULT_WORKSPACE_CLEANUP_SETTLEMENT_GRACE_MS = 5e3;
function startWorkspaceCleanupBackgroundRemoval({ candidates, removeCandidates, onProgress, onResult, onLateResult, onError, onRowFailed, removalTimeoutMs = DEFAULT_WORKSPACE_CLEANUP_REMOVAL_TIMEOUT_MS, removalSettlementGraceMs = DEFAULT_WORKSPACE_CLEANUP_SETTLEMENT_GRACE_MS }) {
	if (candidates.length === 0) {
		try {
			onResult?.({
				removedIds: [],
				failures: []
			});
		} catch (callbackError) {
			console.error("Workspace cleanup result callback failed", callbackError);
		}
		return;
	}
	const count = candidates.length;
	const removedIds = [];
	const failures = [];
	const preservedBranches = [];
	const failedCandidates = [];
	const lateSettlementTrackers = [];
	const provisionallyBlocked = /* @__PURE__ */ new Set();
	const pendingSettlementFailures = /* @__PURE__ */ new Set();
	const skippedAncestors = [];
	let processedCount = 0;
	const emitProgress = () => {
		onProgress({
			totalCount: count,
			processedCount,
			removedCount: removedIds.length,
			failedCount: failures.length
		});
	};
	const reportFailures = (rowFailures) => {
		for (const failure of rowFailures) {
			failures.push(failure);
			try {
				onRowFailed?.(failure);
			} catch (callbackError) {
				console.error("Workspace cleanup row failure callback failed", callbackError);
			}
		}
	};
	const detachAllLateResultReconcilers = (getReportAfterBatchResult) => {
		for (const tracker of lateSettlementTrackers) tracker.detach(getReportAfterBatchResult?.(tracker.candidate));
	};
	emitProgress();
	const queue = [...candidates].sort((a, b) => b.path.length - a.path.length);
	const findBlockingDescendants = (candidate) => failedCandidates.filter((failedCandidate) => isStrictWorkspaceCleanupDescendant(candidate, failedCandidate));
	const skipBlockedAncestor = (candidate, blockers) => {
		const provisional = blockers.every((blocker) => provisionallyBlocked.has(blocker));
		const failure = {
			worktreeId: candidate.worktreeId,
			displayName: candidate.displayName,
			message: getSkippedAncestorMessage(provisional)
		};
		if (provisional) provisionallyBlocked.add(candidate);
		failedCandidates.push(candidate);
		skippedAncestors.push({
			candidate,
			failure,
			provisional
		});
		reportFailures([failure]);
		processedCount += 1;
		emitProgress();
	};
	const resettleSkippedAncestors = () => {
		const { unblocked } = reclassifySkippedWorkspaceCleanupAncestors({
			skippedAncestors,
			findBlockingDescendants,
			provisionallyBlocked,
			failedCandidates,
			failures
		});
		for (const candidate of unblocked) {
			processedCount -= 1;
			queue.push(candidate);
		}
	};
	(async () => {
		while (queue.length > 0) {
			const candidate = queue.shift();
			if (!candidate) break;
			const blockers = findBlockingDescendants(candidate);
			if (blockers.length > 0) {
				skipBlockedAncestor(candidate, blockers);
				continue;
			}
			try {
				const outcome = await waitForWorkspaceCleanupRemovalWithTimeout(removeCandidates([candidate.worktreeId], { approvedCandidates: [candidate] }), removalTimeoutMs, removalSettlementGraceMs);
				if (outcome.status === "rejected") throw outcome.error;
				if (outcome.status === "unresolved") {
					const timeoutFailure = getWorkspaceCleanupTimeoutFailure(candidate);
					failedCandidates.push(candidate);
					provisionallyBlocked.add(candidate);
					pendingSettlementFailures.add(timeoutFailure);
					reportFailures([timeoutFailure]);
					lateSettlementTrackers.push(trackWorkspaceCleanupLateSettlement(outcome.settlement, candidate, (lateResult) => {
						removeArrayEntry(failures, timeoutFailure);
						pendingSettlementFailures.delete(timeoutFailure);
						provisionallyBlocked.delete(candidate);
						removedIds.push(...lateResult.removedIds);
						preservedBranches.push(...lateResult.preservedBranches ?? []);
						reportFailures(lateResult.failures);
						if (lateResult.failures.length === 0) removeArrayEntry(failedCandidates, candidate);
						resettleSkippedAncestors();
						emitProgress();
					}));
					continue;
				}
				const result$1 = outcome.result;
				removedIds.push(...result$1.removedIds);
				preservedBranches.push(...result$1.preservedBranches ?? []);
				reportFailures(result$1.failures);
				if (result$1.failures.length > 0) failedCandidates.push(candidate);
			} catch (error) {
				failedCandidates.push(candidate);
				reportFailures([{
					worktreeId: candidate.worktreeId,
					displayName: candidate.displayName,
					message: error instanceof Error ? error.message : String(error)
				}]);
			} finally {
				processedCount += 1;
				emitProgress();
			}
		}
		detachAllLateResultReconcilers(createPostBatchLateSettlementReporter({
			skippedAncestors,
			failedCandidates,
			provisionallyBlocked,
			removeCandidates,
			removalTimeoutMs,
			removalSettlementGraceMs,
			reportResult: (lateResult, latePendingFailures) => {
				reportLateWorkspaceCleanupResult(lateResult, onLateResult, latePendingFailures);
			}
		}));
		const result = {
			removedIds,
			failures,
			...preservedBranches.length > 0 ? { preservedBranches } : {}
		};
		try {
			onResult?.(result);
		} catch (callbackError) {
			console.error("Workspace cleanup result callback failed", callbackError);
		}
		showWorkspaceCleanupRemovalResultToasts(result, pendingSettlementFailures);
	})().catch((error) => {
		detachAllLateResultReconcilers();
		onError?.(error);
		toast.error(translate("auto.components.workspace.cleanup.backgroundRemoval.error", "Workspace cleanup failed"), { description: error instanceof Error ? error.message : String(error) });
	});
}
function reportLateWorkspaceCleanupResult(result, onLateResult, pendingSettlementFailures) {
	try {
		onLateResult?.(result);
	} catch (callbackError) {
		console.error("Workspace cleanup late result callback failed", callbackError);
	}
	showWorkspaceCleanupRemovalResultToasts(result, pendingSettlementFailures);
}
function removeArrayEntry(entries, entry) {
	const index = entries.indexOf(entry);
	if (index !== -1) entries.splice(index, 1);
}
function filterWorkspaceCleanupRemovalCandidates(candidates, deleteStateByWorktreeId) {
	return candidates.filter((candidate) => deleteStateByWorktreeId[candidate.worktreeId]?.isDeleting !== true);
}
function useWorkspaceCleanupRemovalSession({ mountedRef, setSelectedIds, closeModal, removeCandidates, markWorktreesQueuedForDeletion, clearWorktreeDeleteState }) {
	const [confirming, setConfirming] = (0, import_react.useState)(false);
	const [confirmCandidates, setConfirmCandidates] = (0, import_react.useState)([]);
	const [removalProgress, setRemovalProgress] = (0, import_react.useState)(null);
	const [removalInFlight, setRemovalInFlight] = (0, import_react.useState)(false);
	const [rowFailures, setRowFailures] = (0, import_react.useState)({});
	const removalInFlightRef = (0, import_react.useRef)(false);
	const removalBatchIdRef = (0, import_react.useRef)(0);
	const clearRowFailures = (0, import_react.useCallback)(() => setRowFailures({}), []);
	const resetForOpen = (0, import_react.useCallback)(() => {
		setConfirming(false);
		setRowFailures({});
		setSelectedIds(/* @__PURE__ */ new Set());
	}, [setSelectedIds]);
	const applyScanDefaults = (0, import_react.useCallback)((candidates, deletingWorktreeIds) => {
		if (removalInFlightRef.current) return;
		setSelectedIds(getDefaultSelectedWorkspaceCleanupIds(candidates, deletingWorktreeIds));
		setConfirming(false);
		setRowFailures({});
	}, [setSelectedIds]);
	const openConfirmRemove = (0, import_react.useCallback)((candidates) => {
		const nextCandidates = filterWorkspaceCleanupRemovalCandidates(candidates, useAppStore.getState().deleteStateByWorktreeId);
		if (nextCandidates.length === 0) return;
		setConfirmCandidates(nextCandidates);
		setConfirming(true);
	}, []);
	const cancelConfirmRemove = (0, import_react.useCallback)(() => {
		if (removalProgress) {
			closeModal();
			return;
		}
		setConfirming(false);
		setConfirmCandidates([]);
	}, [closeModal, removalProgress]);
	const backToWorkspaceCleanupList = (0, import_react.useCallback)(() => {
		setConfirming(false);
		setConfirmCandidates([]);
	}, []);
	const clearQueuedDeleteState = (0, import_react.useCallback)((worktreeId) => {
		const deleteState = useAppStore.getState().deleteStateByWorktreeId[worktreeId];
		if (deleteState?.isDeleting && deleteState.error === null && deleteState.phase === "queued") clearWorktreeDeleteState(worktreeId);
	}, [clearWorktreeDeleteState]);
	const deselectRemovedIds = (0, import_react.useCallback)((removedIds) => {
		if (removedIds.length === 0) return;
		setSelectedIds((current) => {
			const next = new Set(current);
			for (const id of removedIds) next.delete(id);
			return next;
		});
	}, [setSelectedIds]);
	return {
		confirming,
		confirmCandidates,
		removalProgress,
		removalInFlight,
		rowFailures,
		removalInFlightRef,
		clearRowFailures,
		resetForOpen,
		applyScanDefaults,
		openConfirmRemove,
		cancelConfirmRemove,
		backToWorkspaceCleanupList,
		confirmRemove: (0, import_react.useCallback)(() => {
			if (confirmCandidates.length === 0 || removalInFlightRef.current) return;
			const removableCandidates = filterWorkspaceCleanupRemovalCandidates(confirmCandidates, useAppStore.getState().deleteStateByWorktreeId);
			if (removableCandidates.length === 0) {
				setConfirming(false);
				setConfirmCandidates([]);
				return;
			}
			removalInFlightRef.current = true;
			setRemovalInFlight(true);
			removalBatchIdRef.current += 1;
			const removalBatchId = removalBatchIdRef.current;
			const removableWorktreeIds = removableCandidates.map((candidate) => candidate.worktreeId);
			setRowFailures({});
			markWorktreesQueuedForDeletion(removableWorktreeIds);
			startWorkspaceCleanupBackgroundRemoval({
				candidates: removableCandidates,
				removeCandidates,
				onProgress: (progress) => {
					if (mountedRef.current) setRemovalProgress(progress);
				},
				onRowFailed: (failure) => {
					clearQueuedDeleteState(failure.worktreeId);
				},
				onResult: (result) => {
					const nextFailures = {};
					for (const failure of result.failures) {
						nextFailures[failure.worktreeId] = failure.message;
						clearQueuedDeleteState(failure.worktreeId);
					}
					if (mountedRef.current) {
						setRowFailures(nextFailures);
						deselectRemovedIds(result.removedIds);
						setRemovalProgress(null);
						setRemovalInFlight(false);
						setConfirming(false);
						setConfirmCandidates([]);
					}
					removalInFlightRef.current = false;
				},
				onLateResult: (result) => {
					for (const failure of result.failures) clearQueuedDeleteState(failure.worktreeId);
					if (!mountedRef.current || removalBatchIdRef.current !== removalBatchId) return;
					setRowFailures((current) => {
						const next = { ...current };
						for (const id of result.removedIds) delete next[id];
						for (const failure of result.failures) next[failure.worktreeId] = failure.message;
						return next;
					});
					deselectRemovedIds(result.removedIds);
				},
				onError: () => {
					for (const worktreeId of removableWorktreeIds) clearWorktreeDeleteState(worktreeId);
					if (mountedRef.current) {
						setRemovalProgress(null);
						setRemovalInFlight(false);
						setConfirming(false);
						setConfirmCandidates([]);
					}
					removalInFlightRef.current = false;
				}
			});
		}, [
			clearQueuedDeleteState,
			clearWorktreeDeleteState,
			confirmCandidates,
			deselectRemovedIds,
			markWorktreesQueuedForDeletion,
			mountedRef,
			removeCandidates
		])
	};
}
function getDefaultSelectedWorkspaceCleanupIds(candidates, deletingWorktreeIds) {
	return new Set(candidates.filter((candidate) => candidate.selectedByDefault && !deletingWorktreeIds.has(candidate.worktreeId)).map((candidate) => candidate.worktreeId));
}
function useWorkspaceCleanupScanSession({ open, mountedRef, openModal, scanWorkspaceCleanup, clearRowFailures }) {
	const openRef = (0, import_react.useRef)(open);
	const latestReadyToastScanAtRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		openRef.current = open;
	}, [open]);
	return (0, import_react.useCallback)((options = {}) => {
		clearRowFailures();
		scanWorkspaceCleanup().then((result) => {
			if (!mountedRef.current || !options.notifyWhenReady || openRef.current) return;
			if (latestReadyToastScanAtRef.current === result.scannedAt) return;
			latestReadyToastScanAtRef.current = result.scannedAt;
			const suggestedCount = result.candidates.filter((candidate) => candidate.selectedByDefault).length;
			toast.success(translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.0e2d235c63", "Inactive workspace scan ready"), {
				description: formatWorkspaceCleanupReadyToastDescription(result.candidates.length, suggestedCount),
				action: {
					label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.4a35c08764", "Review"),
					onClick: () => openModal("workspace-cleanup")
				}
			});
		}).catch((error) => {
			if (!mountedRef.current) return;
			toast.error(translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.662b8ec3f8", "Workspace cleanup scan failed"), { description: error instanceof Error ? error.message : String(error) });
		});
	}, [
		clearRowFailures,
		mountedRef,
		openModal,
		scanWorkspaceCleanup
	]);
}
function formatWorkspaceCleanupReadyToastDescription(inactiveCount, suggestedCount) {
	if (inactiveCount === 0) return "No inactive workspaces found.";
	return `${inactiveCount} inactive ${inactiveCount === 1 ? "workspace" : "workspaces"} found, with ${suggestedCount} cleanup ${suggestedCount === 1 ? "suggestion" : "suggestions"}.`;
}
const DEFAULT_WORKSPACE_CLEANUP_FILTERS = {
	query: "",
	time: "all",
	review: "all",
	git: "all",
	context: "all"
};
function useWorkspaceCleanupDialogSession() {
	const { open, loading, openModal, closeModal, scanWorkspaceCleanup, markCandidateViewed, dismissCandidates, resetDismissals, removeCandidates, markWorktreesQueuedForDeletion, clearWorktreeDeleteState } = useAppStore(useShallow((state) => ({
		open: state.activeModal === "workspace-cleanup",
		loading: state.workspaceCleanupLoading,
		openModal: state.openModal,
		closeModal: state.closeModal,
		scanWorkspaceCleanup: state.scanWorkspaceCleanup,
		markCandidateViewed: state.markWorkspaceCleanupCandidateViewed,
		dismissCandidates: state.dismissWorkspaceCleanupCandidates,
		resetDismissals: state.resetWorkspaceCleanupDismissals,
		removeCandidates: state.removeWorkspaceCleanupCandidates,
		markWorktreesQueuedForDeletion: state.markWorktreesQueuedForDeletion,
		clearWorktreeDeleteState: state.clearWorktreeDeleteState
	})));
	const [selectedIds, setSelectedIds] = (0, import_react.useState)(() => /* @__PURE__ */ new Set());
	const [expandedRowIds, setExpandedRowIds] = (0, import_react.useState)(() => /* @__PURE__ */ new Set());
	const [activeView, setActiveView] = (0, import_react.useState)("ready");
	const [repoSelection, setRepoSelection] = (0, import_react.useState)(() => /* @__PURE__ */ new Set());
	const [filters, setFilters] = (0, import_react.useState)(DEFAULT_WORKSPACE_CLEANUP_FILTERS);
	const [sortKey, setSortKey] = (0, import_react.useState)("activity");
	const [sortDirection, setSortDirection] = (0, import_react.useState)("asc");
	const selectedDefaultsScanAtRef = (0, import_react.useRef)(null);
	const autoScanAttemptedForOpenRef = (0, import_react.useRef)(false);
	const wasOpenRef = (0, import_react.useRef)(false);
	const mountedRef = useMountedRef();
	const removal = useWorkspaceCleanupRemovalSession({
		mountedRef,
		setSelectedIds,
		closeModal,
		removeCandidates,
		markWorktreesQueuedForDeletion,
		clearWorktreeDeleteState
	});
	const { clearRowFailures, resetForOpen, ...publicRemoval } = removal;
	const startScan = useWorkspaceCleanupScanSession({
		open,
		mountedRef,
		openModal,
		scanWorkspaceCleanup,
		clearRowFailures
	});
	(0, import_react.useEffect)(() => {
		if (!open) {
			wasOpenRef.current = false;
			autoScanAttemptedForOpenRef.current = false;
			return;
		}
		if (!wasOpenRef.current) {
			wasOpenRef.current = true;
			autoScanAttemptedForOpenRef.current = false;
			if (!removal.removalInFlightRef.current) {
				setActiveView("ready");
				setFilters(DEFAULT_WORKSPACE_CLEANUP_FILTERS);
				setSortKey("activity");
				setSortDirection("asc");
				resetForOpen();
			}
		}
		if (!loading && !autoScanAttemptedForOpenRef.current && !removal.removalInFlightRef.current) {
			autoScanAttemptedForOpenRef.current = true;
			startScan({ notifyWhenReady: true });
		}
	}, [
		loading,
		open,
		removal.removalInFlightRef,
		resetForOpen,
		startScan
	]);
	const ignoreCandidate = (0, import_react.useCallback)((candidate) => {
		dismissCandidates([candidate]).then(() => {
			if (!mountedRef.current) return;
			setSelectedIds((current) => {
				const next = new Set(current);
				next.delete(candidate.worktreeId);
				return next;
			});
		}).catch((error) => {
			if (!mountedRef.current) return;
			toast.error(translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.7f451a3e2c", "Could not ignore cleanup suggestion"), { description: error instanceof Error ? error.message : String(error) });
		});
	}, [dismissCandidates, mountedRef]);
	return {
		open,
		loading,
		selectedIds,
		setSelectedIds,
		expandedRowIds,
		setExpandedRowIds,
		activeView,
		setActiveView,
		repoSelection,
		setRepoSelection,
		filters,
		setFilters,
		sortKey,
		setSortKey,
		sortDirection,
		setSortDirection,
		selectedDefaultsScanAtRef,
		close: closeModal,
		markCandidateViewed,
		restoreDismissals: (0, import_react.useCallback)(() => {
			resetDismissals();
		}, [resetDismissals]),
		startScan,
		ignoreCandidate,
		...publicRemoval
	};
}
var WORKSPACE_CLEANUP_CLOSE_LINGER_MS = 300;
var EMPTY_REVIEW_INFO = {
	hasReview: false,
	label: null,
	state: null,
	provider: null,
	title: null
};
function formatRelativeTime(timestamp) {
	if (!timestamp) return "Never";
	const deltaMs = Date.now() - timestamp;
	if (deltaMs < 6e4) return "Just now";
	const minutes = Math.floor(deltaMs / 6e4);
	if (minutes < 60) return `${minutes}m ago`;
	const hours = Math.floor(minutes / 60);
	if (hours < 48) return `${hours}h ago`;
	return `${Math.floor(hours / 24)}d ago`;
}
function isDisconnectedRemoteScanError(message) {
	return message === "SSH provider is unavailable." || message === "Remote workspaces are not connected. Reconnect and refresh to check them.";
}
function formatScanNoticeMessage(errors, repoNameById) {
	const visibleErrors = errors.filter((error) => !isDisconnectedRemoteScanError(error.message ?? ""));
	if (visibleErrors.length === 0) return null;
	if (visibleErrors.length === 1) {
		const error = visibleErrors[0];
		return `Could not check ${formatScanErrorRepoName(error, repoNameById)}: ${formatScanErrorReason(error.message)}. Some inactive workspaces may be missing. Refresh to try again.`;
	}
	const repoNames = visibleErrors.slice(0, 3).map((error) => formatScanErrorRepoName(error, repoNameById)).join(", ");
	const moreCount = visibleErrors.length - 3;
	const suffix = moreCount > 0 ? `, +${moreCount} more` : "";
	return `Could not check ${visibleErrors.length} repositories (${repoNames}${suffix}). Some inactive workspaces may be missing. Refresh to try again.`;
}
function formatScanErrorRepoName(error, repoNameById) {
	const repoName = error.repoName?.trim();
	if (repoName) return repoName;
	return (error.repoId ? repoNameById.get(error.repoId)?.trim() : "") || "a repository";
}
function formatScanErrorReason(message) {
	if (!message) return "Git could not list worktrees";
	if (message === "Could not scan workspace cleanup for this repository.") return "Git could not list worktrees";
	return message.replace(/\.$/, "");
}
function WorkspaceCleanupDialog() {
	const session = useWorkspaceCleanupDialogSession();
	const [lingering, setLingering] = (0, import_react.useState)(session.open);
	(0, import_react.useEffect)(() => {
		if (session.open) {
			setLingering(true);
			return;
		}
		const timer = window.setTimeout(() => setLingering(false), WORKSPACE_CLEANUP_CLOSE_LINGER_MS);
		return () => window.clearTimeout(timer);
	}, [session.open]);
	if (!session.open && !lingering) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCleanupDialogContent, { session });
}
function WorkspaceCleanupDialogContent({ session }) {
	const { open, loading, selectedIds, setSelectedIds, expandedRowIds, setExpandedRowIds, activeView, setActiveView, confirming, confirmCandidates, removalProgress, removalInFlight, rowFailures, repoSelection, setRepoSelection, filters, setFilters, sortKey, setSortKey, sortDirection, setSortDirection, selectedDefaultsScanAtRef, removalInFlightRef, close: closeModal, markCandidateViewed, restoreDismissals: resetDismissals, startScan: startWorkspaceCleanupScan, ignoreCandidate, applyScanDefaults, openConfirmRemove, cancelConfirmRemove, backToWorkspaceCleanupList, confirmRemove } = session;
	const scan = useAppStore((s) => s.workspaceCleanupScan);
	const scanProgress = useAppStore((s) => s.workspaceCleanupProgress);
	const error = useAppStore((s) => s.workspaceCleanupError);
	const repos = useAppStore((s) => s.repos);
	const reviewStateInputs = useAppStore(useShallow((s) => ({
		worktreesByRepo: s.worktreesByRepo,
		hostedReviewCache: s.hostedReviewCache,
		repos: s.repos,
		settings: s.settings
	})));
	const deletionPhaseByWorktreeId = useAppStore(useShallow((s) => {
		const phases = {};
		for (const [worktreeId, state] of Object.entries(s.deleteStateByWorktreeId)) if (state.isDeleting) phases[worktreeId] = state.phase ?? "deleting";
		return phases;
	}));
	const deletingWorktreeIds = (0, import_react.useMemo)(() => new Set(Object.keys(deletionPhaseByWorktreeId)), [deletionPhaseByWorktreeId]);
	const [rowsScrollElement, setRowsScrollElement] = (0, import_react.useState)(null);
	const eligibleRepos = (0, import_react.useMemo)(() => repos.filter((repo) => isGitRepoKind(repo)), [repos]);
	const eligibleRepoIds = (0, import_react.useMemo)(() => eligibleRepos.map((repo) => repo.id), [eligibleRepos]);
	(0, import_react.useEffect)(() => {
		if (!open) return;
		setRepoSelection(new Set(eligibleRepoIds));
	}, [
		eligibleRepoIds,
		open,
		setRepoSelection
	]);
	const candidates = (0, import_react.useMemo)(() => scan?.candidates ?? [], [scan?.candidates]);
	const reviewInfoByWorktreeId = (0, import_react.useMemo)(() => {
		const infos = /* @__PURE__ */ new Map();
		for (const candidate of candidates) infos.set(candidate.worktreeId, getWorkspaceCleanupReviewInfo(candidate, reviewStateInputs));
		return infos;
	}, [candidates, reviewStateInputs]);
	const effectiveRepoSelection = (0, import_react.useMemo)(() => {
		if (repoSelection.size > 0 || eligibleRepoIds.length === 0) return repoSelection;
		return new Set(eligibleRepoIds);
	}, [eligibleRepoIds, repoSelection]);
	const selectedScanErrors = (0, import_react.useMemo)(() => (scan?.errors ?? []).filter((error$1) => effectiveRepoSelection.has(error$1.repoId)), [effectiveRepoSelection, scan?.errors]);
	const filteredCandidates = (0, import_react.useMemo)(() => {
		if (effectiveRepoSelection.size === 0 || effectiveRepoSelection.size === eligibleRepoIds.length) return candidates;
		return candidates.filter((candidate) => effectiveRepoSelection.has(candidate.repoId));
	}, [
		candidates,
		effectiveRepoSelection,
		eligibleRepoIds.length
	]);
	const estimatedInactiveCount = (0, import_react.useMemo)(() => {
		if (!open) return null;
		return countEstimatedInactiveWorkspaces(Object.values(reviewStateInputs.worktreesByRepo).flat(), new Map(reviewStateInputs.repos.map((repo) => [repo.id, repo])), Date.now());
	}, [open, reviewStateInputs]);
	const estimateMismatchNotice = !loading && !error && selectedScanErrors.length === 0 && scan && estimatedInactiveCount !== null && estimatedInactiveCount !== candidates.length && filteredCandidates.length === candidates.length ? translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.f637f63882", "Resource Manager counts {{value0}}; this list found {{value1}}. That counter reads Orca's activity record alone, while this scan also checks each workspace's git history and skips disconnected remotes.", {
		value0: estimatedInactiveCount,
		value1: candidates.length
	}) : null;
	(0, import_react.useEffect)(() => {
		if (loading || !scan || selectedDefaultsScanAtRef.current === scan.scannedAt) return;
		selectedDefaultsScanAtRef.current = scan.scannedAt;
		applyScanDefaults(scan.candidates, deletingWorktreeIds);
	}, [
		applyScanDefaults,
		deletingWorktreeIds,
		loading,
		scan,
		selectedDefaultsScanAtRef
	]);
	const visibleCandidates = (0, import_react.useMemo)(() => {
		return sortWorkspaceCleanupCandidates(filteredCandidates.filter((candidate) => !candidate.blockers.includes("dismissed")), "activity", "asc", reviewInfoByWorktreeId);
	}, [filteredCandidates, reviewInfoByWorktreeId]);
	const hiddenCandidates = (0, import_react.useMemo)(() => sortWorkspaceCleanupCandidates(filteredCandidates.filter((candidate) => candidate.blockers.includes("dismissed")), "activity", "asc", reviewInfoByWorktreeId), [filteredCandidates, reviewInfoByWorktreeId]);
	const groups = (0, import_react.useMemo)(() => ({
		ready: visibleCandidates.filter((candidate) => candidate.tier === "ready"),
		review: visibleCandidates.filter((candidate) => candidate.tier === "review"),
		protected: visibleCandidates.filter((candidate) => candidate.tier === "protected")
	}), [visibleCandidates]);
	const cleanupViewCounts = (0, import_react.useMemo)(() => ({
		ready: groups.ready.length,
		review: groups.review.length,
		protected: groups.protected.length,
		hidden: hiddenCandidates.length
	}), [
		groups.protected.length,
		groups.ready.length,
		groups.review.length,
		hiddenCandidates.length
	]);
	const resolvedActiveView = resolveWorkspaceCleanupActiveView({
		requestedView: activeView,
		counts: cleanupViewCounts,
		open,
		loading,
		hasScan: scan != null
	});
	const repoNameById = (0, import_react.useMemo)(() => new Map(repos.map((repo) => [repo.id, repo.displayName || repo.path])), [repos]);
	const scanNoticeMessage = (0, import_react.useMemo)(() => formatScanNoticeMessage(selectedScanErrors, repoNameById), [repoNameById, selectedScanErrors]);
	const hasAnyCandidates = candidates.length > 0;
	const initialLoading = loading && !hasAnyCandidates;
	const activeBaseRows = resolvedActiveView === "hidden" ? hiddenCandidates : groups[resolvedActiveView];
	const activeRows = (0, import_react.useMemo)(() => sortWorkspaceCleanupCandidates(filterWorkspaceCleanupCandidates(activeBaseRows, filters, reviewInfoByWorktreeId, scan?.scannedAt ?? Date.now()), sortKey, sortDirection, reviewInfoByWorktreeId), [
		activeBaseRows,
		filters,
		reviewInfoByWorktreeId,
		scan?.scannedAt,
		sortDirection,
		sortKey
	]);
	const activeRowIds = (0, import_react.useMemo)(() => new Set(activeRows.map((candidate) => candidate.worktreeId)), [activeRows]);
	const activeFilters = hasActiveWorkspaceCleanupFilters(filters);
	const selectedCandidates = (0, import_react.useMemo)(() => {
		const byId = new Map(activeRows.map((candidate) => [candidate.worktreeId, candidate]));
		return [...selectedIds].map((id) => byId.get(id)).filter((candidate) => candidate != null && canQueueWorkspaceCleanupCandidate(candidate) && !deletingWorktreeIds.has(candidate.worktreeId));
	}, [
		activeRows,
		deletingWorktreeIds,
		selectedIds
	]);
	(0, import_react.useEffect)(() => {
		if (!open || confirming) return;
		setSelectedIds((current) => {
			const next = new Set([...current].filter((id) => activeRowIds.has(id) && !deletingWorktreeIds.has(id)));
			return next.size === current.size ? current : next;
		});
	}, [
		activeRowIds,
		confirming,
		deletingWorktreeIds,
		open,
		setSelectedIds
	]);
	const handleOpenChange = (0, import_react.useCallback)((nextOpen) => {
		if (!nextOpen) closeModal();
	}, [closeModal]);
	const refresh = (0, import_react.useCallback)(() => {
		startWorkspaceCleanupScan({ notifyWhenReady: true });
	}, [startWorkspaceCleanupScan]);
	const toggleExpandedRow = (0, import_react.useCallback)((worktreeId) => {
		setExpandedRowIds((current) => toggleSetMember(current, worktreeId));
	}, [setExpandedRowIds]);
	const toggleSelectedRow = (0, import_react.useCallback)((worktreeId) => {
		setSelectedIds((current) => toggleSetMember(current, worktreeId));
	}, [setSelectedIds]);
	const handleRemoveRow = (0, import_react.useCallback)((candidate) => {
		if (loading || removalInFlightRef.current) return;
		setSelectedIds(new Set([candidate.worktreeId]));
		openConfirmRemove([candidate]);
	}, [
		loading,
		openConfirmRemove,
		removalInFlightRef,
		setSelectedIds
	]);
	const handleViewCandidate = (0, import_react.useCallback)((candidate) => {
		markCandidateViewed(candidate);
		closeModal();
		activateAndRevealWorktree(candidate.worktreeId);
	}, [closeModal, markCandidateViewed]);
	const selectedCount = selectedCandidates.length;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange: handleOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogContent, {
			showCloseButton: false,
			className: "flex h-[min(820px,90vh)] w-[calc(100vw-3rem)] max-w-[calc(100vw-3rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-[calc(100vw-3rem)] xl:w-[920px] xl:max-w-[920px]",
			children: !confirming ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogHeader, {
					className: "border-b border-border px-5 py-4",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-start justify-between gap-4",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "min-w-0",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
								className: "text-base",
								children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.b2c1331844", "Delete Inactive Workspaces")
							})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex shrink-0 items-center gap-2",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
								asChild: true,
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
									variant: "outline",
									size: "icon-sm",
									"aria-label": translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.7ae2ad30f4", "Refresh"),
									onClick: refresh,
									disabled: loading,
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCcw, { className: cn("size-3.5", loading && "animate-spin") })
								})
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
								side: "bottom",
								sideOffset: 4,
								children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.7ae2ad30f4", "Refresh")
							})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "ghost",
								size: "icon-sm",
								"aria-label": translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.191f0bc98e", "Close"),
								onClick: () => closeModal(),
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-4" })
							})]
						})]
					})
				}),
				initialLoading ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-start gap-2 border-b border-border bg-muted/25 px-5 py-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "mt-0.5 size-3.5 shrink-0 animate-spin text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-xs font-medium text-foreground",
								children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.7eee951968", "Checking inactive workspaces")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mt-0.5 text-xs text-muted-foreground",
								children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.47123d0108", "Scanning inactive workspaces. You can close this and come back.")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mt-1 text-xs font-medium text-muted-foreground",
								children: formatWorkspaceCleanupProgress(scanProgress)
							})
						]
					})]
				}) : hasAnyCandidates ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-wrap items-center justify-between gap-3 border-b border-border bg-muted/25 px-4 py-2.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex min-w-0 flex-wrap items-center gap-2",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 text-sm font-medium text-foreground",
							children: [
								selectedCount,
								" ",
								translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.ac5ba84cc1", "selected")
							]
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex min-w-0 flex-wrap items-center gap-2",
						children: [eligibleRepos.length > 1 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "w-[220px] max-w-full",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RepoMultiCombobox, {
								repos: eligibleRepos,
								selected: effectiveRepoSelection,
								onChange: (next) => setRepoSelection(new Set(next)),
								onSelectAll: () => setRepoSelection(new Set(eligibleRepoIds)),
								triggerClassName: "h-8 w-full rounded-md border border-border/60 bg-background px-2 text-xs font-medium shadow-xs hover:bg-accent/60"
							})
						}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							variant: "destructive",
							size: "sm",
							onClick: () => openConfirmRemove(selectedCandidates),
							disabled: selectedCount === 0 || loading || removalProgress !== null || removalInFlight,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3.5" }), translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.b771c92598", "Delete selected")]
						})]
					})]
				}) : null,
				loading && scan && hasAnyCandidates ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "border-b border-border bg-muted/25 px-5 py-2",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 shrink-0 animate-spin" }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.9a3be9f2df", "Scanning inactive workspaces. New rows appear here as they finish. You can close this and come back.") }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "font-medium text-foreground",
								children: formatWorkspaceCleanupProgress(scanProgress)
							})
						]
					})
				}) : null,
				error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "border-b border-destructive/30 bg-destructive/10 px-5 py-2 text-xs text-destructive",
					children: error
				}) : scanNoticeMessage ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-2 border-b border-border bg-muted/25 px-5 py-2 text-xs text-muted-foreground",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-3.5 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: scanNoticeMessage })]
				}) : null,
				estimateMismatchNotice ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-start gap-2 border-b border-border bg-muted/25 px-5 py-2 text-xs text-muted-foreground",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Info, { className: "mt-0.5 size-3.5 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: estimateMismatchNotice })]
				}) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "grid min-h-0 flex-1 grid-cols-1 overflow-hidden md:grid-cols-[185px_minmax(0,1fr)]",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CleanupViewNav, {
						activeView: resolvedActiveView,
						counts: cleanupViewCounts,
						onViewChange: setActiveView
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex min-h-0 min-w-0 flex-col border-t border-border md:border-l md:border-t-0",
						children: [filteredCandidates.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCleanupFilterToolbar, {
							filters,
							showRestoreIgnored: resolvedActiveView === "hidden" && hiddenCandidates.length > 0,
							sortKey,
							sortDirection,
							onFiltersChange: setFilters,
							onSortKeyChange: setSortKey,
							onSortDirectionChange: setSortDirection,
							onRestoreIgnored: () => void resetDismissals()
						}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ScrollArea, {
							className: "min-h-0 flex-1",
							viewportRef: setRowsScrollElement,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
								initialLoading ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SkeletonRows, {}) : null,
								!loading && scan && candidates.length === 0 && !scanNoticeMessage ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EmptyState, { title: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.d3eef9463d", "No inactive workspaces to delete.") }) : null,
								!loading && scan && candidates.length === 0 && scanNoticeMessage ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EmptyState, { title: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.97c772c4fe", "No inactive workspaces found in checked repositories.") }) : null,
								!loading && scan && candidates.length > 0 && filteredCandidates.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EmptyState, {
									title: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.a19040cd67", "No inactive workspaces match the selected repos."),
									actionLabel: "Show all repos",
									onAction: () => setRepoSelection(new Set(eligibleRepoIds))
								}) : null,
								!loading && scan && filteredCandidates.length > 0 && visibleCandidates.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EmptyState, {
									title: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.4719327c9c", "All cleanup suggestions are ignored."),
									actionLabel: "Review ignored workspaces",
									onAction: () => setActiveView("hidden")
								}) : null,
								!loading && scan && activeRows.length === 0 && activeBaseRows.length > 0 && activeFilters ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EmptyState, {
									title: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.3d957ff117", "No workspaces match these filters."),
									actionLabel: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.e94b1f8bb4", "Clear filters"),
									onAction: () => setFilters(DEFAULT_WORKSPACE_CLEANUP_FILTERS)
								}) : null,
								!loading && scan && activeRows.length === 0 && visibleCandidates.length > 0 && !activeFilters ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EmptyState, { title: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.f68d538c63", "No workspaces in this cleanup set.") }) : null,
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCleanupCandidateList, {
									rows: activeRows,
									scrollElement: rowsScrollElement,
									renderRow: (candidate, index) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CandidateRow, {
										candidate,
										reviewInfo: reviewInfoByWorktreeId.get(candidate.worktreeId) ?? EMPTY_REVIEW_INFO,
										last: activeRows.length > 1 && index === activeRows.length - 1,
										expanded: expandedRowIds.has(candidate.worktreeId),
										lastActivityLabel: formatRelativeTime(candidate.lastActivityAt),
										deletionPhase: deletionPhaseByWorktreeId[candidate.worktreeId],
										removing: loading || removalInFlight || deletingWorktreeIds.has(candidate.worktreeId),
										selected: selectedIds.has(candidate.worktreeId) && !loading && !deletingWorktreeIds.has(candidate.worktreeId),
										failure: rowFailures[candidate.worktreeId],
										onToggleExpanded: toggleExpandedRow,
										onToggleSelected: toggleSelectedRow,
										onView: handleViewCandidate,
										onIgnore: ignoreCandidate,
										onRemove: handleRemoveRow
									}, candidate.worktreeId)
								})
							] })
						})]
					})]
				})
			] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ConfirmRemove, {
				candidates: confirmCandidates,
				reviewInfoByWorktreeId,
				progress: removalProgress,
				onBack: backToWorkspaceCleanupList,
				onCancel: cancelConfirmRemove,
				onConfirm: confirmRemove
			})
		})
	});
}
function WorkspaceCleanupFilterToolbar({ filters, showRestoreIgnored, sortKey, sortDirection, onFiltersChange, onSortKeyChange, onSortDirectionChange, onRestoreIgnored }) {
	const updateFilter = (key, value) => {
		onFiltersChange({
			...filters,
			[key]: value
		});
	};
	const hasHiddenControls = hasActiveWorkspaceCleanupPanelControls(filters, sortKey, sortDirection);
	const resetPanelControls = () => {
		onFiltersChange({
			...filters,
			time: "all",
			review: "all",
			git: "all",
			context: "all"
		});
		onSortKeyChange("activity");
		onSortDirectionChange("asc");
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-2 border-b border-border bg-muted/15 px-3 py-2",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "relative min-w-0 flex-1",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Search, { className: "pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value: filters.query,
				onChange: (event) => updateFilter("query", event.target.value),
				placeholder: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.searchPlaceholder", "Search workspaces"),
				className: "h-8 pl-8 text-xs"
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, {
			modal: false,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						variant: "outline",
						size: "icon-sm",
						type: "button",
						"aria-label": translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.efb3843e75", "Filter and sort workspaces"),
						className: "relative shrink-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SlidersHorizontal, { className: "size-3.5" }), hasHiddenControls ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							"aria-hidden": "true",
							className: "absolute -top-0.5 -right-0.5 size-2 rounded-full bg-primary"
						}) : null]
					})
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
				side: "top",
				sideOffset: 4,
				children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.efb3843e75", "Filter and sort workspaces")
			})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuContent, {
				align: "end",
				sideOffset: 6,
				className: "w-64 pb-2",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, { children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.93b7381d50", "Filters") }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCleanupMenuSub, {
						label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.ageFilter", "Age"),
						value: filters.time,
						options: [
							["all", "Any age"],
							["30d", "30d+"],
							["90d", "90d+"],
							["archived", "Archived"]
						],
						onChange: (value) => updateFilter("time", value)
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCleanupMenuSub, {
						label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.reviewFilter", "Review"),
						value: filters.review,
						options: [
							["all", "Any review"],
							["no-review", "No PR/MR"],
							["has-review", "Has PR/MR"],
							["open-review", "Open"],
							["closed-review", "Closed"]
						],
						onChange: (value) => updateFilter("review", value)
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCleanupMenuSub, {
						label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.gitFilter", "Git"),
						value: filters.git,
						options: [
							["all", "Any git"],
							["clean", "Clean"],
							["dirty", "Dirty"],
							["unpushed", "Unpushed"],
							["unknown", "Unknown"]
						],
						onChange: (value) => updateFilter("git", value)
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCleanupMenuSub, {
						label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.contextFilter", "Context"),
						value: filters.context,
						options: [
							["all", "Any context"],
							["has-context", "Has context"],
							["no-context", "No context"]
						],
						onChange: (value) => updateFilter("context", value)
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, { children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.a615e24679", "Sort") }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCleanupMenuSub, {
						label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.sortBy", "Sort by"),
						value: sortKey,
						options: [
							["activity", "Activity"],
							["name", "Name"],
							["repo", "Repo"],
							["review", "Review"],
							["git", "Git"]
						],
						onChange: onSortKeyChange
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCleanupMenuSub, {
						label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.sortDirection", "Direction"),
						value: sortDirection,
						options: [["asc", "Ascending"], ["desc", "Descending"]],
						onChange: onSortDirectionChange
					}),
					showRestoreIgnored ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuItem, {
						onSelect: onRestoreIgnored,
						children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.aaee139eab", "Restore ignored suggestions")
					})] }) : null,
					hasHiddenControls ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuItem, {
						onSelect: resetPanelControls,
						children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.e94b1f8bb4", "Clear filters")
					})] }) : null
				]
			})]
		})]
	});
}
function WorkspaceCleanupMenuSub({ label, value, options, onChange }) {
	const valueLabel = options.find(([optionValue]) => optionValue === value)?.[1] ?? value;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSub, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSubTrigger, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		className: "flex min-w-0 flex-1 items-center justify-between gap-3",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "truncate",
			children: label
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "truncate text-[11px] font-medium text-muted-foreground",
			children: valueLabel
		})]
	}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSubContent, {
		className: "w-44",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuRadioGroup, {
			value,
			onValueChange: (next) => onChange(next),
			children: options.map(([optionValue, optionLabel]) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuRadioItem, {
				value: optionValue,
				onSelect: (event) => event.preventDefault(),
				children: optionLabel
			}, optionValue))
		})
	})] });
}
function CleanupViewNav({ activeView, counts, onViewChange }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("aside", {
		className: "border-t border-border bg-background md:border-t-0",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "space-y-1 p-2",
			children: [
				{
					view: "ready",
					label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.4b93a235d8", "Suggested")
				},
				{
					view: "review",
					label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.d1094dd529", "Needs review")
				},
				{
					view: "protected",
					label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.c4f4782c02", "Not suggested")
				},
				{
					view: "hidden",
					label: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.e8b3741ff7", "Ignored")
				}
			].map((item) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
				type: "button",
				className: cn("flex h-8 w-full items-center justify-between gap-2 rounded-md px-2 text-left text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground", activeView === item.view && "bg-accent text-accent-foreground"),
				onClick: () => onViewChange(item.view),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "truncate",
					children: item.label
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "tabular-nums text-muted-foreground",
					children: counts[item.view]
				})]
			}, item.view))
		})
	});
}
function ConfirmRemove({ candidates, reviewInfoByWorktreeId, progress, onBack, onCancel, onConfirm }) {
	const count = candidates.length;
	const deleting = progress !== null;
	const progressValue = progress ? Math.min(100, Math.max(0, progress.processedCount / progress.totalCount * 100)) : 0;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogHeader, {
			className: "border-b border-border px-5 py-4",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-start justify-between gap-4",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex min-w-0 items-start gap-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md border border-destructive/25 bg-destructive/10 text-destructive",
						children: deleting ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-4" })
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
							className: "text-base",
							children: deleting ? translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.deletingCount", "Deleting workspaces: {{value0}}", { value0: count }) : translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.deleteCount", "Delete workspaces: {{value0}}?", { value0: count })
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
							className: "mt-1.5 text-xs leading-5",
							children: deleting ? translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.1d3503357d", "You can close this and come back while deletion continues.") : translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.38ca0b1400", "This permanently deletes their local files. You can't undo this.")
						})]
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon-sm",
					"aria-label": translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.74f6c16279", "Back"),
					onClick: onBack,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-4" })
				})]
			})
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex min-h-0 flex-1 flex-col",
			children: [
				progress ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "border-b border-border bg-muted/25 px-5 py-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-2 text-xs text-muted-foreground",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 shrink-0 animate-spin" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "font-medium text-foreground",
							children: formatWorkspaceCleanupRemovalProgress(progress)
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Progress, {
						value: progressValue,
						className: "mt-2 h-1.5"
					})]
				}) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center justify-between border-b border-border px-5 py-2.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground",
						children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.selectedForDeletionCount", "Selected for deletion: {{value0}}", { value0: count })
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-xs text-muted-foreground",
						children: translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.592fbab446", "Sorted by oldest activity")
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ScrollArea, {
					className: "min-h-0 flex-1",
					children: candidates.map((candidate, index) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ConfirmRemoveRow, {
						candidate,
						reviewInfo: reviewInfoByWorktreeId.get(candidate.worktreeId) ?? EMPTY_REVIEW_INFO,
						last: index === candidates.length - 1
					}, candidate.worktreeId))
				})
			]
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, {
			className: "border-t border-border px-5 py-3",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "outline",
				onClick: onCancel,
				children: deleting ? translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.191f0bc98e", "Close") : translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.b6bae1eed1", "Cancel")
			}), !deleting ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				variant: "destructive",
				onClick: onConfirm,
				disabled: count === 0,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-4" }), translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.deleteButtonCount", "Delete {{value0}}", { value0: count })]
			}) : null]
		})
	] });
}
function ConfirmRemoveRow({ candidate, reviewInfo, last }) {
	const dirtyLabel = getDirtyGitLabel(candidate);
	const branchDiffersFromName = candidate.branch !== candidate.displayName;
	const contextPillLabel = getContextPillLabel(candidate);
	const showGitMetadataChip = shouldShowGitMetadataChip(candidate);
	const status = getCandidateStatus(candidate);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("border-b border-border/60 px-5 py-2.5", last && "border-b-0"),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "min-w-0 truncate text-sm font-medium",
						children: candidate.displayName
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "text-xs text-muted-foreground",
						children: [
							translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.352f15d6fc", "Last active"),
							" ",
							formatRelativeTime(candidate.lastActivityAt)
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatusPill, {
						tone: status.tone,
						children: status.label
					}),
					reviewInfo.label ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatusPill, {
						tone: getReviewPillTone(reviewInfo),
						children: reviewInfo.label
					}) : null,
					contextPillLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatusPill, { children: contextPillLabel }) : null,
					dirtyLabel && showGitMetadataChip ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatusPill, {
						tone: "destructive",
						children: dirtyLabel
					}) : null
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mt-0.5 flex min-w-0 flex-wrap items-baseline gap-x-2 text-xs text-muted-foreground",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "min-w-0 truncate",
					children: candidate.repoName
				}), branchDiffersFromName ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					"aria-hidden": "true",
					children: "·"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "min-w-0 truncate font-mono",
					children: candidate.branch
				})] }) : null]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mt-0.5 min-w-0 truncate font-mono text-[11px] text-muted-foreground/80",
				children: candidate.path
			})
		]
	});
}
function hasActiveWorkspaceCleanupFilters(filters) {
	return filters.query.trim() !== "" || filters.time !== "all" || filters.review !== "all" || filters.git !== "all" || filters.context !== "all";
}
function hasActiveWorkspaceCleanupPanelControls(filters, sortKey, sortDirection) {
	return filters.time !== "all" || filters.review !== "all" || filters.git !== "all" || filters.context !== "all" || sortKey !== "activity" || sortDirection !== "asc";
}
function formatWorkspaceCleanupRemovalProgress(progress) {
	const deletedText = translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.4c2990886e", "{{value0}}/{{value1}} deleted", {
		value0: progress.removedCount,
		value1: progress.totalCount
	});
	if (progress.failedCount === 0) return deletedText;
	return translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.86ba852118", "{{value0}}, {{value1}} failed", {
		value0: deletedText,
		value1: progress.failedCount
	});
}
function formatWorkspaceCleanupProgress(progress) {
	if (!progress || progress.scannedWorktreeCount === 0) return translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.4cc5b73efe", "Finding inactive workspaces...");
	return translate("auto.components.workspace.cleanup.WorkspaceCleanupDialog.7b7bde5181", "Checked workspaces so far: {{value0}}", { value0: progress.scannedWorktreeCount });
}
function SkeletonRows() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "space-y-2",
		children: [
			0,
			1,
			2
		].map((index) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "h-24 animate-pulse rounded-lg border border-border bg-muted/35" }, index))
	});
}
function EmptyState({ title, actionLabel, onAction }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-h-48 flex-col items-center justify-center gap-3 rounded-lg border border-border bg-muted/20 text-sm text-muted-foreground",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: title }), actionLabel && onAction ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
			variant: "outline",
			size: "sm",
			onClick: onAction,
			children: actionLabel
		}) : null]
	});
}
function toggleSetMember(current, value) {
	const next = new Set(current);
	if (next.has(value)) next.delete(value);
	else next.add(value);
	return next;
}
export { WorkspaceCleanupDialog as default };
