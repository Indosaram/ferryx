function isMacPlatform() {
	return navigator.userAgent.includes("Mac");
}
function terminalLinkActionHintPrefix(showActions) {
	return showActions ? "Click for actions, " : "";
}
function getTerminalFileOpenHint(showActions = true) {
	const prefix = terminalLinkActionHintPrefix(showActions);
	return isMacPlatform() ? `${prefix}⌘+click to open, or ⇧⌘+click for default app` : `${prefix}Ctrl+click to open, or Shift+Ctrl+click for default app`;
}
function getTerminalOrcaFileOpenHint(showActions = true) {
	const prefix = showActions ? "Click for actions or " : "";
	return isMacPlatform() ? `${prefix}⌘+click to open in Orca` : `${prefix}Ctrl+click to open in Orca`;
}
function getTerminalHtmlFileOpenHint(showActions = true) {
	const prefix = terminalLinkActionHintPrefix(showActions);
	return isMacPlatform() ? `${prefix}⌘+click to open, or ⇧⌘+click for default browser` : `${prefix}Ctrl+click to open, or Shift+Ctrl+click for default browser`;
}
function terminalHttpLinkActionDestinationsFor(settings, sourceOwner, canOpenRuntimeBrowser) {
	if (!(sourceOwner.kind === "local" || sourceOwner.kind === "runtime" && canOpenRuntimeBrowser)) return { primary: "system" };
	return settings?.openLinksInApp === true ? {
		primary: "orca",
		alternate: "system"
	} : {
		primary: "system",
		alternate: "orca"
	};
}
function terminalUrlOpenHintOptionsFor(settings, sourceOwner, canOpenRuntimeBrowser = false) {
	const sourceCanOpenInOrca = sourceOwner ? sourceOwner.kind === "local" || sourceOwner.kind === "runtime" && canOpenRuntimeBrowser : !settings?.activeRuntimeEnvironmentId?.trim();
	return {
		openLinksInApp: settings?.openLinksInApp === true,
		modifierInverts: settings?.openLinksInAppModifierInverts === true && sourceCanOpenInOrca
	};
}
function getTerminalUrlOpenHint(options = {}) {
	const invertsToOrca = options.modifierInverts === true && options.openLinksInApp !== true;
	const prefix = terminalLinkActionHintPrefix(options.showActions !== false);
	if (invertsToOrca) return isMacPlatform() ? `${prefix}⌘+click to open, or ⇧⌘+click to open in Orca` : `${prefix}Ctrl+click to open, or Shift+Ctrl+click to open in Orca`;
	return isMacPlatform() ? `${prefix}⌘+click to open, or ⇧⌘+click for system browser` : `${prefix}Ctrl+click to open, or Shift+Ctrl+click for system browser`;
}
function getTerminalUrlSystemBrowserHint() {
	return isMacPlatform() ? "⇧⌘+click for system browser" : "Shift+Ctrl+click for system browser";
}
function getTerminalUrlOrcaBrowserHint() {
	return isMacPlatform() ? "⇧⌘+click to open in Orca" : "Shift+Ctrl+click to open in Orca";
}
function getTerminalWorktreePathOpenHint(canOpenWithSystemDefault, showActions = true) {
	const prefix = terminalLinkActionHintPrefix(showActions);
	if (!canOpenWithSystemDefault) {
		const directPrefix = showActions ? "Click for actions or " : "";
		return isMacPlatform() ? `${directPrefix}⌘+click to switch workspace` : `${directPrefix}Ctrl+click to switch workspace`;
	}
	return isMacPlatform() ? `${prefix}⌘+click to switch workspace, or ⇧⌘+click to open in Finder` : `${prefix}Ctrl+click to switch workspace, or Shift+Ctrl+click to open folder`;
}
export { getTerminalUrlOrcaBrowserHint as a, isMacPlatform as c, getTerminalUrlOpenHint as i, terminalHttpLinkActionDestinationsFor as l, getTerminalHtmlFileOpenHint as n, getTerminalUrlSystemBrowserHint as o, getTerminalOrcaFileOpenHint as r, getTerminalWorktreePathOpenHint as s, getTerminalFileOpenHint as t, terminalUrlOpenHintOptionsFor as u };
