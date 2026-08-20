import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
import { t as MessageCircleQuestionMark } from "./message-circle-question-mark-CFrAq4X1.js";
import { n as getWorktreeStatusLabel } from "./worktree-status-DR0Zr8Ht.js";
import { t as AgentWorkingSpinner } from "./AgentWorkingSpinner-BpnTWNKF.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var StatusIndicator_default = import_react.memo(function StatusIndicator$1({ status, className, title, ...rest }) {
	const resolvedTitle = title ?? getWorktreeStatusLabel(status);
	if (status === "working") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex h-3 w-3 shrink-0 items-center justify-center", className),
		title: resolvedTitle,
		...rest,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentWorkingSpinner, { className: "size-2" })
	});
	if (status === "permission") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex h-3 w-3 shrink-0 items-center justify-center", className),
		title: resolvedTitle,
		...rest,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MessageCircleQuestionMark, {
			className: "size-3 text-amber-500",
			"aria-hidden": "true"
		})
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex h-3 w-3 shrink-0 items-center justify-center", className),
		title: resolvedTitle,
		...rest,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: cn("block size-2 rounded-full", status === "done" || status === "active" ? "bg-emerald-500" : "bg-neutral-500/40") })
	});
});
export { StatusIndicator_default as t };
