import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
import { t as CircleCheck } from "./circle-check-CmH3uVJy.js";
import { t as MessageCircleQuestionMark } from "./message-circle-question-mark-CFrAq4X1.js";
import { t as AgentWorkingSpinner } from "./AgentWorkingSpinner-BpnTWNKF.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function agentStateLabel(state) {
	switch (state) {
		case "working": return "Working";
		case "blocked": return "Blocked";
		case "waiting": return "Waiting for input";
		case "interrupted": return "Interrupted";
		case "failed": return "Failed";
		case "done": return "Done";
		case "idle": return "Idle";
		case "permission": return "Needs attention";
	}
}
const AgentStateDot = import_react.memo(function AgentStateDot$1({ state, size = "sm", className }) {
	const box = size === "md" ? "h-3 w-3" : "h-2.5 w-2.5";
	const inner = size === "md" ? "size-2" : "size-1.5";
	const icon = size === "md" ? "size-3" : "size-2.5";
	if (state === "working") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex shrink-0 items-center justify-center", box, className),
		"aria-label": agentStateLabel(state),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentWorkingSpinner, { className: inner })
	});
	if (state === "done") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex shrink-0 items-center justify-center", box, className),
		"aria-label": agentStateLabel(state),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleCheck, {
			className: cn("text-emerald-500", icon),
			"aria-hidden": "true"
		})
	});
	if (state === "permission" || state === "waiting") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex shrink-0 items-center justify-center", box, className),
		"aria-label": agentStateLabel(state),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MessageCircleQuestionMark, {
			className: cn("text-amber-500", icon),
			"aria-hidden": "true"
		})
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex shrink-0 items-center justify-center", box, className),
		"aria-label": agentStateLabel(state),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: cn("block rounded-full", inner, state === "blocked" || state === "interrupted" || state === "failed" ? "bg-red-500" : "bg-neutral-500/40") })
	});
});
export { agentStateLabel as n, AgentStateDot as t };
