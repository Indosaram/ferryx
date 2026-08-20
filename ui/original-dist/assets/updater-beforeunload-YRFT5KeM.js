import { a as ORCA_UPDATER_QUIT_AND_INSTALL_ABORTED_EVENT, i as ORCA_APP_RESTART_STARTED_EVENT, o as ORCA_UPDATER_QUIT_AND_INSTALL_STARTED_EVENT, r as ORCA_APP_RESTART_ABORTED_EVENT } from "./lazy-with-retry-pSZJrSfN.js";
var activeHandler = null;
var closeGuards = /* @__PURE__ */ new Set();
var closeInFlight = false;
function setWindowCloseRequestHandler(handler) {
	activeHandler = handler;
}
function registerWindowCloseGuard(guard) {
	closeGuards.add(guard);
	return () => {
		closeGuards.delete(guard);
	};
}
async function runWindowCloseGuards() {
	for (const guard of closeGuards) if (!await guard()) return false;
	return true;
}
async function dispatchWindowCloseRequest(data) {
	if (closeInFlight) return;
	closeInFlight = true;
	try {
		if (!await runWindowCloseGuards()) return;
	} finally {
		closeInFlight = false;
	}
	if (activeHandler) {
		activeHandler(data);
		return;
	}
	window.api.ui.confirmWindowClose();
}
var intentionalAppRestartInProgress = false;
function isIntentionalAppRestartInProgress() {
	return intentionalAppRestartInProgress;
}
function registerUpdaterBeforeUnloadBypass() {
	const markInProgress = () => {
		intentionalAppRestartInProgress = true;
	};
	const clearInProgress = () => {
		intentionalAppRestartInProgress = false;
	};
	window.addEventListener(ORCA_UPDATER_QUIT_AND_INSTALL_STARTED_EVENT, markInProgress);
	window.addEventListener(ORCA_UPDATER_QUIT_AND_INSTALL_ABORTED_EVENT, clearInProgress);
	window.addEventListener(ORCA_APP_RESTART_STARTED_EVENT, markInProgress);
	window.addEventListener(ORCA_APP_RESTART_ABORTED_EVENT, clearInProgress);
	return () => {
		window.removeEventListener(ORCA_UPDATER_QUIT_AND_INSTALL_STARTED_EVENT, markInProgress);
		window.removeEventListener(ORCA_UPDATER_QUIT_AND_INSTALL_ABORTED_EVENT, clearInProgress);
		window.removeEventListener(ORCA_APP_RESTART_STARTED_EVENT, markInProgress);
		window.removeEventListener(ORCA_APP_RESTART_ABORTED_EVENT, clearInProgress);
		intentionalAppRestartInProgress = false;
	};
}
export { setWindowCloseRequestHandler as a, registerWindowCloseGuard as i, registerUpdaterBeforeUnloadBypass as n, dispatchWindowCloseRequest as r, isIntentionalAppRestartInProgress as t };
