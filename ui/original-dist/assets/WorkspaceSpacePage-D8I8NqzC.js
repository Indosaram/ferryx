import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ArrowDown } from "./arrow-down-Dmgw82W4.js";
import { t as ArrowLeft } from "./arrow-left-BpDalf_n.js";
import { t as ArrowUp } from "./arrow-up-CCUzfqnh.js";
import "./workspace-status-wl52y3xd.js";
import { t as Bot } from "./bot-nZ1Hb9j2.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as Circle } from "./circle-DumnR8X3.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as FileExclamationPoint } from "./file-exclamation-point-DBMojurh.js";
import { r as activateAndRevealWorktree } from "./worktree-activation-BDsaiyMf.js";
import { t as GitBranch } from "./git-branch-CnBuDEti.js";
import { t as GitPullRequest } from "./git-pull-request-BS_5feqf.js";
import { t as HardDrive } from "./hard-drive-BclppwkA.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as Minus } from "./minus-Byrkh1sN.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { t as Search } from "./search-DK1nVA6d.js";
import { t as Server } from "./server-DYdwnXME.js";
import { t as Terminal } from "./terminal-Cen7Un9b.js";
import { $a as isClipboardTextByteLengthOverLimit, Jc as parsePaneKey, Wl as getHostedReviewCacheKey, Xn as issueCacheKey, m_ as Trash2, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import { t as X } from "./x-BrGKE4uz.js";
import { n as ZoomIn, t as ZoomOut } from "./zoom-out-Z8dr_jqi.js";
import "./plugin-manifest-Bs-50M_g.js";
import { a as isExplicitAgentStatusFresh, i as classifyTitleActivity, u as AGENT_STATUS_STALE_AFTER_MS, v as tabHasLivePty } from "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { f as ContextMenuTrigger, n as ContextMenuContent, r as ContextMenuItem, t as ContextMenu } from "./context-menu-D4RKI7hR.js";
import { n as HoverCardContent, r as HoverCardTrigger, t as HoverCard } from "./hover-card-DP92-D-b.js";
import { a as SelectTrigger, n as SelectContent, o as SelectValue, r as SelectItem, t as Select } from "./select-B67U0C6J.js";
import "./useMountedRef-1omUd-IV.js";
import { i as getWorktreeMapFromState, r as getRepoMapFromState } from "./selectors-XOBeaOSb.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./web-session-tabs-sync-CYKZbAxS.js";
import "./pane-agent-owner-BPfoVAtS.js";
import { t as installWindowVisibilityInterval } from "./window-visibility-interval-CtnbYoau.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import "./connection-context-BUPsamzR.js";
import "./localized-catalog-DubKHKUR.js";
import { a as prepareActiveWorktreeFocusAfterDelete, t as runWorktreeBatchDelete } from "./delete-worktree-flow-RxB6NScm.js";
import "./preserved-branch-batch-toast-DHxeGO1o.js";
import { t as Badge } from "./badge-BBptl5GG.js";
import { t as Input } from "./input-DV5rpysh.js";
import { o as branchDisplayName } from "./WorktreeCardHelpers-Detnezco.js";
import { t as refreshGitStatusForWorktree } from "./git-status-refresh-D3FoCs3D.js";
import "./relative-time-format-BdBnutwN.js";
import { a as getWorkspaceSpaceScanDateTimeLabel, i as getWorkspaceSpaceProgressLabel, n as formatCompactCount, o as getWorkspaceSpaceScanTimeLabel, r as getWorkspaceSpaceBranchLabel, s as getWorkspaceSpaceStatusLabel, t as formatBytes } from "./workspace-space-format-DqHSji4k.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function sumSizes(items) {
	return items.reduce((sum, item) => sum + Math.max(0, item.sizeBytes), 0);
}
function splitBalanced(items) {
	const total = sumSizes(items);
	if (items.length <= 1 || total <= 0) return {
		first: [...items],
		second: []
	};
	const target = total / 2;
	let running = 0;
	let splitIndex = 0;
	for (let index = 0; index < items.length; index += 1) {
		const next = running + Math.max(0, items[index].sizeBytes);
		if (index > 0 && Math.abs(target - running) < Math.abs(target - next)) break;
		running = next;
		splitIndex = index + 1;
	}
	splitIndex = Math.min(items.length - 1, Math.max(1, splitIndex));
	return {
		first: items.slice(0, splitIndex),
		second: items.slice(splitIndex)
	};
}
function layoutTreemapRecursive(items, bounds, depth, output) {
	if (items.length === 0 || bounds.width <= 0 || bounds.height <= 0) return;
	if (items.length === 1) {
		const item = items[0];
		output.push({
			...item,
			...bounds,
			depth,
			index: output.length
		});
		return;
	}
	const total = sumSizes(items);
	if (total <= 0) return;
	const { first, second } = splitBalanced(items);
	const ratio = sumSizes(first) / total;
	if (bounds.width >= bounds.height) {
		const firstWidth = bounds.width * ratio;
		layoutTreemapRecursive(first, {
			...bounds,
			width: firstWidth
		}, depth + 1, output);
		layoutTreemapRecursive(second, {
			x: bounds.x + firstWidth,
			y: bounds.y,
			width: bounds.width - firstWidth,
			height: bounds.height
		}, depth + 1, output);
		return;
	}
	const firstHeight = bounds.height * ratio;
	layoutTreemapRecursive(first, {
		...bounds,
		height: firstHeight
	}, depth + 1, output);
	layoutTreemapRecursive(second, {
		x: bounds.x,
		y: bounds.y + firstHeight,
		width: bounds.width,
		height: bounds.height - firstHeight
	}, depth + 1, output);
}
function buildTreemapLayout(items) {
	const filtered = items.filter((item) => item.sizeBytes > 0).sort((a, b) => b.sizeBytes - a.sizeBytes || a.label.localeCompare(b.label));
	const output = [];
	layoutTreemapRecursive(filtered, {
		x: 0,
		y: 0,
		width: 100,
		height: 100
	}, 0, output);
	return output;
}
const WORKSPACE_SPACE_FILTER_QUERY_MAX_BYTES = 2 * 1024;
function isWorkspaceSpaceFilterQueryTooLarge(query, maxBytes = WORKSPACE_SPACE_FILTER_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function getPaneKeyTabId(paneKey) {
	const parsed = parsePaneKey(paneKey);
	if (parsed) return parsed.tabId;
	const separatorIndex = paneKey.indexOf(":");
	if (separatorIndex <= 0 || separatorIndex !== paneKey.lastIndexOf(":") || separatorIndex === paneKey.length - 1) return null;
	return paneKey.slice(0, separatorIndex);
}
function isActiveAgentState(entry) {
	return entry.state === "working" || entry.state === "blocked" || entry.state === "waiting";
}
function countTitleActiveAgentsForTab(tab, runtimePaneTitlesByTabId, ptyIdsByTabId) {
	if (!tabHasLivePty(ptyIdsByTabId, tab.id)) return 0;
	const paneTitles = runtimePaneTitlesByTabId[tab.id];
	if (paneTitles && Object.keys(paneTitles).length > 0) return Object.values(paneTitles).filter((title) => {
		const status$1 = classifyTitleActivity(title);
		return status$1 === "working" || status$1 === "permission";
	}).length;
	const status = classifyTitleActivity(tab.title);
	return status === "working" || status === "permission" ? 1 : 0;
}
function countWorkspaceSpaceActiveAgents({ worktreeId, tabs, agentStatusByPaneKey, migrationUnsupportedByPtyId, runtimePaneTitlesByTabId, ptyIdsByTabId, now }) {
	const tabIds = new Set(tabs.map((tab) => tab.id));
	const tabsWithActiveHook = /* @__PURE__ */ new Set();
	let count = 0;
	for (const [paneKey, entry] of Object.entries(agentStatusByPaneKey)) {
		if (!isActiveAgentState(entry)) continue;
		if (!isExplicitAgentStatusFresh(entry, now, 18e5)) continue;
		const tabId = getPaneKeyTabId(entry.paneKey || paneKey);
		if (!tabId || !tabIds.has(tabId)) continue;
		tabsWithActiveHook.add(tabId);
		count += 1;
	}
	for (const entry of Object.values(migrationUnsupportedByPtyId)) {
		const tabId = entry.tabId ?? (entry.paneKey ? getPaneKeyTabId(entry.paneKey) : null);
		if (entry.worktreeId !== worktreeId && (!tabId || !tabIds.has(tabId))) continue;
		if (tabId) tabsWithActiveHook.add(tabId);
		count += 1;
	}
	for (const tab of tabs) {
		if (tabsWithActiveHook.has(tab.id)) continue;
		count += countTitleActiveAgentsForTab(tab, runtimePaneTitlesByTabId, ptyIdsByTabId);
	}
	return count;
}
function getWorkspaceSpaceSearchText(worktree) {
	return [
		worktree.displayName,
		worktree.repoDisplayName,
		worktree.path,
		worktree.branch,
		worktree.status
	].join(" ").toLowerCase();
}
function getLargestWorkspaceSpaceItemSize(items) {
	let maxSize = 0;
	for (const item of items) if (item.sizeBytes > maxSize) maxSize = item.sizeBytes;
	return maxSize;
}
function getLargestWorkspaceSpaceRowSize(rows) {
	let maxSize = 0;
	for (const row of rows) if (row.sizeBytes > maxSize) maxSize = row.sizeBytes;
	return maxSize;
}
function compareRows(left, right, sortKey) {
	switch (sortKey) {
		case "size": return left.sizeBytes - right.sizeBytes;
		case "name": return left.displayName.localeCompare(right.displayName);
		case "repo": return left.repoDisplayName.localeCompare(right.repoDisplayName) || left.displayName.localeCompare(right.displayName);
		case "activity": return left.lastActivityAt - right.lastActivityAt;
	}
}
function sortWorkspaceSpaceRows(rows, sortKey, direction) {
	const multiplier = direction === "asc" ? 1 : -1;
	return [...rows].sort((left, right) => {
		return compareRows(left, right, sortKey) * multiplier || right.sizeBytes - left.sizeBytes || left.displayName.localeCompare(right.displayName);
	});
}
function filterWorkspaceSpaceRows(rows, query, onlyDeletable) {
	if (isWorkspaceSpaceFilterQueryTooLarge(query)) return [];
	const normalizedQuery = query.trim().toLowerCase();
	return rows.filter((row) => {
		if (onlyDeletable && !row.canDelete) return false;
		if (!normalizedQuery) return true;
		return getWorkspaceSpaceSearchText(row).includes(normalizedQuery);
	});
}
function isWorkspaceSpaceRowReadyToDelete(worktree, readiness) {
	return worktree.canDelete && worktree.status === "ok" && !worktree.isMainWorktree && readiness !== void 0 && !readiness.isActive && readiness.changedFileCount === 0 && readiness.dirtyEditorBufferCount === 0 && readiness.activeAgentCount === 0 && readiness.liveTerminalCount === 0 && readiness.browserTabCount === 0 && !readiness.reviewLabel && !readiness.issueLabel && !readiness.linearIssueLabel;
}
function getWorkspaceSpaceGitStatusRefreshCandidates(rows) {
	return rows.filter((worktree) => worktree.canDelete && worktree.status === "ok" && !worktree.isMainWorktree);
}
function getSelectedDeletableWorkspaceIds(rows, selectedIds, isWorktreeDeleting = () => false) {
	return rows.filter((row) => row.canDelete && row.status === "ok" && selectedIds.has(row.worktreeId) && !isWorktreeDeleting(row.worktreeId)).map((row) => row.worktreeId);
}
function getVisibleDeletableWorkspaceIds(rows, isWorktreeDeleting = () => false) {
	return rows.filter((row) => row.canDelete && row.status === "ok" && !isWorktreeDeleting(row.worktreeId)).map((row) => row.worktreeId);
}
function resolveWorkspaceSpaceInspectedWorktreeId(rows, currentWorktreeId) {
	if (currentWorktreeId && rows.some((row) => row.worktreeId === currentWorktreeId)) return currentWorktreeId;
	return rows.find((row) => row.status === "ok")?.worktreeId ?? null;
}
function resolveWorkspaceSpaceTreemapZoomWorktreeId(rows, currentWorktreeId) {
	return currentWorktreeId && rows.some((row) => row.worktreeId === currentWorktreeId && row.status === "ok") ? currentWorktreeId : null;
}
function pruneWorkspaceSpaceSelectedIds(rows, selectedIds) {
	if (selectedIds.size === 0) return selectedIds;
	const validIds = new Set(rows.map((row) => row.worktreeId));
	let changed = false;
	const nextIds = /* @__PURE__ */ new Set();
	for (const id of selectedIds) if (validIds.has(id)) nextIds.add(id);
	else changed = true;
	return changed ? nextIds : selectedIds;
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var TREEMAP_FILLS = [
	"color-mix(in srgb, var(--chart-2) 34%, var(--card))",
	"color-mix(in srgb, var(--foreground) 20%, var(--card))",
	"color-mix(in srgb, var(--chart-4) 28%, var(--card))",
	"color-mix(in srgb, var(--primary) 24%, var(--card))",
	"color-mix(in srgb, var(--chart-1) 38%, var(--card))"
];
var GIT_STATUS_REFRESH_CONCURRENCY = 6;
function pluralize(count, singular, plural = `${singular}s`) {
	return `${count} ${count === 1 ? singular : plural}`;
}
function formatReviewState(state) {
	return state.charAt(0).toUpperCase() + state.slice(1);
}
function countLiveTerminals(tabs, ptyIdsByTabId) {
	return tabs.filter((tab) => (ptyIdsByTabId[tab.id]?.length ?? 0) > 0).length;
}
function getBranchStatus(status) {
	if (!status?.hasUpstream) return null;
	if (status.ahead === 0 && status.behind === 0) return "Synced with upstream";
	const parts = [];
	if (status.ahead > 0) parts.push(`${status.ahead} ahead`);
	if (status.behind > 0) parts.push(`${status.behind} behind`);
	return parts.join(", ");
}
function getWorkspaceDecisionDetails(worktree, inputs) {
	const workspaceRecord = inputs.worktreeMap.get(worktree.worktreeId);
	const tabs = inputs.tabsByWorktree[worktree.worktreeId] ?? [];
	const openFiles = inputs.openFiles.filter((file) => file.worktreeId === worktree.worktreeId);
	const dirtyEditorBufferCount = openFiles.filter((file) => file.isDirty || inputs.editorDrafts[file.id] !== void 0).length;
	const gitEntries = inputs.gitStatusByWorktree[worktree.worktreeId];
	const branch = workspaceRecord ? branchDisplayName(workspaceRecord.branch) : getWorkspaceSpaceBranchLabel(worktree);
	const repo = inputs.repoMap.get(worktree.repoId);
	const reviewCacheKey = getHostedReviewCacheKey(worktree.repoPath, branch, inputs.settings, worktree.repoId, repo?.connectionId, repo?.executionHostId, repo !== void 0);
	const hostedReview = inputs.hostedReviewCache[reviewCacheKey]?.data;
	const linkedPR = workspaceRecord?.linkedPR ?? null;
	const reviewLabel = hostedReview !== void 0 && hostedReview !== null ? `PR #${hostedReview.number} ${formatReviewState(hostedReview.state)}${hostedReview.status && hostedReview.status !== "none" ? `, ${hostedReview.status}` : ""}` : linkedPR ? `PR #${linkedPR}` : null;
	const linkedIssue = workspaceRecord?.linkedIssue ?? null;
	const issue = linkedIssue && repo ? inputs.issueCache[issueCacheKey(repo.path, repo.id, linkedIssue, inputs.settings, repo.connectionId, repo.executionHostId, true)]?.data : null;
	const issueLabel = linkedIssue ? issue ? `#${issue.number} ${issue.state}: ${issue.title}` : `#${linkedIssue}` : null;
	const linkedLinearIssue = workspaceRecord?.linkedLinearIssue ?? null;
	const linearIssue = linkedLinearIssue ? inputs.linearIssueCache[`selected::${linkedLinearIssue}`]?.data ?? inputs.linearIssueCache[linkedLinearIssue]?.data : null;
	const linearIssueLabel = linkedLinearIssue ? linearIssue ? `${linearIssue.identifier}${linearIssue.state?.name ? ` ${linearIssue.state.name}` : ""}: ${linearIssue.title}` : linkedLinearIssue : null;
	return {
		isActive: inputs.activeWorktreeId === worktree.worktreeId,
		canOpenWorkspace: workspaceRecord !== void 0,
		terminalTabCount: tabs.length,
		liveTerminalCount: countLiveTerminals(tabs, inputs.ptyIdsByTabId),
		activeAgentCount: countWorkspaceSpaceActiveAgents({
			worktreeId: worktree.worktreeId,
			tabs,
			agentStatusByPaneKey: inputs.agentStatusByPaneKey,
			migrationUnsupportedByPtyId: inputs.migrationUnsupportedByPtyId,
			runtimePaneTitlesByTabId: inputs.runtimePaneTitlesByTabId,
			ptyIdsByTabId: inputs.ptyIdsByTabId,
			now: inputs.now
		}),
		completedAgentCount: Object.values(inputs.retainedAgentsByPaneKey).filter((entry) => entry.worktreeId === worktree.worktreeId && entry.entry.state === "done").length,
		openEditorFileCount: openFiles.length,
		dirtyEditorBufferCount,
		browserTabCount: inputs.browserTabsByWorktree[worktree.worktreeId]?.length ?? 0,
		changedFileCount: gitEntries ? gitEntries.length : null,
		branchStatus: getBranchStatus(inputs.remoteStatusesByWorktree[worktree.worktreeId]),
		reviewLabel,
		issueLabel,
		linearIssueLabel
	};
}
function getTreemapFill(rect, selected) {
	if (selected) return "color-mix(in srgb, var(--ring) 40%, var(--card))";
	return TREEMAP_FILLS[rect.index % TREEMAP_FILLS.length];
}
function Metric({ label, value, title }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "min-w-0 px-4 py-3",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "truncate text-[11px] font-medium uppercase tracking-[0.14em] text-muted-foreground",
			children: label
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "mt-1 truncate text-lg font-semibold tabular-nums",
			title,
			children: value
		})]
	});
}
function UpdatedMetric({ scannedAt, isScanning }) {
	const [now, setNow] = (0, import_react.useState)(() => Date.now());
	(0, import_react.useEffect)(() => {
		if (scannedAt === null) return;
		setNow(Date.now());
		return installWindowVisibilityInterval({
			run: () => setNow(Date.now()),
			intervalMs: 6e4
		});
	}, [scannedAt]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Metric, {
		label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.52b629eb84", "Updated"),
		title: scannedAt === null ? void 0 : getWorkspaceSpaceScanDateTimeLabel(scannedAt),
		value: scannedAt === null ? isScanning ? "Scanning" : "—" : getWorkspaceSpaceScanTimeLabel(scannedAt, now)
	});
}
function CheckButton({ checked, disabled, label, onClick }) {
	const isChecked = checked === true;
	const isMixed = checked === "mixed";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
		type: "button",
		role: "checkbox",
		"aria-checked": checked,
		"aria-label": label,
		disabled,
		onPointerDown: (event) => event.stopPropagation(),
		onKeyDown: (event) => event.stopPropagation(),
		onClick: (event) => {
			event.stopPropagation();
			onClick();
		},
		className: cn("flex size-6 shrink-0 items-center justify-center rounded-md transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring", disabled && "cursor-default opacity-35"),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: cn("flex size-4 items-center justify-center rounded-sm border transition-colors", isChecked || isMixed ? "border-foreground bg-foreground text-background" : "border-muted-foreground/50 bg-background/40 text-transparent"),
			children: [isChecked ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, {
				className: "size-3",
				strokeWidth: 3
			}) : null, isMixed ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Minus, {
				className: "size-3",
				strokeWidth: 3
			}) : null]
		})
	});
}
function SortIndicator({ sortKey, activeKey, direction }) {
	if (sortKey !== activeKey) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Circle, { className: "size-3 opacity-0" });
	return direction === "asc" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowUp, { className: "size-3" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowDown, { className: "size-3" });
}
function StatusBadge({ worktree, decisionDetails, deleteState }) {
	if (deleteState?.isDeleting) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
		variant: "outline",
		className: "gap-1.5 text-muted-foreground",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }), translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.33653dbac2", "Deleting")]
	});
	if (deleteState?.error) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
		variant: "outline",
		className: "border-destructive/30 text-destructive",
		children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.39801484e0", "Failed")
	});
	if (worktree.status !== "ok") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
		variant: "outline",
		className: "border-destructive/30 text-destructive",
		children: getWorkspaceSpaceStatusLabel(worktree.status)
	});
	if (worktree.isMainWorktree) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
		variant: "outline",
		children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.2b501ee391", "Keep: main")
	});
	if (decisionDetails?.isActive) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
		variant: "outline",
		children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.7f7895514e", "Keep: active")
	});
	if ((decisionDetails?.changedFileCount ?? 0) > 0) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
		variant: "outline",
		children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.7ab8d7e2d7", "Keep: changed files")
	});
	if (decisionDetails?.changedFileCount === null) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
		variant: "outline",
		children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.ec7b076a75", "Keep: git not checked")
	});
	if ((decisionDetails?.dirtyEditorBufferCount ?? 0) > 0) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
		variant: "outline",
		children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.2055bc6a5a", "Keep: unsaved edits")
	});
	if ((decisionDetails?.activeAgentCount ?? 0) > 0 || (decisionDetails?.liveTerminalCount ?? 0) > 0 || (decisionDetails?.browserTabCount ?? 0) > 0) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
		variant: "outline",
		children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.cbc343a7a8", "Keep: in use")
	});
	if (decisionDetails?.reviewLabel || decisionDetails?.issueLabel || decisionDetails?.linearIssueLabel) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
		variant: "outline",
		children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.720870a18e", "Keep: linked")
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
		variant: "outline",
		className: "border-emerald-500/35 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
		children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.7d7745bb8f", "Can delete")
	});
}
function DecisionLine({ icon, label, value, tone = "default" }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-w-0 items-start gap-2",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: cn("mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-md border border-border/60 bg-muted/30 text-muted-foreground [&>svg]:size-3", tone === "warning" && "border-destructive/25 bg-destructive/8 text-destructive"),
			children: icon
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "min-w-0 flex-1",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "text-[11px] font-medium uppercase tracking-[0.05em] text-muted-foreground",
				children: label
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mt-0.5 truncate text-xs",
				title: value,
				children: value
			})]
		})]
	});
}
function getAgentDecisionLabel(details) {
	if (details.activeAgentCount > 0 && details.completedAgentCount > 0) return `${pluralize(details.activeAgentCount, "active agent")}, ${pluralize(details.completedAgentCount, "completed agent")}`;
	if (details.activeAgentCount > 0) return pluralize(details.activeAgentCount, "active agent");
	if (details.completedAgentCount > 0) return `${pluralize(details.completedAgentCount, "completed agent")} retained`;
	return "No tracked agents running";
}
function getTerminalDecisionLabel(details) {
	if (details.terminalTabCount === 0) return "No terminal tabs";
	return `${details.liveTerminalCount} live of ${pluralize(details.terminalTabCount, "terminal tab")}`;
}
function getGitDecisionLabel(details, gitRefreshState) {
	if (details.changedFileCount === null) {
		if (gitRefreshState?.error) return `Git status unavailable: ${gitRefreshState.error}`;
		return "Git status has not loaded yet";
	}
	if (details.changedFileCount === 0) return "No uncommitted files";
	return pluralize(details.changedFileCount, "changed file");
}
function getEditorDecisionLabel(details) {
	if (details.openEditorFileCount === 0) return "No editor files open";
	if (details.dirtyEditorBufferCount === 0) return `${pluralize(details.openEditorFileCount, "editor file")} open`;
	return `${pluralize(details.dirtyEditorBufferCount, "dirty editor buffer")} of ${pluralize(details.openEditorFileCount, "open file")}`;
}
function getDeleteDecisionLabel(worktree, details) {
	if (details.isActive) return "This is the active workspace";
	if (worktree.status !== "ok") return worktree.error ?? getWorkspaceSpaceStatusLabel(worktree.status);
	if (worktree.isMainWorktree) return "Main worktree is protected";
	if (!worktree.canDelete) return "Workspace is protected";
	return "Can be deleted after review";
}
function WorkspaceDecisionHoverCard({ worktree, details, gitRefreshState, onOpenWorkspace }) {
	const deleteDecision = getDeleteDecisionLabel(worktree, details);
	const issueLabel = [details.issueLabel, details.linearIssueLabel].filter(Boolean).join(" · ") || "No linked issue";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(HoverCardContent, {
		align: "end",
		side: "bottom",
		sideOffset: 8,
		collisionPadding: 12,
		className: "max-h-[min(34rem,calc(100vh-1.5rem))] w-[min(24rem,calc(100vw-1.5rem))] overflow-y-auto p-0 scrollbar-sleek",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "border-b border-border/60 px-4 py-3",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex min-w-0 items-start justify-between gap-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "truncate text-sm font-semibold",
							children: worktree.displayName
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mt-0.5 truncate text-xs text-muted-foreground",
							children: [
								worktree.repoDisplayName,
								" · ",
								formatBytes(worktree.sizeBytes)
							]
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatusBadge, {
						worktree,
						decisionDetails: details
					})]
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-3 px-4 py-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DecisionLine, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, {}),
						label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.d384a4ce9f", "Delete decision"),
						value: deleteDecision,
						tone: worktree.canDelete && worktree.status === "ok" ? "default" : "warning"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DecisionLine, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Bot, {}),
						label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.a8d9e0de79", "Agents"),
						value: getAgentDecisionLabel(details)
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DecisionLine, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Terminal, {}),
						label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.e9528a89b3", "Terminals"),
						value: getTerminalDecisionLabel(details)
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DecisionLine, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FileExclamationPoint, {}),
						label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.0bc756efaf", "Git changes"),
						value: getGitDecisionLabel(details, gitRefreshState),
						tone: (details.changedFileCount ?? 0) > 0 || gitRefreshState?.error ? "warning" : "default"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DecisionLine, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FileExclamationPoint, {}),
						label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.c432278ec7", "Editor buffers"),
						value: getEditorDecisionLabel(details),
						tone: details.dirtyEditorBufferCount > 0 ? "warning" : "default"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DecisionLine, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitBranch, {}),
						label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.b9b4a3a25d", "Branch"),
						value: details.branchStatus ?? getWorkspaceSpaceBranchLabel(worktree)
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DecisionLine, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitPullRequest, {}),
						label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.fb2069acb7", "Review"),
						value: details.reviewLabel ?? "No linked PR"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DecisionLine, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, {}),
						label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.66870929fb", "Issue"),
						value: issueLabel
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center justify-between gap-3 border-t border-border/60 px-4 py-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "min-w-0 truncate font-mono text-[11px] text-muted-foreground",
					children: details.browserTabCount > 0 ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.131662ac65", "{{value0}} open", { value0: pluralize(details.browserTabCount, "browser tab") }) : worktree.path
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					type: "button",
					variant: "outline",
					size: "sm",
					onClick: (event) => {
						event.preventDefault();
						event.stopPropagation();
						onOpenWorkspace();
					},
					disabled: !details.canOpenWorkspace,
					className: "shrink-0 gap-1.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3.5" }), translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.c28643d3da", "Go to workspace")]
				})]
			})
		]
	});
}
function WorkspaceTreemap({ rows, isScanning, selectedWorktreeId, zoomedWorktree, onSelect, onZoomChange }) {
	const selectedWorktree = rows.find((row) => row.worktreeId === selectedWorktreeId) ?? null;
	const canZoomSelected = !!selectedWorktree && selectedWorktree.status === "ok" && selectedWorktree.topLevelItems.length > 0;
	const isZoomed = !!zoomedWorktree;
	const rects = (0, import_react.useMemo)(() => buildTreemapLayout(zoomedWorktree ? zoomedWorktree.topLevelItems.filter((item) => item.sizeBytes > 0).map((item) => ({
		id: item.path,
		label: item.name,
		sizeBytes: item.sizeBytes
	})) : rows.filter((row) => row.status === "ok" && row.sizeBytes > 0).map((row) => ({
		id: row.worktreeId,
		label: row.displayName,
		sizeBytes: row.sizeBytes
	}))), [rows, zoomedWorktree]);
	if (rects.length === 0) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "relative flex h-72 items-center justify-center rounded-lg border border-dashed border-border/70 bg-muted/20 text-sm text-muted-foreground",
		children: [zoomedWorktree ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
			variant: "outline",
			size: "xs",
			onClick: () => onZoomChange(null),
			className: "absolute right-2 top-2 gap-1.5 bg-background/90 px-2.5 backdrop-blur",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ZoomOut, { className: "size-3" }), translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.ef890d31b9", "All")]
		}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: "flex items-center gap-2",
			children: [isScanning ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }) : null, isScanning ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.c5135e7e4a", "Scanning workspace sizes. You can leave this page.") : isZoomed ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.977bdf9a36", "No top-level items to show.") : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.0990a63160", "No scanned workspace sizes yet.")]
		})]
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "relative h-72 overflow-hidden rounded-lg border border-border/70 bg-muted/20",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "absolute right-2 top-2 z-10 flex max-w-[calc(100%-1rem)] items-center gap-2",
			children: zoomedWorktree ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "max-w-56 truncate rounded-md border border-border/70 bg-background/90 px-2 py-1 text-[11px] font-medium shadow-xs backdrop-blur",
				children: zoomedWorktree.displayName
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				variant: "outline",
				size: "xs",
				onClick: () => onZoomChange(null),
				className: "gap-1.5 bg-background/90 px-2.5 backdrop-blur",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ZoomOut, { className: "size-3" }), translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.ef890d31b9", "All")]
			})] }) : canZoomSelected ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				variant: "outline",
				size: "xs",
				onClick: () => onZoomChange(selectedWorktree.worktreeId),
				className: "gap-1.5 bg-background/90 px-2.5 backdrop-blur",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ZoomIn, { className: "size-3" }), translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.d3f9c69ddc", "Zoom")]
			}) : null
		}), rects.map((rect) => {
			const area = rect.width * rect.height;
			const selected = !isZoomed && rect.id === selectedWorktreeId;
			const rectStyle = {
				left: `${rect.x}%`,
				top: `${rect.y}%`,
				width: `${rect.width}%`,
				height: `${rect.height}%`,
				background: getTreemapFill(rect, selected)
			};
			const rectContent = area >= 80 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
				className: "block min-w-0 text-[11px] font-medium leading-tight text-foreground",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "block truncate",
					children: rect.label
				}), area >= 180 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "mt-0.5 block truncate text-muted-foreground",
					children: formatBytes(rect.sizeBytes)
				}) : null]
			}) : null;
			if (isZoomed) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				title: `${rect.label} • ${formatBytes(rect.sizeBytes)}`,
				className: "absolute overflow-hidden border border-background/80 p-2 text-left",
				style: rectStyle,
				children: rectContent
			}, rect.id);
			return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				"aria-label": `${rect.label}, ${formatBytes(rect.sizeBytes)}`,
				title: `${rect.label} • ${formatBytes(rect.sizeBytes)}`,
				onClick: () => onSelect(rect.id),
				className: cn("absolute overflow-hidden border border-background/80 p-2 text-left transition-[filter,outline] hover:brightness-105 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring", selected && "ring-2 ring-ring ring-offset-1 ring-offset-background"),
				style: rectStyle,
				children: rectContent
			}, rect.id);
		})]
	});
}
function SizeBar({ value, max }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "h-1.5 overflow-hidden rounded-full bg-muted",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "h-full rounded-full bg-foreground/65",
			style: { width: `${max > 0 ? Math.max(2, Math.min(100, value / max * 100)) : 0}%` }
		})
	});
}
function BreakdownList({ worktree, isScanning }) {
	if (!worktree) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "flex h-full min-h-72 items-center justify-center rounded-lg border border-dashed border-border/70 bg-muted/15 text-sm text-muted-foreground",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: "flex items-center gap-2",
			children: [isScanning ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }) : null, isScanning ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.c5135e7e4a", "Scanning workspace sizes. You can leave this page.") : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.5c6d25720c", "Select a workspace to inspect.")]
		})
	});
	const maxChildSize = getLargestWorkspaceSpaceItemSize(worktree.topLevelItems);
	const topLevelItemCount = worktree.topLevelItems.length + worktree.omittedTopLevelItemCount;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "min-h-72 rounded-lg border border-border/70 bg-background/35",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "border-b border-border/60 px-4 py-3",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 items-center justify-between gap-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "truncate text-sm font-semibold",
						children: worktree.displayName
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mt-0.5 truncate text-xs text-muted-foreground",
						children: worktree.repoDisplayName
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "shrink-0 text-right",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-sm font-semibold tabular-nums",
						children: formatBytes(worktree.sizeBytes)
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "text-[11px] text-muted-foreground",
						children: [
							formatCompactCount(topLevelItemCount),
							" ",
							translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.b25c2c1086", "top-level items")
						]
					})]
				})]
			})
		}), worktree.status !== "ok" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-start gap-2 px-4 py-4 text-xs text-destructive",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-3.5 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-0 break-words",
				children: worktree.error ?? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.0ba046fbc5", "Scan failed.")
			})]
		}) : worktree.topLevelItems.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "px-4 py-8 text-center text-sm text-muted-foreground",
			children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.16988df079", "No files found.")
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "max-h-72 overflow-y-auto scrollbar-sleek px-3 py-3",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "space-y-2",
				children: worktree.topLevelItems.slice(0, 12).map((item) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(BreakdownRow, {
					item,
					maxSize: maxChildSize
				}, `${item.path}:${item.name}`))
			})
		})]
	});
}
function BreakdownRow({ item, maxSize }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-1.5 rounded-md px-2 py-1.5 hover:bg-accent/50",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex min-w-0 items-center justify-between gap-3 text-xs",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-0 truncate font-medium",
				children: item.name
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "shrink-0 tabular-nums text-muted-foreground",
				children: formatBytes(item.sizeBytes)
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SizeBar, {
			value: item.sizeBytes,
			max: maxSize
		})]
	});
}
function WorkspaceRow({ worktree, maxSize, selected, inspected, decisionDetails, gitRefreshState, deleteState, onToggleSelected, onInspect, onOpenWorkspace, onDelete, onForceDelete }) {
	const isDeleting = deleteState?.isDeleting ?? false;
	const deleteError = deleteState?.error ?? null;
	const canForceDelete = deleteState?.canForceDelete ?? false;
	const canDelete = isWorkspaceSpaceRowReadyToDelete(worktree, decisionDetails) && !isDeleting;
	const handleForceDelete = (event) => {
		event.preventDefault();
		event.stopPropagation();
		onForceDelete();
	};
	const row = /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		role: "button",
		tabIndex: 0,
		"aria-busy": isDeleting,
		onClick: onInspect,
		onKeyDown: (event) => {
			if (event.key !== "Enter" && event.key !== " ") return;
			event.preventDefault();
			onInspect();
		},
		className: cn("grid w-full cursor-pointer grid-cols-[1.75rem_minmax(0,1.25fr)_minmax(9rem,0.55fr)_8rem_9.5rem] items-center gap-3 border-b border-border/45 px-3 py-2.5 text-left text-sm transition-colors last:border-b-0 hover:bg-accent/45 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring", inspected && "bg-accent/55", isDeleting && "cursor-wait opacity-50 grayscale hover:bg-transparent"),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CheckButton, {
				checked: canDelete && selected,
				disabled: !canDelete,
				label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.0d1c78d749", "Select {{value0}}", { value0: worktree.displayName }),
				onClick: onToggleSelected
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex min-w-0 items-center gap-2",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "min-w-0 truncate font-medium",
								children: worktree.displayName
							}),
							worktree.isRemote ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Server, { className: "size-3.5 shrink-0 text-muted-foreground" }) : null,
							worktree.isSparse ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: "outline",
								children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.9155381019", "Sparse")
							}) : null
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mt-1 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitBranch, { className: "size-3 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "truncate",
							children: getWorkspaceSpaceBranchLabel(worktree)
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mt-0.5 truncate font-mono text-[11px] text-muted-foreground",
						children: worktree.path
					}),
					deleteError ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mt-2 flex min-w-0 items-start gap-2 rounded-md border border-destructive/35 bg-destructive/8 px-2 py-1.5 text-[11px] text-destructive",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-3 shrink-0" }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "min-w-0 flex-1 break-words",
								title: deleteError,
								children: deleteError
							}),
							canForceDelete ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								type: "button",
								variant: "destructive",
								size: "xs",
								onClick: handleForceDelete,
								className: "h-6 shrink-0 gap-1 px-2",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3" }), translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.a998501630", "Force")]
							}) : null
						]
					}) : null
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 text-xs",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "truncate font-medium",
					children: worktree.repoDisplayName
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mt-0.5 truncate font-mono text-[11px] text-muted-foreground",
					children: worktree.repoPath
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 space-y-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "text-right text-sm font-medium tabular-nums",
					children: worktree.status === "ok" ? formatBytes(worktree.sizeBytes) : "—"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SizeBar, {
					value: worktree.sizeBytes,
					max: maxSize
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "flex justify-end",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(HoverCard, {
					openDelay: 250,
					closeDelay: 120,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(HoverCardTrigger, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "inline-flex",
							onClick: (event) => event.stopPropagation(),
							onKeyDown: (event) => event.stopPropagation(),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatusBadge, {
								worktree,
								decisionDetails,
								deleteState
							})
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceDecisionHoverCard, {
						worktree,
						details: decisionDetails,
						gitRefreshState,
						onOpenWorkspace
					})]
				})
			})
		]
	});
	if (!canDelete) return row;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenu, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuTrigger, {
		asChild: true,
		children: row
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
		variant: "destructive",
		onSelect: onDelete,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3.5" }), translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.792a214457", "Delete workspace")]
	}) })] });
}
function WorkspaceSpaceManagerPanel() {
	const analysis = useAppStore((state) => state.workspaceSpaceAnalysis);
	const progress = useAppStore((state) => state.workspaceSpaceScanProgress);
	const scanError = useAppStore((state) => state.workspaceSpaceScanError);
	const isScanning = useAppStore((state) => state.workspaceSpaceScanning);
	const refreshWorkspaceSpace = useAppStore((state) => state.refreshWorkspaceSpace);
	const cancelWorkspaceSpaceScan = useAppStore((state) => state.cancelWorkspaceSpaceScan);
	const removeWorkspaceSpaceWorktrees = useAppStore((state) => state.removeWorkspaceSpaceWorktrees);
	const removeWorktree = useAppStore((state) => state.removeWorktree);
	const deleteStateByWorktreeId = useAppStore((state) => state.deleteStateByWorktreeId);
	const repoMap = useAppStore((state) => getRepoMapFromState(state));
	const worktreeMap = useAppStore((state) => getWorktreeMapFromState(state));
	const tabsByWorktree = useAppStore((state) => state.tabsByWorktree);
	const ptyIdsByTabId = useAppStore((state) => state.ptyIdsByTabId);
	const agentStatusByPaneKey = useAppStore((state) => state.agentStatusByPaneKey);
	const migrationUnsupportedByPtyId = useAppStore((state) => state.migrationUnsupportedByPtyId);
	const runtimePaneTitlesByTabId = useAppStore((state) => state.runtimePaneTitlesByTabId);
	const agentStatusEpoch = useAppStore((state) => state.agentStatusEpoch);
	const retainedAgentsByPaneKey = useAppStore((state) => state.retainedAgentsByPaneKey);
	const openFiles = useAppStore((state) => state.openFiles);
	const editorDrafts = useAppStore((state) => state.editorDrafts);
	const browserTabsByWorktree = useAppStore((state) => state.browserTabsByWorktree);
	const gitStatusByWorktree = useAppStore((state) => state.gitStatusByWorktree);
	const remoteStatusesByWorktree = useAppStore((state) => state.remoteStatusesByWorktree);
	const hostedReviewCache = useAppStore((state) => state.hostedReviewCache);
	const issueCache = useAppStore((state) => state.issueCache);
	const linearIssueCache = useAppStore((state) => state.linearIssueCache);
	const settings = useAppStore((state) => state.settings);
	const activeWorktreeId = useAppStore((state) => state.activeWorktreeId);
	const setGitStatus = useAppStore((state) => state.setGitStatus);
	const updateWorktreeGitIdentity = useAppStore((state) => state.updateWorktreeGitIdentity);
	const setUpstreamStatus = useAppStore((state) => state.setUpstreamStatus);
	const fetchUpstreamStatus = useAppStore((state) => state.fetchUpstreamStatus);
	const [query, setQuery] = (0, import_react.useState)("");
	const [onlyDeletable, setOnlyDeletable] = (0, import_react.useState)(false);
	const [sortKey, setSortKey] = (0, import_react.useState)("size");
	const [sortDirection, setSortDirection] = (0, import_react.useState)("desc");
	const [selectedIds, setSelectedIds] = (0, import_react.useState)(() => /* @__PURE__ */ new Set());
	const [inspectedWorktreeId, setInspectedWorktreeId] = (0, import_react.useState)(null);
	const [treemapZoomWorktreeId, setTreemapZoomWorktreeId] = (0, import_react.useState)(null);
	const [gitRefreshStateByWorktreeId, setGitRefreshStateByWorktreeId] = (0, import_react.useState)({});
	const inFlightGitStatusRefreshes = (0, import_react.useRef)(/* @__PURE__ */ new Set());
	const refresh = (0, import_react.useCallback)(() => {
		refreshWorkspaceSpace().catch(() => {});
	}, [refreshWorkspaceSpace]);
	const cancelScan = (0, import_react.useCallback)(() => {
		cancelWorkspaceSpaceScan();
	}, [cancelWorkspaceSpaceScan]);
	const sourceRows = (0, import_react.useMemo)(() => analysis?.worktrees ?? [], [analysis?.worktrees]);
	const decisionDetailsByWorktreeId = (0, import_react.useMemo)(() => {
		const details = /* @__PURE__ */ new Map();
		const now = Date.now();
		for (const worktree of sourceRows) details.set(worktree.worktreeId, getWorkspaceDecisionDetails(worktree, {
			repoMap,
			worktreeMap,
			tabsByWorktree,
			ptyIdsByTabId,
			agentStatusByPaneKey,
			migrationUnsupportedByPtyId,
			runtimePaneTitlesByTabId,
			retainedAgentsByPaneKey,
			openFiles,
			editorDrafts,
			browserTabsByWorktree,
			gitStatusByWorktree,
			remoteStatusesByWorktree,
			hostedReviewCache,
			issueCache,
			linearIssueCache,
			settings,
			activeWorktreeId,
			now
		}));
		return details;
	}, [
		activeWorktreeId,
		agentStatusEpoch,
		agentStatusByPaneKey,
		browserTabsByWorktree,
		editorDrafts,
		gitStatusByWorktree,
		hostedReviewCache,
		issueCache,
		linearIssueCache,
		openFiles,
		ptyIdsByTabId,
		repoMap,
		remoteStatusesByWorktree,
		retainedAgentsByPaneKey,
		migrationUnsupportedByPtyId,
		runtimePaneTitlesByTabId,
		settings,
		sourceRows,
		tabsByWorktree,
		worktreeMap
	]);
	const isWorktreeDeleting = (0, import_react.useCallback)((worktreeId) => deleteStateByWorktreeId[worktreeId]?.isDeleting ?? false, [deleteStateByWorktreeId]);
	const refreshWorkspaceGitStatus = (0, import_react.useCallback)((worktree) => {
		const currentState = useAppStore.getState();
		if (currentState.gitStatusByWorktree[worktree.worktreeId] !== void 0) return Promise.resolve();
		if (inFlightGitStatusRefreshes.current.has(worktree.worktreeId)) return Promise.resolve();
		inFlightGitStatusRefreshes.current.add(worktree.worktreeId);
		setGitRefreshStateByWorktreeId((current) => ({
			...current,
			[worktree.worktreeId]: {
				isRefreshing: true,
				error: null
			}
		}));
		return refreshGitStatusForWorktree({
			settings,
			worktreeId: worktree.worktreeId,
			worktreePath: worktree.path,
			connectionId: currentState.repos.find((repo) => repo.id === worktree.repoId)?.connectionId ?? void 0,
			deps: {
				setGitStatus,
				updateWorktreeGitIdentity,
				setUpstreamStatus,
				fetchUpstreamStatus
			}
		}).then(() => {
			if (useAppStore.getState().gitStatusByWorktree[worktree.worktreeId] === void 0) setGitStatus(worktree.worktreeId, {
				conflictOperation: "unknown",
				entries: [],
				ignoredPaths: []
			});
			setGitRefreshStateByWorktreeId((current) => ({
				...current,
				[worktree.worktreeId]: {
					isRefreshing: false,
					error: null
				}
			}));
		}).catch((error) => {
			setGitRefreshStateByWorktreeId((current) => ({
				...current,
				[worktree.worktreeId]: {
					isRefreshing: false,
					error: error instanceof Error ? error.message : String(error)
				}
			}));
		}).finally(() => {
			inFlightGitStatusRefreshes.current.delete(worktree.worktreeId);
		});
	}, [
		fetchUpstreamStatus,
		setGitStatus,
		setUpstreamStatus,
		settings,
		updateWorktreeGitIdentity
	]);
	const isWorktreeUnavailableForDelete = (0, import_react.useCallback)((worktreeId) => {
		if (isWorktreeDeleting(worktreeId)) return true;
		const worktree = sourceRows.find((row) => row.worktreeId === worktreeId);
		return !worktree || !isWorkspaceSpaceRowReadyToDelete(worktree, decisionDetailsByWorktreeId.get(worktreeId));
	}, [
		decisionDetailsByWorktreeId,
		isWorktreeDeleting,
		sourceRows
	]);
	const rows = (0, import_react.useMemo)(() => sortWorkspaceSpaceRows(filterWorkspaceSpaceRows(sourceRows, query, onlyDeletable), sortKey, sortDirection), [
		onlyDeletable,
		query,
		sortDirection,
		sortKey,
		sourceRows
	]);
	const nextInspectedWorktreeId = resolveWorkspaceSpaceInspectedWorktreeId(sourceRows, inspectedWorktreeId);
	const nextSelectedIds = pruneWorkspaceSpaceSelectedIds(sourceRows, selectedIds);
	const nextTreemapZoomWorktreeId = resolveWorkspaceSpaceTreemapZoomWorktreeId(sourceRows, treemapZoomWorktreeId);
	if (inspectedWorktreeId !== nextInspectedWorktreeId) setInspectedWorktreeId(nextInspectedWorktreeId);
	if (nextSelectedIds !== selectedIds) setSelectedIds(nextSelectedIds);
	if (treemapZoomWorktreeId !== nextTreemapZoomWorktreeId) setTreemapZoomWorktreeId(nextTreemapZoomWorktreeId);
	(0, import_react.useEffect)(() => {
		const candidates = getWorkspaceSpaceGitStatusRefreshCandidates(sourceRows);
		if (candidates.length === 0) return;
		let cancelled = false;
		let nextIndex = 0;
		const runWorker = async () => {
			while (!cancelled) {
				const worktree = candidates[nextIndex];
				nextIndex += 1;
				if (!worktree) return;
				await refreshWorkspaceGitStatus(worktree);
			}
		};
		const workerCount = Math.min(GIT_STATUS_REFRESH_CONCURRENCY, candidates.length);
		Promise.all(Array.from({ length: workerCount }, () => runWorker()));
		return () => {
			cancelled = true;
		};
	}, [refreshWorkspaceGitStatus, sourceRows]);
	const inspectedWorktree = rows.find((row) => row.worktreeId === nextInspectedWorktreeId) ?? rows.find((row) => row.status === "ok") ?? null;
	const zoomedWorktree = sourceRows.find((row) => row.worktreeId === nextTreemapZoomWorktreeId && row.status === "ok") ?? null;
	const maxSize = getLargestWorkspaceSpaceRowSize(rows);
	const selectedDeletableIds = (0, import_react.useMemo)(() => getSelectedDeletableWorkspaceIds(rows, nextSelectedIds, isWorktreeUnavailableForDelete), [
		isWorktreeUnavailableForDelete,
		nextSelectedIds,
		rows
	]);
	const selectedDeletableIdSet = (0, import_react.useMemo)(() => new Set(selectedDeletableIds), [selectedDeletableIds]);
	const visibleDeletableIds = (0, import_react.useMemo)(() => getVisibleDeletableWorkspaceIds(rows, isWorktreeUnavailableForDelete), [isWorktreeUnavailableForDelete, rows]);
	const allVisibleSelected = visibleDeletableIds.length > 0 && visibleDeletableIds.every((id) => nextSelectedIds.has(id));
	const someVisibleSelected = visibleDeletableIds.some((id) => nextSelectedIds.has(id));
	const visibleSelectionState = allVisibleSelected ? true : someVisibleSelected ? "mixed" : false;
	const isInitialScan = isScanning && !analysis;
	const hasRows = sourceRows.length > 0;
	const progressLabel = getWorkspaceSpaceProgressLabel(progress);
	const repoErrors = analysis?.repos.filter((repo) => repo.error !== null) ?? [];
	const selectedReclaimableBytes = (0, import_react.useMemo)(() => rows.filter((row) => selectedDeletableIdSet.has(row.worktreeId)).reduce((sum, row) => sum + row.reclaimableBytes, 0), [rows, selectedDeletableIdSet]);
	const toggleSort = (key) => {
		if (sortKey === key) {
			setSortDirection((current) => current === "asc" ? "desc" : "asc");
			return;
		}
		setSortKey(key);
		setSortDirection(key === "name" || key === "repo" ? "asc" : "desc");
	};
	const selectSortKey = (key) => {
		setSortKey(key);
		setSortDirection(key === "name" || key === "repo" ? "asc" : "desc");
	};
	const toggleSelection = (worktreeId) => {
		setSelectedIds((current) => {
			const next = new Set(current);
			if (next.has(worktreeId)) next.delete(worktreeId);
			else next.add(worktreeId);
			return next;
		});
	};
	const toggleVisibleSelection = () => {
		setSelectedIds((current) => {
			const next = new Set(current);
			if (allVisibleSelected) for (const id of visibleDeletableIds) next.delete(id);
			else for (const id of visibleDeletableIds) next.add(id);
			return next;
		});
	};
	const handleDeletedWorktrees = (0, import_react.useCallback)((deletedIds) => {
		if (deletedIds.length === 0) return;
		removeWorkspaceSpaceWorktrees(deletedIds);
		setInspectedWorktreeId((current) => current && deletedIds.includes(current) ? null : current);
		setTreemapZoomWorktreeId((current) => current && deletedIds.includes(current) ? null : current);
		setSelectedIds((current) => {
			const next = new Set(current);
			for (const id of deletedIds) next.delete(id);
			return next;
		});
		toast.success(deletedIds.length === 1 ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.9afc97f9a3", "Workspace deleted") : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.eee5240810", "Workspaces deleted"), { description: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.63efebe0e6", "{{value0}} {{value1}} removed from Space.", {
			value0: deletedIds.length,
			value1: deletedIds.length === 1 ? "workspace" : "workspaces"
		}) });
	}, [removeWorkspaceSpaceWorktrees]);
	const deleteWorktrees = (0, import_react.useCallback)((worktreeIds) => {
		if (worktreeIds.length === 0) return;
		runWorktreeBatchDelete(worktreeIds, {
			forceConfirm: true,
			onDeleted: handleDeletedWorktrees
		});
	}, [handleDeletedWorktrees]);
	const forceDeleteWorktree = (0, import_react.useCallback)((worktree) => {
		const commitFocus = prepareActiveWorktreeFocusAfterDelete(worktree.worktreeId);
		removeWorktree(worktree.worktreeId, true, { allowUnverifiedPtyStop: true }).then((result) => {
			if (!result.ok) {
				toast.error(translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.2965415393", "Force delete failed"), { description: result.error });
				return;
			}
			commitFocus();
			handleDeletedWorktrees([worktree.worktreeId]);
		}).catch((error) => {
			toast.error(translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.2965415393", "Force delete failed"), { description: error instanceof Error ? error.message : String(error) });
		});
	}, [handleDeletedWorktrees, removeWorktree]);
	const deleteSelected = () => {
		if (selectedDeletableIds.length === 0) return;
		deleteWorktrees(selectedDeletableIds);
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-5",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "grid overflow-hidden rounded-lg border border-border/65 bg-background/35 md:grid-cols-4 md:divide-x md:divide-border/60",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Metric, {
						label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.09960d86bd", "Scanned"),
						value: analysis ? formatBytes(analysis.totalSizeBytes) : "—"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Metric, {
						label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.83f1a0a932", "Reclaimable"),
						value: analysis ? formatBytes(analysis.reclaimableBytes) : "—"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Metric, {
						label: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.43171f3e60", "Workspaces"),
						value: analysis ? analysis.unavailableWorktreeCount > 0 ? `${analysis.scannedWorktreeCount}/${analysis.worktreeCount}` : String(analysis.scannedWorktreeCount) : "—"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UpdatedMetric, {
						scannedAt: analysis?.scannedAt ?? null,
						isScanning
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-wrap items-center justify-between gap-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex min-w-0 items-center gap-2 text-xs text-muted-foreground",
					children: [isScanning ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 shrink-0 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HardDrive, { className: "size-4 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "truncate",
						children: analysis ? isScanning ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.34174bd83d", "{{value0}}. You can leave this page; the last result stays visible.", { value0: progressLabel ?? "Scanning workspace sizes" }) : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.d595295d7d", "{{value0}} can be reclaimed from linked worktrees.", { value0: formatBytes(analysis.reclaimableBytes) }) : isScanning ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.265d956765", "{{value0}}. You can leave this page.", { value0: progressLabel ?? "Scanning workspace sizes" }) : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.e91dd2a9ae", "Run a scan to inspect workspace sizes.")
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					variant: "outline",
					size: "sm",
					onClick: isScanning ? cancelScan : refresh,
					disabled: progress?.state === "cancelling",
					className: "w-28 gap-1.5",
					children: [isScanning ? progress?.state === "cancelling" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3.5" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: "size-3.5" }), isScanning ? progress?.state === "cancelling" ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.1fce91d1b9", "Stopping") : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.8dc9ddac8a", "Cancel") : analysis ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.508673bac0", "Refresh") : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.8c7c57fbf8", "Scan")]
				})]
			}),
			scanError ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-start gap-2 rounded-md border border-destructive/35 bg-destructive/8 px-3 py-2 text-xs text-destructive",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-3.5 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "min-w-0 break-words",
					children: [scanError, analysis ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.20a4204dce", "Last successful results remain visible.") : ""]
				})]
			}) : null,
			repoErrors.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "space-y-1.5 rounded-md border border-border/70 bg-muted/20 px-3 py-2 text-xs text-muted-foreground",
				children: repoErrors.map((repo) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-start gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-3.5 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "min-w-0 break-words",
						children: [
							repo.displayName,
							": ",
							repo.error
						]
					})]
				}, repo.repoId))
			}) : null,
			hasRows || isInitialScan ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "grid gap-4 xl:grid-cols-[minmax(0,1.4fr)_minmax(20rem,0.6fr)]",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceTreemap, {
					rows: sourceRows,
					isScanning: isInitialScan,
					selectedWorktreeId: inspectedWorktree?.worktreeId ?? null,
					zoomedWorktree,
					onSelect: setInspectedWorktreeId,
					onZoomChange: setTreemapZoomWorktreeId
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(BreakdownList, {
					worktree: inspectedWorktree,
					isScanning: isInitialScan
				})]
			}) : null,
			hasRows ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "sticky top-0 z-10 -mx-1 flex flex-wrap items-center justify-between gap-2 rounded-md border border-border/70 bg-background/95 px-3 py-2 shadow-xs backdrop-blur",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 text-xs text-muted-foreground",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
							className: "font-medium text-foreground",
							children: [
								selectedDeletableIds.length,
								" ",
								translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.65402b7192", "selected")
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mx-1.5",
							children: "·"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [
							formatBytes(selectedReclaimableBytes),
							" ",
							translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.0cb1501ccf", "reclaimable")
						] })
					]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex shrink-0 items-center gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "ghost",
						size: "sm",
						onClick: () => setSelectedIds(/* @__PURE__ */ new Set()),
						disabled: selectedDeletableIds.length === 0,
						className: "!px-3",
						children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.e4a12c455b", "Clear")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						variant: "destructive",
						size: "sm",
						onClick: deleteSelected,
						disabled: selectedDeletableIds.length === 0,
						className: "min-w-[9.5rem] gap-1.5 !px-3.5",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3.5" }), translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.5caccea440", "Delete selected")]
					})]
				})]
			}) : null,
			hasRows ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-wrap items-center gap-2",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "relative min-w-[16rem] flex-1",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Search, { className: "pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: query,
							onChange: (event) => setQuery(event.target.value),
							placeholder: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.6f8f6a6b04", "Filter workspaces"),
							className: "pl-9"
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Select, {
						value: sortKey,
						onValueChange: (value) => selectSortKey(value),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectTrigger, {
							className: "w-36",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectValue, {})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(SelectContent, { children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
								value: "size",
								children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.33aef3e9cc", "Size")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
								value: "name",
								children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.243287ac60", "Name")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
								value: "repo",
								children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.81f14d9924", "Repository")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
								value: "activity",
								children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.d7ac56452e", "Activity")
							})
						] })]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: onlyDeletable ? "secondary" : "outline",
						size: "sm",
						onClick: () => setOnlyDeletable((current) => !current),
						className: "w-32",
						"aria-label": translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.81aaf1de65", "Show only deletable workspaces"),
						children: onlyDeletable ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.b2f82ed5ae", "Deletable") : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.ef890d31b9", "All")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						variant: "outline",
						size: "sm",
						onClick: toggleVisibleSelection,
						disabled: visibleDeletableIds.length === 0,
						className: "w-32 gap-1.5",
						"aria-label": allVisibleSelected ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.697d60c456", "Clear visible selection") : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.1d0f8300d1", "Select visible deletable workspaces"),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "size-3.5" }), allVisibleSelected ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.e4a12c455b", "Clear") : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.f39d291997", "Select")]
					})
				]
			}) : null,
			hasRows || isInitialScan ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "overflow-x-auto rounded-lg border border-border/70 bg-background/30",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-[46rem]",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "grid grid-cols-[1.75rem_minmax(0,1.25fr)_minmax(9rem,0.55fr)_8rem_9.5rem] gap-3 border-b border-border/60 px-3 py-2 text-[11px] font-medium uppercase tracking-[0.14em] text-muted-foreground",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "flex items-center",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CheckButton, {
									checked: visibleSelectionState,
									disabled: visibleDeletableIds.length === 0,
									label: allVisibleSelected ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.697d60c456", "Clear visible selection") : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.1d0f8300d1", "Select visible deletable workspaces"),
									onClick: toggleVisibleSelection
								})
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
								type: "button",
								onClick: () => toggleSort("name"),
								className: "flex items-center gap-1 text-left",
								children: [translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.e4aebea158", "Workspace"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SortIndicator, {
									sortKey: "name",
									activeKey: sortKey,
									direction: sortDirection
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
								type: "button",
								onClick: () => toggleSort("repo"),
								className: "flex items-center gap-1 text-left",
								children: [translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.81f14d9924", "Repository"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SortIndicator, {
									sortKey: "repo",
									activeKey: sortKey,
									direction: sortDirection
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
								type: "button",
								onClick: () => toggleSort("size"),
								className: "flex items-center justify-end gap-1 text-right",
								children: [translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.33aef3e9cc", "Size"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SortIndicator, {
									sortKey: "size",
									activeKey: sortKey,
									direction: sortDirection
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-right",
								children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.be37293b10", "State")
							})
						]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "max-h-[28rem] overflow-y-auto scrollbar-sleek",
						children: isInitialScan ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center justify-center gap-2 px-4 py-10 text-center text-sm text-muted-foreground",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }), translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.a02d84d2d2", "Scanning workspaces. You can leave this page.")]
						}) : rows.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "px-4 py-10 text-center text-sm text-muted-foreground",
							children: translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.e031e93219", "No matching workspaces.")
						}) : rows.map((worktree) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceRow, {
							worktree,
							maxSize,
							selected: nextSelectedIds.has(worktree.worktreeId),
							inspected: inspectedWorktree?.worktreeId === worktree.worktreeId,
							decisionDetails: decisionDetailsByWorktreeId.get(worktree.worktreeId) ?? getWorkspaceDecisionDetails(worktree, {
								repoMap,
								worktreeMap,
								tabsByWorktree,
								ptyIdsByTabId,
								agentStatusByPaneKey,
								migrationUnsupportedByPtyId,
								runtimePaneTitlesByTabId,
								retainedAgentsByPaneKey,
								openFiles,
								editorDrafts,
								browserTabsByWorktree,
								gitStatusByWorktree,
								remoteStatusesByWorktree,
								hostedReviewCache,
								issueCache,
								linearIssueCache,
								settings,
								activeWorktreeId,
								now: Date.now()
							}),
							gitRefreshState: gitRefreshStateByWorktreeId[worktree.worktreeId],
							deleteState: deleteStateByWorktreeId[worktree.worktreeId],
							onToggleSelected: () => toggleSelection(worktree.worktreeId),
							onInspect: () => setInspectedWorktreeId(worktree.worktreeId),
							onOpenWorkspace: () => activateAndRevealWorktree(worktree.worktreeId),
							onDelete: () => deleteWorktrees([worktree.worktreeId]),
							onForceDelete: () => forceDeleteWorktree(worktree)
						}, worktree.worktreeId))
					})]
				})
			}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "rounded-lg border border-border/70 bg-background/30 px-4 py-10 text-center text-sm text-muted-foreground",
				children: scanError ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.8194a4fb29", "Scan failed before any workspace sizes were collected.") : analysis ? translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.61e25239da", "No workspace rows were available from the scan.") : translate("auto.components.status.bar.WorkspaceSpaceManagerPanel.e91dd2a9ae", "Run a scan to inspect workspace sizes.")
			})
		]
	});
}
function WorkspaceSpacePage() {
	const closeSpacePage = useAppStore((state) => state.closeSpacePage);
	(0, import_react.useEffect)(() => {
		const hasVisibleOverlay = () => Array.from(document.querySelectorAll("[role=\"dialog\"], [role=\"listbox\"], [role=\"menu\"]")).some((element) => {
			if (!(element instanceof HTMLElement)) return false;
			if (element.closest("[aria-hidden=\"true\"]")) return false;
			const style = window.getComputedStyle(element);
			return style.display !== "none" && style.visibility !== "hidden" && element.getClientRects().length > 0;
		});
		const handleKeyDown = (event) => {
			if (event.key !== "Escape") return;
			if (hasVisibleOverlay()) return;
			if (event.target?.matches("input, textarea, select, [contenteditable=\"true\"], [contenteditable=\"\"]")) return;
			event.preventDefault();
			closeSpacePage();
		};
		window.addEventListener("keydown", handleKeyDown, { capture: true });
		return () => window.removeEventListener("keydown", handleKeyDown, { capture: true });
	}, [closeSpacePage]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex h-full min-h-0 flex-col bg-background",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex shrink-0 items-center gap-3 border-b border-border px-5 py-3",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				variant: "outline",
				size: "sm",
				onClick: closeSpacePage,
				className: "shrink-0 gap-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowLeft, { className: "size-3.5" }), translate("auto.components.workspace.space.WorkspaceSpacePage.ecf72fdc3b", "Back")]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 items-center gap-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-muted/30",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HardDrive, { className: "size-4 text-muted-foreground" })
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex min-w-0 items-center gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", {
							className: "truncate text-base font-semibold text-foreground",
							children: translate("auto.components.workspace.space.WorkspaceSpacePage.45f6302dbc", "Space")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: "secondary",
							children: translate("auto.components.workspace.space.WorkspaceSpacePage.e8d6ba11ab", "Beta")
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "truncate text-xs text-muted-foreground",
						children: translate("auto.components.workspace.space.WorkspaceSpacePage.8d0048e1cb", "Workspace disk usage and reclaimable worktree storage.")
					})]
				})]
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "flex-1 overflow-y-auto p-5 scrollbar-sleek",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mx-auto max-w-7xl",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceSpaceManagerPanel, {})
			})
		})]
	});
}
export { WorkspaceSpacePage as default };
