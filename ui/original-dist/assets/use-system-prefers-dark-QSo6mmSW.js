import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var SYSTEM_DARK_QUERY = "(prefers-color-scheme: dark)";
var subscribers = /* @__PURE__ */ new Set();
var mediaQueryList = null;
var unsubscribeMediaQuery = null;
var hasSnapshot = false;
var snapshot = true;
function readMediaQueryList() {
	if (mediaQueryList) return mediaQueryList;
	if (typeof window === "undefined" || typeof window.matchMedia !== "function") return null;
	mediaQueryList = window.matchMedia(SYSTEM_DARK_QUERY);
	return mediaQueryList;
}
function refreshSnapshot() {
	snapshot = readMediaQueryList()?.matches ?? true;
	hasSnapshot = true;
}
function getSystemPrefersDarkSnapshot() {
	if (!hasSnapshot) refreshSnapshot();
	return snapshot;
}
function subscribeToSystemPrefersDarkChange(onChange) {
	subscribers.add(onChange);
	if (!unsubscribeMediaQuery) {
		const media = readMediaQueryList();
		if (media) {
			snapshot = media.matches;
			hasSnapshot = true;
			const handleChange = (event) => {
				snapshot = event.matches;
				for (const subscriber of subscribers) subscriber();
			};
			media.addEventListener("change", handleChange);
			unsubscribeMediaQuery = () => media.removeEventListener("change", handleChange);
		}
	}
	return () => {
		subscribers.delete(onChange);
		if (subscribers.size > 0) return;
		unsubscribeMediaQuery?.();
		unsubscribeMediaQuery = null;
		mediaQueryList = null;
		hasSnapshot = false;
	};
}
function useSystemPrefersDark() {
	return (0, import_react.useSyncExternalStore)(subscribeToSystemPrefersDarkChange, getSystemPrefersDarkSnapshot, () => true);
}
export { useSystemPrefersDark as n, getSystemPrefersDarkSnapshot as t };
