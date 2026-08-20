import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as ChevronRight } from "./chevron-right-CZtMe6Ev.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as FolderOpen } from "./folder-open-B2ZB-rfY.js";
import "./worktree-activation-BDsaiyMf.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as Plug } from "./plug-BxC_10cO.js";
import { Ou as getRuntimeEnvironmentIdForWorktree, m_ as Trash2, op as getActiveRuntimeTarget, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { i as PopoverTrigger, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
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
import { t as SelectedTextCopyMenu } from "./SelectedTextCopyMenu-CX36uwfX.js";
import { a as goToWorkspacePortOwner, c as openWorkspacePortInBrowser, f as scanWorkspacePortsForTarget, h as addressForPort, i as getPortOpenBrowserTooltipLabel, l as refreshWorkspacePortScanAfterStop, m as workspacePortScanKeyForTarget, n as useLocalhostLabelRouteForPort, o as killWorkspacePortForTarget, r as canStopWorkspacePort, u as resolvePortOpenInOrcaBrowser } from "./workspace-port-localhost-label-selector-BFjPG9iO.js";
import { n as getWorkspacePortGroups, t as getExternalWorkspacePorts } from "./workspace-port-groups-BBhpEP1l.js";
import { t as STATUS_BAR_CONTEXT_MENU_EXEMPT_PROPS } from "./status-bar-context-menu-policy-Dc4KA2ce.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function PortAction({ label, tooltipLabel = label, onClick, disabled, children }) {
	const handleClick = (event) => {
		onClick(event);
		if (event.detail > 0) event.currentTarget.blur();
	};
	const button = /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
		type: "button",
		variant: "ghost",
		size: "icon-xs",
		className: "size-5 text-muted-foreground hover:text-foreground disabled:pointer-events-none disabled:text-muted-foreground/35",
		"aria-label": label,
		onClick: handleClick,
		disabled,
		children
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, {
		delayDuration: 200,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
			asChild: true,
			children: disabled ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "inline-flex",
				children: button
			}) : button
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
			side: "top",
			sideOffset: 4,
			className: "z-[70]",
			children: tooltipLabel
		})]
	});
}
function PortRow({ port, activeWorktreeId, external }) {
	const settings = useAppStore((s) => s.settings);
	const localhostLabelRoute = useLocalhostLabelRouteForPort(port);
	const runtimeEnvironmentId = useAppStore((s) => getRuntimeEnvironmentIdForWorktree(s, port.kind === "workspace" ? port.owner.worktreeId : activeWorktreeId));
	const createBrowserTab = useAppStore((s) => s.createBrowserTab);
	const setRemoteBrowserPageHandle = useAppStore((s) => s.setRemoteBrowserPageHandle);
	const setWorkspacePortScan = useAppStore((s) => s.setWorkspacePortScan);
	const setWorkspacePortScanForKey = useAppStore((s) => s.setWorkspacePortScanForKey);
	const setWorkspacePortScanRefreshing = useAppStore((s) => s.setWorkspacePortScanRefreshing);
	const recordFeatureInteraction = useAppStore((s) => s.recordFeatureInteraction);
	const runtimeTarget = (0, import_react.useMemo)(() => getActiveRuntimeTarget({
		...settings,
		activeRuntimeEnvironmentId: runtimeEnvironmentId
	}), [runtimeEnvironmentId, settings]);
	const processLabel = port.processName ?? (port.pid ? `PID ${port.pid}` : "Unknown process");
	const canStop = canStopWorkspacePort(port);
	const openBrowserLabel = translate("auto.components.status.bar.ports.status.popover.rows.085f4f0334", "Open in Browser");
	const handleOpen = (0, import_react.useCallback)((event) => {
		event.stopPropagation();
		recordFeatureInteraction("ports");
		openWorkspacePortInBrowser({
			port,
			activeWorktreeId,
			runtimeTarget,
			createBrowserTab,
			setRemoteBrowserPageHandle,
			openInOrcaBrowser: resolvePortOpenInOrcaBrowser({
				settings,
				event: event.detail > 0 ? event : null,
				isMac: navigator.userAgent.includes("Mac")
			}),
			localhostLabelRoute
		}).then((result) => {
			if (!result.ok) toast.error(translate("auto.components.status.bar.ports.status.popover.rows.b854ec9ff5", "Failed to open browser"), { description: result.reason });
		});
	}, [
		activeWorktreeId,
		createBrowserTab,
		localhostLabelRoute,
		port,
		recordFeatureInteraction,
		runtimeTarget,
		settings,
		setRemoteBrowserPageHandle
	]);
	const handleCopy = (0, import_react.useCallback)((event) => {
		event.stopPropagation();
		recordFeatureInteraction("ports");
		const address = addressForPort(port);
		window.api.ui.writeClipboardText(address);
		toast.success(translate("auto.components.status.bar.ports.status.popover.rows.480d8f2347", "Copied {{value0}}", { value0: address }));
	}, [port, recordFeatureInteraction]);
	const handleStop = (0, import_react.useCallback)((event) => {
		event.stopPropagation();
		if (!canStopWorkspacePort(port)) return;
		recordFeatureInteraction("ports");
		const run = async () => {
			const result = await killWorkspacePortForTarget(runtimeTarget, {
				repoId: port.owner.repoId,
				pid: port.pid,
				port: port.port
			});
			if (!result.ok) {
				toast.error(result.reason);
				return;
			}
			toast.success(translate("auto.components.status.bar.ports.status.popover.rows.acdb6df590", "Stopped process on {{value0}}", { value0: port.port }));
			const refreshResult = await refreshWorkspacePortScanAfterStop({
				runtimeTarget,
				setWorkspacePortScan,
				setWorkspacePortScanForKey,
				getWorkspacePortScansByKey: () => useAppStore.getState().workspacePortScansByKey,
				setWorkspacePortScanRefreshing
			});
			if (!refreshResult.ok) toast.error(translate("auto.components.status.bar.ports.status.popover.rows.e4a709548c", "Failed to refresh ports"), { description: refreshResult.reason });
		};
		run();
	}, [
		port,
		recordFeatureInteraction,
		runtimeTarget,
		setWorkspacePortScan,
		setWorkspacePortScanForKey,
		setWorkspacePortScanRefreshing
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "group/port grid min-w-0 grid-cols-[4.5rem_minmax(0,1fr)] items-start gap-2 rounded-md px-2 py-1.5 hover:bg-accent/50",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "select-text font-mono text-[12px] font-semibold tabular-nums text-foreground",
			children: port.port
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "min-w-0 space-y-0.5",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "relative flex h-5 min-w-0 items-center",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, {
					delayDuration: 200,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "block min-w-0 select-text truncate text-[11px] text-muted-foreground",
							children: processLabel
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
						side: "top",
						sideOffset: 4,
						children: processLabel
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "absolute inset-y-0 right-0 flex items-center gap-0.5 rounded-md border border-border/40 bg-popover/95 px-0.5 can-hover:opacity-0 shadow-xs transition-opacity group-hover/port:opacity-100 group-focus-within/port:opacity-100",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PortAction, {
							label: openBrowserLabel,
							tooltipLabel: getPortOpenBrowserTooltipLabel(openBrowserLabel),
							onClick: handleOpen,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3" })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PortAction, {
							label: translate("auto.components.status.bar.ports.status.popover.rows.536d48a5dc", "Copy {{value0}}", { value0: addressForPort(port) }),
							onClick: handleCopy,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { className: "size-3" })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PortAction, {
							label: translate("auto.components.status.bar.ports.status.popover.rows.0e72c8d9fb", "Stop Process"),
							disabled: !canStop,
							onClick: handleStop,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3" })
						})
					]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "select-text truncate text-[10px] text-muted-foreground/70",
				children: external ? port.kind : addressForPort(port)
			})]
		})]
	});
}
function WorkspaceGroupRows({ group, activeWorktreeId }) {
	const handleGoToWorkspace = (0, import_react.useCallback)((event) => {
		event.stopPropagation();
		const ownerPort = group.ports[0];
		if (!ownerPort || !goToWorkspacePortOwner(ownerPort)) toast.error(translate("auto.components.status.bar.ports.status.popover.rows.f2b813345f", "Workspace unavailable"));
	}, [group.ports]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
		className: "border-t border-border/40 first:border-t-0",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "sticky top-0 z-10 flex items-center justify-between gap-2 border-b border-border/40 bg-popover px-3 py-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-0 truncate text-[12px] font-medium text-foreground",
				children: group.displayName
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex shrink-0 items-center gap-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PortAction, {
					label: translate("auto.components.status.bar.ports.status.popover.rows.a49ea79246", "Go to Worktree"),
					onClick: handleGoToWorkspace,
					disabled: group.ports.length === 0,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderOpen, { className: "size-3" })
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "font-mono text-[10px] text-muted-foreground/70",
					children: group.ports.length
				})]
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "px-1 pb-1",
			children: group.ports.map((port) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PortRow, {
				port,
				activeWorktreeId
			}, port.id))
		})]
	});
}
function PortsStatusSegment({ iconOnly }) {
	const settings = useAppStore((s) => s.settings);
	const scan = useAppStore((s) => s.workspacePortScan?.result ?? null);
	const refreshing = useAppStore((s) => s.workspacePortScanRefreshing);
	const activeWorktreeId = useAppStore((s) => s.activeWorktreeId);
	const setWorkspacePortScan = useAppStore((s) => s.setWorkspacePortScan);
	const setWorkspacePortScanForKey = useAppStore((s) => s.setWorkspacePortScanForKey);
	const recordFeatureInteraction = useAppStore((s) => s.recordFeatureInteraction);
	const [open, setOpen] = (0, import_react.useState)(false);
	const [externalOpen, setExternalOpen] = (0, import_react.useState)(false);
	const runtimeTarget = (0, import_react.useMemo)(() => getActiveRuntimeTarget(settings), [settings]);
	const scanKey = workspacePortScanKeyForTarget(runtimeTarget);
	const workspaceGroups = (0, import_react.useMemo)(() => getWorkspacePortGroups(scan), [scan]);
	const externalPorts = (0, import_react.useMemo)(() => getExternalWorkspacePorts(scan), [scan]);
	const workspacePortCount = workspaceGroups.reduce((count, group) => count + group.ports.length, 0);
	const totalCount = workspacePortCount + externalPorts.length;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
		open,
		onOpenChange: (0, import_react.useCallback)((nextOpen) => {
			setOpen(nextOpen);
			if (!nextOpen) return;
			recordFeatureInteraction("ports");
			scanWorkspacePortsForTarget(runtimeTarget).then((result) => {
				setWorkspacePortScanForKey(scanKey, result);
				setWorkspacePortScan({
					key: scanKey,
					result
				});
			}).catch((error) => {
				const message = error instanceof Error ? error.message : String(error);
				setWorkspacePortScan({
					key: scanKey,
					result: {
						platform: "unknown",
						scannedAt: Date.now(),
						ports: [],
						unavailableReason: message || "Workspace port scan failed."
					}
				});
			});
		}, [
			recordFeatureInteraction,
			runtimeTarget,
			scanKey,
			setWorkspacePortScan,
			setWorkspacePortScanForKey
		]),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, {
			delayDuration: 150,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						...STATUS_BAR_CONTEXT_MENU_EXEMPT_PROPS,
						className: "inline-flex cursor-pointer items-center gap-1.5 rounded px-1 py-0.5 hover:bg-accent/70",
						"aria-label": translate("auto.components.status.bar.PortsStatusSegment.b8bc3e420a", "Ports, {{value0}} workspace {{value1}}", {
							value0: workspacePortCount,
							value1: workspacePortCount === 1 ? "port" : "ports"
						}),
						children: [
							refreshing ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin text-muted-foreground" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plug, { className: "size-3 text-muted-foreground" }),
							!iconOnly && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-[11px] font-medium tabular-nums text-muted-foreground",
								children: workspacePortCount
							}),
							iconOnly && totalCount > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-[11px] tabular-nums text-muted-foreground",
								children: workspacePortCount
							})
						]
					})
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
				side: "top",
				sideOffset: 6,
				children: translate("auto.components.status.bar.PortsStatusSegment.ca41be2802", "Ports — {{value0}} workspace {{value1}}{{value2}}", {
					value0: workspacePortCount,
					value1: workspacePortCount === 1 ? translate("auto.components.status.bar.PortsStatusSegment.45834a9ace", "port") : translate("auto.components.status.bar.PortsStatusSegment.8caaa86e9a", "ports"),
					value2: externalPorts.length > 0 ? translate("auto.components.status.bar.PortsStatusSegment.a8e4bdb412", " · {{value0}} external", { value0: externalPorts.length }) : ""
				})
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverContent, {
			side: "top",
			align: "end",
			sideOffset: 8,
			...STATUS_BAR_CONTEXT_MENU_EXEMPT_PROPS,
			className: "w-[24rem] max-w-[calc(100vw-2rem)] p-0",
			onOpenAutoFocus: (event) => event.preventDefault(),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(SelectedTextCopyMenu, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center justify-between gap-2 border-b border-border px-3 py-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex min-w-0 items-center gap-1.5 text-[11px] font-medium text-foreground",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plug, { className: "size-3 shrink-0 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "truncate",
						children: translate("auto.components.status.bar.PortsStatusSegment.c22ea609fd", "Ports")
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-[11px] tabular-nums text-muted-foreground",
					children: translate("auto.components.status.bar.PortsStatusSegment.2b84c4d11f", "{{value0}} workspace · {{value1}} external", {
						value0: workspacePortCount,
						value1: externalPorts.length
					})
				})]
			}), scan?.unavailableReason ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "px-3 py-3 text-xs text-muted-foreground",
				children: translate("auto.components.status.bar.PortsStatusSegment.95495019ed", "Port scan unavailable on {{value0}}: {{value1}}", {
					value0: scan.platform,
					value1: scan.unavailableReason
				})
			}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "max-h-[28rem] overflow-y-auto scrollbar-sleek",
				children: [workspaceGroups.length > 0 ? workspaceGroups.map((group) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceGroupRows, {
					group,
					activeWorktreeId
				}, group.worktreeId)) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "px-3 py-4 text-center text-xs text-muted-foreground",
					children: refreshing ? translate("auto.components.status.bar.PortsStatusSegment.c174bbbfed", "Scanning for workspace ports...") : translate("auto.components.status.bar.PortsStatusSegment.3a87d54dfb", "No workspace ports detected")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
					className: "border-t border-border/60",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						className: "sticky top-0 z-10 flex w-full items-center gap-1.5 border-b border-border/40 bg-popover px-3 py-2 text-left text-[11px] font-medium uppercase tracking-[0.05em] text-muted-foreground hover:bg-accent/50 hover:text-foreground",
						"aria-expanded": externalOpen,
						onClick: () => {
							recordFeatureInteraction("ports");
							setExternalOpen((value) => !value);
						},
						children: [
							externalOpen ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: "size-3" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "size-3" }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.status.bar.PortsStatusSegment.7dac3ecc9d", "External Ports") }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "ml-auto font-mono text-[10px]",
								children: externalPorts.length
							})
						]
					}), externalOpen && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "px-1 pb-1",
						children: externalPorts.length > 0 ? externalPorts.map((port) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PortRow, {
							port,
							activeWorktreeId,
							external: true
						}, port.id)) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "px-2 py-2 text-xs text-muted-foreground",
							children: translate("auto.components.status.bar.PortsStatusSegment.4ebf90c12e", "No external ports detected")
						})
					})]
				})]
			})] })
		})]
	});
}
export { PortsStatusSegment };
