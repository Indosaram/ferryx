import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn } from "./button-DszXJEV6.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as Badge } from "./badge-BBptl5GG.js";
var GitCommitHorizontal = createLucideIcon("git-commit-horizontal", [
	["circle", {
		cx: "12",
		cy: "12",
		r: "3",
		key: "1v7zrd"
	}],
	["line", {
		x1: "3",
		x2: "9",
		y1: "12",
		y2: "12",
		key: "1dyftd"
	}],
	["line", {
		x1: "15",
		x2: "21",
		y1: "12",
		y2: "12",
		key: "oup4p8"
	}]
]);
require_react();
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function DetachedHeadBadge({ display, label = "source-control", side = "right", className, tabIndex }) {
	const visibleLabel = label === "sidebar" ? display.sidebarLabel : display.sourceControlLabel;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
			variant: "outline",
			"aria-label": display.tooltip,
			tabIndex,
			className: cn("h-[18px] shrink-0 gap-1 rounded px-1.5 text-[10px] font-medium leading-none", "border-[color:color-mix(in_srgb,var(--git-decoration-modified)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--git-decoration-modified)_8%,transparent)] text-[color:var(--git-decoration-modified)]", className),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitCommitHorizontal, { className: "size-2.5" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-0 truncate",
				children: visibleLabel
			})]
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side,
		sideOffset: 8,
		children: display.tooltip
	})] });
}
export { GitCommitHorizontal as n, DetachedHeadBadge as t };
