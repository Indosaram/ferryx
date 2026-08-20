import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as CircleStop } from "./circle-stop-ByijsK0o.js";
import { t as MonitorSmartphone } from "./monitor-smartphone-BFYO8mmD.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as Pencil } from "./pencil-CLc9a5do.js";
import { t as RotateCcw } from "./rotate-ccw-hK0RKgaG.js";
import { t as ServerOff } from "./server-off-xGmv1av8.js";
import { t as Server } from "./server-DYdwnXME.js";
import { m_ as Trash2 } from "./store-CgXrfmaH.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { n as isConnectingSshStatus } from "./ssh-connection-recoverability-CNHp0WBp.js";
import { n as DEFAULT_SSH_RELAY_GRACE_PERIOD_SECONDS } from "./ssh-types-Caw2Ltsn.js";
function isRuntimeWorkspaceWindowClosed(status) {
	if (!status) return false;
	return status.graphStatus !== "ready" && status.desktopWindowStatus === "openable";
}
function runtimeHostConnectionState({ hasStatusEntry, status }) {
	if (!hasStatusEntry) return "checking";
	const remoteControl = status?.remoteControl;
	if (remoteControl?.state === "reconnecting") return "reconnecting";
	if (!status) return "disconnected";
	if (remoteControl?.state === "closed" && remoteControl.lastError) return "disconnected";
	if (isRuntimeWorkspaceWindowClosed(status)) return "workspace-window-closed";
	return "connected";
}
function runtimeStatusForOverall(state) {
	switch (state) {
		case "connected":
		case "workspace-window-closed": return "connected";
		case "checking":
		case "reconnecting": return "connecting";
		case "disconnected": return "disconnected";
	}
}
function isConnectedRuntimeHostState(state) {
	return state === "connected" || state === "workspace-window-closed";
}
function isSshTargetConnecting(status) {
	return isConnectingSshStatus(status);
}
function shouldClearPendingSshReset({ pendingTargetId, pendingResetIsBusy, connectionStatus }) {
	return pendingTargetId !== null && !pendingResetIsBusy && isSshTargetConnecting(connectionStatus);
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
const STATUS_LABELS = {
	disconnected: "Disconnected",
	connecting: "Connecting…",
	"auth-failed": "Auth failed",
	"deploying-relay": "Deploying relay…",
	connected: "Connected",
	reconnecting: "Reconnecting…",
	"reconnection-failed": "Reconnection failed",
	get error() {
		return translate("auto.components.settings.SshTargetCard.18968ede9e", "Error");
	}
};
function statusColor(status) {
	switch (status) {
		case "connected": return "bg-emerald-500";
		case "connecting":
		case "deploying-relay":
		case "reconnecting": return "bg-yellow-500";
		case "auth-failed":
		case "reconnection-failed":
		case "error": return "bg-red-500";
		case "disconnected": return "bg-muted-foreground/40";
	}
}
function formatGraceDuration(seconds) {
	if (seconds % 86400 === 0) return `${seconds / 86400}d`;
	if (seconds % 3600 === 0) return `${seconds / 3600}h`;
	if (seconds % 60 === 0) return `${seconds / 60}m`;
	return `${seconds}s`;
}
function formatTerminalPersistence(target) {
	const graceSeconds = target.relayGracePeriodSeconds ?? 0;
	if (graceSeconds === 0) return translate("auto.components.settings.SshTargetCard.8ce71262f4", "terminals until reset");
	return translate("auto.components.settings.SshTargetCard.a883f5a00f", "terminal timeout: {{value0}}", { value0: formatGraceDuration(graceSeconds) });
}
function SshTargetCard({ target, state, testing, busyAction, onConnect, onDisconnect, onTerminateSessions, onResetRelay, onTest, onEdit, onRemove }) {
	const status = state?.status ?? "disconnected";
	const [actionInFlight, setActionInFlight] = (0, import_react.useState)(null);
	const hasActionInFlight = actionInFlight !== null || busyAction !== void 0;
	const terminateInFlight = actionInFlight === "terminate" || busyAction === "terminate";
	const resetInFlight = actionInFlight === "reset" || busyAction === "reset";
	const removeInFlight = busyAction === "remove";
	const mountedRef = (0, import_react.useRef)(true);
	const endpoint = target.username ? `${target.username}@${target.host}:${target.port}` : `${target.host}:${target.port}`;
	const terminalPersistence = formatTerminalPersistence(target);
	const handleCardRef = (0, import_react.useCallback)((node) => {
		mountedRef.current = node !== null;
	}, []);
	const clearActionInFlight = () => {
		if (mountedRef.current) setActionInFlight(null);
	};
	const handleConnect = () => {
		if (actionInFlight) return;
		setActionInFlight("connect");
		Promise.resolve(onConnect(target.id)).finally(clearActionInFlight);
	};
	const handleDisconnect = () => {
		if (actionInFlight) return;
		setActionInFlight("disconnect");
		Promise.resolve(onDisconnect(target.id)).finally(clearActionInFlight);
	};
	const handleTerminateSessions = () => {
		if (actionInFlight) return;
		setActionInFlight("terminate");
		Promise.resolve(onTerminateSessions(target.id)).finally(clearActionInFlight);
	};
	const handleResetRelay = () => {
		if (actionInFlight) return;
		setActionInFlight("reset");
		Promise.resolve(onResetRelay(target.id)).finally(clearActionInFlight);
	};
	const renderEndRemoteTerminalsButton = () => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
			variant: "ghost",
			size: "icon",
			onClick: handleTerminateSessions,
			className: "size-7 text-muted-foreground hover:text-red-400",
			disabled: hasActionInFlight,
			"aria-label": terminateInFlight ? translate("auto.components.settings.SshTargetCard.c77f1abfe3", "Ending remote terminals") : translate("auto.components.settings.SshTargetCard.da16e108e6", "End remote terminals"),
			children: terminateInFlight ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleStop, { className: "size-3" })
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "top",
		sideOffset: 4,
		children: translate("auto.components.settings.SshTargetCard.da16e108e6", "End remote terminals")
	})] });
	const renderResetRelayButton = () => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
			variant: "ghost",
			size: "icon",
			onClick: handleResetRelay,
			className: "size-7 text-muted-foreground hover:text-red-400",
			disabled: hasActionInFlight,
			"aria-label": resetInFlight ? translate("auto.components.settings.SshTargetCard.97dea4e8cf", "Resetting remote relay") : translate("auto.components.settings.SshTargetCard.762a48c662", "Reset remote relay"),
			children: resetInFlight ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RotateCcw, { className: "size-3" })
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "top",
		sideOffset: 4,
		children: translate("auto.components.settings.SshTargetCard.762a48c662", "Reset remote relay")
	})] });
	const renderSecondaryIconActions = (includeEndRemoteTerminals) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-1",
		children: [
			includeEndRemoteTerminals ? renderEndRemoteTerminalsButton() : null,
			isSshTargetConnecting(status) ? null : renderResetRelayButton(),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon",
					onClick: () => onEdit(target),
					className: "size-7",
					disabled: hasActionInFlight,
					"aria-label": translate("auto.components.settings.SshTargetCard.3d8af2949f", "Edit target"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pencil, { className: "size-3" })
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
				side: "top",
				sideOffset: 4,
				children: translate("auto.components.settings.SshTargetCard.3d8af2949f", "Edit target")
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon",
					onClick: () => onRemove(target.id),
					className: "size-7 text-muted-foreground hover:text-red-400",
					disabled: hasActionInFlight,
					"aria-label": removeInFlight ? translate("auto.components.settings.SshTargetCard.3d21a22d0e", "Removing target") : translate("auto.components.settings.SshTargetCard.7f7b3d7ab4", "Remove target"),
					children: removeInFlight ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3" })
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
				side: "top",
				sideOffset: 4,
				children: translate("auto.components.settings.SshTargetCard.7f7b3d7ab4", "Remove target")
			})] })
		]
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: handleCardRef,
		"data-ssh-target-card": "",
		"data-ssh-target-label": target.label,
		className: "flex items-center gap-3 rounded-lg border border-border/50 bg-card/40 px-4 py-3",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Server, { className: "size-4 shrink-0 text-muted-foreground" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 flex-1",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-2",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "truncate text-sm font-medium",
								children: target.label
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: `size-2 shrink-0 rounded-full ${statusColor(status)}` }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-[11px] text-muted-foreground",
								children: STATUS_LABELS[status]
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
						className: "truncate text-xs text-muted-foreground",
						children: [
							endpoint,
							target.identityFile ? ` \u2022 ${target.identityFile}` : "",
							` \u2022 ${terminalPersistence}`
						]
					}),
					state?.error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "mt-0.5 truncate text-xs text-red-400",
						children: state.error
					}) : null
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "flex shrink-0 items-center gap-1",
				children: status === "connected" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [renderSecondaryIconActions(true), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					variant: "ghost",
					size: "xs",
					onClick: handleDisconnect,
					className: "gap-1.5",
					disabled: hasActionInFlight,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ServerOff, { className: "size-3" }), translate("auto.components.settings.SshTargetCard.4c86f30877", "Disconnect")]
				})] }) : isSshTargetConnecting(status) ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [renderSecondaryIconActions(false), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					variant: "ghost",
					size: "xs",
					disabled: true,
					className: "gap-1.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }), translate("auto.components.settings.SshTargetCard.1810b51482", "Connecting")]
				})] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
					renderSecondaryIconActions(true),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						variant: "ghost",
						size: "xs",
						onClick: () => onTest(target.id),
						disabled: testing || hasActionInFlight,
						className: "gap-1.5",
						children: [testing ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MonitorSmartphone, { className: "size-3" }), translate("auto.components.settings.SshTargetCard.0e53e9f8e8", "Test")]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						variant: "ghost",
						size: "xs",
						onClick: handleConnect,
						className: "gap-1.5",
						disabled: hasActionInFlight,
						children: [actionInFlight === "connect" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Server, { className: "size-3" }), translate("auto.components.settings.SshTargetCard.ec6543cee9", "Connect")]
					})
				] })
			})
		]
	});
}
export { shouldClearPendingSshReset as a, runtimeStatusForOverall as c, isSshTargetConnecting as i, SshTargetCard as n, isConnectedRuntimeHostState as o, statusColor as r, runtimeHostConnectionState as s, STATUS_LABELS as t };
