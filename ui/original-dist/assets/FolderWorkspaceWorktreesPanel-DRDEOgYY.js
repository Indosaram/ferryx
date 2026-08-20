import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./lazy-with-retry-pSZJrSfN.js";
import "./button-DszXJEV6.js";
import "./open-in-app-catalog-DpQuLNDD.js";
import "./workspace-status-wl52y3xd.js";
import "./WorktreeContextMenu-BmbFRbIZ.js";
import "./repo-icon-Dcv6msBx.js";
import "./worktree-activation-BDsaiyMf.js";
import "./DetachedHeadBadge-0sSKcoh7.js";
import { c as getLineageNestedRowGeometry, s as getLineageChildrenInlineStyle, t as WorktreeCard_default } from "./WorktreeCard-DHnjoc37.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./dropdown-menu-Dth6LPK-.js";
import "./label-D-n9s_wS.js";
import "./hover-card-DP92-D-b.js";
import "./popover-CgR1mzy7.js";
import "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import "./selectors-XOBeaOSb.js";
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
import "./delete-worktree-flow-RxB6NScm.js";
import "./preserved-branch-batch-toast-DHxeGO1o.js";
import "./sleep-worktree-flow-83wDDapJ.js";
import "./activate-tab-and-focus-pane-dvS5VCkm.js";
import "./stale-agent-row-D6vAh16E.js";
import "./badge-BBptl5GG.js";
import "./command-D8Tw17HJ.js";
import "./RepoBadgeLabel-BMcVlWTu.js";
import "./use-active-skill-discovery-runtime-target-CdctmJyj.js";
import "./useInstalledAgentSkills-BYdWqfUf.js";
import "./project-skill-runtime-BxhwlKnI.js";
import "./JiraIcon-C4R1p9Vj.js";
import "./LinearIcon-CTELA_97.js";
import "./worktree-agent-rows-C1pW_DbE.js";
import "./dialog-BbelfMSB.js";
import "./WorktreeOpenInMenu-DH_mb2Hm.js";
import "./input-DV5rpysh.js";
import "./ime-composition-keyboard-event-HdRxQ6x2.js";
import "./worktree-title-derived-agent-rows-xbcpjeY8.js";
import "./worktree-status-DR0Zr8Ht.js";
import "./WorktreeCardHelpers-Detnezco.js";
import "./AgentWorkingSpinner-BpnTWNKF.js";
import "./StatusIndicator-CJ9TRLK4.js";
import "./SelectedTextCopyMenu-CX36uwfX.js";
import "./lib-CtirWBBB.js";
import "./lib-D08jHVMa.js";
import "./purify.es-C_rn83UJ.js";
import "./MermaidBlock-gW3wAx0A.js";
import "./CommentMarkdown-bsrexQcY.js";
import "./useWorkspaceEmojiShortcodeInput-DLLHCF-J.js";
import "./workspace-port-localhost-label-selector-BFjPG9iO.js";
import "./ssh-connect-verb-CzMNDLCH.js";
import "./ssh-connect-in-flight-PkUz5iol.js";
import "./linear-agent-skill-runtime-DMZ1aTsy.js";
import "./CliSkillRuntimeSetup-BpNtfsr6.js";
import "./AgentStateDot-DFt63YGw.js";
import "./icons-jFAuHbv9.js";
import "./agent-catalog-CBF2CV5Q.js";
import "./agent-row-conversation-name-DXwI1NP0.js";
import "./useWorktreeAgentRows-DfUM0dP9.js";
import "./worktree-list-virtual-rows-B1jWEldM.js";
import "./crash-diagnostics-DaEtfKCs.js";
import { t as getAttachedWorktreesForFolderWorkspace } from "./folder-workspace-attached-worktrees-CE59j94w.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function stopNestedWorktreeCardBubble(event) {
	event.stopPropagation();
}
function FolderWorkspaceWorktreesPanel() {
	const activeWorktreeId = useAppStore((s) => s.activeWorktreeId);
	const activeWorkspaceKey = useAppStore((s) => s.activeWorkspaceKey);
	const experimentalNewWorktreeCardStyle = useAppStore((s) => s.settings?.experimentalNewWorktreeCardStyle) === true;
	const folderWorkspaces = useAppStore((s) => s.folderWorkspaces);
	const workspaceLineageByChildKey = useAppStore((s) => s.workspaceLineageByChildKey);
	const worktreeLineageById = useAppStore((s) => s.worktreeLineageById);
	const worktreesByRepo = useAppStore((s) => s.worktreesByRepo);
	const repos = useAppStore((s) => s.repos);
	const [collapsedLineageWorktreeIds, setCollapsedLineageWorktreeIds] = (0, import_react.useState)(() => /* @__PURE__ */ new Set());
	const repoById = new Map(repos.map((repo) => [repo.id, repo]));
	const { folderWorkspace, childWorktrees, lineageChildrenByParentId, rootChildWorktrees } = getAttachedWorktreesForFolderWorkspace({
		activeWorkspaceKey,
		activeWorktreeId,
		folderWorkspaces,
		workspaceLineageByChildKey,
		worktreeLineageById,
		worktreesByRepo
	});
	const toggleLineage = (worktreeId) => {
		setCollapsedLineageWorktreeIds((current) => {
			const next = new Set(current);
			if (next.has(worktreeId)) next.delete(worktreeId);
			else next.add(worktreeId);
			return next;
		});
	};
	const renderChildWorktree = (worktree, ancestorIds = /* @__PURE__ */ new Set()) => {
		const lineageChildren = lineageChildrenByParentId.get(worktree.id) ?? [];
		const lineageCollapsed = collapsedLineageWorktreeIds.has(worktree.id);
		const nextAncestorIds = new Set([...ancestorIds, worktree.id]);
		const safeLineageChildren = lineageChildren.filter((child) => !nextAncestorIds.has(child.id));
		const hasSafeLineageChildren = safeLineageChildren.length > 0;
		const lineageGeometry = getLineageNestedRowGeometry({
			experimentalNewWorktreeCardStyle,
			inheritedCardContentIndent: 0,
			lineageDepth: ancestorIds.size
		});
		return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeCard_default, {
			worktree,
			repo: repoById.get(worktree.repoId),
			isActive: activeWorktreeId === worktree.id,
			isActiveSurface: false,
			hideRepoBadge: false,
			nativeDragEnabled: false,
			flushSurface: true,
			contentIndent: lineageGeometry.cardContentIndent,
			affiliateListMode: true,
			lineageChildCount: safeLineageChildren.length,
			lineageCollapsed,
			lineageChildren: !lineageCollapsed && hasSafeLineageChildren ? safeLineageChildren.map((child) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				onClick: stopNestedWorktreeCardBubble,
				onDoubleClick: stopNestedWorktreeCardBubble,
				onDragStart: stopNestedWorktreeCardBubble,
				style: lineageGeometry.surfaceInset > 0 ? { paddingLeft: lineageGeometry.surfaceInset } : void 0,
				children: renderChildWorktree(child, nextAncestorIds)
			}, child.id)) : void 0,
			lineageChildrenStyle: hasSafeLineageChildren ? getLineageChildrenInlineStyle(lineageGeometry.lineageChildrenInlineOffset) : void 0,
			onLineageToggle: hasSafeLineageChildren ? (event) => {
				event.preventDefault();
				event.stopPropagation();
				toggleLineage(worktree.id);
			} : void 0
		}, worktree.id);
	};
	if (!folderWorkspace) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "flex min-h-0 flex-1 items-center justify-center p-6 text-center text-sm text-muted-foreground",
		children: translate("auto.components.rightSidebar.FolderWorkspaceWorktreesPanel.unavailable", "Workspaces are only shown for folder workspaces.")
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-h-0 flex-1 flex-col overflow-hidden bg-background",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "border-b border-border px-4 py-3",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "truncate text-sm font-medium text-foreground",
				children: folderWorkspace.name
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mt-1 text-xs text-muted-foreground",
				children: childWorktrees.length === 1 ? translate("auto.components.rightSidebar.FolderWorkspaceWorktreesPanel.countOne", "1 attached worktree") : translate("auto.components.rightSidebar.FolderWorkspaceWorktreesPanel.countMany", "{{value0}} attached worktrees", { value0: childWorktrees.length })
			})]
		}), childWorktrees.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex flex-1 flex-col items-center justify-center px-6 text-center",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "text-sm font-medium text-foreground",
				children: translate("auto.components.rightSidebar.FolderWorkspaceWorktreesPanel.emptyTitle", "No attached worktrees yet")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mt-2 max-w-[16rem] text-xs leading-5 text-muted-foreground",
				children: translate("auto.components.rightSidebar.FolderWorkspaceWorktreesPanel.emptyCopy", "Worktrees created from this workspace will show up here.")
			})]
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "scrollbar-sleek min-h-0 flex-1 overflow-y-auto py-2 pl-1 pr-2",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "space-y-1",
				children: rootChildWorktrees.map((worktree) => renderChildWorktree(worktree))
			})
		})]
	});
}
export { FolderWorkspaceWorktreesPanel as default };
