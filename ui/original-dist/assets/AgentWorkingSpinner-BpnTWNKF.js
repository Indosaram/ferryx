import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
require_react();
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var SPINNER_ANIMATION_NAME = "agent-spinner-rotate";
function syncSpinnerPhase(el) {
	if (el === null || typeof el.getAnimations !== "function") return;
	const animation = el.getAnimations().find((candidate) => "animationName" in candidate && candidate.animationName === SPINNER_ANIMATION_NAME);
	if (animation !== void 0) animation.startTime = 0;
}
function handleSpinnerAnimationStart(event) {
	if (event.animationName === SPINNER_ANIMATION_NAME) syncSpinnerPhase(event.currentTarget);
}
function AgentWorkingSpinner({ className }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		onAnimationStart: handleSpinnerAnimationStart,
		"data-agent-spinner": "",
		className: cn("agent-working-spinner block rounded-full border-2 border-yellow-500 border-t-transparent motion-reduce:border-t-yellow-500", className)
	});
}
export { AgentWorkingSpinner as t };
