import { Bg as isLocalWindowsDesktopClient, Fm as MAX_EDITOR_AUTO_SAVE_DELAY_MS, Im as MIN_EDITOR_AUTO_SAVE_DELAY_MS, Zg as clampNumber, kc as joinPath, km as DEFAULT_EDITOR_AUTO_SAVE_DELAY_MS, kp as areLocalWindowsWslPathAliases } from "./store-CgXrfmaH.js";
const ORCA_EDITOR_QUIESCE_FILE_SAVES_EVENT = "orca:editor-quiesce-file-saves";
const ORCA_EDITOR_EXTERNAL_FILE_CHANGE_EVENT = "orca:editor-external-file-change";
const ORCA_EDITOR_SAVE_FILE_EVENT = "orca:editor-save-file";
const ORCA_EDITOR_SAVE_AND_CLOSE_EVENT = "orca:save-and-close";
const ORCA_EDITOR_FILE_SAVED_EVENT = "orca:editor-file-saved";
const ORCA_EDITOR_REQUEST_CMD_SAVE_EVENT = "orca:editor-request-cmd-save";
const ORCA_EDITOR_REQUEST_FILE_CLOSE_EVENT = "orca:editor-request-file-close";
function isExternalReloadableEditorTab(file) {
	return file.mode === "edit" || file.mode === "markdown-preview" || file.mode === "diff" && (file.diffSource === "unstaged" || file.diffSource === "staged");
}
function canAutoSaveOpenFile(file) {
	if (file.readOnly === true) return false;
	return file.mode === "edit" || file.mode === "diff" && file.diffSource === "unstaged";
}
function isAutosaveSuspendedForFile(file) {
	return file.externalMutation === "changed" || file.pendingDiskBaselineVerification === true || file.pendingLiveDiskVerification === true || file.pendingOwnerMigration === true;
}
function normalizeAutoSaveDelayMs(value) {
	const numericValue = typeof value === "string" ? Number(value) : typeof value === "number" ? value : null;
	return clampNumber(numericValue !== null && Number.isFinite(numericValue) ? numericValue : DEFAULT_EDITOR_AUTO_SAVE_DELAY_MS, 250, MAX_EDITOR_AUTO_SAVE_DELAY_MS);
}
function getOpenFilesForExternalFileChange(openFiles, target) {
	if (target.indexedOpenFiles) return target.indexedOpenFiles.matches(openFiles);
	const absolutePath = joinPath(target.worktreePath, target.relativePath);
	const hasRuntimeOwnerFilter = Object.hasOwn(target, "runtimeEnvironmentId");
	const targetRuntimeOwner = target.runtimeEnvironmentId?.trim() || null;
	return openFiles.filter((file) => {
		if (file.worktreeId !== target.worktreeId) return false;
		if (hasRuntimeOwnerFilter && (file.runtimeEnvironmentId?.trim() || null) !== targetRuntimeOwner) return false;
		if (file.mode === "edit" || file.mode === "markdown-preview") return file.filePath === absolutePath || target.allowLocalWindowsWslAliases === true && isLocalWindowsDesktopClient() && areLocalWindowsWslPathAliases(file.filePath, absolutePath);
		if (file.mode === "diff") return (file.diffSource === "unstaged" || file.diffSource === "staged") && file.relativePath === target.relativePath;
		return false;
	});
}
async function requestEditorSaveQuiesce(target) {
	await new Promise((resolve) => {
		let claimed = false;
		window.dispatchEvent(new CustomEvent(ORCA_EDITOR_QUIESCE_FILE_SAVES_EVENT, { detail: {
			...target,
			claim: () => {
				claimed = true;
			},
			resolve
		} }));
		if (!claimed) resolve();
	});
}
async function requestEditorFileSave(target) {
	await new Promise((resolve, reject) => {
		let claimed = false;
		window.dispatchEvent(new CustomEvent(ORCA_EDITOR_SAVE_FILE_EVENT, { detail: {
			...target,
			claim: () => {
				claimed = true;
			},
			resolve,
			reject: (message) => reject(new Error(message))
		} }));
		if (!claimed) reject(/* @__PURE__ */ new Error("Editor save controller is unavailable."));
	});
}
function requestEditorFileClose(fileId) {
	window.dispatchEvent(new CustomEvent(ORCA_EDITOR_REQUEST_FILE_CLOSE_EVENT, { detail: { fileId } }));
}
function notifyEditorExternalFileChange(target) {
	window.dispatchEvent(new CustomEvent(ORCA_EDITOR_EXTERNAL_FILE_CHANGE_EVENT, { detail: target }));
}
export { ORCA_EDITOR_REQUEST_FILE_CLOSE_EVENT as a, canAutoSaveOpenFile as c, isExternalReloadableEditorTab as d, normalizeAutoSaveDelayMs as f, requestEditorSaveQuiesce as g, requestEditorFileSave as h, ORCA_EDITOR_REQUEST_CMD_SAVE_EVENT as i, getOpenFilesForExternalFileChange as l, requestEditorFileClose as m, ORCA_EDITOR_FILE_SAVED_EVENT as n, ORCA_EDITOR_SAVE_AND_CLOSE_EVENT as o, notifyEditorExternalFileChange as p, ORCA_EDITOR_QUIESCE_FILE_SAVES_EVENT as r, ORCA_EDITOR_SAVE_FILE_EVENT as s, ORCA_EDITOR_EXTERNAL_FILE_CHANGE_EVENT as t, isAutosaveSuspendedForFile as u };
