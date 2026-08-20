import { t as getShortcutPlatform } from "./shortcut-platform-BbPBGzth.js";
function isScreenSubmitShortcut(event) {
	if (event.isComposing || event.nativeEvent?.isComposing) return false;
	if (event.key !== "Enter" || event.altKey || event.shiftKey) return false;
	return getShortcutPlatform() === "darwin" ? Boolean(event.metaKey) && !event.ctrlKey : Boolean(event.ctrlKey) && !event.metaKey;
}
function getScreenSubmitModifierLabel() {
	return getShortcutPlatform() === "darwin" ? "⌘" : "Ctrl";
}
function getScreenSubmitShortcutLabel() {
	return getShortcutPlatform() === "darwin" ? "⌘ Enter" : "Ctrl+Enter";
}
export { getScreenSubmitShortcutLabel as n, isScreenSubmitShortcut as r, getScreenSubmitModifierLabel as t };
