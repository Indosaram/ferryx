import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn } from "./button-DszXJEV6.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
var Earth = createLucideIcon("earth", [
	["path", {
		d: "M21.54 15H17a2 2 0 0 0-2 2v4.54",
		key: "1djwo0"
	}],
	["path", {
		d: "M7 3.34V5a3 3 0 0 0 3 3a2 2 0 0 1 2 2c0 1.1.9 2 2 2a2 2 0 0 0 2-2c0-1.1.9-2 2-2h3.17",
		key: "1tzkfa"
	}],
	["path", {
		d: "M11 21.95V18a2 2 0 0 0-2-2a2 2 0 0 1-2-2v-1a2 2 0 0 0-2-2H2.05",
		key: "14pb5j"
	}],
	["circle", {
		cx: "12",
		cy: "12",
		r: "10",
		key: "1mglay"
	}]
]);
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function SetupGuideProgressRing({ done, total, className, sizeClassName = "size-5", strokeWidth = 2, tooltipLabel }) {
	const boundedTotal = Math.max(total, 1);
	const boundedDone = Math.min(Math.max(done, 0), boundedTotal);
	const progressLabel = `${boundedDone}/${boundedTotal}`;
	const center = 20 / 2;
	const radius = 7;
	const circumference = 2 * Math.PI * radius;
	const offset = circumference * (1 - boundedDone / boundedTotal);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: cn("relative flex shrink-0 items-center justify-center text-muted-foreground", sizeClassName, className),
			"aria-label": translate("auto.components.setup.guide.SetupGuideProgressRing.dac3a4724a", "{{value0}} of {{value1}} setup steps complete", {
				value0: boundedDone,
				value1: boundedTotal
			}),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
				className: cn("-rotate-90", sizeClassName),
				viewBox: "0 0 20 20",
				"aria-hidden": true,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
					cx: center,
					cy: center,
					r: radius,
					fill: "none",
					stroke: "currentColor",
					strokeWidth,
					className: "opacity-25"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
					cx: center,
					cy: center,
					r: radius,
					fill: "none",
					stroke: "currentColor",
					strokeWidth,
					strokeLinecap: "round",
					strokeDasharray: circumference,
					strokeDashoffset: offset
				})]
			})
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "top",
		sideOffset: 4,
		children: tooltipLabel ?? progressLabel
	})] });
}
export { Earth as n, SetupGuideProgressRing as t };
