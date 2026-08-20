const PRIMARY_SELECTION_MAX_LENGTH = 65536;
var PRIMARY_SELECTION_MAX_BYTES = PRIMARY_SELECTION_MAX_LENGTH * 4;
var PRIMARY_SELECTION_NATIVE_PASTE_SUPPRESSION_MS = 750;
var enabled = false;
var primarySelectionText = "";
var nativePasteSuppressionUntil = 0;
function isLinuxUserAgent(userAgent) {
	return !userAgent.includes("Mac") && !userAgent.includes("Windows") && userAgent.includes("Linux");
}
function getUserAgent() {
	return typeof navigator === "undefined" ? "" : navigator.userAgent;
}
function getSelectionClipboardApi() {
	if (typeof window === "undefined") return null;
	const uiApi = window.api?.ui;
	if (typeof uiApi?.readSelectionClipboardText !== "function" || typeof uiApi.writeSelectionClipboardText !== "function") return null;
	return uiApi;
}
function shouldUseSystemPrimarySelectionClipboard(userAgent = getUserAgent()) {
	return isLinuxUserAgent(userAgent) && getSelectionClipboardApi() !== null;
}
function canStorePrimarySelectionText(text) {
	return enabled && text.length > 0 && text.length <= 65536;
}
function setPrimarySelectionEnabled(nextEnabled) {
	enabled = nextEnabled;
	if (!enabled) {
		primarySelectionText = "";
		nativePasteSuppressionUntil = 0;
	}
}
function armPrimarySelectionNativePasteSuppression(now = Date.now()) {
	if (!enabled || !isLinuxUserAgent(getUserAgent())) return;
	nativePasteSuppressionUntil = now + PRIMARY_SELECTION_NATIVE_PASTE_SUPPRESSION_MS;
}
function consumePrimarySelectionNativePasteSuppression(now = Date.now()) {
	if (!enabled || nativePasteSuppressionUntil === 0 || now > nativePasteSuppressionUntil) return false;
	nativePasteSuppressionUntil = 0;
	return true;
}
function isPrimarySelectionEnabled() {
	return enabled;
}
function setPrimarySelectionText(text) {
	if (!canStorePrimarySelectionText(text)) return false;
	primarySelectionText = text;
	const selectionClipboardApi = shouldUseSystemPrimarySelectionClipboard() ? getSelectionClipboardApi() : null;
	if (selectionClipboardApi) {
		selectionClipboardApi.writeSelectionClipboardText(text).catch(() => {});
		return true;
	}
	return true;
}
async function readPrimarySelectionText() {
	if (!enabled) return "";
	const selectionClipboardApi = shouldUseSystemPrimarySelectionClipboard() ? getSelectionClipboardApi() : null;
	if (!selectionClipboardApi) return primarySelectionText;
	try {
		return await selectionClipboardApi.readSelectionClipboardText({ maxBytes: PRIMARY_SELECTION_MAX_BYTES });
	} catch {
		return primarySelectionText;
	}
}
export { readPrimarySelectionText as a, isPrimarySelectionEnabled as i, armPrimarySelectionNativePasteSuppression as n, setPrimarySelectionEnabled as o, consumePrimarySelectionNativePasteSuppression as r, setPrimarySelectionText as s, PRIMARY_SELECTION_MAX_LENGTH as t };
