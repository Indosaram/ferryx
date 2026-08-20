const THEME_TRANSITION_DISABLED_CLASS = "theme-transition-disabled";
var DARK_MODE_QUERY = "(prefers-color-scheme: dark)";
var pendingTransitionDisableFrames = [];
function cancelPendingTransitionDisableFrames(cancelFrame) {
	for (const frameId of pendingTransitionDisableFrames) cancelFrame(frameId);
	pendingTransitionDisableFrames = [];
}
function systemPrefersDark(matchMedia = window.matchMedia.bind(window)) {
	return matchMedia(DARK_MODE_QUERY).matches;
}
function resolveDocumentTheme(theme, matchMedia) {
	if (theme === "dark") return true;
	if (theme === "light") return false;
	return systemPrefersDark(matchMedia);
}
function applyDocumentTheme(theme, options = {}) {
	const root = options.root ?? document.documentElement;
	const disableTransitions = options.disableTransitions ?? true;
	const shouldUseDarkTheme = resolveDocumentTheme(theme, options.matchMedia);
	if (disableTransitions) root.classList.add(THEME_TRANSITION_DISABLED_CLASS);
	root.classList.toggle("dark", shouldUseDarkTheme);
	root.classList.toggle("light", !shouldUseDarkTheme);
	if (!disableTransitions) return;
	const requestFrame = options.requestAnimationFrame ?? window.requestAnimationFrame.bind(window);
	cancelPendingTransitionDisableFrames(options.cancelAnimationFrame ?? window.cancelAnimationFrame.bind(window));
	const firstFrame = requestFrame(() => {
		pendingTransitionDisableFrames = pendingTransitionDisableFrames.filter((id) => id !== firstFrame);
		const secondFrame = requestFrame(() => {
			pendingTransitionDisableFrames = pendingTransitionDisableFrames.filter((id) => id !== secondFrame);
			root.classList.remove(THEME_TRANSITION_DISABLED_CLASS);
		});
		pendingTransitionDisableFrames.push(secondFrame);
	});
	pendingTransitionDisableFrames.push(firstFrame);
}
export { resolveDocumentTheme as n, applyDocumentTheme as t };
