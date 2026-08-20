function fitPanes(manager) {
	manager.fitAllPanes();
}
function focusActivePane(manager) {
	if (typeof document !== "undefined" && document.querySelector("[data-tab-rename-input=\"true\"]")) return;
	if (shouldPreserveEditableFocus(typeof document === "undefined" ? null : document.activeElement)) return;
	const panes = manager.getPanes();
	(manager.getActivePane() ?? panes[0])?.terminal.focus();
}
function fitAndFocusPanes(manager) {
	fitPanes(manager);
	focusActivePane(manager);
}
function isWindowsUserAgent(userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent) {
	return userAgent.includes("Windows");
}
function isMacUserAgent(userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent) {
	return userAgent.includes("Mac");
}
function isLinuxUserAgent(userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent) {
	return !isMacUserAgent(userAgent) && !isWindowsUserAgent(userAgent) && userAgent.includes("Linux");
}
function shouldPreserveEditableFocus(element) {
	if (!(element instanceof HTMLElement)) return false;
	if (element.classList.contains("xterm-helper-textarea") || element.closest(".xterm")) return false;
	return element.isContentEditable || element.tagName === "INPUT" || element.tagName === "TEXTAREA" || element.tagName === "SELECT";
}
function shellEscapePath(path, targetShell) {
	if (targetShell === "windows") return /^[a-zA-Z0-9_./@:\\-]+$/.test(path) ? path : `"${path}"`;
	if (/^[a-zA-Z0-9_./@:-]+$/.test(path)) return path;
	return `'${path.replace(/'/g, "'\\''")}'`;
}
export { isMacUserAgent as a, isLinuxUserAgent as i, fitPanes as n, isWindowsUserAgent as o, focusActivePane as r, shellEscapePath as s, fitAndFocusPanes as t };
