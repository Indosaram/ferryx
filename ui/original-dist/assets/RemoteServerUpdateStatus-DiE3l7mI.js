import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as CircleAlert } from "./circle-alert-keRTpMg-.js";
import { t as CircleCheck } from "./circle-check-CmH3uVJy.js";
import { t as Download } from "./download-CBd7RZDR.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as ServerOff } from "./server-off-xGmv1av8.js";
import { t as Wrench } from "./wrench-DbYIAUyT.js";
import { t as Badge } from "./badge-BBptl5GG.js";
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function getRemoteServerUpdatePhaseLabel(phase) {
	switch (phase) {
		case "checking": return translate("auto.components.settings.RemoteServerUpdateStatus.checking", "Checking…");
		case "available": return translate("auto.components.settings.RemoteServerUpdateStatus.available", "Update available");
		case "current": return translate("auto.components.settings.RemoteServerUpdateStatus.current", "Up to date");
		case "manual": return translate("auto.components.settings.RemoteServerUpdateStatus.manual", "Manual update");
		case "offline": return translate("auto.components.settings.RemoteServerUpdateStatus.offline", "Offline");
		case "queued": return translate("auto.components.settings.RemoteServerUpdateStatus.queued", "Queued");
		case "checking-update": return translate("auto.components.settings.RemoteServerUpdateStatus.checkingUpdate", "Checking update…");
		case "downloading": return translate("auto.components.settings.RemoteServerUpdateStatus.downloading", "Downloading…");
		case "restarting": return translate("auto.components.settings.RemoteServerUpdateStatus.restarting", "Restarting…");
		case "updated": return translate("auto.components.settings.RemoteServerUpdateStatus.updated", "Updated");
		case "failed": return translate("auto.components.settings.RemoteServerUpdateStatus.failed", "Update failed");
	}
}
function phaseIcon(phase) {
	switch (phase) {
		case "checking":
		case "queued":
		case "checking-update":
		case "restarting": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "animate-spin" });
		case "downloading": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Download, {});
		case "current":
		case "updated": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleCheck, {});
		case "manual": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Wrench, {});
		case "offline": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ServerOff, {});
		case "failed": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleAlert, {});
		case "available": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Download, {});
	}
}
function RemoteServerUpdateStatus({ entry, compact = false }) {
	const progress = entry.phase === "downloading" && entry.progress !== null ? ` ${Math.round(entry.progress)}%` : "";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
		variant: entry.phase === "failed" ? "destructive" : "outline",
		className: compact ? "px-1.5 text-[11px]" : void 0,
		children: [
			phaseIcon(entry.phase),
			getRemoteServerUpdatePhaseLabel(entry.phase),
			progress
		]
	});
}
function getRemoteServerManualUpdateHelp(entry) {
	if (entry.support?.reason === "manual-service-update-required") return translate("auto.components.settings.RemoteServerUpdateStatus.serviceManagerHelp", "Update Orca through the service manager that starts this server.");
	if (entry.support?.reason === "unpackaged-build") return translate("auto.components.settings.RemoteServerUpdateStatus.unpackedHelp", "Development builds must be updated from their source checkout.");
	return translate("auto.components.settings.RemoteServerUpdateStatus.legacyHelp", "Update this server manually once to enable remote updates.");
}
export { getRemoteServerManualUpdateHelp as n, RemoteServerUpdateStatus as t };
