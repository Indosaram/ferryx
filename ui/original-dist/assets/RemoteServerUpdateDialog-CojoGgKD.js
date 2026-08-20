import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { t as Progress } from "./progress-BsVdJvWF.js";
import "./useMountedRef-1omUd-IV.js";
import "./badge-BBptl5GG.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { n as getRemoteServerManualUpdateHelp, t as RemoteServerUpdateStatus } from "./RemoteServerUpdateStatus-DiE3l7mI.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function versionDescription(entry) {
	if (entry.currentVersion && entry.targetVersion && entry.currentVersion !== entry.targetVersion) return `${entry.currentVersion} → ${entry.targetVersion}`;
	if (entry.currentVersion) return `v${entry.currentVersion}`;
	return translate("auto.components.settings.RemoteServerUpdateDialog.versionUnavailable", "Version unavailable");
}
function entryHelp(entry) {
	if (entry.error) return entry.error;
	if (entry.phase === "manual") return getRemoteServerManualUpdateHelp(entry);
	if (entry.phase === "restarting") return translate("auto.components.settings.RemoteServerUpdateDialog.restartingHelp", "Waiting for the replacement server to reconnect on the new version.");
	return null;
}
function ServerUpdateRow({ entry, disabled, onUpdate }) {
	const canUpdate = entry.phase === "available" || entry.phase === "failed";
	const help = entryHelp(entry);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-2 px-3 py-3",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-start gap-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1 space-y-0.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap items-center gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-sm font-medium",
							children: entry.name
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RemoteServerUpdateStatus, {
							entry,
							compact: true
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-muted-foreground",
						children: versionDescription(entry)
					})]
				}), canUpdate ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "outline",
					size: "xs",
					onClick: onUpdate,
					disabled,
					children: entry.phase === "failed" ? translate("auto.components.settings.RemoteServerUpdateDialog.retry", "Retry") : translate("auto.components.settings.RemoteServerUpdateDialog.update", "Update this server")
				}) : null]
			}),
			entry.phase === "downloading" && entry.progress !== null ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Progress, {
				value: entry.progress,
				"aria-label": translate("auto.components.settings.RemoteServerUpdateDialog.downloadProgress", "{{value0}} download progress", { value0: entry.name })
			}) : null,
			help ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: entry.phase === "failed" ? "text-xs break-words text-destructive" : "text-xs break-words text-muted-foreground",
				children: help
			}) : null
		]
	});
}
function RemoteServerUpdateDialog() {
	const open = useAppStore((state) => state.remoteServerUpdateDialogOpen);
	const setOpen = useAppStore((state) => state.setRemoteServerUpdateDialogOpen);
	const entries = [...useAppStore((state) => state.remoteServerUpdates).values()];
	const checking = useAppStore((state) => state.remoteServerUpdatesChecking);
	const running = useAppStore((state) => state.remoteServerUpdatesRunning);
	const refresh = useAppStore((state) => state.refreshRemoteServerUpdates);
	const start = useAppStore((state) => state.startRemoteServerUpdates);
	const eligible = entries.filter((entry) => entry.phase === "available" || entry.phase === "failed");
	const allCurrent = entries.length > 0 && !checking && !running && entries.every((entry) => entry.phase === "current" || entry.phase === "updated");
	const liveTabCount = eligible.reduce((total, entry) => total + entry.liveTabCount, 0);
	const liveLeafCount = eligible.reduce((total, entry) => total + entry.liveLeafCount, 0);
	const liveTabLabel = liveTabCount === 1 ? translate("auto.components.settings.RemoteServerUpdateDialog.liveTabOne", "1 live tab") : translate("auto.components.settings.RemoteServerUpdateDialog.liveTabs", "{{value0}} live tabs", { value0: liveTabCount });
	const liveLeafLabel = liveLeafCount === 1 ? translate("auto.components.settings.RemoteServerUpdateDialog.livePaneOne", "1 live pane") : translate("auto.components.settings.RemoteServerUpdateDialog.livePanes", "{{value0}} live panes", { value0: liveLeafCount });
	(0, import_react.useEffect)(() => {
		if (open) refresh();
	}, [open, refresh]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange: setOpen,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "max-h-[min(720px,calc(100vh-2rem))] gap-4 sm:max-w-2xl",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.settings.RemoteServerUpdateDialog.title", "Update Remote Orca Servers") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.settings.RemoteServerUpdateDialog.description", "Review paired servers and update supported installs from this Orca client.") })] }),
				eligible.length > 0 && (liveTabCount > 0 || liveLeafCount > 0) ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex gap-2 rounded-lg border border-border bg-muted/40 p-3 text-xs",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-4 shrink-0 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", { children: translate("auto.components.settings.RemoteServerUpdateDialog.restartWarning", "Updating restarts these servers. {{value0}} and {{value1}} may briefly disconnect.", {
						value0: liveTabLabel,
						value1: liveLeafLabel
					}) })]
				}) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "scrollbar-sleek min-h-0 overflow-y-auto rounded-lg border border-border/50 bg-card/30",
					children: entries.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "px-4 py-8 text-center text-sm text-muted-foreground",
						children: checking ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
							className: "inline-flex items-center gap-2",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }), translate("auto.components.settings.RemoteServerUpdateDialog.checking", "Checking paired servers…")]
						}) : translate("auto.components.settings.RemoteServerUpdateDialog.empty", "No paired Remote Orca Servers.")
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "divide-y divide-border/50",
						children: entries.map((entry) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ServerUpdateRow, {
							entry,
							disabled: running || checking,
							onUpdate: () => void start([entry.environmentId])
						}, entry.environmentId))
					})
				}),
				checking ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "inline-flex items-center gap-2 text-xs text-muted-foreground",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }), translate("auto.components.settings.RemoteServerUpdateDialog.checking", "Checking paired servers…")]
				}) : allCurrent ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-xs text-muted-foreground",
					children: translate("auto.components.settings.RemoteServerUpdateDialog.noUpdates", "All servers are up to date.")
				}) : null,
				eligible.length > 1 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogFooter, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					size: "sm",
					autoFocus: eligible.length > 0,
					onClick: () => void start(),
					disabled: checking || running,
					children: translate("auto.components.settings.RemoteServerUpdateDialog.updateAll", "Update all {{value0}} servers", { value0: eligible.length })
				}) }) : null
			]
		})
	});
}
var RemoteServerUpdateDialog_default = RemoteServerUpdateDialog;
export { RemoteServerUpdateDialog, RemoteServerUpdateDialog_default as default };
