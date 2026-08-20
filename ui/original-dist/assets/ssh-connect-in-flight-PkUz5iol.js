import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var inFlightLockIds = /* @__PURE__ */ new Map();
var lastLockId = 0;
var listeners = /* @__PURE__ */ new Set();
function emit() {
	for (const listener of listeners) listener();
}
function subscribeSshConnectInFlight(listener) {
	listeners.add(listener);
	return () => {
		listeners.delete(listener);
	};
}
function acquire(targetId) {
	const held = inFlightLockIds.get(targetId);
	if (held !== void 0) return held;
	lastLockId += 1;
	inFlightLockIds.set(targetId, lastLockId);
	emit();
	return lastLockId;
}
function releaseOwned(targetId, lockId) {
	if (inFlightLockIds.get(targetId) !== lockId) return;
	inFlightLockIds.delete(targetId);
	emit();
}
function beginSshConnect(targetId) {
	acquire(targetId);
}
function endSshConnect(targetId) {
	if (!inFlightLockIds.delete(targetId)) return;
	emit();
}
function isSshConnectInFlight(targetId) {
	return inFlightLockIds.has(targetId);
}
function trackSshConnect(targetId, request) {
	const lockId = acquire(targetId);
	const release = () => {
		releaseOwned(targetId, lockId);
	};
	request.then(release, release);
	return request;
}
function useSshConnectInFlight(targetId) {
	const getSnapshot = (0, import_react.useCallback)(() => inFlightLockIds.has(targetId), [targetId]);
	return (0, import_react.useSyncExternalStore)(subscribeSshConnectInFlight, getSnapshot, getSnapshot);
}
export { useSshConnectInFlight as a, trackSshConnect as i, endSshConnect as n, isSshConnectInFlight as r, beginSshConnect as t };
