import { Ac as normalizeRelativePath, ku as getSettingsForWorktreeRuntimeOwner, t as useAppStore } from "./store-CgXrfmaH.js";
function getRightSidebarWorktreeRuntimeSettings(worktreeId) {
	return getSettingsForWorktreeRuntimeOwner(useAppStore.getState(), worktreeId);
}
var SEARCH_GLOB_LITERAL_META = new Set([
	"\\",
	"*",
	"?",
	"[",
	"]",
	"{",
	"}",
	"!",
	","
]);
function escapeSearchGlobLiteralSegment(segment) {
	let escaped = "";
	for (const ch of segment) escaped += SEARCH_GLOB_LITERAL_META.has(ch) ? `\\${ch}` : ch;
	return escaped;
}
function folderRelativePathToIncludeGlob(relativePath) {
	const normalized = normalizeRelativePath(relativePath).replace(/\/+$/, "");
	if (!normalized) return "";
	return `${normalized.split("/").map(escapeSearchGlobLiteralSegment).join("/")}/**`;
}
function selectedExplorerFolderRelativePath(activeElement) {
	return (activeElement?.closest("[data-orca-explorer-shell]"))?.getAttribute("data-selected-folder-relative-path") ?? null;
}
export { selectedExplorerFolderRelativePath as n, getRightSidebarWorktreeRuntimeSettings as r, folderRelativePathToIncludeGlob as t };
