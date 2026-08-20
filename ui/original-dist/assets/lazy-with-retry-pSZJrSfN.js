import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { n as init_defineProperty, t as _defineProperty } from "./defineProperty-BAtR-r70.js";
const ORCA_EDITOR_SAVE_DIRTY_FILES_EVENT = "orca:editor-save-dirty-files";
const ORCA_EDITOR_PREPARE_HOT_EXIT_EVENT = "orca:editor-prepare-hot-exit";
function requestEditorHotExitBackup(eventTarget) {
	return new Promise((resolve, reject) => {
		let claimed = false;
		eventTarget.dispatchEvent(new CustomEvent(ORCA_EDITOR_PREPARE_HOT_EXIT_EVENT, { detail: {
			claim: () => {
				claimed = true;
			},
			resolve,
			reject: (message) => {
				reject(new Error(message));
			}
		} }));
		if (!claimed) resolve();
	});
}
async function prepareRendererForAppRestart(eventTarget, { startedEventName, abortedEventName, awaitCheckpoint }) {
	eventTarget.dispatchEvent(new Event(startedEventName));
	try {
		await requestEditorHotExitBackup(eventTarget);
		if (!eventTarget.dispatchEvent(new Event("beforeunload", { cancelable: true }))) throw new Error("Renderer shutdown checkpoint was not completed.");
		await awaitCheckpoint();
	} catch (error) {
		eventTarget.dispatchEvent(new Event(abortedEventName));
		throw error;
	}
}
const ORCA_RENDERER_UNLOAD_PREVENTED_EVENT = "orca:renderer-unload-prevented";
const ORCA_UPDATER_QUIT_AND_INSTALL_STARTED_EVENT = "orca:updater-quit-and-install-started";
const ORCA_UPDATER_QUIT_AND_INSTALL_ABORTED_EVENT = "orca:updater-quit-and-install-aborted";
const ORCA_APP_RESTART_STARTED_EVENT = "orca:app-restart-started";
const ORCA_APP_RESTART_ABORTED_EVENT = "orca:app-restart-aborted";
var RELOAD_SETTLE_GRACE_MS = 1e4;
function waitForRefusedNavigation(win) {
	let graceTimer;
	let onUnloadPrevented = () => void 0;
	const cancel = () => {
		if (graceTimer !== void 0) {
			clearTimeout(graceTimer);
			graceTimer = void 0;
		}
		win.removeEventListener(ORCA_RENDERER_UNLOAD_PREVENTED_EVENT, onUnloadPrevented);
	};
	return {
		outcome: new Promise((resolve) => {
			const settle = (result) => {
				cancel();
				resolve(result);
			};
			onUnloadPrevented = () => settle("unload-vetoed");
			win.addEventListener(ORCA_RENDERER_UNLOAD_PREVENTED_EVENT, onUnloadPrevented);
			graceTimer = setTimeout(() => settle("never-landed"), RELOAD_SETTLE_GRACE_MS);
		}),
		cancel
	};
}
async function requestLazyChunkRecoveryReload(win, awaitCheckpoint = () => window.api?.app?.awaitBeforeUnloadCheckpoint?.() ?? Promise.resolve()) {
	try {
		await prepareRendererForAppRestart(win, {
			startedEventName: ORCA_APP_RESTART_STARTED_EVENT,
			abortedEventName: ORCA_APP_RESTART_ABORTED_EVENT,
			awaitCheckpoint
		});
	} catch {
		return "checkpoint-refused";
	}
	let cancelRefusalWait = () => void 0;
	try {
		const refused = waitForRefusedNavigation(win);
		cancelRefusalWait = refused.cancel;
		win.location.reload();
		return await refused.outcome;
	} catch {
		return "request-failed";
	} finally {
		cancelRefusalWait();
		win.dispatchEvent(new Event(ORCA_APP_RESTART_ABORTED_EVENT));
	}
}
var import_react = /* @__PURE__ */ __toESM(require_react());
init_defineProperty();
var LazyChunkLoadError = class extends Error {
	constructor(cause, reloadKey = "unknown") {
		super("Lazy chunk load failed after reload recovery was exhausted");
		_defineProperty(this, "reloadKey", void 0);
		this.name = "LazyChunkLoadError";
		this.reloadKey = reloadKey;
		this.cause = cause;
	}
};
function isLazyChunkLoadError(error) {
	return error instanceof LazyChunkLoadError;
}
var RELOAD_GUARD_KEY = "orca:lazy-chunk-reload-attempted";
var FALLBACK_RELOAD_TOKEN = `doc-${Math.random().toString(36).slice(2)}`;
var DEFAULT_RETRIES = 2;
var DEFAULT_BASE_DELAY_MS = 250;
function currentDocumentReloadToken() {
	const timeOrigin = typeof performance === "undefined" ? NaN : performance.timeOrigin;
	return Number.isFinite(timeOrigin) && timeOrigin > 0 ? String(timeOrigin) : FALLBACK_RELOAD_TOKEN;
}
function readChunkReloadGuardState() {
	if (typeof window === "undefined") return "unavailable";
	try {
		const stored = window.sessionStorage.getItem(RELOAD_GUARD_KEY);
		if (stored === null) return "not-attempted";
		return stored === currentDocumentReloadToken() ? "reload-not-landed" : "reload-landed";
	} catch {
		return "unavailable";
	}
}
function markChunkReloadAttempted() {
	try {
		window.sessionStorage.setItem(RELOAD_GUARD_KEY, currentDocumentReloadToken());
		return true;
	} catch {
		return false;
	}
}
function clearChunkReloadGuard() {
	try {
		window.sessionStorage.removeItem(RELOAD_GUARD_KEY);
	} catch {}
}
var MAX_RELOAD_REQUESTS_PER_DOCUMENT = 2;
var reloadRequestsThisDocument = 0;
var reloadRequestInFlight = false;
function recordReloadBreadcrumb(name, reloadKey, message, outcome) {
	try {
		window.api?.crashReports.recordBreadcrumb({
			name,
			data: {
				reloadKey,
				message,
				...outcome === void 0 ? {} : { outcome }
			}
		});
	} catch {}
}
var wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
function containedChunkFailure(lastError, reloadKey) {
	return isKnownDynamicImportFailure(lastError) ? new LazyChunkLoadError(lastError, reloadKey) : lastError;
}
function isKnownDynamicImportFailure(error) {
	if (!(error instanceof Error)) return false;
	if (error.name === "ChunkLoadError") return true;
	if (error.name === "SyntaxError") return true;
	return [
		/failed to fetch dynamically imported module/i,
		/error loading dynamically imported module/i,
		/importing a module script failed/i,
		/failed to load module script/i,
		/loading chunk .+ failed/i,
		/unexpected token/i,
		/unexpected end of (input|script|json)/i
	].some((pattern) => pattern.test(error.message));
}
async function loadLazyWithRetry(factory, options = {}) {
	const retries = options.retries ?? DEFAULT_RETRIES;
	const baseDelayMs = options.baseDelayMs ?? DEFAULT_BASE_DELAY_MS;
	let lastError;
	for (let attempt = 0; attempt <= retries; attempt += 1) try {
		return await factory();
	} catch (error) {
		lastError = error;
		if (attempt < retries) await wait(baseDelayMs * 2 ** attempt);
	}
	const reloadKey = options.reloadKey ?? "unknown";
	const failureMessage = lastError instanceof Error ? lastError.message : String(lastError);
	const reloadGuardState = readChunkReloadGuardState();
	if (typeof window !== "undefined" && reloadGuardState === "not-attempted" && reloadRequestsThisDocument < MAX_RELOAD_REQUESTS_PER_DOCUMENT) {
		if (!markChunkReloadAttempted()) throw lastError;
		reloadRequestsThisDocument += 1;
		reloadRequestInFlight = true;
		recordReloadBreadcrumb("lazy_chunk_reload", reloadKey, failureMessage);
		let outcome = "request-failed";
		try {
			outcome = await requestLazyChunkRecoveryReload(window);
		} finally {
			reloadRequestInFlight = false;
			clearChunkReloadGuard();
			recordReloadBreadcrumb("lazy_chunk_reload_vetoed", reloadKey, failureMessage, outcome);
		}
		throw containedChunkFailure(lastError, reloadKey);
	}
	if (reloadGuardState === "reload-landed") throw containedChunkFailure(lastError, reloadKey);
	if (reloadGuardState === "reload-not-landed") {
		if (!reloadRequestInFlight && isKnownDynamicImportFailure(lastError)) {
			clearChunkReloadGuard();
			recordReloadBreadcrumb("lazy_chunk_reload_vetoed", reloadKey, failureMessage, "guard-not-landed");
		}
		throw containedChunkFailure(lastError, reloadKey);
	}
	if (reloadGuardState === "not-attempted") throw containedChunkFailure(lastError, reloadKey);
	throw lastError;
}
function lazyWithRetry(factory, options) {
	return (0, import_react.lazy)(() => loadLazyWithRetry(factory, options));
}
export { ORCA_UPDATER_QUIT_AND_INSTALL_ABORTED_EVENT as a, ORCA_EDITOR_PREPARE_HOT_EXIT_EVENT as c, ORCA_APP_RESTART_STARTED_EVENT as i, ORCA_EDITOR_SAVE_DIRTY_FILES_EVENT as l, lazyWithRetry as n, ORCA_UPDATER_QUIT_AND_INSTALL_STARTED_EVENT as o, ORCA_APP_RESTART_ABORTED_EVENT as r, ORCA_RENDERER_UNLOAD_PREVENTED_EVENT as s, isLazyChunkLoadError as t };
