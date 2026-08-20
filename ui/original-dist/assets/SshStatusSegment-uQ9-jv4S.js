import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import { n as Cloud, t as MonitorSmartphone } from "./monitor-smartphone-BFYO8mmD.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as ServerOff } from "./server-off-xGmv1av8.js";
import { t as Server } from "./server-DYdwnXME.js";
import { dp as unwrapRuntimeRpcResult, i as isUserManagedRuntimeEnvironment, j as getHostDisplayLabelOverrides, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import "./plugin-manifest-Bs-50M_g.js";
import { ct as toRuntimeExecutionHostId, nt as isRuntimeOwnedSshTargetId } from "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { i as DropdownMenuItem, l as DropdownMenuSeparator, m as DropdownMenuTrigger, r as DropdownMenuContent, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
import "./tooltip-DPmd1AoJ.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { n as isConnectingSshStatus, t as canConnectSshStatus } from "./ssh-connection-recoverability-CNHp0WBp.js";
import { t as sshConnectVerb } from "./ssh-connect-verb-CzMNDLCH.js";
import { a as useSshConnectInFlight, n as endSshConnect, r as isSshConnectInFlight, t as beginSshConnect } from "./ssh-connect-in-flight-PkUz5iol.js";
import "./ssh-types-Caw2Ltsn.js";
import { c as runtimeStatusForOverall, o as isConnectedRuntimeHostState, r as statusColor, s as runtimeHostConnectionState, t as STATUS_LABELS } from "./SshTargetCard-Dy6I1ha8.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function runtimeStatusLabel(state) {
	switch (state) {
		case "connected": return translate("auto.components.status.bar.SshStatusSegment.runtime_online", "Connected");
		case "workspace-window-closed": return translate("auto.components.status.bar.SshStatusSegment.runtime_workspace_window_closed", "Workspace window closed");
		case "checking": return translate("auto.components.status.bar.SshStatusSegment.runtime_checking", "Checking");
		case "reconnecting": return translate("auto.components.status.bar.SshStatusSegment.runtime_reconnecting", "Reconnecting");
		case "disconnected": return translate("auto.components.status.bar.SshStatusSegment.runtime_unavailable", "Disconnected");
	}
}
function runtimeDotColor(state) {
	switch (state) {
		case "connected": return "bg-emerald-500";
		case "workspace-window-closed":
		case "checking":
		case "reconnecting": return "bg-yellow-500";
		case "disconnected": return "bg-muted-foreground/40";
	}
}
function runtimeStatusTone(state) {
	if (state === "checking" || state === "reconnecting" || state === "workspace-window-closed") return "text-yellow-500";
	return "text-muted-foreground";
}
function runtimeActionLabel(state) {
	switch (state) {
		case "connected":
		case "workspace-window-closed": return translate("auto.components.status.bar.SshStatusSegment.59b553e2aa", "Disconnect");
		case "disconnected": return translate("auto.components.status.bar.SshStatusSegment.63f36455cc", "Connect");
		case "checking":
		case "reconnecting": return null;
	}
}
function RuntimeHostStatusRow({ label, state, detail, onConnect, onDisconnect }) {
	const [busy, setBusy] = (0, import_react.useState)(false);
	const mountedRef = useMountedRef();
	const actionLabel = runtimeActionLabel(state);
	const handleAction = (0, import_react.useCallback)(async () => {
		const action = isConnectedRuntimeHostState(state) ? onDisconnect : onConnect;
		if (!action) return;
		setBusy(true);
		try {
			await action();
		} finally {
			if (mountedRef.current) setBusy(false);
		}
	}, [
		mountedRef,
		onConnect,
		onDisconnect,
		state
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-2.5 px-2 py-1.5",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: `size-1.5 shrink-0 rounded-full ${runtimeDotColor(state)}` }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 flex-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "truncate text-[12px] font-medium",
					children: label
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex min-w-0 items-center gap-1.5 text-[10px] text-muted-foreground",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.status.bar.SshStatusSegment.remote_server", "Remote Server") }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							"aria-hidden": "true",
							children: "·"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
							className: `inline-flex min-w-0 items-center gap-1 ${runtimeStatusTone(state)}`,
							children: [state === "checking" || state === "reconnecting" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-2.5 shrink-0 animate-spin" }) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "truncate",
								children: runtimeStatusLabel(state)
							})]
						}),
						detail ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							"aria-hidden": "true",
							children: "·"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "truncate",
							children: detail
						})] }) : null
					]
				})]
			}),
			busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 shrink-0 animate-spin text-muted-foreground" }) : actionLabel && (isConnectedRuntimeHostState(state) ? onDisconnect : onConnect) ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				onClick: () => void handleAction(),
				className: "shrink-0 rounded px-1.5 py-0.5 text-[10px] text-muted-foreground hover:bg-accent/70 hover:text-foreground",
				children: actionLabel
			}) : null
		]
	});
}
function connectedHostCountLabel(count) {
	return count === 1 ? translate("auto.components.status.bar.SshStatusSegment.connectedHostCount_one", "{{count}} host", { count }) : translate("auto.components.status.bar.SshStatusSegment.connectedHostCount_other", "{{count}} hosts", { count });
}
function connectingHostsLabel() {
	return translate("auto.components.status.bar.SshStatusSegment.connecting", "Connecting…");
}
function workspaceSyncProblemLabel(phase) {
	return phase === "conflict" ? translate("auto.components.status.bar.SshStatusSegment.workspaceConflict", "Workspace conflict") : translate("auto.components.status.bar.SshStatusSegment.workspaceSyncError", "Workspace sync error");
}
function syncStatusLabel(status) {
	switch (status?.phase) {
		case "pulling":
		case "pushing": return "Workspace syncing";
		case "conflict": return "Workspace sync conflict";
		case "error": return "Workspace sync error";
		case "offline": return "Workspace sync unavailable";
		case "synced":
		case "idle":
		case void 0: return null;
	}
}
function syncStatusTone(status) {
	switch (status?.phase) {
		case "conflict":
		case "error": return "text-destructive";
		case "offline": return "text-muted-foreground";
		case "pulling":
		case "pushing": return "text-yellow-500";
		case "synced": return "text-emerald-500";
		case "idle":
		case void 0: return "text-muted-foreground";
	}
}
function SshTargetStatusRow({ targetId, label, status, syncStatus }) {
	const [busy, setBusy] = (0, import_react.useState)(false);
	const mountedRef = useMountedRef();
	const recordFeatureInteraction = useAppStore((s) => s.recordFeatureInteraction);
	const connectInFlight = useSshConnectInFlight(targetId);
	const visibleSyncStatusLabel = syncStatusLabel(syncStatus);
	const handleConnect = (0, import_react.useCallback)(async () => {
		if (isSshConnectInFlight(targetId)) return;
		beginSshConnect(targetId);
		setBusy(true);
		try {
			await window.api.ssh.connect({ targetId });
			recordFeatureInteraction("ssh");
		} catch (err) {
			toast.error(err instanceof Error ? err.message : translate("auto.components.status.bar.SshStatusSegment.2c29e2de68", "Connection failed"));
		} finally {
			endSshConnect(targetId);
			if (mountedRef.current) setBusy(false);
		}
	}, [
		mountedRef,
		recordFeatureInteraction,
		targetId
	]);
	const handleDisconnect = (0, import_react.useCallback)(async () => {
		setBusy(true);
		try {
			await window.api.ssh.disconnect({ targetId });
			recordFeatureInteraction("ssh");
		} catch (err) {
			toast.error(err instanceof Error ? err.message : translate("auto.components.status.bar.SshStatusSegment.bf07aee59e", "Disconnect failed"));
		} finally {
			if (mountedRef.current) setBusy(false);
		}
	}, [
		mountedRef,
		recordFeatureInteraction,
		targetId
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-2.5 px-2 py-1.5",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: `size-1.5 shrink-0 rounded-full ${statusColor(status)}` }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 flex-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "truncate text-[12px] font-medium",
					children: label
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex min-w-0 items-center gap-1.5 text-[10px] text-muted-foreground",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.status.bar.SshTargetStatusRow.sshHost", "SSH Host") }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							"aria-hidden": "true",
							children: "·"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: STATUS_LABELS[status] }),
						visibleSyncStatusLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							"aria-hidden": "true",
							children: "·"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
							className: `inline-flex min-w-0 items-center gap-1 ${syncStatusTone(syncStatus)}`,
							children: [syncStatus?.phase === "pulling" || syncStatus?.phase === "pushing" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-2.5 shrink-0 animate-spin" }) : syncStatus?.phase === "conflict" || syncStatus?.phase === "error" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-2.5 shrink-0" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Cloud, { className: "size-2.5 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "truncate",
								children: visibleSyncStatusLabel
							})]
						})] }) : null
					]
				})]
			}),
			busy || connectInFlight ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 shrink-0 animate-spin text-muted-foreground" }) : canConnectSshStatus(status) ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				onClick: () => void handleConnect(),
				className: "shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium text-foreground hover:bg-accent/70",
				children: sshConnectVerb(status)
			}) : status === "connected" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				onClick: () => void handleDisconnect(),
				className: "shrink-0 rounded px-1.5 py-0.5 text-[10px] text-muted-foreground hover:bg-accent/70 hover:text-foreground",
				children: translate("auto.components.status.bar.SshStatusSegment.59b553e2aa", "Disconnect")
			}) : null
		]
	});
}
async function connectRuntimeEnvironmentAndRecordStatus(environmentId, timeoutMs) {
	const setStatus = useAppStore.getState().setRuntimeEnvironmentStatus;
	try {
		setStatus(environmentId, {
			status: unwrapRuntimeRpcResult(await window.api.runtimeEnvironments.connect({
				selector: environmentId,
				timeoutMs
			})),
			checkedAt: Date.now()
		});
		return true;
	} catch {
		setStatus(environmentId, {
			status: null,
			checkedAt: Date.now()
		});
		return false;
	}
}
function overallStatus(statuses) {
	if (statuses.length === 0) return "disconnected";
	if (statuses.every((s) => s === "connected")) return "connected";
	if (statuses.some((s) => s === "connecting")) return "connecting";
	if (statuses.some((s) => s === "connected")) return "partial";
	return "disconnected";
}
function overallDotColor(status, connectedCount) {
	switch (status) {
		case "connected": return "bg-emerald-500";
		case "partial": return connectedCount > 0 ? "bg-emerald-500" : "bg-muted-foreground/40";
		case "connecting": return "bg-yellow-500";
		case "disconnected": return "bg-muted-foreground/40";
	}
}
function sshStatusForOverall(status) {
	if (status === "connected") return "connected";
	return isConnectingSshStatus(status) ? "connecting" : "disconnected";
}
function runtimeHostConnectionDetail(remoteControl) {
	if (!remoteControl) return;
	if (remoteControl.lastError) return remoteControl.lastError;
	if (remoteControl.lastClose?.reason) return translate("auto.components.status.bar.SshStatusSegment.runtime_last_close_reason", "Closed: {{value0}}", { value0: remoteControl.lastClose.reason });
	if (remoteControl.state === "reconnecting") return translate("auto.components.status.bar.SshStatusSegment.runtime_reconnect_attempt", "Attempt {{value0}}", { value0: String(remoteControl.reconnectAttempt + 1) });
}
async function connectRuntimeHostForNavigation(args) {
	if (!await args.refreshStatus(args.environmentId, 5e3)) return false;
	const repos = await args.fetchRepos(args.environmentId);
	await Promise.all(repos.map((repo) => args.fetchWorktrees(repo.id)));
	await args.fetchLineage();
	return true;
}
function SshStatusSegment({ compact, iconOnly }) {
	const sshConnectionStates = useAppStore((s) => s.sshConnectionStates);
	const sshTargetLabels = useAppStore((s) => s.sshTargetLabels);
	const settings = useAppStore((s) => s.settings);
	const runtimeEnvironments = useAppStore((s) => s.runtimeEnvironments);
	const runtimeStatusByEnvironmentId = useAppStore((s) => s.runtimeStatusByEnvironmentId);
	const setRuntimeEnvironmentStatus = useAppStore((s) => s.setRuntimeEnvironmentStatus);
	const hydrateRuntimeEnvironmentStatuses = useAppStore((s) => s.hydrateRuntimeEnvironmentStatuses);
	const remoteWorkspaceSyncStatusByTargetId = useAppStore((s) => s.remoteWorkspaceSyncStatusByTargetId);
	const setActiveView = useAppStore((s) => s.setActiveView);
	const openSettingsTarget = useAppStore((s) => s.openSettingsTarget);
	const recordFeatureInteraction = useAppStore((s) => s.recordFeatureInteraction);
	const hostLabelOverrides = (0, import_react.useMemo)(() => getHostDisplayLabelOverrides(settings), [settings]);
	const targets = Array.from(sshTargetLabels.entries()).filter(([id]) => !isRuntimeOwnedSshTargetId(id)).map(([id, label]) => {
		return {
			id,
			label,
			status: sshConnectionStates.get(id)?.status ?? "disconnected",
			syncStatus: remoteWorkspaceSyncStatusByTargetId[id]
		};
	});
	const runtimeHosts = runtimeEnvironments.filter(isUserManagedRuntimeEnvironment).map((environment) => {
		const statusEntry = runtimeStatusByEnvironmentId.get(environment.id);
		const override = hostLabelOverrides.get(toRuntimeExecutionHostId(environment.id));
		return {
			id: environment.id,
			label: override || environment.name || environment.id,
			hasStatusEntry: Boolean(statusEntry),
			status: statusEntry?.status ?? null,
			active: settings?.activeRuntimeEnvironmentId === environment.id,
			remoteControl: statusEntry?.status?.remoteControl ?? null
		};
	});
	const runtimeHostRows = runtimeHosts.map((host) => ({
		...host,
		state: runtimeHostConnectionState(host)
	}));
	const connectedRuntimeHosts = runtimeHostRows.filter((host) => isConnectedRuntimeHostState(host.state));
	const inactiveRuntimeHosts = runtimeHostRows.filter((host) => !isConnectedRuntimeHostState(host.state));
	const connectedTargets = targets.filter((target) => target.status === "connected");
	const disconnectedTargets = targets.filter((target) => target.status !== "connected");
	const connectRuntimeHost = (0, import_react.useCallback)(async (environmentId) => {
		const store = useAppStore.getState();
		if (!await connectRuntimeHostForNavigation({
			environmentId,
			refreshStatus: connectRuntimeEnvironmentAndRecordStatus,
			fetchRepos: store.fetchRuntimeEnvironmentRepos,
			fetchWorktrees: store.fetchWorktrees,
			fetchLineage: store.fetchWorktreeLineage
		})) {
			toast.error(translate("auto.components.status.bar.SshStatusSegment.runtime_connect_unavailable", "Remote host is not reachable"));
			return;
		}
		recordFeatureInteraction("ssh");
	}, [recordFeatureInteraction]);
	const disconnectRuntimeHost = (0, import_react.useCallback)(async (environmentId) => {
		try {
			await window.api.runtimeEnvironments.disconnect({ selector: environmentId });
			setRuntimeEnvironmentStatus(environmentId, {
				status: null,
				checkedAt: Date.now()
			}, { suppressDisconnectToast: true });
			recordFeatureInteraction("ssh");
		} catch (err) {
			toast.error(err instanceof Error ? err.message : translate("auto.components.status.bar.SshStatusSegment.runtime_disconnect_failed", "Disconnect failed"));
		}
	}, [recordFeatureInteraction, setRuntimeEnvironmentStatus]);
	if (targets.length === 0 && runtimeHosts.length === 0) return null;
	const statuses = [...targets.map((t) => sshStatusForOverall(t.status)), ...runtimeHostRows.map((host) => runtimeStatusForOverall(host.state))];
	const overall = overallStatus(statuses);
	const connectedHostCount = statuses.filter((status) => status === "connected").length;
	const anyConnecting = overall === "connecting";
	const syncProblem = targets.find((t) => t.syncStatus?.phase === "conflict" || t.syncStatus?.phase === "error");
	const syncProblemLabel = syncProblem ? workspaceSyncProblemLabel(syncProblem.syncStatus?.phase) : null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, {
		onOpenChange: (open) => {
			if (open) {
				hydrateRuntimeEnvironmentStatuses();
				recordFeatureInteraction("ssh");
			}
		},
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				className: "inline-flex items-center gap-1.5 cursor-pointer rounded px-1 py-0.5 hover:bg-accent/70",
				"aria-label": translate("auto.components.status.bar.SshStatusSegment.fdc57e9970", "Remote host connection status"),
				children: iconOnly ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "inline-flex items-center gap-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: `inline-block size-2 rounded-full ${syncProblem ? "bg-destructive" : overallDotColor(overall, connectedHostCount)}` }), syncProblem ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-3 text-destructive" }) : anyConnecting ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin text-muted-foreground" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MonitorSmartphone, { className: "size-3 text-muted-foreground" })]
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "inline-flex items-center gap-1.5",
					children: [
						syncProblem ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-3 text-destructive" }) : anyConnecting ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin text-yellow-500" }) : overall === "connected" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Server, { className: "size-3 text-emerald-500" }) : overall === "partial" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Server, { className: "size-3 text-muted-foreground" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ServerOff, { className: "size-3 text-muted-foreground" }),
						!compact && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[11px]",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: syncProblem ? "text-destructive" : "text-muted-foreground",
								children: syncProblemLabel ?? (anyConnecting ? connectingHostsLabel() : connectedHostCountLabel(connectedHostCount))
							})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: `inline-block size-1.5 rounded-full ${syncProblem ? "bg-destructive" : overallDotColor(overall, connectedHostCount)}` })
					]
				})
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuContent, {
			side: "top",
			align: "start",
			sideOffset: 8,
			className: "w-[min(20rem,calc(100vw-1rem))]",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "px-2 pt-1.5 pb-1 text-[10px] font-medium uppercase tracking-[0.08em] text-muted-foreground",
					children: translate("auto.components.status.bar.SshStatusSegment.6e8a9a4242", "Remote Hosts")
				}),
				connectedRuntimeHosts.map((host) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RuntimeHostStatusRow, {
					label: host.label,
					state: host.state,
					detail: runtimeHostConnectionDetail(host.remoteControl),
					onConnect: () => connectRuntimeHost(host.id),
					onDisconnect: () => disconnectRuntimeHost(host.id)
				}, host.id)),
				connectedTargets.map((t) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SshTargetStatusRow, {
					targetId: t.id,
					label: t.label,
					status: t.status,
					syncStatus: t.syncStatus
				}, t.id)),
				inactiveRuntimeHosts.map((host) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RuntimeHostStatusRow, {
					label: host.label,
					state: host.state,
					detail: runtimeHostConnectionDetail(host.remoteControl),
					onConnect: () => connectRuntimeHost(host.id),
					onDisconnect: () => disconnectRuntimeHost(host.id)
				}, host.id)),
				disconnectedTargets.map((t) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SshTargetStatusRow, {
					targetId: t.id,
					label: t.label,
					status: t.status,
					syncStatus: t.syncStatus
				}, t.id)),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuItem, {
					onSelect: () => {
						recordFeatureInteraction("ssh");
						openSettingsTarget({
							pane: "servers",
							repoId: null
						});
						setActiveView("settings");
					},
					children: translate("auto.components.status.bar.SshStatusSegment.3ad70e0365", "Manage Remote Hosts…")
				})
			]
		})]
	});
}
export { SshStatusSegment, connectRuntimeHostForNavigation };
