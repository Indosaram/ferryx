import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function RepoBadgeMark({ color, className }) {
	const style = color ? { backgroundColor: color } : void 0;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		"aria-hidden": "true",
		className: cn("block size-1.5 shrink-0", className),
		style
	});
}
function RepoBadgeLabel({ name, color, className, badgeClassName }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		className: cn("inline-flex min-w-0 items-center gap-1.5", className),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RepoBadgeMark, {
			color,
			className: badgeClassName
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "truncate",
			children: name
		})]
	});
}
var RepoBadgeLabel_default = RepoBadgeLabel;
export { RepoBadgeMark as n, RepoBadgeLabel_default as t };
