import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { l as createLucideIcon } from "./button-DszXJEV6.js";
import { rp as callRuntimeRpc, t as useAppStore } from "./store-CgXrfmaH.js";
var Crosshair = createLucideIcon("crosshair", [
	["circle", {
		cx: "12",
		cy: "12",
		r: "10",
		key: "1mglay"
	}],
	["line", {
		x1: "22",
		x2: "18",
		y1: "12",
		y2: "12",
		key: "l9bcsi"
	}],
	["line", {
		x1: "6",
		x2: "2",
		y1: "12",
		y2: "12",
		key: "13hhkx"
	}],
	["line", {
		x1: "12",
		x2: "12",
		y1: "6",
		y2: "2",
		key: "10w3f3"
	}],
	["line", {
		x1: "12",
		x2: "12",
		y1: "22",
		y2: "18",
		key: "15g9kq"
	}]
]);
var GitCompare = createLucideIcon("git-compare", [
	["circle", {
		cx: "18",
		cy: "18",
		r: "3",
		key: "1xkwt0"
	}],
	["circle", {
		cx: "6",
		cy: "6",
		r: "3",
		key: "1lh9wr"
	}],
	["path", {
		d: "M13 6h3a2 2 0 0 1 2 2v7",
		key: "1yeb86"
	}],
	["path", {
		d: "M11 18H8a2 2 0 0 1-2-2V9",
		key: "19pyzm"
	}]
]);
var OctagonX = createLucideIcon("octagon-x", [
	["path", {
		d: "m15 9-6 6",
		key: "1uzhvr"
	}],
	["path", {
		d: "M2.586 16.726A2 2 0 0 1 2 15.312V8.688a2 2 0 0 1 .586-1.414l4.688-4.688A2 2 0 0 1 8.688 2h6.624a2 2 0 0 1 1.414.586l4.688 4.688A2 2 0 0 1 22 8.688v6.624a2 2 0 0 1-.586 1.414l-4.688 4.688a2 2 0 0 1-1.414.586H8.688a2 2 0 0 1-1.414-.586z",
		key: "2d38gg"
	}],
	["path", {
		d: "m9 9 6 6",
		key: "z0biqf"
	}]
]);
function shouldShutdownSimulatorForPaneUnmountFromTabs(tabs, tabId) {
	const simulatorTabs = tabs.filter((tab) => tab.contentType === "simulator");
	if (tabId && simulatorTabs.some((tab) => tab.id === tabId)) return false;
	return simulatorTabs.length === 0;
}
var DEFAULT_SHUTDOWN_GRACE_MS = 1500;
var pendingShutdownTimersByWorktree = /* @__PURE__ */ new Map();
function getUnifiedTabsForWorktree(worktreeId) {
	return useAppStore.getState().unifiedTabsByWorktree[worktreeId] ?? [];
}
function shutdownManagedSimulator(worktreeId) {
	return callRuntimeRpc({ kind: "local" }, "emulator.shutdown", {
		worktree: worktreeId,
		managedOnly: true
	});
}
async function shutdownManagedSimulatorIfNoPane(worktreeId, tabId, options = {}) {
	if (!shouldShutdownSimulatorForPaneUnmountFromTabs((options.getTabsForWorktree ?? getUnifiedTabsForWorktree)(worktreeId), tabId)) return false;
	const shutdown = options.shutdownManagedSimulator ?? shutdownManagedSimulator;
	await Promise.resolve(shutdown(worktreeId)).catch(() => {});
	return true;
}
function cancelPendingSimulatorPaneShutdown(worktreeId) {
	const timer = pendingShutdownTimersByWorktree.get(worktreeId);
	if (!timer) return;
	clearTimeout(timer);
	pendingShutdownTimersByWorktree.delete(worktreeId);
}
function scheduleSimulatorPaneManagedShutdown(worktreeId, tabId, options = {}) {
	const getTabsForWorktree = options.getTabsForWorktree ?? getUnifiedTabsForWorktree;
	if (!shouldShutdownSimulatorForPaneUnmountFromTabs(getTabsForWorktree(worktreeId), tabId)) return false;
	cancelPendingSimulatorPaneShutdown(worktreeId);
	const delayMs = options.delayMs ?? DEFAULT_SHUTDOWN_GRACE_MS;
	const shutdown = options.shutdownManagedSimulator ?? shutdownManagedSimulator;
	const timer = setTimeout(() => {
		pendingShutdownTimersByWorktree.delete(worktreeId);
		if (!shouldShutdownSimulatorForPaneUnmountFromTabs(getTabsForWorktree(worktreeId))) return;
		shutdownManagedSimulatorIfNoPane(worktreeId, void 0, {
			getTabsForWorktree,
			shutdownManagedSimulator: shutdown
		});
	}, delayMs);
	pendingShutdownTimersByWorktree.set(worktreeId, timer);
	return true;
}
const EMULATOR_MANUAL_LAUNCH_STARTED_EVENT = "orca:emulator-launch-started";
const EMULATOR_MANUAL_LAUNCH_FAILED_EVENT = "orca:emulator-launch-failed";
var manualLaunchesByWorktree = /* @__PURE__ */ new Set();
var PRELAUNCHED_SIMULATOR_SESSION_TTL_MS = 3e4;
var PRELAUNCHED_SIMULATOR_SESSION_MAX = 16;
var prelaunchedSessionsByWorktree = /* @__PURE__ */ new Map();
function prunePrelaunchedSimulatorSessions(now = performance.now()) {
	for (const [worktreeId, entry] of prelaunchedSessionsByWorktree) if (now - entry.rememberedAt >= PRELAUNCHED_SIMULATOR_SESSION_TTL_MS) prelaunchedSessionsByWorktree.delete(worktreeId);
	while (prelaunchedSessionsByWorktree.size > PRELAUNCHED_SIMULATOR_SESSION_MAX) {
		const oldest = prelaunchedSessionsByWorktree.keys().next().value;
		if (oldest === void 0) return;
		prelaunchedSessionsByWorktree.delete(oldest);
	}
}
function beginManualSimulatorLaunch(worktreeId) {
	manualLaunchesByWorktree.add(worktreeId);
}
function finishManualSimulatorLaunch(worktreeId) {
	manualLaunchesByWorktree.delete(worktreeId);
}
function isManualSimulatorLaunchPending(worktreeId) {
	return manualLaunchesByWorktree.has(worktreeId);
}
function rememberPrelaunchedSimulatorSession(worktreeId, info) {
	if (!info?.streamUrl && !info?.wsUrl) return;
	const rememberedAt = performance.now();
	prelaunchedSessionsByWorktree.delete(worktreeId);
	prelaunchedSessionsByWorktree.set(worktreeId, {
		info,
		rememberedAt
	});
	prunePrelaunchedSimulatorSessions(rememberedAt);
}
function consumePrelaunchedSimulatorSession(worktreeId) {
	prunePrelaunchedSimulatorSessions();
	const info = prelaunchedSessionsByWorktree.get(worktreeId)?.info ?? null;
	prelaunchedSessionsByWorktree.delete(worktreeId);
	return info;
}
function dispatchManualSimulatorLaunchStarted(worktreeId) {
	dispatchManualSimulatorLaunchEvent(EMULATOR_MANUAL_LAUNCH_STARTED_EVENT, { worktreeId });
}
function dispatchManualSimulatorLaunchFailed(worktreeId, message) {
	dispatchManualSimulatorLaunchEvent(EMULATOR_MANUAL_LAUNCH_FAILED_EVENT, {
		worktreeId,
		message
	});
}
function dispatchManualSimulatorLaunchEvent(type, detail) {
	if (typeof window === "undefined") return;
	window.setTimeout(() => window.dispatchEvent(new CustomEvent(type, { detail })), 0);
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var driverByBrowserPageId = /* @__PURE__ */ new Map();
var changeListeners = /* @__PURE__ */ new Set();
var snapshotListeners = /* @__PURE__ */ new Set();
var version$1 = 0;
function onBrowserDriverChange(listener) {
	changeListeners.add(listener);
	return () => changeListeners.delete(listener);
}
function subscribe$1(listener) {
	snapshotListeners.add(listener);
	return () => {
		snapshotListeners.delete(listener);
	};
}
function getSnapshot$1() {
	return version$1;
}
function getServerSnapshot$1() {
	return 0;
}
function notifyChange(event) {
	version$1 += 1;
	for (const listener of changeListeners) listener(event);
	for (const listener of snapshotListeners) listener();
}
function setDriverForBrowserPage(browserPageId, driver) {
	if (driver.kind === "idle") driverByBrowserPageId.delete(browserPageId);
	else driverByBrowserPageId.set(browserPageId, driver);
	notifyChange({
		browserPageId,
		driver
	});
}
function getDriverForBrowserPage(browserPageId) {
	return driverByBrowserPageId.get(browserPageId) ?? { kind: "idle" };
}
function isBrowserPageMobileDriven(browserPageId) {
	return driverByBrowserPageId.get(browserPageId)?.kind === "mobile";
}
function hasMobileDriverForAnyBrowserPage(browserPageIds) {
	return browserPageIds.some((pageId) => Boolean(pageId && isBrowserPageMobileDriven(pageId)));
}
function getBrowserMobileDrivenPageIds(browserPageIds) {
	const mobileDrivenPageIds = /* @__PURE__ */ new Set();
	for (const pageId of browserPageIds) if (pageId && isBrowserPageMobileDriven(pageId)) mobileDrivenPageIds.add(pageId);
	return mobileDrivenPageIds;
}
function useBrowserMobileDriverForAny(browserPageIds) {
	(0, import_react.useSyncExternalStore)(subscribe$1, getSnapshot$1, getServerSnapshot$1);
	return hasMobileDriverForAnyBrowserPage(browserPageIds);
}
function useBrowserMobileDrivenPageIds(browserPageIds) {
	(0, import_react.useSyncExternalStore)(subscribe$1, getSnapshot$1, getServerSnapshot$1);
	return getBrowserMobileDrivenPageIds(browserPageIds);
}
function hydrateBrowserDrivers(drivers) {
	const affectedPageIds = new Set(driverByBrowserPageId.keys());
	driverByBrowserPageId.clear();
	for (const { browserPageId, driver } of drivers) {
		affectedPageIds.add(browserPageId);
		if (driver.kind !== "idle") driverByBrowserPageId.set(browserPageId, driver);
	}
	for (const browserPageId of affectedPageIds) notifyChange({
		browserPageId,
		driver: getDriverForBrowserPage(browserPageId)
	});
}
var leaseCountsByPageId = /* @__PURE__ */ new Map();
var pageIdByToken = /* @__PURE__ */ new Map();
var listeners = /* @__PURE__ */ new Set();
var version = 0;
var nextLeaseId = 0;
var AUTOMATION_VISIBILITY_PAINT_TIMEOUT_MS = 2e3;
function emitChange() {
	version += 1;
	for (const listener of listeners) listener();
}
function subscribe(listener) {
	listeners.add(listener);
	return () => {
		listeners.delete(listener);
	};
}
function onBrowserAutomationVisibilityChange(listener) {
	return subscribe(listener);
}
function getSnapshot() {
	return version;
}
function getServerSnapshot() {
	return 0;
}
function nextAnimationFrame() {
	if (typeof window === "undefined" || typeof window.requestAnimationFrame !== "function") return Promise.resolve();
	return new Promise((resolve) => window.requestAnimationFrame(() => resolve()));
}
async function waitForAutomationVisiblePaint() {
	let timeout = null;
	const paint = (async () => {
		await nextAnimationFrame();
		await nextAnimationFrame();
		return true;
	})();
	const timedOut = new Promise((resolve) => {
		timeout = setTimeout(() => resolve(false), AUTOMATION_VISIBILITY_PAINT_TIMEOUT_MS);
	});
	try {
		return await Promise.race([paint, timedOut]);
	} finally {
		if (timeout !== null) clearTimeout(timeout);
	}
}
function isBrowserAutomationVisible(browserPageId) {
	return (leaseCountsByPageId.get(browserPageId) ?? 0) > 0;
}
function useBrowserAutomationVisibilityForAny(browserPageIds) {
	(0, import_react.useSyncExternalStore)(subscribe, getSnapshot, getServerSnapshot);
	return browserPageIds.some((pageId) => Boolean(pageId && isBrowserAutomationVisible(pageId)));
}
function getBrowserAutomationVisiblePageIds(browserPageIds) {
	const visible = /* @__PURE__ */ new Set();
	for (const pageId of browserPageIds) if (isBrowserAutomationVisible(pageId)) visible.add(pageId);
	return visible;
}
function useBrowserAutomationVisiblePageIds(browserPageIds) {
	(0, import_react.useSyncExternalStore)(subscribe, getSnapshot, getServerSnapshot);
	return getBrowserAutomationVisiblePageIds(browserPageIds);
}
function acquireBrowserAutomationVisibility(browserPageId) {
	const token = `browser-automation-${Date.now()}-${++nextLeaseId}`;
	pageIdByToken.set(token, browserPageId);
	leaseCountsByPageId.set(browserPageId, (leaseCountsByPageId.get(browserPageId) ?? 0) + 1);
	emitChange();
	return token;
}
function releaseBrowserAutomationVisibility(token) {
	const browserPageId = pageIdByToken.get(token);
	if (!browserPageId) return false;
	pageIdByToken.delete(token);
	const nextCount = (leaseCountsByPageId.get(browserPageId) ?? 1) - 1;
	if (nextCount > 0) leaseCountsByPageId.set(browserPageId, nextCount);
	else leaseCountsByPageId.delete(browserPageId);
	emitChange();
	return true;
}
async function acquireForMainProcess(browserPageId) {
	if (typeof browserPageId !== "string" || browserPageId.length === 0) return null;
	const token = acquireBrowserAutomationVisibility(browserPageId);
	if (await waitForAutomationVisiblePaint()) return token;
	releaseBrowserAutomationVisibility(token);
	return null;
}
function installBrowserAutomationVisibilityBridge() {
	if (typeof window === "undefined") return;
	window.__orcaBrowserAutomationVisibility = {
		acquire: acquireForMainProcess,
		release: releaseBrowserAutomationVisibility
	};
}
installBrowserAutomationVisibilityBridge();
export { cancelPendingSimulatorPaneShutdown as C, GitCompare as D, OctagonX as E, Crosshair as O, rememberPrelaunchedSimulatorSession as S, shutdownManagedSimulatorIfNoPane as T, consumePrelaunchedSimulatorSession as _, useBrowserAutomationVisibilityForAny as a, finishManualSimulatorLaunch as b, hydrateBrowserDrivers as c, setDriverForBrowserPage as d, useBrowserMobileDrivenPageIds as f, beginManualSimulatorLaunch as g, EMULATOR_MANUAL_LAUNCH_STARTED_EVENT as h, releaseBrowserAutomationVisibility as i, isBrowserPageMobileDriven as l, EMULATOR_MANUAL_LAUNCH_FAILED_EVENT as m, isBrowserAutomationVisible as n, useBrowserAutomationVisiblePageIds as o, useBrowserMobileDriverForAny as p, onBrowserAutomationVisibilityChange as r, getDriverForBrowserPage as s, acquireBrowserAutomationVisibility as t, onBrowserDriverChange as u, dispatchManualSimulatorLaunchFailed as v, scheduleSimulatorPaneManagedShutdown as w, isManualSimulatorLaunchPending as x, dispatchManualSimulatorLaunchStarted as y };
