import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn, t as Button } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as ChevronRight } from "./chevron-right-CZtMe6Ev.js";
import { r as activateAndRevealWorktree } from "./worktree-activation-BDsaiyMf.js";
import { t as Globe } from "./globe-c32i33v9.js";
import { t as HardDrive } from "./hard-drive-BclppwkA.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { t as RotateCw } from "./rotate-cw-DBuQrdY8.js";
import { t as Terminal } from "./terminal-Cen7Un9b.js";
import { Cp as getRepoIdFromWorktreeId, Jc as parsePaneKey, Op as parsePtySessionId, Vm as ORPHAN_WORKTREE_ID, m_ as Trash2, t as useAppStore, wp as getWorktreePathBasenameFromId } from "./store-CgXrfmaH.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import { t as X } from "./x-BrGKE4uz.js";
import "./plugin-manifest-Bs-50M_g.js";
import { $ as getRepoExecutionHostId, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { i as PopoverTrigger, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { g as useWorktreeMap, t as getAllWorktreesFromState } from "./selectors-XOBeaOSb.js";
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
import { n as runWorktreeDelete } from "./delete-worktree-flow-RxB6NScm.js";
import "./preserved-branch-batch-toast-DHxeGO1o.js";
import { t as activateTabAndFocusPane } from "./activate-tab-and-focus-pane-dvS5VCkm.js";
import "./terminal-tab-actions-BaU95skQ.js";
import { t as Badge } from "./badge-BBptl5GG.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import "./relative-time-format-BdBnutwN.js";
import { i as subscribeDaemonSessionInventoryInvalidated, n as useDaemonActions, t as DaemonActionDialog } from "./useDaemonActions-Dxlrlfjm.js";
import { t as STATUS_BAR_CONTEXT_MENU_EXEMPT_PROPS } from "./status-bar-context-menu-policy-Dc4KA2ce.js";
import { t as countEstimatedInactiveWorkspaces } from "./inactive-workspace-estimate-B_n4w59B.js";
import { i as getWorkspaceSpaceProgressLabel, o as getWorkspaceSpaceScanTimeLabel, t as formatBytes } from "./workspace-space-format-DqHSji4k.js";
var MemoryStick = createLucideIcon("memory-stick", [
	["path", {
		d: "M12 12v-2",
		key: "fwoke6"
	}],
	["path", {
		d: "M12 18v-2",
		key: "qj6yno"
	}],
	["path", {
		d: "M16 12v-2",
		key: "heuere"
	}],
	["path", {
		d: "M16 18v-2",
		key: "s1ct0w"
	}],
	["path", {
		d: "M2 11h1.5",
		key: "15p63e"
	}],
	["path", {
		d: "M20 18v-2",
		key: "12ehxp"
	}],
	["path", {
		d: "M20.5 11H22",
		key: "khsy7a"
	}],
	["path", {
		d: "M4 18v-2",
		key: "1c3oqr"
	}],
	["path", {
		d: "M8 12v-2",
		key: "1mwtfd"
	}],
	["path", {
		d: "M8 18v-2",
		key: "qcmpov"
	}],
	["rect", {
		x: "2",
		y: "6",
		width: "20",
		height: "10",
		rx: "2",
		key: "1qcswk"
	}]
]);
var import_react = /* @__PURE__ */ __toESM(require_react());
function mayDestroyWithoutOwnerEvidence(session) {
	return session.agentOwnership === "absent";
}
function addBinding(ptyIdToTabId, tabId, ptyId) {
	if (!ptyId || ptyIdToTabId.has(ptyId)) return;
	ptyIdToTabId.set(ptyId, tabId);
}
function buildResourceSessionBindingIndex(inputs) {
	const ptyIdToTabId = /* @__PURE__ */ new Map();
	const tabIdToWorktreeId = /* @__PURE__ */ new Map();
	for (const [worktreeId, tabs] of Object.entries(inputs.tabsByWorktree)) for (const tab of tabs) tabIdToWorktreeId.set(tab.id, worktreeId);
	for (const [tabId, ptyIds] of Object.entries(inputs.ptyIdsByTabId)) for (const ptyId of ptyIds) addBinding(ptyIdToTabId, tabId, ptyId);
	for (const tabs of Object.values(inputs.tabsByWorktree)) for (const tab of tabs) addBinding(ptyIdToTabId, tab.id, tab.ptyId);
	for (const [tabId, layout] of Object.entries(inputs.terminalLayoutsByTabId ?? {})) {
		if (!tabIdToWorktreeId.has(tabId)) continue;
		for (const ptyId of Object.values(layout.ptyIdsByLeafId ?? {})) addBinding(ptyIdToTabId, tabId, ptyId);
	}
	for (const [tabId, sessionId] of Object.entries(inputs.deferredSshSessionIdsByTabId ?? {})) {
		if (!tabIdToWorktreeId.has(tabId)) continue;
		addBinding(ptyIdToTabId, tabId, sessionId);
	}
	return {
		ptyIdToTabId,
		tabIdToWorktreeId,
		boundPtyIds: inputs.workspaceSessionReady ? new Set(ptyIdToTabId.keys()) : /* @__PURE__ */ new Set()
	};
}
function selectUnboundDaemonSessions(sessions, inputs) {
	if (!inputs.workspaceSessionReady) return [];
	const { boundPtyIds } = buildResourceSessionBindingIndex(inputs);
	return sessions.filter((session) => !boundPtyIds.has(session.id) && mayDestroyWithoutOwnerEvidence(session));
}
function countUnboundDaemonSessions(sessions, inputs) {
	return selectUnboundDaemonSessions(sessions, inputs).length;
}
function deriveRepoIdFromWorktreeId(worktreeId) {
	return getRepoIdFromWorktreeId(worktreeId);
}
function deriveWorktreeNameFromWorktreeId(worktreeId) {
	return getWorktreePathBasenameFromId(worktreeId) ?? worktreeId;
}
function shortCwd(cwd) {
	if (!cwd) return "";
	const sep = cwd.includes("\\") ? "\\" : "/";
	const parts = cwd.split(/[\\/]+/).filter(Boolean);
	return parts.length > 2 ? parts.slice(-2).join(sep) : cwd;
}
function parsePaneKey$1(paneKey) {
	if (!paneKey) return null;
	const parsed = parsePaneKey(paneKey);
	return parsed ? {
		tabId: parsed.tabId,
		leafId: parsed.leafId
	} : null;
}
function resolveSnapshotSessionLabel(session, worktreeId, ctx) {
	const parsed = parsePaneKey$1(session.paneKey);
	if (parsed) {
		const tabs = ctx.tabsByWorktree[worktreeId] ?? [];
		const tabIndex = tabs.findIndex((t) => t.id === parsed.tabId);
		const tab = tabIndex !== -1 ? tabs[tabIndex] : void 0;
		if (tab) {
			const custom = tab.customTitle?.trim();
			if (custom) return custom;
			return tab.defaultTitle?.trim() || tab.title?.trim() || `Terminal ${tabIndex + 1}`;
		}
	}
	if (session.pid > 0) return `pid ${session.pid}`;
	const fallback = session.sessionId?.slice(0, 8);
	return fallback ? `session ${fallback}` : "(unknown session)";
}
function resolveDaemonSessionLabel(session, resolvedWorktreeId, tabId, ctx) {
	if (tabId && resolvedWorktreeId) {
		const tabs = ctx.tabsByWorktree[resolvedWorktreeId] ?? [];
		const tabIndex = tabs.findIndex((t) => t.id === tabId);
		const tab = tabIndex !== -1 ? tabs[tabIndex] : void 0;
		if (tab) {
			const custom = tab.customTitle?.trim();
			if (custom) return custom;
			const runtimeMap = ctx.runtimePaneTitlesByTabId[tabId];
			if (runtimeMap) {
				const live = Object.values(runtimeMap).find((t) => t?.trim());
				if (live) return live;
			}
			const fallback = tab.defaultTitle?.trim() || tab.title?.trim();
			if (fallback) return fallback;
		}
	}
	if (session.cwd) return shortCwd(session.cwd);
	if (resolvedWorktreeId) return shortCwd(resolvedWorktreeId);
	if (session.title) return session.title;
	return "unknown";
}
const UNATTRIBUTED_REPO_ID = "__unattributed__";
const UNATTRIBUTED_REPO_NAME = "Unattributed";
function mergeSnapshotAndSessions(snapshot, daemonSessions, ctx) {
	const repos = /* @__PURE__ */ new Map();
	const seenSessionIds = /* @__PURE__ */ new Set();
	const index = buildResourceSessionBindingIndex(ctx);
	const boundPtyIds = index.boundPtyIds;
	const ownershipBySessionId = new Map(daemonSessions.map((session) => [session.id, session.agentOwnership]));
	function isRepoRemote(repoId) {
		return ctx.repoConnectionIdById.get(repoId) != null;
	}
	function isRuntimeScopedRepo(repoId) {
		return ctx.repoRuntimeScopedById.get(repoId) === true;
	}
	function ensureRepo(repoId, repoName, initiallyHasRemoteChildren = false) {
		const existing = repos.get(repoId);
		if (existing) return existing;
		const next = {
			repoId,
			repoName,
			cpu: null,
			memory: null,
			hasRemoteChildren: initiallyHasRemoteChildren || isRepoRemote(repoId),
			worktrees: []
		};
		repos.set(repoId, next);
		return next;
	}
	function findWorktreeRow(repo, worktreeId) {
		return repo.worktrees.find((w) => w.worktreeId === worktreeId);
	}
	if (snapshot) for (const wt of snapshot.worktrees) {
		if (isRuntimeScopedRepo(wt.repoId)) continue;
		const repo = ensureRepo(wt.repoId, wt.repoName);
		const sessions = wt.sessions.map((s) => {
			seenSessionIds.add(s.sessionId);
			const tabId = index.ptyIdToTabId.get(s.sessionId) ?? null;
			return {
				sessionId: s.sessionId,
				paneKey: s.paneKey,
				pid: s.pid,
				label: resolveSnapshotSessionLabel(s, wt.worktreeId, ctx),
				bound: ctx.workspaceSessionReady && boundPtyIds.has(s.sessionId),
				agentOwnership: ownershipBySessionId.get(s.sessionId) ?? "unknown",
				tabId,
				cpu: s.cpu,
				memory: s.memory,
				hasLocalSamples: true
			};
		});
		repo.worktrees.push({
			worktreeId: wt.worktreeId,
			worktreeName: wt.worktreeName,
			repoId: wt.repoId,
			repoName: wt.repoName,
			cpu: wt.cpu,
			memory: wt.memory,
			history: wt.history,
			hasLocalSamples: true,
			isRemote: isRepoRemote(wt.repoId),
			sessions,
			browsers: []
		});
	}
	for (const session of daemonSessions) {
		if (seenSessionIds.has(session.id)) continue;
		seenSessionIds.add(session.id);
		const tabId = index.ptyIdToTabId.get(session.id) ?? null;
		let worktreeId = tabId ? index.tabIdToWorktreeId.get(tabId) ?? null : null;
		if (!worktreeId) worktreeId = parsePtySessionId(session.id).worktreeId;
		const isUnattributed = !worktreeId;
		const finalWorktreeId = worktreeId ?? `__unattributed__::${session.id}`;
		const finalRepoId = isUnattributed ? UNATTRIBUTED_REPO_ID : deriveRepoIdFromWorktreeId(finalWorktreeId);
		const finalRepoName = isUnattributed ? UNATTRIBUTED_REPO_NAME : ctx.repoDisplayNameById.get(finalRepoId) || finalRepoId;
		const finalWorktreeName = isUnattributed ? session.title || session.id.slice(0, 12) : deriveWorktreeNameFromWorktreeId(finalWorktreeId);
		if (isRuntimeScopedRepo(finalRepoId)) continue;
		const repoIsRemote = isRepoRemote(finalRepoId);
		const repo = ensureRepo(finalRepoId, finalRepoName, repoIsRemote);
		if (repoIsRemote) repo.hasRemoteChildren = true;
		let row = findWorktreeRow(repo, finalWorktreeId);
		if (!row) {
			row = {
				worktreeId: finalWorktreeId,
				worktreeName: finalWorktreeName,
				repoId: finalRepoId,
				repoName: finalRepoName,
				cpu: null,
				memory: null,
				history: [],
				hasLocalSamples: false,
				isRemote: repoIsRemote,
				sessions: [],
				browsers: []
			};
			repo.worktrees.push(row);
		}
		row.sessions.push({
			sessionId: session.id,
			paneKey: null,
			pid: 0,
			label: resolveDaemonSessionLabel(session, worktreeId, tabId, ctx),
			bound: ctx.workspaceSessionReady && boundPtyIds.has(session.id),
			agentOwnership: session.agentOwnership,
			tabId,
			cpu: null,
			memory: null,
			hasLocalSamples: false
		});
	}
	for (const [worktreeId, browsers] of Object.entries(ctx.browserTabsByWorktree ?? {})) {
		const worktree = ctx.worktreeById?.get(worktreeId);
		if (!worktree || browsers.length === 0) continue;
		const repoName = ctx.repoDisplayNameById.get(worktree.repoId) || worktree.repoId;
		const repo = ensureRepo(worktree.repoId, repoName);
		let row = findWorktreeRow(repo, worktreeId);
		if (!row) {
			row = {
				worktreeId,
				worktreeName: worktree.displayName,
				repoId: worktree.repoId,
				repoName,
				cpu: null,
				memory: null,
				history: [],
				hasLocalSamples: false,
				isRemote: isRepoRemote(worktree.repoId),
				sessions: [],
				browsers: []
			};
			repo.worktrees.push(row);
		}
		row.browsers = browsers;
	}
	for (const repo of repos.values()) {
		let cpuSum = 0;
		let memSum = 0;
		let anyLocal = false;
		for (const wt of repo.worktrees) if (wt.cpu !== null && wt.memory !== null) {
			cpuSum += wt.cpu;
			memSum += wt.memory;
			anyLocal = true;
		}
		repo.cpu = anyLocal ? cpuSum : null;
		repo.memory = anyLocal ? memSum : null;
	}
	return [...repos.values()];
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function WorkspaceSpaceCompactPanel({ onOpenFullPage }) {
	const analysis = useAppStore((state) => state.workspaceSpaceAnalysis);
	const progress = useAppStore((state) => state.workspaceSpaceScanProgress);
	const scanError = useAppStore((state) => state.workspaceSpaceScanError);
	const isScanning = useAppStore((state) => state.workspaceSpaceScanning);
	const refreshWorkspaceSpace = useAppStore((state) => state.refreshWorkspaceSpace);
	const cancelWorkspaceSpaceScan = useAppStore((state) => state.cancelWorkspaceSpaceScan);
	const progressLabel = getWorkspaceSpaceProgressLabel(progress);
	const scan = (0, import_react.useCallback)(() => {
		refreshWorkspaceSpace().catch(() => {});
	}, [refreshWorkspaceSpace]);
	const cancelScan = (0, import_react.useCallback)(() => {
		cancelWorkspaceSpaceScan();
	}, [cancelWorkspaceSpaceScan]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "border-t border-border/50 bg-muted/15 px-3 py-2",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center justify-between gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex min-w-0 items-center gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(HardDrive, { className: "size-3.5 shrink-0 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex min-w-0 items-center gap-1.5 text-[11px] font-medium text-foreground",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "truncate",
								children: translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.8ff597593d", "Space")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: "secondary",
								className: "px-1.5 py-0 text-[9px]",
								children: translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.c361440dc0", "Beta")
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "truncate text-[11px] text-muted-foreground",
							children: analysis ? isScanning ? translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.3d8d47ce77", "{{value0}} · last result kept", { value0: progressLabel ?? "Scanning workspace sizes" }) : analysis.unavailableWorktreeCount > 0 ? translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.bef4dc0457", "{{value0}} reclaimable · {{value1}} unavailable", {
								value0: formatBytes(analysis.reclaimableBytes),
								value1: analysis.unavailableWorktreeCount
							}) : translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.bef4dc0457", "{{value0}} reclaimable · {{value1}} workspaces", {
								value0: formatBytes(analysis.reclaimableBytes),
								value1: analysis.scannedWorktreeCount
							}) : isScanning ? progressLabel ?? translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.39786e3b73", "Scanning workspace sizes.") : translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.0583c806ac", "Workspace disk usage is not scanned.")
						})]
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex shrink-0 items-center gap-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						variant: "outline",
						size: "xs",
						onClick: isScanning ? cancelScan : scan,
						disabled: progress?.state === "cancelling",
						className: "w-24",
						children: [isScanning ? progress?.state === "cancelling" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: "size-3" }), isScanning ? progress?.state === "cancelling" ? translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.5691353a21", "Stopping") : translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.2af2174d6d", "Cancel") : analysis ? translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.f5e1a84d79", "Refresh") : translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.0582df6d2e", "Scan")]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "ghost",
						size: "xs",
						onClick: onOpenFullPage,
						children: translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.6a5dc3c61a", "Review")
					})]
				})]
			}),
			analysis ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mt-2 grid grid-cols-3 gap-1 text-[10px] tabular-nums",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded border border-border/60 bg-background/40 px-2 py-1",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "text-muted-foreground",
							children: translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.f4d2651498", "Scanned")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "truncate font-medium text-foreground",
							children: formatBytes(analysis.totalSizeBytes)
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded border border-border/60 bg-background/40 px-2 py-1",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "text-muted-foreground",
							children: translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.9be86c46a0", "Freeable")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "truncate font-medium text-foreground",
							children: formatBytes(analysis.reclaimableBytes)
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded border border-border/60 bg-background/40 px-2 py-1",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "text-muted-foreground",
							children: translate("auto.components.status.bar.WorkspaceSpaceCompactPanel.a471aa9c24", "Updated")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "truncate font-medium text-foreground",
							children: getWorkspaceSpaceScanTimeLabel(analysis.scannedAt)
						})]
					})
				]
			}) : null,
			scanError ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mt-1.5 flex items-start gap-1.5 text-[11px] text-destructive",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-3 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "min-w-0 truncate",
					children: scanError
				})]
			}) : null
		]
	});
}
function isResourceSessionActivationKey(key) {
	return key === "Enter" || key === " ";
}
function navigateResourceSessionToTab(tabId, paneKey, deps) {
	deps.setOpen(false);
	for (const [worktreeId, tabs] of Object.entries(deps.tabsByWorktree)) if (tabs.some((tab) => tab.id === tabId)) {
		deps.activateAndRevealWorktree(worktreeId);
		break;
	}
	deps.setActiveView("terminal");
	const parsed = paneKey ? parsePaneKey(paneKey) : null;
	deps.activateTabAndFocusPane(tabId, parsed?.tabId === tabId ? parsed.leafId : null, {
		flashFocusedPane: true,
		scrollToBottomIfOutputSinceLastView: true
	});
}
var EMPTY_TABS_BY_WORKTREE = {};
var EMPTY_PTY_IDS_BY_TAB_ID = {};
var EMPTY_TERMINAL_LAYOUTS_BY_TAB_ID = {};
var EMPTY_DEFERRED_SSH_SESSION_IDS_BY_TAB_ID = {};
var EMPTY_RUNTIME_PANE_TITLES_BY_TAB_ID = {};
var EMPTY_BROWSER_TABS_BY_WORKTREE = {};
var EMPTY_REPOS = [];
var EMPTY_WORKTREES = [];
function getResourceUsageTabsByWorktree(state, open) {
	return open ? state.tabsByWorktree : EMPTY_TABS_BY_WORKTREE;
}
function getResourceUsagePtyIdsByTabId(state, open) {
	return open ? state.ptyIdsByTabId : EMPTY_PTY_IDS_BY_TAB_ID;
}
function getResourceUsageTerminalLayoutsByTabId(state, open) {
	return open ? state.terminalLayoutsByTabId : EMPTY_TERMINAL_LAYOUTS_BY_TAB_ID;
}
function getResourceUsageDeferredSshSessionIdsByTabId(state, open) {
	return open ? state.deferredSshSessionIdsByTabId : EMPTY_DEFERRED_SSH_SESSION_IDS_BY_TAB_ID;
}
function getResourceUsageRuntimePaneTitlesByTabId(state, open) {
	return open ? state.runtimePaneTitlesByTabId : EMPTY_RUNTIME_PANE_TITLES_BY_TAB_ID;
}
function getResourceUsageBrowserTabsByWorktree(state, open) {
	return open ? state.browserTabsByWorktree : EMPTY_BROWSER_TABS_BY_WORKTREE;
}
function getResourceUsageRepos(state, open) {
	return open ? state.repos : EMPTY_REPOS;
}
function getResourceUsageAllWorktrees(state, open) {
	return open ? getAllWorktreesFromState(state) : EMPTY_WORKTREES;
}
function resolveResourceUsageSpaceScanReady({ snapshot, open, activeView, scannedAt, scanning }) {
	if (snapshot.previousScanning && !scanning && scannedAt !== null && scannedAt !== snapshot.lastSeenScannedAt) return {
		ready: !open && activeView !== "space",
		previousScanning: scanning,
		lastSeenScannedAt: scannedAt
	};
	if (snapshot.ready && (open || activeView === "space")) return {
		ready: false,
		previousScanning: scanning,
		lastSeenScannedAt: scannedAt
	};
	if (snapshot.previousScanning !== scanning) return {
		...snapshot,
		previousScanning: scanning
	};
	return snapshot;
}
function formatTerminalSessionCount(count) {
	return count === 1 ? translate("auto.components.status.bar.resource.manager.terminal.copy.terminalSessionCount_one", "{{count}} terminal session", { count }) : translate("auto.components.status.bar.resource.manager.terminal.copy.terminalSessionCount_other", "{{count}} terminal sessions", { count });
}
function spaceScanReadyLabel() {
	return translate("auto.components.status.bar.resource.manager.terminal.copy.spaceScanReady", "Space scan ready");
}
function getResourceManagerTooltipLines(args) {
	const rawMemoryLabel = args.memoryLabel.trim();
	const lines = [{
		id: "summary",
		text: translate("auto.components.status.bar.resource.manager.terminal.copy.tooltipSummary", "Resource Manager - {{memory}} - {{sessions}}", {
			memory: rawMemoryLabel === "" || rawMemoryLabel === "-" || rawMemoryLabel === "—" ? translate("auto.components.status.bar.resource.manager.terminal.copy.memoryUnavailable", "memory unavailable") : rawMemoryLabel,
			sessions: formatTerminalSessionCount(args.sessionCount)
		}),
		emphasized: false
	}];
	if (args.spaceScanReady) lines.push({
		id: "space-scan",
		text: spaceScanReadyLabel(),
		emphasized: true
	});
	lines.push({
		id: "sessions-hint",
		text: args.sessionCount > 0 ? translate("auto.components.status.bar.resource.manager.terminal.copy.sessionsGroupedByWorkspace", "Terminal sessions are grouped by workspace.") : translate("auto.components.status.bar.resource.manager.terminal.copy.noTerminalSessions", "No terminal sessions yet."),
		emphasized: false
	});
	return lines;
}
function getResourceManagerAriaLabel(args) {
	const sessions = formatTerminalSessionCount(args.sessionCount);
	if (args.spaceScanReady) return translate("auto.components.status.bar.resource.manager.terminal.copy.ariaLabelWithSpaceScan", "Resource Manager, {{sessions}}, {{spaceScan}}", {
		sessions,
		spaceScan: spaceScanReadyLabel()
	});
	return translate("auto.components.status.bar.resource.manager.terminal.copy.ariaLabel", "Resource Manager, {{sessions}}", { sessions });
}
function getResourceMemoryMetricCopy(metric) {
	if (metric === "working-set") return {
		columnLabel: "WS",
		summaryLabel: "Σ WS",
		description: translate("auto.components.status.bar.resource.memory.metric.workingSetDescription", "Summed working set (WS). Shared pages can appear in more than one process.")
	};
	return {
		columnLabel: "RSS",
		summaryLabel: "Σ RSS",
		description: translate("auto.components.status.bar.resource.memory.metric.rssDescription", "Summed resident set size (RSS). Shared or aliased pages can appear in more than one process.")
	};
}
function requiresKillConfirmation(session) {
	return session.bound || !mayDestroyWithoutOwnerEvidence(session);
}
const EMPTY_DAEMON_SESSION_INVENTORY = {
	sessions: [],
	count: 0
};
function inventoryFromSessions(sessions) {
	return {
		sessions: sessions.slice(),
		count: sessions.length
	};
}
function removeSessionsFromInventory(inventory, sessionIds) {
	if (sessionIds.size === 0 || inventory.sessions.length === 0) return inventory;
	const sessions = inventory.sessions.filter((session) => !sessionIds.has(session.id));
	if (sessions.length === inventory.sessions.length) return inventory;
	return {
		sessions,
		count: sessions.length
	};
}
function removeSessionFromInventory(inventory, sessionId) {
	return removeSessionsFromInventory(inventory, new Set([sessionId]));
}
function useResourceSessionInventory(ready) {
	const mountedRef = useMountedRef();
	const refreshGenerationRef = (0, import_react.useRef)(0);
	const lifecycleRevisionRef = (0, import_react.useRef)(0);
	const removedAtRevisionRef = (0, import_react.useRef)(/* @__PURE__ */ new Map());
	const knownSessionIdsRef = (0, import_react.useRef)(/* @__PURE__ */ new Set());
	const [storedState, setStoredState] = (0, import_react.useState)(() => ({
		ready,
		sessionInventory: EMPTY_DAEMON_SESSION_INVENTORY,
		sessionsError: false
	}));
	const state = storedState.ready === ready ? storedState : {
		ready,
		sessionInventory: EMPTY_DAEMON_SESSION_INVENTORY,
		sessionsError: false
	};
	if (state !== storedState) setStoredState(state);
	const refreshSessions = (0, import_react.useCallback)(async () => {
		if (!ready) return;
		const generation = ++refreshGenerationRef.current;
		const lifecycleRevision = lifecycleRevisionRef.current;
		try {
			const sessions = await window.api.pty.listSessions();
			if (!mountedRef.current || generation !== refreshGenerationRef.current) return;
			const currentRemovedAtRevision = removedAtRevisionRef.current;
			const liveSessions = sessions.filter(({ id }) => (currentRemovedAtRevision.get(id) ?? 0) <= lifecycleRevision);
			for (const [id, removedAtRevision] of currentRemovedAtRevision) if (removedAtRevision <= lifecycleRevision) currentRemovedAtRevision.delete(id);
			knownSessionIdsRef.current = new Set(liveSessions.map(({ id }) => id));
			setStoredState({
				ready: true,
				sessionInventory: inventoryFromSessions(liveSessions),
				sessionsError: false
			});
		} catch {
			if (mountedRef.current && generation === refreshGenerationRef.current) setStoredState((current) => ({
				...current,
				sessionsError: true
			}));
		}
	}, [mountedRef, ready]);
	const clearSessionsError = (0, import_react.useCallback)(() => {
		setStoredState((current) => ({
			...current,
			sessionsError: false
		}));
	}, []);
	const removeSession = (0, import_react.useCallback)((sessionId) => {
		const lifecycleRevision = ++lifecycleRevisionRef.current;
		removedAtRevisionRef.current.set(sessionId, lifecycleRevision);
		knownSessionIdsRef.current.delete(sessionId);
		setStoredState((current) => ({
			...current,
			sessionInventory: removeSessionFromInventory(current.sessionInventory, sessionId)
		}));
	}, []);
	const removeSessions = (0, import_react.useCallback)((sessionIds) => {
		const lifecycleRevision = ++lifecycleRevisionRef.current;
		for (const sessionId of sessionIds) {
			removedAtRevisionRef.current.set(sessionId, lifecycleRevision);
			knownSessionIdsRef.current.delete(sessionId);
		}
		setStoredState((current) => ({
			...current,
			sessionInventory: removeSessionsFromInventory(current.sessionInventory, sessionIds)
		}));
	}, []);
	(0, import_react.useEffect)(() => {
		refreshGenerationRef.current += 1;
		if (!ready) {
			removedAtRevisionRef.current.clear();
			knownSessionIdsRef.current.clear();
			return;
		}
		refreshSessions();
	}, [ready, refreshSessions]);
	(0, import_react.useEffect)(() => {
		if (!ready) return;
		return subscribeDaemonSessionInventoryInvalidated(() => {
			refreshSessions();
		});
	}, [ready, refreshSessions]);
	(0, import_react.useEffect)(() => {
		if (!ready) return;
		let disposed = false;
		let refreshTimer = null;
		let lifecycleRefresh = null;
		const pendingSpawnIds = /* @__PURE__ */ new Set();
		const scheduleLifecycleRefresh = () => {
			if (disposed || refreshTimer !== null || lifecycleRefresh !== null) return;
			refreshTimer = window.setTimeout(() => {
				refreshTimer = null;
				if (pendingSpawnIds.size === 0) return;
				pendingSpawnIds.clear();
				const refresh = refreshSessions();
				lifecycleRefresh = refresh;
				refresh.finally(() => {
					if (disposed || lifecycleRefresh !== refresh) return;
					lifecycleRefresh = null;
					for (const id of pendingSpawnIds) if (knownSessionIdsRef.current.has(id)) pendingSpawnIds.delete(id);
					scheduleLifecycleRefresh();
				});
			}, 0);
		};
		const unsubscribeSpawned = window.api.pty.onSpawned(({ id }) => {
			if (knownSessionIdsRef.current.has(id)) return;
			pendingSpawnIds.add(id);
			scheduleLifecycleRefresh();
		});
		const unsubscribeExit = window.api.pty.onExit(({ id }) => {
			pendingSpawnIds.delete(id);
			if (pendingSpawnIds.size === 0 && refreshTimer !== null) {
				window.clearTimeout(refreshTimer);
				refreshTimer = null;
			}
			removeSession(id);
		});
		return () => {
			disposed = true;
			pendingSpawnIds.clear();
			if (refreshTimer !== null) window.clearTimeout(refreshTimer);
			unsubscribeSpawned();
			unsubscribeExit();
		};
	}, [
		ready,
		refreshSessions,
		removeSession
	]);
	return {
		sessionInventory: state.sessionInventory,
		sessionsError: state.sessionsError,
		refreshSessions,
		clearSessionsError,
		removeSession,
		removeSessions
	};
}
var POLL_MS = 2e3;
var METRIC_COLUMNS_CLS = "flex items-center shrink-0 tabular-nums";
var CPU_COLUMN_CLS = "w-12 text-right";
var MEM_COLUMN_CLS = "w-16 text-right";
var ROW_TRAILING_GUTTER_CLS = "w-5 shrink-0 flex items-center justify-end";
function formatMemory(bytes) {
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
	if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}
function formatCpu(percent) {
	return `${percent.toFixed(1)}%`;
}
function formatMetricCpu(value) {
	return value === null ? "—" : formatCpu(value);
}
function formatMetricMemory(value) {
	return value === null ? "—" : formatMemory(value);
}
function SparklineImpl({ samples, width = 48, height = 14 }) {
	const points = (0, import_react.useMemo)(() => {
		const safe = Array.isArray(samples) ? samples : [];
		if (safe.length < 2) {
			const midY = (height / 2).toFixed(1);
			return `0,${midY} ${width},${midY}`;
		}
		let min = safe[0];
		let max = safe[0];
		for (const v of safe) {
			if (v < min) min = v;
			if (v > max) max = v;
		}
		const range = max - min || 1;
		const stepX = width / (safe.length - 1);
		const out = [];
		for (let i = 0; i < safe.length; i++) {
			const x = (i * stepX).toFixed(1);
			const y = (height - (safe[i] - min) / range * height).toFixed(1);
			out.push(`${x},${y}`);
		}
		return out.join(" ");
	}, [
		samples,
		width,
		height
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		width,
		height,
		viewBox: `0 0 ${width} ${height}`,
		"aria-hidden": true,
		preserveAspectRatio: "none",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polyline", {
			points,
			fill: "none",
			strokeWidth: 1,
			strokeLinecap: "round",
			strokeLinejoin: "round",
			className: "stroke-muted-foreground/70"
		})
	});
}
var Sparkline = (0, import_react.memo)(SparklineImpl, (a, b) => {
	if (a.width !== b.width || a.height !== b.height) return false;
	const sa = Array.isArray(a.samples) ? a.samples : [];
	const sb = Array.isArray(b.samples) ? b.samples : [];
	if (sa === sb) return true;
	if (sa.length !== sb.length) return false;
	for (let i = 0; i < sa.length; i++) if (sa[i] !== sb[i]) return false;
	return true;
});
function MetricPair({ cpu, memory, size = "base" }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn(METRIC_COLUMNS_CLS, size === "small" ? "text-[11px]" : "text-xs", cpu === null && memory === null ? "text-muted-foreground/50" : "text-muted-foreground"),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: CPU_COLUMN_CLS,
			children: formatMetricCpu(cpu)
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: MEM_COLUMN_CLS,
			children: formatMetricMemory(memory)
		})]
	});
}
function AppSubRow({ label, values }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "px-3 py-1.5 pl-6 flex items-center justify-between gap-2",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "text-[11px] text-muted-foreground truncate",
			children: label
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center gap-2 shrink-0",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MetricPair, {
				cpu: values.cpu,
				memory: values.memory,
				size: "small"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: ROW_TRAILING_GUTTER_CLS,
				"aria-hidden": true
			})]
		})]
	});
}
function AppSection({ app, isCollapsed, onToggle }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "border-t border-border/50",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				onClick: onToggle,
				className: "pl-2 py-2 pr-0.5 transition-colors hover:bg-muted/50",
				"aria-label": isCollapsed ? translate("auto.components.status.bar.ResourceUsageStatusSegment.e419d27083", "Expand Orca") : translate("auto.components.status.bar.ResourceUsageStatusSegment.53dd5560ae", "Collapse Orca"),
				"aria-expanded": !isCollapsed,
				children: isCollapsed ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "h-3 w-3 text-muted-foreground" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: "h-3 w-3 text-muted-foreground" })
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex-1 min-w-0 py-2 pr-3 flex items-center justify-between",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-[11px] font-semibold uppercase tracking-wide truncate text-muted-foreground",
					children: translate("auto.components.status.bar.ResourceUsageStatusSegment.288a4dd177", "Orca")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-2 shrink-0",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkline, { samples: app.history }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MetricPair, {
							cpu: app.cpu,
							memory: app.memory
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: ROW_TRAILING_GUTTER_CLS,
							"aria-hidden": true
						})
					]
				})]
			})]
		}), !isCollapsed && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "border-t border-border/30",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AppSubRow, {
					label: translate("auto.components.status.bar.ResourceUsageStatusSegment.81cd37af99", "Main"),
					values: app.main
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AppSubRow, {
					label: translate("auto.components.status.bar.ResourceUsageStatusSegment.d406915b78", "Renderer"),
					values: app.renderer
				}),
				(app.other.cpu > 0 || app.other.memory > 0) && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AppSubRow, {
					label: translate("auto.components.status.bar.ResourceUsageStatusSegment.0f9e50eb07", "Other"),
					values: app.other
				})
			]
		})]
	});
}
function compareMetricDesc(a, b) {
	if (a === null && b === null) return 0;
	if (a === null) return 1;
	if (b === null) return -1;
	return b - a;
}
function sortWorktrees(list, sort) {
	const copy = [...list];
	if (sort === "memory") copy.sort((a, b) => compareMetricDesc(a.memory, b.memory));
	else if (sort === "cpu") copy.sort((a, b) => compareMetricDesc(a.cpu, b.cpu));
	else copy.sort((a, b) => a.worktreeName.localeCompare(b.worktreeName));
	return copy;
}
function sortProjectGroups(groups, sort) {
	const copy = [...groups];
	if (sort === "memory") copy.sort((a, b) => compareMetricDesc(a.memory, b.memory));
	else if (sort === "cpu") copy.sort((a, b) => compareMetricDesc(a.cpu, b.cpu));
	else copy.sort((a, b) => a.repoName.localeCompare(b.repoName));
	return copy;
}
function SessionRow({ session, worktreeId, onNavigate, onKill }) {
	const clickable = session.tabId !== null && session.bound;
	const handleClick = () => {
		if (clickable && session.tabId) onNavigate(session.tabId, session.paneKey);
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("group/sessrow flex items-center gap-2 pl-10 pr-3 py-1.5", clickable && "cursor-pointer hover:bg-accent/40"),
		onClick: clickable ? handleClick : void 0,
		role: clickable ? "button" : void 0,
		tabIndex: clickable ? 0 : -1,
		onKeyDown: clickable ? (e) => {
			if (isResourceSessionActivationKey(e.key)) {
				e.preventDefault();
				handleClick();
			}
		} : void 0,
		"data-worktree-id": worktreeId,
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: cn("size-1.5 shrink-0 rounded-full", session.bound ? "bg-emerald-500" : "bg-muted-foreground/40") }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "text-[11px] text-muted-foreground truncate min-w-0 flex-1",
				children: session.label
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MetricPair, {
				cpu: session.cpu,
				memory: session.memory,
				size: "small"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: ROW_TRAILING_GUTTER_CLS,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					onClick: (e) => {
						e.stopPropagation();
						onKill(session);
					},
					className: cn("rounded p-0.5 text-muted-foreground transition-opacity hover:bg-destructive/10 hover:text-destructive", session.bound && "can-hover:opacity-0 group-hover/sessrow:opacity-100 group-focus-within/sessrow:opacity-100 focus-visible:opacity-100"),
					"aria-label": translate("auto.components.status.bar.ResourceUsageStatusSegment.fa6d36758d", "Kill session {{value0}}", { value0: session.sessionId }),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3" })
				})
			})
		]
	});
}
function BrowserRow({ browser }) {
	const label = browser.title?.trim() || browser.label?.trim() || browser.url;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-2 pl-10 pr-3 py-1.5",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Globe, {
				className: "size-3 shrink-0 text-muted-foreground",
				"aria-hidden": true
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-0 flex-1 truncate text-[11px] text-muted-foreground",
				children: label
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MetricPair, {
				cpu: null,
				memory: null,
				size: "small"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: ROW_TRAILING_GUTTER_CLS,
				"aria-hidden": true
			})
		]
	});
}
function WorktreeRow({ worktree, storeRecord, activeWorktreeId, isCollapsed, onToggle, onNavigate, onDelete, onKillSession, navigateToTab }) {
	const hasResources = worktree.sessions.length > 0 || worktree.browsers.length > 0;
	const isSynthetic = worktree.worktreeId === "__orphan__" || worktree.repoId === "__unattributed__";
	const isNavigable = !isSynthetic;
	const showWorktreeActions = !isSynthetic && storeRecord !== null && worktree.worktreeId !== activeWorktreeId;
	const isMainWorktree = storeRecord?.isMainWorktree ?? false;
	const rowLabel = storeRecord?.displayName?.trim() || worktree.worktreeName;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "border-b border-border/20 last:border-b-0",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "group/wtrow flex items-center ml-2 transition-colors hover:bg-muted/60",
				children: [
					hasResources ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						onClick: onToggle,
						className: "pl-2 py-2 pr-0.5 shrink-0",
						"aria-label": isCollapsed ? translate("auto.components.status.bar.ResourceUsageStatusSegment.c4a8968bdd", "Expand workspace") : translate("auto.components.status.bar.ResourceUsageStatusSegment.bbcd9b7b85", "Collapse workspace"),
						children: isCollapsed ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "h-3 w-3 text-muted-foreground" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: "h-3 w-3 text-muted-foreground" })
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "pl-2 py-2 pr-0.5 shrink-0 w-[calc(0.5rem+0.75rem+0.125rem)]",
						"aria-hidden": true
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						onClick: onNavigate,
						"aria-label": translate("auto.components.status.bar.ResourceUsageStatusSegment.d659d71d2d", "Resume workspace {{value0}}", { value0: rowLabel }),
						className: "flex-1 min-w-0 py-2 pr-2 pl-1 text-left flex items-center gap-1.5",
						disabled: !isNavigable,
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-xs font-medium truncate",
							children: rowLabel
						}), worktree.isRemote && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "shrink-0 text-[9px] uppercase tracking-wide text-muted-foreground/70",
							children: translate("auto.components.status.bar.ResourceUsageStatusSegment.21cacb16d1", "· remote")
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-2 shrink-0 pr-3",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "relative",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: cn("block transition-opacity", showWorktreeActions && "group-hover/wtrow:opacity-0 group-hover/wtrow:pointer-events-none group-focus-within/wtrow:opacity-0 group-focus-within/wtrow:pointer-events-none [@media(hover:none)]:opacity-0 [@media(hover:none)]:pointer-events-none"),
									"aria-hidden": showWorktreeActions ? void 0 : true,
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkline, { samples: worktree.history })
								}), showWorktreeActions && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "absolute inset-0 flex items-center justify-end gap-0.5 can-hover:opacity-0 can-hover:pointer-events-none transition-opacity group-hover/wtrow:opacity-100 group-hover/wtrow:pointer-events-auto group-focus-within/wtrow:opacity-100 group-focus-within/wtrow:pointer-events-auto",
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, {
										delayDuration: 300,
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
											asChild: true,
											children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
												type: "button",
												onClick: onDelete,
												disabled: isMainWorktree,
												"aria-label": translate("auto.components.status.bar.ResourceUsageStatusSegment.16bc3c998a", "Delete workspace {{value0}}", { value0: rowLabel }),
												className: cn("p-0.5 rounded text-muted-foreground transition-colors", isMainWorktree ? "opacity-40 cursor-not-allowed" : "hover:bg-destructive/10 hover:text-destructive"),
												children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3" })
											})
										}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
											side: "top",
											sideOffset: 4,
											className: "z-[70] max-w-[200px] text-pretty",
											children: isMainWorktree ? translate("auto.components.status.bar.ResourceUsageStatusSegment.946724a70a", "The main workspace cannot be deleted.") : translate("auto.components.status.bar.ResourceUsageStatusSegment.a82253b458", "Delete workspace.")
										})]
									})
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MetricPair, {
								cpu: worktree.cpu,
								memory: worktree.memory
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: ROW_TRAILING_GUTTER_CLS,
								"aria-hidden": true
							})
						]
					})
				]
			}),
			!isCollapsed && worktree.sessions.map((session) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SessionRow, {
				session,
				worktreeId: worktree.worktreeId,
				onNavigate: navigateToTab,
				onKill: onKillSession
			}, session.sessionId)),
			!isCollapsed && worktree.browsers.map((browser) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(BrowserRow, { browser }, browser.id))
		]
	});
}
function ResourceTree({ repos, sortOption, collapsedRepos, toggleRepo, collapsedWorktrees, activeWorktreeId, toggleWorktree, navigateToWorktree, navigateToTab, onDelete, onKillSession }) {
	const worktreeById = useWorktreeMap();
	const sortedRepos = (0, import_react.useMemo)(() => {
		return sortProjectGroups(repos, sortOption).map((repo) => ({
			...repo,
			worktrees: sortWorktrees(repo.worktrees, sortOption)
		}));
	}, [repos, sortOption]);
	const renderWorktree = (wt) => {
		return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeRow, {
			worktree: wt,
			storeRecord: worktreeById.get(wt.worktreeId) ?? null,
			activeWorktreeId,
			isCollapsed: collapsedWorktrees.has(wt.worktreeId),
			onToggle: () => toggleWorktree(wt.worktreeId),
			onNavigate: () => navigateToWorktree(wt.worktreeId),
			onDelete: () => onDelete(wt.worktreeId),
			onKillSession,
			navigateToTab
		}, wt.worktreeId);
	};
	if (sortedRepos.length === 1) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_jsx_runtime.Fragment, { children: sortedRepos[0].worktrees.map(renderWorktree) });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_jsx_runtime.Fragment, { children: sortedRepos.map((group) => {
		const repoCollapsed = collapsedRepos.has(group.repoId);
		return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "border-b border-border/50 last:border-b-0",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					onClick: () => toggleRepo(group.repoId),
					className: "pl-2 py-2 pr-0.5 transition-colors hover:bg-muted/50",
					"aria-label": repoCollapsed ? translate("auto.components.status.bar.ResourceUsageStatusSegment.b12e31dfcb", "Expand repo") : translate("auto.components.status.bar.ResourceUsageStatusSegment.73a3fd68a9", "Collapse repo"),
					children: repoCollapsed ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "h-3 w-3 text-muted-foreground" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: "h-3 w-3 text-muted-foreground" })
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex-1 min-w-0 py-2 pr-3 flex items-center justify-between gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "flex items-center gap-1.5 min-w-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[11px] font-semibold uppercase tracking-wide truncate text-muted-foreground",
							children: group.repoName
						}), group.hasRemoteChildren && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "shrink-0 text-[9px] uppercase tracking-wide text-muted-foreground/70",
							children: translate("auto.components.status.bar.ResourceUsageStatusSegment.21cacb16d1", "· remote")
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-2 shrink-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MetricPair, {
							cpu: group.cpu,
							memory: group.memory
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: ROW_TRAILING_GUTTER_CLS,
							"aria-hidden": true
						})]
					})]
				})]
			}), !repoCollapsed && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "border-t border-border/30",
				children: group.worktrees.map(renderWorktree)
			})]
		}, group.repoId);
	}) });
}
function ResourceUsageStatusSegment({ iconOnly }) {
	const snapshot = useAppStore((s) => s.memorySnapshot);
	const memorySnapshotError = useAppStore((s) => s.memorySnapshotError);
	const fetchSnapshot = useAppStore((s) => s.fetchMemorySnapshot);
	const workspaceSessionReady = useAppStore((s) => s.workspaceSessionReady);
	const setActiveView = useAppStore((s) => s.setActiveView);
	const openModal = useAppStore((s) => s.openModal);
	const openSpacePage = useAppStore((s) => s.openSpacePage);
	const recordFeatureInteraction = useAppStore((s) => s.recordFeatureInteraction);
	const activeView = useAppStore((s) => s.activeView);
	const activeWorktreeId = useAppStore((s) => s.activeWorktreeId);
	const workspaceSpaceScannedAt = useAppStore((s) => s.workspaceSpaceAnalysis?.scannedAt ?? null);
	const workspaceSpaceScanning = useAppStore((s) => s.workspaceSpaceScanning);
	const [open, setOpen] = (0, import_react.useState)(false);
	const [sortOption, setSortOption] = (0, import_react.useState)("memory");
	const [collapsedRepos, setCollapsedRepos] = (0, import_react.useState)(/* @__PURE__ */ new Set());
	const [collapsedWorktrees, setCollapsedWorktrees] = (0, import_react.useState)(/* @__PURE__ */ new Set());
	const [appCollapsed, setAppCollapsed] = (0, import_react.useState)(true);
	const { sessionInventory, sessionsError, refreshSessions, clearSessionsError, removeSession, removeSessions } = useResourceSessionInventory(workspaceSessionReady);
	const sessions = sessionInventory.sessions;
	const [killConfirm, setKillConfirm] = (0, import_react.useState)(null);
	const [killing, setKilling] = (0, import_react.useState)(false);
	const [spaceScanSnapshot, setSpaceScanSnapshot] = (0, import_react.useState)(() => ({
		ready: false,
		previousScanning: workspaceSpaceScanning,
		lastSeenScannedAt: workspaceSpaceScannedAt
	}));
	const runtimePaneTitlesByTabId = useAppStore((s) => getResourceUsageRuntimePaneTitlesByTabId(s, open));
	const repos = useAppStore((s) => getResourceUsageRepos(s, open));
	const allWorktrees = useAppStore((s) => getResourceUsageAllWorktrees(s, open));
	const tabsByWorktree = useAppStore((s) => getResourceUsageTabsByWorktree(s, open));
	const browserTabsByWorktree = useAppStore((s) => getResourceUsageBrowserTabsByWorktree(s, open));
	const ptyIdsByTabId = useAppStore((s) => getResourceUsagePtyIdsByTabId(s, open));
	const terminalLayoutsByTabId = useAppStore((s) => getResourceUsageTerminalLayoutsByTabId(s, open));
	const deferredSshSessionIdsByTabId = useAppStore((s) => getResourceUsageDeferredSshSessionIdsByTabId(s, open));
	const resourceSnapshot = snapshot;
	const resourceSessionBindings = (0, import_react.useMemo)(() => ({
		ptyIdsByTabId,
		tabsByWorktree,
		terminalLayoutsByTabId,
		deferredSshSessionIdsByTabId,
		workspaceSessionReady
	}), [
		ptyIdsByTabId,
		tabsByWorktree,
		terminalLayoutsByTabId,
		deferredSshSessionIdsByTabId,
		workspaceSessionReady
	]);
	const popoverBodyRef = (0, import_react.useRef)(null);
	const popoverBodyFocusFrameRef = (0, import_react.useRef)(null);
	const mountedRef = useMountedRef();
	const cancelPopoverBodyFocusFrame = (0, import_react.useCallback)(() => {
		if (popoverBodyFocusFrameRef.current === null) return;
		cancelAnimationFrame(popoverBodyFocusFrameRef.current);
		popoverBodyFocusFrameRef.current = null;
	}, []);
	const setPopoverBodyNode = (0, import_react.useCallback)((node) => {
		if (!node) cancelPopoverBodyFocusFrame();
		popoverBodyRef.current = node;
	}, [cancelPopoverBodyFocusFrame]);
	const daemonActions = useDaemonActions({ onRestartSettled: () => {
		clearSessionsError();
		fetchSnapshot();
		refreshSessions();
	} });
	const nextSpaceScanSnapshot = resolveResourceUsageSpaceScanReady({
		snapshot: spaceScanSnapshot,
		open,
		activeView,
		scannedAt: workspaceSpaceScannedAt,
		scanning: workspaceSpaceScanning
	});
	if (nextSpaceScanSnapshot.ready !== spaceScanSnapshot.ready || nextSpaceScanSnapshot.previousScanning !== spaceScanSnapshot.previousScanning || nextSpaceScanSnapshot.lastSeenScannedAt !== spaceScanSnapshot.lastSeenScannedAt) setSpaceScanSnapshot(nextSpaceScanSnapshot);
	const spaceScanReady = nextSpaceScanSnapshot.ready;
	(0, import_react.useEffect)(() => {
		if (workspaceSessionReady) fetchSnapshot();
	}, [workspaceSessionReady, fetchSnapshot]);
	(0, import_react.useEffect)(() => {
		if (!open) return;
		fetchSnapshot();
		refreshSessions();
		const memTimer = window.setInterval(() => {
			fetchSnapshot();
		}, POLL_MS);
		return () => {
			window.clearInterval(memTimer);
		};
	}, [
		open,
		fetchSnapshot,
		refreshSessions
	]);
	(0, import_react.useEffect)(() => {
		if (!open) clearSessionsError();
	}, [open, clearSessionsError]);
	const repoDisplayNameById = (0, import_react.useMemo)(() => {
		const map = /* @__PURE__ */ new Map();
		for (const repo of repos) {
			const display = repo.displayName?.trim();
			if (display) map.set(repo.id, display);
		}
		return map;
	}, [repos]);
	const repoConnectionIdById = (0, import_react.useMemo)(() => {
		const map = /* @__PURE__ */ new Map();
		for (const repo of repos) map.set(repo.id, repo.connectionId ?? null);
		return map;
	}, [repos]);
	const repoRuntimeScopedById = (0, import_react.useMemo)(() => {
		const map = /* @__PURE__ */ new Map();
		for (const repo of repos) {
			const parsed = parseExecutionHostId(getRepoExecutionHostId(repo));
			map.set(repo.id, parsed?.kind === "runtime");
		}
		return map;
	}, [repos]);
	const repoById = (0, import_react.useMemo)(() => new Map(repos.map((repo) => [repo.id, repo])), [repos]);
	const worktreeById = (0, import_react.useMemo)(() => new Map(allWorktrees.map((worktree) => [worktree.id, worktree])), [allWorktrees]);
	const oldWorkspaceCount = (0, import_react.useMemo)(() => countEstimatedInactiveWorkspaces(allWorktrees, repoById, Date.now()), [allWorktrees, repoById]);
	const unifiedRepos = (0, import_react.useMemo)(() => open ? mergeSnapshotAndSessions(resourceSnapshot, sessions, {
		...resourceSessionBindings,
		runtimePaneTitlesByTabId,
		repoDisplayNameById,
		repoConnectionIdById,
		repoRuntimeScopedById,
		browserTabsByWorktree,
		worktreeById
	}) : [], [
		open,
		resourceSnapshot,
		sessions,
		resourceSessionBindings,
		runtimePaneTitlesByTabId,
		repoDisplayNameById,
		repoConnectionIdById,
		repoRuntimeScopedById,
		browserTabsByWorktree,
		worktreeById
	]);
	const orphanCount = (0, import_react.useMemo)(() => {
		if (!open || !workspaceSessionReady) return 0;
		return countUnboundDaemonSessions(sessions, resourceSessionBindings);
	}, [
		open,
		sessions,
		resourceSessionBindings,
		workspaceSessionReady
	]);
	const triggerSessionCount = sessionInventory.count;
	const memoryMetricCopy = getResourceMemoryMetricCopy(resourceSnapshot?.processMemoryMetric ?? "rss");
	const { totalMemory, totalCpu, memBadgeLabel } = (0, import_react.useMemo)(() => {
		const memory = resourceSnapshot?.totalMemory ?? 0;
		return {
			totalMemory: memory,
			totalCpu: resourceSnapshot?.totalCpu ?? 0,
			memBadgeLabel: resourceSnapshot ? formatMemory(memory) : "—"
		};
	}, [resourceSnapshot]);
	const daemonUnreachable = sessionsError && (memorySnapshotError !== null || snapshot === null);
	const sessionsOnlyError = sessionsError && memorySnapshotError === null;
	const resourceManagerTooltipLines = getResourceManagerTooltipLines({
		memoryLabel: resourceSnapshot ? `${memBadgeLabel} · ${memoryMetricCopy.summaryLabel}` : memBadgeLabel,
		sessionCount: triggerSessionCount,
		spaceScanReady
	});
	const resourceManagerAriaLabel = getResourceManagerAriaLabel({
		sessionCount: triggerSessionCount,
		spaceScanReady
	});
	const toggleRepo = (0, import_react.useCallback)((repoId) => {
		setCollapsedRepos((prev) => {
			const next = new Set(prev);
			if (next.has(repoId)) next.delete(repoId);
			else next.add(repoId);
			return next;
		});
	}, []);
	const toggleWorktree = (0, import_react.useCallback)((worktreeId) => {
		setCollapsedWorktrees((prev) => {
			const next = new Set(prev);
			if (next.has(worktreeId)) next.delete(worktreeId);
			else next.add(worktreeId);
			return next;
		});
	}, []);
	const navigateToWorktree = (0, import_react.useCallback)((worktreeId) => {
		if (worktreeId === "__orphan__" || worktreeId.startsWith(`__unattributed__::`)) return;
		activateAndRevealWorktree(worktreeId);
	}, []);
	const navigateToTab = (0, import_react.useCallback)((tabId, paneKey) => {
		navigateResourceSessionToTab(tabId, paneKey, {
			tabsByWorktree,
			setOpen,
			setActiveView,
			activateAndRevealWorktree,
			activateTabAndFocusPane
		});
	}, [tabsByWorktree, setActiveView]);
	const deleteWorktree = (0, import_react.useCallback)((worktreeId) => {
		setOpen(false);
		runWorktreeDelete(worktreeId);
	}, []);
	const handleOpenWorkspaceCleanup = (0, import_react.useCallback)(() => {
		setOpen(false);
		queueMicrotask(() => openModal("workspace-cleanup"));
	}, [openModal]);
	const handleKillSession = (0, import_react.useCallback)((session) => {
		if (!requiresKillConfirmation(session)) {
			removeSession(session.sessionId);
			(async () => {
				try {
					await window.api.pty.kill(session.sessionId);
				} catch {}
				await refreshSessions();
			})();
			return;
		}
		setKillConfirm(session);
	}, [refreshSessions, removeSession]);
	const handleKillOrphans = (0, import_react.useCallback)(async () => {
		if (!workspaceSessionReady) return;
		const orphans = selectUnboundDaemonSessions(sessions, resourceSessionBindings);
		if (orphans.length === 0) return;
		removeSessions(new Set(orphans.map((s) => s.id)));
		await Promise.allSettled(orphans.map((s) => window.api.pty.kill(s.id)));
		refreshSessions();
	}, [
		sessions,
		resourceSessionBindings,
		workspaceSessionReady,
		refreshSessions,
		removeSessions
	]);
	const runKillConfirmed = (0, import_react.useCallback)(async () => {
		if (!killConfirm) return;
		const target = killConfirm;
		setKilling(true);
		removeSession(target.sessionId);
		try {
			await window.api.pty.kill(target.sessionId);
		} catch {} finally {
			if (mountedRef.current) {
				setKilling(false);
				setKillConfirm(null);
				cancelPopoverBodyFocusFrame();
				if (popoverBodyRef.current) popoverBodyFocusFrameRef.current = requestAnimationFrame(() => {
					popoverBodyFocusFrameRef.current = null;
					popoverBodyRef.current?.focus();
				});
				refreshSessions();
			}
		}
	}, [
		cancelPopoverBodyFocusFrame,
		killConfirm,
		mountedRef,
		refreshSessions,
		removeSession
	]);
	const openSpaceResults = (0, import_react.useCallback)(() => {
		setOpen(false);
		openSpacePage();
	}, [openSpacePage]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
		open,
		onOpenChange: (nextOpen) => {
			if (nextOpen) recordFeatureInteraction("resource-manager");
			setOpen(nextOpen);
		},
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, {
				delayDuration: 150,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							...STATUS_BAR_CONTEXT_MENU_EXEMPT_PROPS,
							className: "relative inline-flex items-center gap-1.5 cursor-pointer rounded px-1 py-0.5 hover:bg-accent/70",
							"aria-label": daemonUnreachable ? translate("auto.components.status.bar.ResourceUsageStatusSegment.59f178fe11", "{{value0}}, daemon unreachable", { value0: resourceManagerAriaLabel }) : resourceManagerAriaLabel,
							children: [
								spaceScanReady ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "absolute -right-0.5 -top-0.5 size-1.5 rounded-full bg-primary",
									"aria-hidden": "true"
								}) : null,
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MemoryStick, { className: "size-3 text-muted-foreground" }),
								!iconOnly && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "text-[11px] font-medium tabular-nums text-muted-foreground",
										children: memBadgeLabel
									}),
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "text-muted-foreground/50",
										children: "·"
									}),
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Terminal, { className: "size-3 text-muted-foreground" }),
									/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
										className: "text-[11px] tabular-nums text-muted-foreground",
										children: [triggerSessionCount, orphanCount > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
											className: "text-yellow-500 ml-0.5",
											children: [
												"(",
												orphanCount,
												")"
											]
										})]
									})
								] }),
								iconOnly && triggerSessionCount > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "text-[11px] tabular-nums text-muted-foreground",
									children: triggerSessionCount
								}),
								daemonUnreachable && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, {
									className: "size-3 text-yellow-500",
									"aria-label": translate("auto.components.status.bar.ResourceUsageStatusSegment.ca95d077db", "Daemon unreachable")
								})
							]
						})
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
					side: "top",
					sideOffset: 6,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "space-y-0.5",
						children: resourceManagerTooltipLines.map((line) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: line.emphasized ? "text-primary" : "",
							children: line.text
						}, line.id))
					})
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(PopoverContent, {
				side: "top",
				align: "end",
				sideOffset: 8,
				...STATUS_BAR_CONTEXT_MENU_EXEMPT_PROPS,
				className: "w-[26rem] max-w-[calc(100vw-2rem)] p-0",
				onOpenAutoFocus: (event) => event.preventDefault(),
				onFocusOutside: (event) => event.preventDefault(),
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center justify-between gap-2 border-b border-border px-3 py-1.5",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex min-w-0 items-center gap-1.5 text-[11px] font-medium text-foreground",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MemoryStick, { className: "size-3 shrink-0 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "truncate",
								children: translate("auto.components.status.bar.StatusBar.d1e1a7a6bf", "Resource Manager")
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center gap-0.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, {
								delayDuration: 200,
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
									asChild: true,
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
										type: "button",
										onClick: () => daemonActions.setPending("restart"),
										disabled: daemonActions.isBusy,
										"aria-label": translate("auto.components.status.bar.ResourceUsageStatusSegment.c9382662bb", "Restart daemon"),
										className: "inline-flex size-6 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-40",
										children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RotateCw, { className: "size-3" })
									})
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
									side: "top",
									sideOffset: 6,
									children: translate("auto.components.status.bar.ResourceUsageStatusSegment.c9382662bb", "Restart daemon")
								})]
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, {
								delayDuration: 200,
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
									asChild: true,
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
										type: "button",
										onClick: () => daemonActions.setPending("killAll"),
										disabled: daemonActions.isBusy,
										"aria-label": translate("auto.components.status.bar.ResourceUsageStatusSegment.bd19fd7a59", "Kill all sessions"),
										className: "inline-flex size-6 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive disabled:opacity-40",
										children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3" })
									})
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
									side: "top",
									sideOffset: 6,
									children: translate("auto.components.status.bar.ResourceUsageStatusSegment.bd19fd7a59", "Kill all sessions")
								})]
							})]
						})]
					}),
					daemonUnreachable && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-start gap-2 border-b border-border bg-yellow-500/10 px-3 py-2 text-[11px] text-foreground",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-3 shrink-0 text-yellow-500" }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "font-medium",
									children: translate("auto.components.status.bar.ResourceUsageStatusSegment.f8e0d794b4", "Daemon is not responding")
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-muted-foreground",
									children: translate("auto.components.status.bar.ResourceUsageStatusSegment.f85af9cda6", "Resource snapshots and terminal sessions are unavailable.")
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								variant: "outline",
								size: "sm",
								className: "shrink-0",
								onClick: () => daemonActions.setPending("restart"),
								disabled: daemonActions.isBusy,
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RotateCw, { className: "mr-1 size-3" }), translate("auto.components.status.bar.ResourceUsageStatusSegment.93b0de3c21", "Restart")]
							})
						]
					}),
					!daemonUnreachable && sessionsOnlyError && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-2 border-b border-border bg-muted/40 px-3 py-1.5 text-[11px] text-muted-foreground",
						role: "status",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-3 shrink-0 text-yellow-500" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.status.bar.ResourceUsageStatusSegment.e7cf14ec78", "Terminal sessions unavailable. The list may be stale.") })]
					}),
					resourceSnapshot && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "px-3 py-2 border-b border-border flex items-baseline justify-between gap-3 text-xs tabular-nums",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-baseline gap-3 min-w-0",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, {
									delayDuration: 200,
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
										asChild: true,
										children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
											tabIndex: 0,
											className: "font-medium text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:rounded",
											children: formatCpu(totalCpu)
										})
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
										side: "top",
										sideOffset: 6,
										className: "z-[70] max-w-xs",
										children: translate("auto.components.status.bar.ResourceUsageStatusSegment.1fedf94eae", "Combined CPU load. Values above 100% mean more than one core is working at once.")
									})]
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "text-muted-foreground/50",
									children: "·"
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, {
									delayDuration: 200,
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
										asChild: true,
										children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
											tabIndex: 0,
											className: "font-medium text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:rounded",
											children: [
												formatMemory(totalMemory),
												" ",
												/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
													className: "font-normal text-muted-foreground",
													children: memoryMetricCopy.summaryLabel
												})
											]
										})
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
										side: "top",
										sideOffset: 6,
										className: "z-[70] max-w-xs",
										children: memoryMetricCopy.description
									})]
								})
							]
						}), orphanCount > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "shrink-0 text-yellow-500",
							"aria-live": "polite",
							children: orphanCount === 1 ? translate("auto.components.status.bar.ResourceUsageStatusSegment.30ff2c3c31", "{{value0}} orphan", { value0: orphanCount }) : translate("auto.components.status.bar.ResourceUsageStatusSegment.b8f4a2c1d0e3", "{{value0}} orphans", { value0: orphanCount })
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						ref: setPopoverBodyNode,
						tabIndex: -1,
						className: "flex h-[420px] flex-col outline-none",
						children: [(unifiedRepos.length > 0 || resourceSnapshot) && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center justify-between px-3 py-1 bg-muted/30 border-b border-border/50 text-[10px] uppercase tracking-wide shrink-0",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								onClick: () => setSortOption("name"),
								className: cn("hover:text-foreground transition-colors", sortOption === "name" ? "font-semibold text-foreground" : "text-muted-foreground/80"),
								"aria-pressed": sortOption === "name",
								children: translate("auto.components.status.bar.ResourceUsageStatusSegment.2aa2de6cb9", "Name")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex items-center gap-2 shrink-0",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: cn(METRIC_COLUMNS_CLS, "text-[10px]"),
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
										type: "button",
										onClick: () => setSortOption("cpu"),
										className: cn(CPU_COLUMN_CLS, "hover:text-foreground transition-colors", sortOption === "cpu" ? "font-semibold text-foreground" : "text-muted-foreground/80"),
										"aria-pressed": sortOption === "cpu",
										children: translate("auto.components.status.bar.ResourceUsageStatusSegment.298f4be7f2", "CPU")
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
										type: "button",
										onClick: () => setSortOption("memory"),
										className: cn(MEM_COLUMN_CLS, "hover:text-foreground transition-colors", sortOption === "memory" ? "font-semibold text-foreground" : "text-muted-foreground/80"),
										"aria-pressed": sortOption === "memory",
										children: memoryMetricCopy.columnLabel
									})]
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: ROW_TRAILING_GUTTER_CLS,
									"aria-hidden": true
								})]
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex-1 overflow-y-auto scrollbar-sleek",
							children: [
								unifiedRepos.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ResourceTree, {
									repos: unifiedRepos,
									sortOption,
									collapsedRepos,
									toggleRepo,
									collapsedWorktrees,
									activeWorktreeId,
									toggleWorktree,
									navigateToWorktree,
									navigateToTab,
									onDelete: deleteWorktree,
									onKillSession: handleKillSession
								}),
								unifiedRepos.length === 0 && resourceSnapshot && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "px-3 py-4 text-center text-xs text-muted-foreground",
									children: translate("auto.components.status.bar.ResourceUsageStatusSegment.27a74f91f0", "Nothing running right now")
								}),
								resourceSnapshot && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AppSection, {
									app: resourceSnapshot.app,
									isCollapsed: appCollapsed,
									onToggle: () => setAppCollapsed((v) => !v)
								}),
								!resourceSnapshot && !daemonUnreachable && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "px-3 py-4 text-center text-xs text-muted-foreground",
									children: translate("auto.components.status.bar.ResourceUsageStatusSegment.888dad8c55", "Loading…")
								})
							]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "border-t border-border/50 px-3 py-2 shrink-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							onClick: handleOpenWorkspaceCleanup,
							className: "relative inline-flex w-full items-center justify-center rounded-md border border-border/70 px-2.5 py-1.5 text-xs font-medium text-foreground transition-colors hover:bg-accent/60",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "min-w-0 truncate px-4 text-center",
								children: translate("auto.components.status.bar.ResourceUsageStatusSegment.92924a14e3", "Review inactive workspaces ({{value0}})", { value0: oldWorkspaceCount })
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, {
								className: "absolute right-2.5 size-3.5 text-muted-foreground",
								"aria-hidden": true
							})]
						}), orphanCount > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
							type: "button",
							onClick: () => void handleKillOrphans(),
							className: "mt-2 inline-flex w-full items-center justify-center rounded-md border border-border/70 px-2.5 py-1.5 text-xs font-medium text-foreground transition-colors hover:bg-accent/60",
							children: orphanCount === 1 ? translate("auto.components.status.bar.ResourceUsageStatusSegment.c7e3b1a0d9f2", "Kill {{value0}} orphan terminal", { value0: orphanCount }) : translate("auto.components.status.bar.ResourceUsageStatusSegment.d8f4c2b1e0a3", "Kill {{value0}} orphan terminals", { value0: orphanCount })
						}) : null]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceSpaceCompactPanel, { onOpenFullPage: openSpaceResults })
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
				open: killConfirm !== null,
				onOpenChange: (next) => {
					if (next) return;
					if (killing) return;
					setKillConfirm(null);
				},
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
					className: "max-w-md",
					showCloseButton: !killing,
					onPointerDownOutside: (e) => {
						if (killing) e.preventDefault();
					},
					onEscapeKeyDown: (e) => {
						if (killing) e.preventDefault();
					},
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
						className: "text-sm",
						children: translate("auto.components.status.bar.ResourceUsageStatusSegment.e9a5d3c2b1f0", "Kill {{value0}}?", { value0: killConfirm?.label ?? translate("auto.components.status.bar.ResourceUsageStatusSegment.138b99bd80", "this session") })
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
						className: "text-xs",
						children: translate("auto.components.status.bar.ResourceUsageStatusSegment.67c4ecda49", "Force-quits this terminal. Any unsaved work in the pane is lost. This can't be undone.")
					})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "outline",
						onClick: () => setKillConfirm(null),
						disabled: killing,
						children: translate("auto.components.status.bar.ResourceUsageStatusSegment.946d9f94d0", "Cancel")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						variant: "destructive",
						onClick: () => void runKillConfirmed(),
						disabled: killing,
						children: [killing ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }) : null, killing ? translate("auto.components.status.bar.ResourceUsageStatusSegment.41ae4fa725", "Killing…") : translate("auto.components.status.bar.ResourceUsageStatusSegment.b10695d6ce", "Kill session")]
					})] })]
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DaemonActionDialog, { api: daemonActions })
		]
	});
}
export { ResourceUsageStatusSegment, SessionRow, WorktreeRow };
