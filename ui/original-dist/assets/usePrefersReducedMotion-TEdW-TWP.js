import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";
function readPrefersReducedMotion() {
	if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
	return window.matchMedia(REDUCED_MOTION_QUERY).matches;
}
function usePrefersReducedMotion() {
	const [prefersReducedMotion, setPrefersReducedMotion] = (0, import_react.useState)(readPrefersReducedMotion);
	(0, import_react.useEffect)(() => {
		if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
		const media = window.matchMedia(REDUCED_MOTION_QUERY);
		const onChange = (event) => {
			setPrefersReducedMotion(event.matches);
		};
		media.addEventListener("change", onChange);
		return () => media.removeEventListener("change", onChange);
	}, []);
	return prefersReducedMotion;
}
export { usePrefersReducedMotion as t };
