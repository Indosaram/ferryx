const ORCA_BROWSER_FOCUS_REQUEST_EVENT = "orca:browser-focus-request";
var FOCUS_REQUEST_TTL_MS = 3e4;
var pendingBrowserFocusByPageId = /* @__PURE__ */ new Map();
var expiredRequestCleanupTimer = null;
function clearExpiredRequestCleanupTimerIfIdle() {
	if (pendingBrowserFocusByPageId.size > 0 || expiredRequestCleanupTimer === null) return;
	clearTimeout(expiredRequestCleanupTimer);
	expiredRequestCleanupTimer = null;
}
function purgeExpiredFocusRequests(now = Date.now()) {
	for (const [pageId, request] of pendingBrowserFocusByPageId) if (request.expiresAt <= now) pendingBrowserFocusByPageId.delete(pageId);
	clearExpiredRequestCleanupTimerIfIdle();
}
function scheduleExpiredRequestCleanup() {
	if (expiredRequestCleanupTimer !== null || pendingBrowserFocusByPageId.size === 0) return;
	let nextExpiresAt = Infinity;
	for (const request of pendingBrowserFocusByPageId.values()) nextExpiresAt = Math.min(nextExpiresAt, request.expiresAt);
	expiredRequestCleanupTimer = setTimeout(() => {
		expiredRequestCleanupTimer = null;
		purgeExpiredFocusRequests();
		scheduleExpiredRequestCleanup();
	}, Math.max(0, nextExpiresAt - Date.now()));
}
function queueBrowserFocusRequest(detail) {
	const now = Date.now();
	purgeExpiredFocusRequests(now);
	pendingBrowserFocusByPageId.set(detail.pageId, {
		target: detail.target,
		expiresAt: now + FOCUS_REQUEST_TTL_MS
	});
	scheduleExpiredRequestCleanup();
}
function requestBrowserFocus(detail) {
	queueBrowserFocusRequest(detail);
	window.dispatchEvent(new CustomEvent(ORCA_BROWSER_FOCUS_REQUEST_EVENT, { detail }));
}
function consumeBrowserFocusRequest(pageId) {
	purgeExpiredFocusRequests();
	const pending = pendingBrowserFocusByPageId.get(pageId) ?? null;
	if (!pending) return null;
	pendingBrowserFocusByPageId.delete(pageId);
	clearExpiredRequestCleanupTimerIfIdle();
	return pending.target;
}
export { requestBrowserFocus as i, consumeBrowserFocusRequest as n, queueBrowserFocusRequest as r, ORCA_BROWSER_FOCUS_REQUEST_EVENT as t };
