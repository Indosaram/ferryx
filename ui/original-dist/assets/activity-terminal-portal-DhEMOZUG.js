import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var currentTargets = [];
var emptyTargets = [];
var subscribers = /* @__PURE__ */ new Set();
var ACTIVITY_TERMINAL_PORTAL_FIELD_EQUALS = Object.values({
	slotId: (left, right) => left.slotId === right.slotId,
	requestToken: (left, right) => left.requestToken === right.requestToken,
	target: (left, right) => left.target === right.target,
	worktreeId: (left, right) => left.worktreeId === right.worktreeId,
	tabId: (left, right) => left.tabId === right.tabId,
	paneKey: (left, right) => left.paneKey === right.paneKey,
	forceUnavailable: (left, right) => left.forceUnavailable === right.forceUnavailable,
	active: (left, right) => left.active === right.active
});
function haveSameActivityTerminalPortals(left, right) {
	return left.length === right.length && left.every((target, index) => {
		const candidate = right[index];
		return candidate !== void 0 && ACTIVITY_TERMINAL_PORTAL_FIELD_EQUALS.every((isEqual) => isEqual(target, candidate));
	});
}
function setActivityTerminalPortals(targets) {
	if (currentTargets === targets || haveSameActivityTerminalPortals(currentTargets, targets)) return;
	currentTargets = targets;
	for (const subscriber of subscribers) subscriber();
}
function subscribeActivityTerminalPortals(onStoreChange) {
	subscribers.add(onStoreChange);
	return () => {
		subscribers.delete(onStoreChange);
	};
}
function useActivityTerminalPortals(enabled) {
	const subscribe = (0, import_react.useCallback)((onStoreChange) => enabled ? subscribeActivityTerminalPortals(onStoreChange) : () => {}, [enabled]);
	const getSnapshot = (0, import_react.useCallback)(() => enabled ? currentTargets : emptyTargets, [enabled]);
	return (0, import_react.useSyncExternalStore)(subscribe, getSnapshot, getSnapshot);
}
function findActivityTerminalPortal(targets, query) {
	const matchingTab = targets.filter((target) => target.worktreeId === query.worktreeId && target.tabId === query.tabId);
	if (query.slotId !== void 0 || query.paneKey !== void 0 || query.requestToken !== void 0) {
		const exact = matchingTab.find((target) => (query.slotId === void 0 || target.slotId === query.slotId) && (query.paneKey === void 0 || target.paneKey === query.paneKey) && (query.requestToken === void 0 || target.requestToken === query.requestToken));
		if (exact) return exact;
	}
	return matchingTab.find((target) => target.active) ?? (matchingTab.length === 1 ? matchingTab[0] : null) ?? null;
}
export { setActivityTerminalPortals as n, useActivityTerminalPortals as r, findActivityTerminalPortal as t };
