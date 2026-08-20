import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./useTranslation-DX5IRIhk.js";
import "./lazy-with-retry-pSZJrSfN.js";
import { t as Button } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import "./repo-icon-Dcv6msBx.js";
import { t as AgentKanbanBoard } from "./AgentKanbanBoard-poUnfROE.js";
import { t as Settings } from "./settings-BX3azETW.js";
import "./AgentTerminalDialog-BBP0-dz2.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { l as DropdownMenuSeparator, m as DropdownMenuTrigger, r as DropdownMenuContent, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
import "./label-D-n9s_wS.js";
import "./popover-CgR1mzy7.js";
import "./scroll-area-DifvZO0h.js";
import "./switch-NhZdOYtg.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import "./connection-context-BUPsamzR.js";
import "./localized-catalog-DubKHKUR.js";
import { a as SettingsSegmentedControl, s as SettingsSwitch } from "./SettingsFormControls-C0chb_HE.js";
import { t as activateTabAndFocusPane } from "./activate-tab-and-focus-pane-dvS5VCkm.js";
import "./terminal-appearance-D3oO-Ew5.js";
import "./agent-status-connection-ownership-D5nXPHBo.js";
import "./ShortcutKeyCombo-Ch456Md0.js";
import "./worktree-agent-rows-C1pW_DbE.js";
import "./dialog-BbelfMSB.js";
import "./input-DV5rpysh.js";
import "./worktree-title-derived-agent-rows-xbcpjeY8.js";
import "./AgentWorkingSpinner-BpnTWNKF.js";
import "./AgentStateDot-DFt63YGw.js";
import "./icons-jFAuHbv9.js";
import "./agent-catalog-CBF2CV5Q.js";
import "./agent-row-conversation-name-DXwI1NP0.js";
import "./crash-diagnostics-DaEtfKCs.js";
import { o as SheetTitle, r as SheetContent, t as Sheet } from "./sheet-DkBtbPMV.js";
import { i as useWorkspaceKanbanOutsideDismiss, n as WORKSPACE_TOP_CHROME_HEIGHT, r as isWorkspaceBoardKeepOpenTarget, t as STATUS_BAR_RESERVE_HEIGHT } from "./workspace-chrome-metrics-CNC6jyKs.js";
import "./use-system-prefers-dark-QSo6mmSW.js";
import "./paste-payload-metadata-pr3nuODB.js";
import "./DashboardHostBadge-DXORvYCI.js";
import "./terminal-shortcut-policy-BOkUsz_T.js";
/* empty css               */
import "./launch-agent-in-new-tab-44JGNfKl.js";
import "./terminal-keyboard-protocol-De0UZ6qG.js";
import { t as buildDashboardSnapshot } from "./build-dashboard-snapshot-B3HmQPph.js";
import { t as launchDashboardAgent } from "./launch-dashboard-agent-B64ro2Tz.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function AgentDashboardSettingsMenu({ onSwitchToPopout, onOpenChange }) {
	const mode = useAppStore((s) => s.settings?.experimentalAgentDashboardMode ?? "in-window");
	const showIdle = useAppStore((s) => s.settings?.experimentalAgentDashboardShowIdle === true);
	const updateSettings = useAppStore((s) => s.updateSettings);
	const handleModeChange = (next) => {
		if (next === mode) return;
		updateSettings({ experimentalAgentDashboardMode: next });
		if (next === "popout") onSwitchToPopout();
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, {
		modal: false,
		onOpenChange,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon-xs",
					"aria-label": translate("dashboardPopout.settingsLabel", "Agent Dashboard settings"),
					className: "text-muted-foreground",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Settings, { className: "size-3.5" })
				})
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
			side: "top",
			sideOffset: 4,
			children: translate("dashboardPopout.settingsTooltip", "Board settings")
		})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuContent, {
			align: "end",
			sideOffset: 8,
			collisionPadding: 8,
			className: "w-72 p-2",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex items-start justify-between gap-3 rounded-md px-1.5 py-1.5",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "min-w-0 space-y-0.5",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "block text-[12px] font-medium leading-4 text-foreground",
							children: translate("auto.components.settings.ExperimentalPane.agentDashboard.modeLabel", "Open as")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "block text-[11px] leading-4 text-muted-foreground",
							children: translate("auto.components.settings.ExperimentalPane.agentDashboard.modeCopy", "Show the dashboard as an in-window board beside the sidebar or a separate pop-out window.")
						})]
					})
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "px-1.5 pb-1",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSegmentedControl, {
						value: mode,
						onChange: handleModeChange,
						ariaLabel: translate("auto.components.settings.ExperimentalPane.agentDashboard.modeAriaLabel", "Agent Dashboard open mode"),
						size: "sm",
						equalWidth: true,
						options: [{
							value: "in-window",
							label: translate("auto.components.settings.ExperimentalPane.agentDashboard.modeInWindow", "In-window")
						}, {
							value: "popout",
							label: translate("auto.components.settings.ExperimentalPane.agentDashboard.modePopout", "Pop-out")
						}]
					})
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-start justify-between gap-3 rounded-md px-1.5 py-1.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "min-w-0 space-y-0.5",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "block text-[12px] font-medium leading-4 text-foreground",
							children: translate("dashboardPopout.settings.showIdle", "Show idle agents")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "block text-[11px] leading-4 text-muted-foreground",
							children: translate("dashboardPopout.settings.showIdleCopy", "Include agents that have gone quiet for 30 minutes without reporting completion. Hidden by default.")
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSwitch, {
						checked: showIdle,
						onChange: () => {
							updateSettings({ experimentalAgentDashboardShowIdle: !showIdle });
						},
						ariaLabel: translate("dashboardPopout.settings.showIdle", "Show idle agents")
					})]
				})
			]
		})]
	});
}
function useLiveDashboardSnapshot() {
	const repos = useAppStore((s) => s.repos);
	const worktreesByRepo = useAppStore((s) => s.worktreesByRepo);
	const tabsByWorktree = useAppStore((s) => s.tabsByWorktree);
	const agentStatusByPaneKey = useAppStore((s) => s.agentStatusByPaneKey);
	const retainedAgentsByPaneKey = useAppStore((s) => s.retainedAgentsByPaneKey);
	const migrationUnsupportedByPtyId = useAppStore((s) => s.migrationUnsupportedByPtyId);
	const runtimeAgentOrchestrationByPaneKey = useAppStore((s) => s.runtimeAgentOrchestrationByPaneKey);
	const terminalLayoutsByTabId = useAppStore((s) => s.terminalLayoutsByTabId);
	const ptyIdsByTabId = useAppStore((s) => s.ptyIdsByTabId);
	const runtimePaneTitlesByTabId = useAppStore((s) => s.runtimePaneTitlesByTabId);
	const acknowledgedAgentsByPaneKey = useAppStore((s) => s.acknowledgedAgentsByPaneKey);
	const hostedReviewCache = useAppStore((s) => s.hostedReviewCache);
	const prCache = useAppStore((s) => s.prCache);
	const settings = useAppStore((s) => s.settings);
	const workspaceStatuses = useAppStore((s) => s.workspaceStatuses);
	const detectedWorktreesByRepo = useAppStore((s) => s.detectedWorktreesByRepo);
	const folderWorkspaces = useAppStore((s) => s.folderWorkspaces);
	const projectGroups = useAppStore((s) => s.projectGroups);
	const sshTargetLabels = useAppStore((s) => s.sshTargetLabels);
	const sshConnectionStates = useAppStore((s) => s.sshConnectionStates);
	const sshStateByEnvironment = useAppStore((s) => s.sshStateByEnvironment);
	const runtimeStatusByEnvironmentId = useAppStore((s) => s.runtimeStatusByEnvironmentId);
	const restoredRuntimeHostIdByWorkspaceSessionKey = useAppStore((s) => s.restoredRuntimeHostIdByWorkspaceSessionKey);
	const runtimeEnvironments = useAppStore((s) => s.runtimeEnvironments);
	const runtimeEnvironmentCatalogHydrated = useAppStore((s) => s.runtimeEnvironmentCatalogHydrated);
	const removedRuntimeEnvironmentIds = useAppStore((s) => s.removedRuntimeEnvironmentIds);
	const paneForegroundAgentByPaneKey = useAppStore((s) => s.paneForegroundAgentByPaneKey);
	const detectedAgentIds = useAppStore((s) => s.detectedAgentIds);
	const remoteDetectedAgentIds = useAppStore((s) => s.remoteDetectedAgentIds);
	const runtimeDetectedAgentIds = useAppStore((s) => s.runtimeDetectedAgentIds);
	return (0, import_react.useMemo)(() => buildDashboardSnapshot({
		repos,
		worktreesByRepo,
		tabsByWorktree,
		agentStatusByPaneKey,
		retainedAgentsByPaneKey,
		migrationUnsupportedByPtyId,
		runtimeAgentOrchestrationByPaneKey,
		terminalLayoutsByTabId,
		ptyIdsByTabId,
		runtimePaneTitlesByTabId,
		acknowledgedAgentsByPaneKey,
		hostedReviewCache,
		prCache,
		settings,
		workspaceStatuses,
		detectedWorktreesByRepo,
		folderWorkspaces,
		projectGroups,
		sshTargetLabels,
		sshConnectionStates,
		sshStateByEnvironment,
		runtimeStatusByEnvironmentId,
		restoredRuntimeHostIdByWorkspaceSessionKey,
		runtimeEnvironments,
		runtimeEnvironmentCatalogHydrated,
		removedRuntimeEnvironmentIds,
		paneForegroundAgentByPaneKey,
		detectedAgentIds,
		remoteDetectedAgentIds,
		runtimeDetectedAgentIds,
		agentLaunchConfigByPaneKey: useAppStore.getState().agentLaunchConfigByPaneKey
	}, Date.now()), [
		repos,
		worktreesByRepo,
		tabsByWorktree,
		agentStatusByPaneKey,
		retainedAgentsByPaneKey,
		migrationUnsupportedByPtyId,
		runtimeAgentOrchestrationByPaneKey,
		terminalLayoutsByTabId,
		ptyIdsByTabId,
		runtimePaneTitlesByTabId,
		acknowledgedAgentsByPaneKey,
		hostedReviewCache,
		prCache,
		settings,
		workspaceStatuses,
		detectedWorktreesByRepo,
		folderWorkspaces,
		projectGroups,
		sshTargetLabels,
		sshConnectionStates,
		sshStateByEnvironment,
		runtimeStatusByEnvironmentId,
		restoredRuntimeHostIdByWorkspaceSessionKey,
		runtimeEnvironments,
		runtimeEnvironmentCatalogHydrated,
		removedRuntimeEnvironmentIds,
		paneForegroundAgentByPaneKey,
		detectedAgentIds,
		remoteDetectedAgentIds,
		runtimeDetectedAgentIds,
		useAppStore((s) => s.agentStatusEpoch)
	]);
}
var AGENT_BOARD_ESCAPE_BLOCKING_OVERLAY_SELECTOR = [
	"[data-slot=\"dropdown-menu-content\"][data-state=\"open\"]",
	"[data-slot=\"context-menu-content\"][data-state=\"open\"]",
	"[data-slot=\"popover-content\"][data-state=\"open\"]",
	"[role=\"dialog\"][data-state=\"open\"]:not([data-agent-dashboard-sheet])",
	"[role=\"alertdialog\"][data-state=\"open\"]",
	"[role=\"menu\"][data-state=\"open\"]",
	"[role=\"listbox\"][data-state=\"open\"]"
].join(", ");
function AgentDashboardDrawerBody({ onClose, onMenuOpenChange }) {
	const snapshot = useLiveDashboardSnapshot();
	const handleAckAgent = (0, import_react.useCallback)((paneKey) => {
		useAppStore.getState().acknowledgeAgents([paneKey]);
	}, []);
	const handleRevealAgent = (0, import_react.useCallback)((args) => {
		useAppStore.getState().setActiveWorktree(args.worktreeId, args.executionHostId);
		activateTabAndFocusPane(args.tabId, args.leafId, { flashFocusedPane: true });
		onClose();
	}, [onClose]);
	const handleSwitchToPopout = (0, import_react.useCallback)(() => {
		onClose();
		window.api.dashboard.openPopout?.();
	}, [onClose]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentKanbanBoard, {
		snapshot,
		initialView: "board",
		containerClassName: "h-full w-full bg-transparent",
		onAckAgent: handleAckAgent,
		onRevealAgent: handleRevealAgent,
		onSpawnAgent: launchDashboardAgent,
		onClose,
		onOpenMap: (0, import_react.useCallback)(() => {
			onClose();
			window.api.dashboard.openPopout?.("map");
		}, [onClose]),
		headerActions: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentDashboardSettingsMenu, {
			onSwitchToPopout: handleSwitchToPopout,
			onOpenChange: onMenuOpenChange
		})
	});
}
function AgentDashboardDrawer({ leftSidebarStyle, statusBarVisible }) {
	const open = useAppStore((s) => s.agentDashboardDrawerOpen);
	const setOpen = useAppStore((s) => s.setAgentDashboardDrawerOpen);
	const sidebarOpen = useAppStore((s) => s.sidebarOpen);
	const sidebarWidth = useAppStore((s) => s.sidebarWidth);
	const [menuOpen, setMenuOpen] = (0, import_react.useState)(false);
	const close = (0, import_react.useCallback)(() => {
		setMenuOpen(false);
		setOpen(false);
	}, [setOpen]);
	(0, import_react.useEffect)(() => {
		if (!open) setMenuOpen(false);
	}, [open]);
	const handleSheetOpenChange = (0, import_react.useCallback)((nextOpen) => {
		if (nextOpen) setOpen(true);
	}, [setOpen]);
	const boardRef = (0, import_react.useRef)(null);
	useWorkspaceKanbanOutsideDismiss({
		open,
		boardRef,
		preserveOpenForMenu: menuOpen,
		onOpenChange: setOpen
	});
	(0, import_react.useEffect)(() => {
		if (!open) return;
		const handleKeyDown = (event) => {
			if (event.key !== "Escape") return;
			if (document.querySelector(AGENT_BOARD_ESCAPE_BLOCKING_OVERLAY_SELECTOR)) return;
			event.preventDefault();
			close();
		};
		document.addEventListener("keydown", handleKeyDown, true);
		return () => document.removeEventListener("keydown", handleKeyDown, true);
	}, [close, open]);
	const drawerLeft = sidebarOpen ? sidebarWidth : 0;
	const drawerLeftCss = sidebarOpen ? `var(--workspace-sidebar-live-width, ${sidebarWidth}px)` : "0px";
	const drawerBottom = `${statusBarVisible ? 24 : 0}px`;
	const guardSidebarInteraction = (event) => {
		const originalEvent = event.detail.originalEvent;
		if (menuOpen || isWorkspaceBoardKeepOpenTarget(originalEvent.target)) {
			event.preventDefault();
			return;
		}
		const liveDrawerLeft = boardRef.current?.closest("[data-slot=\"sheet-content\"]")?.getBoundingClientRect().left ?? drawerLeft;
		const pointerX = "clientX" in originalEvent && typeof originalEvent.clientX === "number" ? originalEvent.clientX : null;
		if (pointerX !== null && pointerX < liveDrawerLeft) event.preventDefault();
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sheet, {
		open,
		onOpenChange: handleSheetOpenChange,
		modal: false,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(SheetContent, {
			side: "left",
			showCloseButton: false,
			"aria-describedby": void 0,
			className: "workspace-kanban-sheet-content bg-worktree-sidebar p-0 sm:max-w-none",
			overlayStyle: {
				top: 36,
				bottom: drawerBottom,
				left: drawerLeftCss,
				pointerEvents: "none"
			},
			style: {
				...leftSidebarStyle,
				left: drawerLeftCss,
				top: 36,
				bottom: drawerBottom,
				height: "auto",
				width: `min(calc(100vw - ${drawerLeftCss}), 1294px)`
			},
			"data-agent-dashboard-sheet": "",
			onOpenAutoFocus: (event) => {
				event.preventDefault();
			},
			onPointerDownOutside: guardSidebarInteraction,
			onInteractOutside: guardSidebarInteraction,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SheetTitle, {
				className: "sr-only",
				children: translate("dashboardPopout.title", "Agents")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				ref: boardRef,
				className: "flex min-h-0 flex-1 flex-col",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentDashboardDrawerBody, {
					onClose: close,
					onMenuOpenChange: setMenuOpen
				})
			})]
		})
	});
}
function AgentDashboardSidebarHost({ sidebarOpen, workspaceBoardOpen, closeWorkspaceBoard, leftSidebarStyle, statusBarVisible }) {
	const drawerOpen = useAppStore((s) => s.agentDashboardDrawerOpen);
	const setDrawerOpen = useAppStore((s) => s.setAgentDashboardDrawerOpen);
	(0, import_react.useEffect)(() => {
		if (!sidebarOpen && drawerOpen) setDrawerOpen(false);
	}, [
		drawerOpen,
		setDrawerOpen,
		sidebarOpen
	]);
	(0, import_react.useEffect)(() => {
		if (drawerOpen) closeWorkspaceBoard();
	}, [closeWorkspaceBoard, drawerOpen]);
	(0, import_react.useEffect)(() => {
		if (workspaceBoardOpen) setDrawerOpen(false);
	}, [setDrawerOpen, workspaceBoardOpen]);
	return sidebarOpen ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentDashboardDrawer, {
		leftSidebarStyle,
		statusBarVisible
	}) : null;
}
export { AgentDashboardSidebarHost as default };
