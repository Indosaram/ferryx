import { t as useAppStore } from "./store-CgXrfmaH.js";
import { w as keybindingMatchesAction } from "./plugin-manifest-Bs-50M_g.js";
import { t as getShortcutPlatform } from "./shortcut-platform-BbPBGzth.js";
function editorShortcutMatches(actionId, event) {
	return keybindingMatchesAction(actionId, event, getShortcutPlatform(), useAppStore.getState().keybindings);
}
function installEditorSaveShortcut(target, onSave) {
	const handleKeyDown = (event) => {
		if (event.repeat || !editorShortcutMatches("editor.save", event)) return;
		event.preventDefault();
		event.stopPropagation();
		onSave();
	};
	target.addEventListener("keydown", handleKeyDown, true);
	return () => target.removeEventListener("keydown", handleKeyDown, true);
}
function installEditorFindShortcut(target, onFind) {
	const handleKeyDown = (event) => {
		if (!editorShortcutMatches("editor.find", event)) return;
		event.preventDefault();
		event.stopPropagation();
		if (!event.repeat) onFind();
	};
	target.addEventListener("keydown", handleKeyDown, true);
	return () => target.removeEventListener("keydown", handleKeyDown, true);
}
function installMonacoDiffChangeNavigationShortcut(editor) {
	const handleKeyDown = (event) => {
		let direction = null;
		if (editorShortcutMatches("editor.nextChange", event)) direction = "next";
		else if (editorShortcutMatches("editor.previousChange", event)) direction = "previous";
		if (!direction) return;
		event.preventDefault();
		event.stopPropagation();
		if (!event.repeat) editor.goToDiff(direction);
	};
	const target = editor.getContainerDomNode();
	target.addEventListener("keydown", handleKeyDown, true);
	return () => target.removeEventListener("keydown", handleKeyDown, true);
}
function installEditorAddReviewNoteShortcut(target, onAddReviewNote) {
	const handleKeyDown = (event) => {
		if (!editorShortcutMatches("editor.addReviewNote", event)) return;
		if (event.repeat) return;
		if (onAddReviewNote()) {
			event.preventDefault();
			event.stopPropagation();
		}
	};
	target.addEventListener("keydown", handleKeyDown, true);
	return () => target.removeEventListener("keydown", handleKeyDown, true);
}
function installOpenDraftAddReviewNoteGuard(target) {
	const handleKeyDown = (event) => {
		if (!editorShortcutMatches("editor.addReviewNote", event)) return;
		event.preventDefault();
		event.stopPropagation();
	};
	target.addEventListener("keydown", handleKeyDown, true);
	return () => target.removeEventListener("keydown", handleKeyDown, true);
}
function installMonacoEditorFindShortcut(editor) {
	return installEditorFindShortcut(editor.getContainerDomNode(), () => {
		editor.getAction("actions.find")?.run();
	});
}
export { installMonacoEditorFindShortcut as a, installMonacoDiffChangeNavigationShortcut as i, installEditorAddReviewNoteShortcut as n, installOpenDraftAddReviewNoteGuard as o, installEditorSaveShortcut as r, editorShortcutMatches as t };
