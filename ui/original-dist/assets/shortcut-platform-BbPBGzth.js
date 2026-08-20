function getShortcutPlatform() {
	if (navigator.userAgent.includes("Mac")) return "darwin";
	return navigator.userAgent.includes("Windows") ? "win32" : "linux";
}
export { getShortcutPlatform as t };
