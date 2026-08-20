import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
import { t as Server } from "./server-DYdwnXME.js";
import { st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function fallbackHostLabel(executionHostId) {
	const parsed = parseExecutionHostId(executionHostId);
	if (parsed?.kind === "ssh") return parsed.targetId;
	if (parsed?.kind === "runtime") return parsed.environmentId;
	return null;
}
function dashboardHostTooltipLabel({ hostKind, executionHostId, hostLabel }) {
	if (hostKind !== "ssh" && hostKind !== "remote") return null;
	const label = hostLabel?.trim() || fallbackHostLabel(executionHostId);
	if (hostKind === "ssh") return label ? translate("dashboardPopout.host.sshNamed", "SSH host · {{host}}", { host: label }) : translate("dashboardPopout.host.ssh", "SSH host");
	return label ? translate("dashboardPopout.host.remoteNamed", "Remote Orca host · {{host}}", { host: label }) : translate("dashboardPopout.host.remote", "Remote Orca host");
}
function DashboardHostBadge({ hostKind, executionHostId, hostLabel, keyboardFocusable = false, className, iconClassName }) {
	const tooltipLabel = dashboardHostTooltipLabel({
		hostKind,
		executionHostId,
		hostLabel
	});
	if (!tooltipLabel) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: cn("inline-flex shrink-0 items-center justify-center text-muted-foreground", keyboardFocusable && "pointer-events-auto focus-visible:rounded-sm focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none", className),
			"data-dashboard-host-badge": hostKind,
			"aria-label": tooltipLabel,
			tabIndex: keyboardFocusable ? 0 : void 0,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Server, {
				className: cn("size-3", iconClassName),
				"aria-hidden": true
			})
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "top",
		sideOffset: 4,
		children: tooltipLabel
	})] });
}
export { DashboardHostBadge as t };
