import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var snapshot = {
	devices: [],
	loaded: false,
	loading: false,
	error: false
};
var activeRequest = null;
var latestRequestId = 0;
var listeners = /* @__PURE__ */ new Set();
function publish(nextSnapshot) {
	snapshot = nextSnapshot;
	for (const listener of listeners) listener();
}
function subscribe(listener) {
	listeners.add(listener);
	return () => {
		listeners.delete(listener);
	};
}
function getSnapshot() {
	return snapshot;
}
function supersededResult() {
	return activeRequest?.promise ?? snapshot.devices;
}
function getPairedMobileDevicesSnapshot() {
	return snapshot.devices;
}
function replacePairedMobileDevices(devices) {
	latestRequestId += 1;
	activeRequest = null;
	publish({
		devices: [...devices],
		loaded: true,
		loading: false,
		error: false
	});
}
function refreshPairedMobileDevices({ force = false } = {}) {
	if (activeRequest && !force) return activeRequest.promise;
	const requestId = latestRequestId + 1;
	latestRequestId = requestId;
	publish({
		...snapshot,
		loading: true
	});
	const promise = window.api.mobile.listDevices().then((result) => {
		const devices = [...result.devices];
		if (requestId !== latestRequestId) return supersededResult();
		publish({
			devices,
			loaded: true,
			loading: false,
			error: false
		});
		return devices;
	}).catch((error) => {
		if (requestId !== latestRequestId) return supersededResult();
		publish({
			...snapshot,
			loaded: true,
			loading: false,
			error: true
		});
		throw error;
	}).finally(() => {
		if (activeRequest?.id === requestId) activeRequest = null;
	});
	activeRequest = {
		id: requestId,
		promise
	};
	return promise;
}
var enabledConsumerCount = 0;
function recoverPairedMobileDevicesOnReconnect() {
	if (enabledConsumerCount > 0 && snapshot.error) refreshPairedMobileDevices({ force: true }).catch(() => {});
}
function addRecoveryConsumer() {
	if (enabledConsumerCount === 0) {
		window.addEventListener("focus", recoverPairedMobileDevicesOnReconnect);
		window.addEventListener("online", recoverPairedMobileDevicesOnReconnect);
	}
	enabledConsumerCount += 1;
	return () => {
		enabledConsumerCount -= 1;
		if (enabledConsumerCount === 0) {
			window.removeEventListener("focus", recoverPairedMobileDevicesOnReconnect);
			window.removeEventListener("online", recoverPairedMobileDevicesOnReconnect);
		}
	};
}
function usePairedMobileDevices({ enabled = true, refreshOnMount = true } = {}) {
	const currentSnapshot = (0, import_react.useSyncExternalStore)(subscribe, getSnapshot, getSnapshot);
	const refresh = (0, import_react.useCallback)(refreshPairedMobileDevices, []);
	(0, import_react.useEffect)(() => {
		if (!enabled || !refreshOnMount || currentSnapshot.loaded || currentSnapshot.loading) return;
		refreshPairedMobileDevices().catch(() => {});
	}, [
		currentSnapshot.loaded,
		currentSnapshot.loading,
		enabled,
		refreshOnMount
	]);
	(0, import_react.useEffect)(() => {
		if (!enabled) return;
		return addRecoveryConsumer();
	}, [enabled]);
	return {
		...currentSnapshot,
		hasPairedDevice: currentSnapshot.devices.length > 0,
		refresh
	};
}
export { replacePairedMobileDevices as n, usePairedMobileDevices as r, getPairedMobileDevicesSnapshot as t };
