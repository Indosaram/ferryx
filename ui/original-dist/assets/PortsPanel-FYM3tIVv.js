import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn, t as Button } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import { t as Box } from "./box-CsJZiF6f.js";
import { t as ChevronRight } from "./chevron-right-CZtMe6Ev.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import "./worktree-activation-BDsaiyMf.js";
import { t as Info } from "./info-D3uaWrfJ.js";
import { t as Pencil } from "./pencil-CLc9a5do.js";
import { t as Plus } from "./plus-Db0kWPVa.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { t as Server } from "./server-DYdwnXME.js";
import { Ou as getRuntimeEnvironmentIdForWorktree, m_ as Trash2, op as getActiveRuntimeTarget, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { f as ContextMenuTrigger, i as ContextMenuLabel, n as ContextMenuContent, r as ContextMenuItem, s as ContextMenuSeparator, t as ContextMenu } from "./context-menu-D4RKI7hR.js";
import { i as TooltipTrigger, n as TooltipContent, r as TooltipProvider, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { c as useActiveWorktree, f as useRepoById } from "./selectors-XOBeaOSb.js";
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
import { i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { _ as advertisedBrowserUrlForDetectedPort, c as openWorkspacePortInBrowser, f as scanWorkspacePortsForTarget, g as addressForPortForwardEntry, h as addressForPort, i as getPortOpenBrowserTooltipLabel, l as refreshWorkspacePortScanAfterStop, o as killWorkspacePortForTarget, p as workspacePortRuntimeTargetKey, t as resolveLocalhostLabelRouteForPort, u as resolvePortOpenInOrcaBrowser, v as advertisedBrowserUrlForForwardedRow, y as browserUrlForPortForwardEntry } from "./workspace-port-localhost-label-selector-BFjPG9iO.js";
import { n as openWorkspaceBrowserTab } from "./workspace-browser-tab-open-aiO4ahIP.js";
var Unplug = createLucideIcon("unplug", [
	["path", {
		d: "m19 5 3-3",
		key: "yk6iyv"
	}],
	["path", {
		d: "m2 22 3-3",
		key: "19mgm9"
	}],
	["path", {
		d: "M6.3 20.3a2.4 2.4 0 0 0 3.4 0L12 18l-6-6-2.3 2.3a2.4 2.4 0 0 0 0 3.4Z",
		key: "goz73y"
	}],
	["path", {
		d: "M7.5 13.5 10 11",
		key: "7xgeeb"
	}],
	["path", {
		d: "M10.5 16.5 13 14",
		key: "10btkg"
	}],
	["path", {
		d: "m12 6 6 6 2.3-2.3a2.4 2.4 0 0 0 0-3.4l-2.6-2.6a2.4 2.4 0 0 0-3.4 0Z",
		key: "1snsnr"
	}]
]);
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function getLocalWorkspacePortSections(scan, activeRepoId, activeWorktreeId) {
	const ports = scan?.ports ?? [];
	return {
		activePorts: ports.filter((port) => port.kind === "workspace" && port.owner.repoId === activeRepoId && port.owner.worktreeId === activeWorktreeId),
		otherWorkspacePorts: ports.filter((port) => port.kind === "workspace" && port.owner.repoId === activeRepoId && port.owner.worktreeId !== activeWorktreeId),
		externalPorts: ports.flatMap((port) => {
			if (port.kind !== "workspace") return [port];
			return port.owner.repoId === activeRepoId ? [] : [workspacePortAsExternal(port)];
		})
	};
}
function workspacePortAsExternal(port) {
	return {
		id: port.id,
		bindHost: port.bindHost,
		connectHost: port.connectHost,
		port: port.port,
		pid: port.pid,
		processName: port.processName,
		protocol: port.protocol,
		kind: "external"
	};
}
var LOCAL_PORT_MENU_CONTENT_CLASS = "!rounded-md !border-border/60 !bg-popover !text-popover-foreground !shadow-[0_10px_24px_rgba(0,0,0,0.18)] !backdrop-blur-none";
var LOCAL_PORT_MENU_ITEM_CLASS = "rounded-md focus:bg-accent focus:text-accent-foreground dark:focus:bg-accent";
var LOCAL_PORT_MENU_LABEL_CLASS = "px-2 py-1 text-[11px] font-semibold text-muted-foreground";
function safeLocalPort(remotePort) {
	if (remotePort < 1024) return remotePort + 1e4;
	return remotePort;
}
var LOOPBACK_HOSTS = new Set([
	"localhost",
	"127.0.0.1",
	"::1",
	"0.0.0.0",
	"::"
]);
function normalizeHost(host) {
	if (!host || LOOPBACK_HOSTS.has(host)) return "localhost";
	return host;
}
function PortsPanel({ isVisible }) {
	if (useRepoById(useActiveWorktree()?.repoId ?? null)?.connectionId) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SshPortsPanel, {});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LocalWorkspacePortsPanel, { isVisible });
}
function LocalWorkspacePortsPanel({ isVisible }) {
	const activeWorktree = useActiveWorktree();
	const activeRepo = useRepoById(activeWorktree?.repoId ?? null);
	const settings = useAppStore((s) => s.settings);
	const createBrowserTab = useAppStore((s) => s.createBrowserTab);
	const setRemoteBrowserPageHandle = useAppStore((s) => s.setRemoteBrowserPageHandle);
	const scansByKey = useAppStore((s) => s.workspacePortScansByKey);
	const refreshing = useAppStore((s) => s.workspacePortScanRefreshing);
	const setWorkspacePortScan = useAppStore((s) => s.setWorkspacePortScan);
	const setWorkspacePortScanForKey = useAppStore((s) => s.setWorkspacePortScanForKey);
	const setWorkspacePortScanRefreshing = useAppStore((s) => s.setWorkspacePortScanRefreshing);
	const [detailsPort, setDetailsPort] = (0, import_react.useState)(null);
	const [collapsedSections, setCollapsedSections] = (0, import_react.useState)({
		other: true,
		external: true
	});
	const runtimeTarget = (0, import_react.useMemo)(() => {
		const activeRuntimeEnvironmentId = getRuntimeEnvironmentIdForWorktree(useAppStore.getState(), activeWorktree?.id);
		return getActiveRuntimeTarget({
			...settings,
			activeRuntimeEnvironmentId
		});
	}, [activeWorktree?.id, settings]);
	const scanKey = `${workspacePortRuntimeTargetKey(runtimeTarget)}:all`;
	const refresh = (0, import_react.useCallback)(() => {
		if (!activeRepo) return Promise.resolve();
		setWorkspacePortScanRefreshing(true);
		return scanWorkspacePortsForTarget(runtimeTarget).then((nextScan) => {
			setWorkspacePortScanForKey(scanKey, nextScan);
			setWorkspacePortScan({
				key: scanKey,
				result: nextScan
			});
		}).catch((error) => {
			const message = error instanceof Error ? error.message : String(error);
			toast.error(translate("auto.components.right.sidebar.PortsPanel.a00f3a2840", "Failed to refresh ports"), { description: message || translate("auto.components.right.sidebar.PortsPanel.740aca88ab", "Workspace port scan failed.") });
		}).finally(() => {
			setWorkspacePortScanRefreshing(false);
		});
	}, [
		activeRepo,
		runtimeTarget,
		scanKey,
		setWorkspacePortScan,
		setWorkspacePortScanForKey,
		setWorkspacePortScanRefreshing
	]);
	const displayScan = isVisible ? scansByKey[scanKey] ?? null : null;
	const toggleSection = (0, import_react.useCallback)((sectionId) => {
		setCollapsedSections((current) => ({
			...current,
			[sectionId]: !current[sectionId]
		}));
	}, []);
	const handleStopPort = (0, import_react.useCallback)(async (port) => {
		if (!activeRepo || !port.pid) return;
		const result = await killWorkspacePortForTarget(runtimeTarget, {
			repoId: activeRepo.id,
			pid: port.pid,
			port: port.port
		});
		if (!result.ok) {
			toast.error(result.reason);
			return;
		}
		toast.success(translate("auto.components.right.sidebar.PortsPanel.97b562d21d", "Stopped process on :{{value0}}", { value0: port.port }));
		const refreshResult = await refreshWorkspacePortScanAfterStop({
			runtimeTarget,
			setWorkspacePortScan,
			setWorkspacePortScanForKey,
			getWorkspacePortScansByKey: () => useAppStore.getState().workspacePortScansByKey,
			setWorkspacePortScanRefreshing
		});
		if (!refreshResult.ok) toast.error(translate("auto.components.right.sidebar.PortsPanel.a00f3a2840", "Failed to refresh ports"), { description: refreshResult.reason });
	}, [
		activeRepo,
		runtimeTarget,
		setWorkspacePortScan,
		setWorkspacePortScanForKey,
		setWorkspacePortScanRefreshing
	]);
	const handleOpenPortInBrowser = (0, import_react.useCallback)(async (port, event) => {
		const result = await openWorkspacePortInBrowser({
			port,
			activeWorktreeId: activeWorktree?.id,
			runtimeTarget,
			createBrowserTab,
			setRemoteBrowserPageHandle,
			openInOrcaBrowser: resolvePortOpenInOrcaBrowser({
				settings,
				event,
				isMac: navigator.userAgent.includes("Mac")
			}),
			localhostLabelRoute: resolveLocalhostLabelRouteForPort(useAppStore.getState(), port)
		});
		if (!result.ok) toast.error(translate("auto.components.right.sidebar.PortsPanel.98e9a414f8", "Failed to open browser"), { description: result.reason });
	}, [
		activeWorktree?.id,
		createBrowserTab,
		runtimeTarget,
		setRemoteBrowserPageHandle,
		settings
	]);
	const { activePorts, otherWorkspacePorts, externalPorts } = (0, import_react.useMemo)(() => getLocalWorkspacePortSections(displayScan, activeRepo?.id, activeWorktree?.id), [
		activeRepo?.id,
		activeWorktree?.id,
		displayScan
	]);
	if (!activeRepo) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col items-center justify-center h-full px-4 text-center text-muted-foreground",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Server, {
			size: 32,
			className: "mb-3 opacity-50"
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
			className: "text-sm",
			children: translate("auto.components.right.sidebar.PortsPanel.c1b115c375", "No workspace selected")
		})]
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col h-full overflow-y-auto scrollbar-sleek",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center justify-between px-3 py-2 border-b border-border",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-[11px] font-semibold uppercase tracking-wider text-muted-foreground",
					children: translate("auto.components.right.sidebar.PortsPanel.6bc058dbe1", "Ports")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "ghost",
						size: "icon-xs",
						className: "text-muted-foreground hover:text-foreground",
						onClick: () => void refresh(),
						disabled: refreshing,
						"aria-label": translate("auto.components.right.sidebar.PortsPanel.7822e3edc6", "Refresh Ports"),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, {
							size: 14,
							className: cn(refreshing && "animate-spin")
						})
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
					side: "top",
					sideOffset: 4,
					children: translate("auto.components.right.sidebar.PortsPanel.7822e3edc6", "Refresh Ports")
				})] })]
			}),
			displayScan?.unavailableReason && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "px-3 py-2 text-xs text-muted-foreground border-b border-border",
				children: translate("auto.components.right.sidebar.PortsPanel.f59c783b7a", "Port scan unavailable on {{value0}}: {{value1}}", {
					value0: displayScan.platform,
					value1: displayScan.unavailableReason
				})
			}),
			!displayScan?.unavailableReason && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LocalPortSection, {
					id: "active",
					title: translate("auto.components.right.sidebar.PortsPanel.935dda7718", "Active Workspace"),
					ports: activePorts,
					emptyText: refreshing && !displayScan ? translate("auto.components.right.sidebar.PortsPanel.0d63d94db3", "Scanning...") : translate("auto.components.right.sidebar.PortsPanel.38b16cfbef", "No ports detected"),
					collapsed: collapsedSections.active ?? false,
					onToggle: () => toggleSection("active"),
					onStopPort: (port) => void handleStopPort(port),
					onShowDetails: setDetailsPort,
					onOpenInBrowser: handleOpenPortInBrowser
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LocalPortSection, {
					id: "other",
					title: translate("auto.components.right.sidebar.PortsPanel.4db4b5e435", "Other Workspaces"),
					ports: otherWorkspacePorts,
					collapsed: collapsedSections.other ?? false,
					onToggle: () => toggleSection("other"),
					onStopPort: (port) => void handleStopPort(port),
					onShowDetails: setDetailsPort,
					onOpenInBrowser: handleOpenPortInBrowser
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LocalPortSection, {
					id: "external",
					title: translate("auto.components.right.sidebar.PortsPanel.d32820d3e2", "External"),
					ports: externalPorts,
					collapsed: collapsedSections.external ?? false,
					onToggle: () => toggleSection("external"),
					onStopPort: (port) => void handleStopPort(port),
					onShowDetails: setDetailsPort,
					onOpenInBrowser: handleOpenPortInBrowser
				})
			] }),
			!displayScan?.unavailableReason && displayScan && activePorts.length === 0 && otherWorkspacePorts.length === 0 && externalPorts.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-col items-center justify-center flex-1 px-4 text-center text-muted-foreground",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Server, {
					size: 32,
					className: "mb-3 opacity-50"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-sm",
					children: translate("auto.components.right.sidebar.PortsPanel.a2a9fc6899", "No local ports detected")
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LocalPortDetailsDialog, {
				port: detailsPort,
				onClose: () => setDetailsPort(null)
			})
		]
	});
}
function LocalPortSection({ id, title, ports, emptyText, collapsed, onToggle, onStopPort, onShowDetails, onOpenInBrowser }) {
	if (ports.length === 0 && !emptyText) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "px-3 pt-2",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
			type: "button",
			className: "sticky top-0 z-10 mb-1 flex w-full items-center gap-1 border-b border-border/40 bg-background py-1 text-left text-muted-foreground transition-colors hover:text-foreground",
			onClick: onToggle,
			"aria-expanded": !collapsed,
			"aria-controls": `local-port-section-${id}`,
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, {
					size: 12,
					className: cn("shrink-0 transition-transform", !collapsed && "rotate-90")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-[10px] font-semibold uppercase tracking-wider text-muted-foreground",
					children: title
				}),
				ports.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-[10px] text-muted-foreground/60 ml-1",
					children: ports.length
				})
			]
		}), !collapsed && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			id: `local-port-section-${id}`,
			children: ports.length > 0 ? ports.map((port) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LocalPortRow, {
				port,
				onStop: onStopPort,
				onShowDetails,
				onOpenInBrowser
			}, port.id)) : emptyText && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "py-1 text-xs text-muted-foreground",
				children: emptyText
			})
		})]
	});
}
function LocalPortRow({ port, onStop, onShowDetails, onOpenInBrowser }) {
	const handleCopy = (0, import_react.useCallback)(() => {
		window.api.ui.writeClipboardText(addressForPort(port));
	}, [port]);
	const handleOpenBrowser = (0, import_react.useCallback)((event) => {
		onOpenInBrowser(port, event);
	}, [onOpenInBrowser, port]);
	const handleCopyButtonClick = (0, import_react.useCallback)((event) => {
		handleCopy();
		if (event.detail > 0) event.currentTarget.blur();
	}, [handleCopy]);
	const handleOpenBrowserButtonClick = (0, import_react.useCallback)((event) => {
		handleOpenBrowser(event.detail > 0 ? event : void 0);
		if (event.detail > 0) event.currentTarget.blur();
	}, [handleOpenBrowser]);
	const handleStopButtonClick = (0, import_react.useCallback)((event) => {
		onStop(port);
		if (event.detail > 0) event.currentTarget.blur();
	}, [onStop, port]);
	const processLabel = port.processName ?? (port.pid ? `PID ${port.pid}` : "Unknown process");
	const address = addressForPort(port);
	const ownerLabel = port.kind === "workspace" ? port.owner.displayName : port.kind === "container" ? "Container or forwarded service" : "Unassigned";
	const openBrowserLabel = translate("auto.components.right.sidebar.PortsPanel.b22b128b2a", "Open in Browser");
	const confidenceLabel = port.kind === "workspace" ? port.owner.confidence === "cwd" ? "cwd" : "command" : null;
	const canStopProcess = port.kind === "workspace" && Boolean(port.pid) && port.processName !== "Electron";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenu, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "group flex items-center gap-2 py-1 px-1 -mx-1 rounded hover:bg-accent/50 transition-colors",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 flex-1 items-center gap-2 rounded focus:outline-none focus-visible:ring-1 focus-visible:ring-ring",
				tabIndex: 0,
				"aria-label": translate("auto.components.right.sidebar.PortsPanel.5be4f7f727", "Port {{value0}} menu", { value0: port.port }),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex size-5 shrink-0 items-center justify-center text-muted-foreground",
					children: port.kind === "container" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Box, { size: 13 }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Server, { size: 13 })
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex min-w-0 items-center gap-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "text-xs font-medium text-foreground",
								children: [":", port.port]
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "truncate text-xs text-muted-foreground",
								children: processLabel
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "flex min-w-0 items-center gap-1.5 text-[11px] text-muted-foreground",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "truncate",
								children: address
							})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex min-w-0 items-center gap-1.5 text-[10px] text-muted-foreground/70",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "truncate",
								children: ownerLabel
							}), confidenceLabel && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "shrink-0 text-muted-foreground/70",
								children: confidenceLabel
							})]
						})
					]
				})]
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipProvider, {
			delayDuration: 400,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-0.5 can-hover:opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "ghost",
							size: "icon-xs",
							className: "text-muted-foreground hover:text-foreground",
							onClick: handleOpenBrowserButtonClick,
							"aria-label": openBrowserLabel,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { size: 13 })
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
						side: "top",
						sideOffset: 4,
						children: getPortOpenBrowserTooltipLabel(openBrowserLabel)
					})] }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "ghost",
							size: "icon-xs",
							className: "text-muted-foreground hover:text-foreground",
							onClick: handleCopyButtonClick,
							"aria-label": translate("auto.components.right.sidebar.PortsPanel.fe2730d050", "Copy {{value0}}", { value0: address }),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { size: 13 })
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
						side: "top",
						sideOffset: 4,
						children: translate("auto.components.right.sidebar.PortsPanel.1004af16ab", "Copy {{value0}}", { value0: address })
					})] }),
					canStopProcess && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "ghost",
							size: "icon-xs",
							className: "text-muted-foreground hover:text-destructive",
							onClick: handleStopButtonClick,
							"aria-label": translate("auto.components.right.sidebar.PortsPanel.f9528da632", "Stop Process"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { size: 13 })
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
						side: "top",
						sideOffset: 4,
						children: translate("auto.components.right.sidebar.PortsPanel.f9528da632", "Stop Process")
					})] })
				]
			})
		})]
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuContent, {
		className: LOCAL_PORT_MENU_CONTENT_CLASS,
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuLabel, {
				className: LOCAL_PORT_MENU_LABEL_CLASS,
				children: `:${port.port}`
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
				className: LOCAL_PORT_MENU_ITEM_CLASS,
				onSelect: () => handleOpenBrowser(),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { size: 13 }), openBrowserLabel]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
				className: LOCAL_PORT_MENU_ITEM_CLASS,
				onSelect: handleCopy,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { size: 13 }), translate("auto.components.right.sidebar.PortsPanel.792baeb7ed", "Copy Address")]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
				className: LOCAL_PORT_MENU_ITEM_CLASS,
				onSelect: () => {
					window.api.ui.writeClipboardText(JSON.stringify(port, null, 2));
				},
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { size: 13 }), translate("auto.components.right.sidebar.PortsPanel.bdac206faf", "Copy Details")]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
				className: LOCAL_PORT_MENU_ITEM_CLASS,
				onSelect: () => onShowDetails(port),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Info, { size: 13 }), translate("auto.components.right.sidebar.PortsPanel.a223459512", "Show Details")]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuSeparator, {}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
				className: LOCAL_PORT_MENU_ITEM_CLASS,
				variant: "destructive",
				disabled: !canStopProcess,
				onSelect: () => onStop(port),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { size: 13 }), translate("auto.components.right.sidebar.PortsPanel.f9528da632", "Stop Process")]
			})
		]
	})] });
}
function LocalPortDetailsDialog({ port, onClose }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: Boolean(port),
		onOpenChange: (open) => !open && onClose(),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: port ? translate("auto.components.right.sidebar.PortsPanel.472054d94c", "Port :{{value0}}", { value0: port.port }) : translate("auto.components.right.sidebar.PortsPanel.d41a8241ec", "Port") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: port ? `${port.processName ?? "Unknown process"} · ${addressForPort(port)}` : "" })] }), port && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("dl", {
			className: "grid grid-cols-[88px_1fr] gap-x-3 gap-y-2 text-xs",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
					className: "text-muted-foreground",
					children: translate("auto.components.right.sidebar.PortsPanel.1c1c18cefc", "Address")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
					className: "min-w-0 break-all text-foreground",
					children: addressForPort(port)
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
					className: "text-muted-foreground",
					children: translate("auto.components.right.sidebar.PortsPanel.0f1d8cd324", "Bind")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
					className: "min-w-0 break-all text-foreground",
					children: `${port.bindHost}:${port.port}`
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
					className: "text-muted-foreground",
					children: translate("auto.components.right.sidebar.PortsPanel.729be0b4e5", "Kind")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
					className: "text-foreground",
					children: port.kind
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
					className: "text-muted-foreground",
					children: translate("auto.components.right.sidebar.PortsPanel.b1ff94fa27", "Protocol")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
					className: "text-foreground",
					children: port.protocol
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
					className: "text-muted-foreground",
					children: translate("auto.components.right.sidebar.PortsPanel.5dd86dcf2f", "Process")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
					className: "min-w-0 break-all text-foreground",
					children: port.processName ?? translate("auto.components.right.sidebar.PortsPanel.3e13cb63ee", "Unknown")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
					className: "text-muted-foreground",
					children: translate("auto.components.right.sidebar.PortsPanel.57d930fa45", "PID")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
					className: "text-foreground",
					children: port.pid ?? translate("auto.components.right.sidebar.PortsPanel.3e13cb63ee", "Unknown")
				}),
				port.kind === "workspace" && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
						className: "text-muted-foreground",
						children: translate("auto.components.right.sidebar.PortsPanel.c7b4702b7b", "Workspace")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
						className: "min-w-0 break-all text-foreground",
						children: port.owner.displayName
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
						className: "text-muted-foreground",
						children: translate("auto.components.right.sidebar.PortsPanel.153145e675", "Evidence")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
						className: "text-foreground",
						children: port.owner.confidence
					})
				] })
			]
		})] })
	});
}
function SshPortsPanel() {
	const settings = useAppStore((s) => s.settings);
	const portForwardsByConnection = useAppStore((s) => s.portForwardsByConnection);
	const detectedPortsByConnection = useAppStore((s) => s.detectedPortsByConnection);
	const sshConnectionStates = useAppStore((s) => s.sshConnectionStates);
	const activeWorktree = useActiveWorktree();
	const activeConnectionId = useRepoById(activeWorktree?.repoId ?? null)?.connectionId ?? null;
	const isDisconnected = activeConnectionId ? sshConnectionStates.get(activeConnectionId)?.status !== "connected" : true;
	const allForwards = (0, import_react.useMemo)(() => {
		if (!activeConnectionId) return [];
		return portForwardsByConnection[activeConnectionId] ?? [];
	}, [portForwardsByConnection, activeConnectionId]);
	const forwardedKeys = (0, import_react.useMemo)(() => {
		const set = /* @__PURE__ */ new Set();
		for (const f of allForwards) set.add(`${normalizeHost(f.remoteHost)}:${f.remotePort}`);
		return set;
	}, [allForwards]);
	const allDetected = (0, import_react.useMemo)(() => {
		if (!activeConnectionId) return [];
		return (detectedPortsByConnection[activeConnectionId] ?? []).filter((p) => !forwardedKeys.has(`${normalizeHost(p.host)}:${p.port}`)).map((p) => ({
			...p,
			targetId: activeConnectionId
		})).sort((a, b) => a.port - b.port);
	}, [
		detectedPortsByConnection,
		activeConnectionId,
		forwardedKeys
	]);
	const [forwardedCollapsed, setForwardedCollapsed] = (0, import_react.useState)(false);
	const [detectedCollapsed, setDetectedCollapsed] = (0, import_react.useState)(false);
	const [dialogState, setDialogState] = (0, import_react.useState)({ mode: "closed" });
	const handleForwardDetected = (0, import_react.useCallback)((port) => {
		setDialogState({
			mode: "add",
			defaults: {
				remotePort: port.port,
				remoteHost: normalizeHost(port.host),
				label: port.processName,
				targetId: port.targetId
			}
		});
	}, []);
	const handleEdit = (0, import_react.useCallback)((entry) => {
		setDialogState({
			mode: "edit",
			entry
		});
	}, []);
	const handleOpenForwardInBrowser = (0, import_react.useCallback)((entry, event) => {
		const url = browserUrlForPortForwardEntry(entry);
		if (!resolvePortOpenInOrcaBrowser({
			settings,
			event,
			isMac: navigator.userAgent.includes("Mac")
		})) {
			window.api.shell.openUrl(url);
			return;
		}
		if (!activeWorktree?.id) {
			toast.error(translate("auto.components.right.sidebar.PortsPanel.409afcc145", "No workspace selected for the browser."));
			return;
		}
		openWorkspaceBrowserTab({
			workspaceId: activeWorktree.id,
			url,
			intent: { kind: "url" }
		}).catch((error) => {
			toast.error(error instanceof Error ? error.message : String(error));
		});
	}, [activeWorktree?.id, settings]);
	const handleDialogClose = (0, import_react.useCallback)(() => {
		setDialogState({ mode: "closed" });
	}, []);
	if (isDisconnected) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col items-center justify-center h-full px-4 text-center text-muted-foreground",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Unplug, {
				size: 32,
				className: "mb-3 opacity-50"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-sm font-medium",
				children: translate("auto.components.right.sidebar.PortsPanel.a2f1a47f42", "SSH connection lost")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-xs mt-1",
				children: translate("auto.components.right.sidebar.PortsPanel.d4c3cd679c", "Reconnecting...")
			})
		]
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col h-full overflow-y-auto scrollbar-sleek",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center justify-between px-3 py-2 border-b border-border",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-[11px] font-semibold uppercase tracking-wider text-muted-foreground",
					children: translate("auto.components.right.sidebar.PortsPanel.6bc058dbe1", "Ports")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					type: "button",
					className: "flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors",
					onClick: () => setDialogState({
						mode: "add",
						defaults: { targetId: activeConnectionId ?? void 0 }
					}),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { size: 14 }), translate("auto.components.right.sidebar.PortsPanel.a103dae837", "Add")]
				})]
			}),
			allForwards.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "px-3 pt-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					type: "button",
					className: "flex items-center gap-1 w-full text-left mb-1",
					onClick: () => setForwardedCollapsed((v) => !v),
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, {
							size: 12,
							className: cn("text-muted-foreground transition-transform", !forwardedCollapsed && "rotate-90")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[10px] font-semibold uppercase tracking-wider text-muted-foreground",
							children: translate("auto.components.right.sidebar.PortsPanel.ddbe58d74e", "Forwarded")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[10px] text-muted-foreground/60 ml-1",
							children: allForwards.length
						})
					]
				}), !forwardedCollapsed && allForwards.map((entry) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ForwardedPortRow, {
					entry,
					onEdit: () => handleEdit(entry),
					onOpenInBrowser: (event) => handleOpenForwardInBrowser(entry, event)
				}, entry.id))]
			}),
			allDetected.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "px-3 pt-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					type: "button",
					className: "flex items-center gap-1 w-full text-left mb-1",
					onClick: () => setDetectedCollapsed((v) => !v),
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, {
							size: 12,
							className: cn("text-muted-foreground transition-transform", !detectedCollapsed && "rotate-90")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[10px] font-semibold uppercase tracking-wider text-muted-foreground",
							children: translate("auto.components.right.sidebar.PortsPanel.36b1b2984a", "Detected")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[10px] text-muted-foreground/60 ml-1",
							children: allDetected.length
						})
					]
				}), !detectedCollapsed && allDetected.map((port) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DetectedPortRow, {
					port,
					onForward: () => handleForwardDetected(port)
				}, `${port.targetId}-${port.host}-${port.port}`))]
			}),
			allForwards.length === 0 && allDetected.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-col items-center justify-center flex-1 px-4 text-center text-muted-foreground",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-sm",
						children: translate("auto.components.right.sidebar.PortsPanel.1f0d2a24f9", "No forwarded ports")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs mt-1 mb-3",
						children: translate("auto.components.right.sidebar.PortsPanel.04efd3dad4", "Forward a port to access remote services on your local machine.")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						className: "text-xs px-3 py-1.5 rounded bg-primary text-primary-foreground hover:bg-primary/90 transition-colors",
						onClick: () => setDialogState({
							mode: "add",
							defaults: { targetId: activeConnectionId ?? void 0 }
						}),
						children: translate("auto.components.right.sidebar.PortsPanel.907eb53ed2", "Forward a Port")
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PortForwardDialog, {
				state: dialogState,
				activeConnectionId,
				onClose: handleDialogClose
			})
		]
	});
}
function ForwardedPortRow({ entry, onEdit, onOpenInBrowser }) {
	const [removing, setRemoving] = (0, import_react.useState)(false);
	const mountedRef = useMountedRef();
	const forwardedAddress = addressForPortForwardEntry(entry);
	const handleRemove = (0, import_react.useCallback)(async () => {
		setRemoving(true);
		try {
			await window.api.ssh.removePortForward({ id: entry.id });
		} catch {}
		if (mountedRef.current) setRemoving(false);
	}, [entry.id, mountedRef]);
	const handleCopy = (0, import_react.useCallback)(() => {
		window.api.ui.writeClipboardText(forwardedAddress);
	}, [forwardedAddress]);
	const handleOpenBrowser = (0, import_react.useCallback)((event) => {
		onOpenInBrowser(event);
	}, [onOpenInBrowser]);
	const handleCopyButtonClick = (0, import_react.useCallback)((event) => {
		handleCopy();
		if (event.detail > 0) event.currentTarget.blur();
	}, [handleCopy]);
	const handleOpenBrowserButtonClick = (0, import_react.useCallback)((event) => {
		handleOpenBrowser(event.detail > 0 ? event : void 0);
		if (event.detail > 0) event.currentTarget.blur();
	}, [handleOpenBrowser]);
	const handleEditButtonClick = (0, import_react.useCallback)((event) => {
		onEdit();
		if (event.detail > 0) event.currentTarget.blur();
	}, [onEdit]);
	const handleRemoveButtonClick = (0, import_react.useCallback)((event) => {
		handleRemove();
		if (event.detail > 0) event.currentTarget.blur();
	}, [handleRemove]);
	const advertisedBrowserUrl = advertisedBrowserUrlForForwardedRow(entry);
	const openBrowserLabel = translate("auto.components.right.sidebar.PortsPanel.b22b128b2a", "Open in Browser");
	const openBrowserTitle = getPortOpenBrowserTooltipLabel(advertisedBrowserUrl ? translate("auto.components.right.sidebar.PortsPanel.75aeea592f", "Open {{value0}} in Browser", { value0: advertisedBrowserUrl }) : openBrowserLabel);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "group flex items-center gap-2 py-1 px-1 -mx-1 rounded hover:bg-accent/50 transition-colors",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex-1 min-w-0",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-1.5",
				children: [entry.label && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-xs font-medium text-foreground truncate",
					children: entry.label
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: cn("text-xs text-muted-foreground truncate", !entry.label && "text-foreground"),
					children: [
						":",
						entry.localPort,
						" → :",
						entry.remotePort
					]
				})]
			}), advertisedBrowserUrl && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "text-[11px] text-muted-foreground/70 truncate",
				children: translate("auto.components.right.sidebar.PortsPanel.de349d4560", "opens {{value0}}", { value0: advertisedBrowserUrl })
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center gap-0.5 can-hover:opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					className: "p-1 rounded hover:bg-accent transition-colors text-muted-foreground hover:text-foreground",
					onClick: handleOpenBrowserButtonClick,
					title: openBrowserTitle,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { size: 13 })
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					className: "p-1 rounded hover:bg-accent transition-colors text-muted-foreground hover:text-foreground",
					onClick: handleCopyButtonClick,
					title: translate("auto.components.right.sidebar.PortsPanel.1004af16ab", "Copy {{value0}}", { value0: forwardedAddress }),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { size: 13 })
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					className: "p-1 rounded hover:bg-accent transition-colors text-muted-foreground hover:text-foreground",
					onClick: handleEditButtonClick,
					title: translate("auto.components.right.sidebar.PortsPanel.b3548e59f4", "Edit"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pencil, { size: 13 })
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					className: cn("p-1 rounded hover:bg-accent transition-colors text-muted-foreground hover:text-foreground", removing && "opacity-50"),
					onClick: handleRemoveButtonClick,
					disabled: removing,
					title: translate("auto.components.right.sidebar.PortsPanel.e740075063", "Remove"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { size: 13 })
				})
			]
		})]
	});
}
function DetectedPortRow({ port, onForward }) {
	const advertisedBrowserUrl = advertisedBrowserUrlForDetectedPort(port);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "group flex items-center gap-2 py-1 px-1 -mx-1 rounded hover:bg-accent/50 transition-colors",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex-1 min-w-0",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "text-xs text-foreground",
					children: [":", port.port]
				}), port.processName && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-xs text-muted-foreground truncate",
					children: port.processName
				})]
			}), advertisedBrowserUrl && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "text-[11px] text-muted-foreground/70 truncate",
				children: translate("auto.components.right.sidebar.PortsPanel.c7e920aa7c", "advertised as {{value0}}", { value0: advertisedBrowserUrl })
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
			type: "button",
			className: "text-[11px] px-2 py-0.5 rounded can-hover:opacity-0 group-hover:opacity-100 transition-opacity bg-accent hover:bg-accent/80 text-foreground",
			onClick: onForward,
			children: translate("auto.components.right.sidebar.PortsPanel.c9d106547a", "Forward")
		})]
	});
}
function digitsOnly(value) {
	return value.replace(/\D/g, "");
}
var INPUT_CLASS = "block w-full mt-0.5 px-2 py-1.5 text-xs rounded border border-border bg-background text-foreground focus:outline-none focus:ring-1 focus:ring-ring";
function PortForwardDialog({ state, activeConnectionId, onClose }) {
	const isOpen = state.mode !== "closed";
	const isEdit = state.mode === "edit";
	const initialRemotePort = state.mode === "edit" ? state.entry.remotePort.toString() : state.mode === "add" ? state.defaults.remotePort?.toString() ?? "" : "";
	const initialLocalPort = state.mode === "edit" ? state.entry.localPort.toString() : state.mode === "add" && state.defaults.remotePort != null ? safeLocalPort(state.defaults.remotePort).toString() : "";
	const initialRemoteHost = state.mode === "edit" ? state.entry.remoteHost : state.mode === "add" ? state.defaults.remoteHost ?? "localhost" : "localhost";
	const initialLabel = state.mode === "edit" ? state.entry.label ?? "" : state.mode === "add" ? state.defaults.label ?? "" : "";
	const targetId = state.mode === "edit" ? state.entry.connectionId : state.mode === "add" ? state.defaults.targetId ?? activeConnectionId ?? "" : activeConnectionId ?? "";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: isOpen,
		onOpenChange: (open) => {
			if (!open) onClose();
		},
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			showCloseButton: false,
			className: "max-w-[340px]",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
				className: "text-sm",
				children: isEdit ? translate("auto.components.right.sidebar.PortsPanel.80206251c8", "Edit Port Forward") : translate("auto.components.right.sidebar.PortsPanel.907eb53ed2", "Forward a Port")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
				className: "text-xs",
				children: isEdit ? translate("auto.components.right.sidebar.PortsPanel.10360598a4", "Update the port forwarding configuration.") : translate("auto.components.right.sidebar.PortsPanel.31e80cff2d", "Forward a remote port to your local machine.")
			})] }), isOpen && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PortForwardForm, {
				mode: state.mode,
				editId: state.mode === "edit" ? state.entry.id : void 0,
				initialRemotePort,
				initialLocalPort,
				initialRemoteHost,
				initialLabel,
				targetId,
				onClose
			}, state.mode === "edit" ? `edit-${state.entry.id}` : `add-${targetId}-${initialRemotePort}-${initialRemoteHost}`)]
		})
	});
}
function PortForwardForm({ mode, editId, initialRemotePort, initialLocalPort, initialRemoteHost, initialLabel, targetId, onClose }) {
	const [remotePort, setRemotePort] = (0, import_react.useState)(initialRemotePort);
	const [localPort, setLocalPort] = (0, import_react.useState)(initialLocalPort);
	const [remoteHost, setRemoteHost] = (0, import_react.useState)(initialRemoteHost);
	const [label, setLabel] = (0, import_react.useState)(initialLabel);
	const [error, setError] = (0, import_react.useState)(null);
	const [submitting, setSubmitting] = (0, import_react.useState)(false);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
		onSubmit: (0, import_react.useCallback)(async (e) => {
			e.preventDefault();
			setError(null);
			const rPort = Number.parseInt(remotePort, 10);
			const lPort = Number.parseInt(localPort || remotePort, 10);
			if (Number.isNaN(rPort) || rPort < 1 || rPort > 65535) {
				setError("Remote port must be 1–65535");
				return;
			}
			if (Number.isNaN(lPort) || lPort < 1 || lPort > 65535) {
				setError("Local port must be 1–65535");
				return;
			}
			setSubmitting(true);
			try {
				await (mode === "edit" && editId ? window.api.ssh.updatePortForward({
					id: editId,
					targetId,
					localPort: lPort,
					remoteHost: remoteHost || "localhost",
					remotePort: rPort,
					label: label || void 0
				}) : window.api.ssh.addPortForward({
					targetId,
					localPort: lPort,
					remoteHost: remoteHost || "localhost",
					remotePort: rPort,
					label: label || void 0
				}));
				onClose();
			} catch (err) {
				const msg = err instanceof Error ? err.message : String(err);
				if (msg.includes("EADDRINUSE") || msg.includes("already in use")) setError(`Port ${lPort} is already in use. Choose a different local port.`);
				else if (msg.includes("EACCES") || msg.includes("permission denied")) setError(`Port ${lPort} requires elevated privileges. Use a local port \u2265 1024.`);
				else setError(msg);
			}
			setSubmitting(false);
		}, [
			mode,
			editId,
			remotePort,
			localPort,
			remoteHost,
			label,
			targetId,
			onClose
		]),
		className: "space-y-3",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-2",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
						className: "block",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[11px] text-muted-foreground",
							children: translate("auto.components.right.sidebar.PortsPanel.9e5a4118b0", "Remote Port")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
							type: "text",
							inputMode: "numeric",
							value: remotePort,
							onChange: (e) => {
								const val = digitsOnly(e.target.value);
								setRemotePort(val);
								const prev = Number.parseInt(remotePort, 10);
								const cur = Number.parseInt(localPort, 10);
								if (!localPort || cur === prev || cur === safeLocalPort(prev)) {
									const parsed = Number.parseInt(val, 10);
									setLocalPort(Number.isNaN(parsed) ? "" : safeLocalPort(parsed).toString());
								}
							},
							className: INPUT_CLASS,
							placeholder: "3000",
							autoFocus: true,
							required: true
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
						className: "block",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[11px] text-muted-foreground",
							children: translate("auto.components.right.sidebar.PortsPanel.b950b1948b", "Local Port")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
							type: "text",
							inputMode: "numeric",
							value: localPort,
							onChange: (e) => setLocalPort(digitsOnly(e.target.value)),
							className: INPUT_CLASS,
							placeholder: translate("auto.components.right.sidebar.PortsPanel.d57545ff92", "Same as remote")
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
						className: "block",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[11px] text-muted-foreground",
							children: translate("auto.components.right.sidebar.PortsPanel.a3721a50b0", "Remote Host")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
							type: "text",
							value: remoteHost,
							onChange: (e) => setRemoteHost(e.target.value),
							className: INPUT_CLASS,
							placeholder: translate("auto.components.right.sidebar.PortsPanel.17bea6e391", "localhost")
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
						className: "block",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[11px] text-muted-foreground",
							children: translate("auto.components.right.sidebar.PortsPanel.8dfed0a15c", "Label (optional)")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
							type: "text",
							value: label,
							onChange: (e) => setLabel(e.target.value),
							className: INPUT_CLASS,
							placeholder: translate("auto.components.right.sidebar.PortsPanel.4eb801ce93", "dev-server")
						})]
					})
				]
			}),
			error && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "text-[11px] text-destructive",
				children: error
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex justify-end gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "outline",
					size: "sm",
					onClick: onClose,
					children: translate("auto.components.right.sidebar.PortsPanel.3ea4a02a8f", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "submit",
					size: "sm",
					disabled: submitting || !remotePort,
					children: submitting ? mode === "edit" ? translate("auto.components.right.sidebar.PortsPanel.d7c83cfd24", "Saving...") : translate("auto.components.right.sidebar.PortsPanel.9f475dc994", "Forwarding...") : mode === "edit" ? translate("auto.components.right.sidebar.PortsPanel.9079776663", "Save") : translate("auto.components.right.sidebar.PortsPanel.c9d106547a", "Forward")
				})]
			})
		]
	});
}
export { PortsPanel as default, getLocalWorkspacePortSections, killWorkspacePortForTarget, openWorkspacePortInBrowser, scanWorkspacePortsForTarget };
