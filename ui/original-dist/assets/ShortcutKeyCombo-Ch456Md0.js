import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function KeyCap({ label, className }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex min-w-6 items-center justify-center rounded border border-border/80 bg-secondary/70 px-1.5 py-0.5 text-xs font-medium text-muted-foreground shadow-sm", className),
		children: label
	});
}
function ShortcutKeyCombo({ keys, className, separatorClassName, keyCapClassName, doubleTap = false }) {
	const isMac = navigator.userAgent.includes("Mac");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex items-center gap-1", className),
		title: doubleTap && keys.length > 0 ? translate("auto.components.ShortcutKeyCombo.07eb4985a1", "Double-tap {{value0}}", { value0: keys[0] }) : void 0,
		children: keys.map((key, index) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_react.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(KeyCap, {
			label: key,
			className: keyCapClassName
		}), !isMac && !doubleTap && index < keys.length - 1 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: separatorClassName ?? "mx-0.5 text-xs text-muted-foreground",
			children: "+"
		}) : null] }, `${key}-${index}`))
	});
}
export { ShortcutKeyCombo as t };
