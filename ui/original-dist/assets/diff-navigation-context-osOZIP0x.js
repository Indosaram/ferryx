import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { i as installMonacoDiffChangeNavigationShortcut } from "./editor-shortcuts-Cg6u73ie.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var noop = () => {};
var DiffEditorRegistrationContext = (0, import_react.createContext)({
	registerDiffEditor: noop,
	unregisterDiffEditor: noop
});
var DiffNavigationContext = (0, import_react.createContext)({
	goToPreviousDiff: noop,
	goToNextDiff: noop,
	changeCount: 0
});
function countChanges(diffEditor) {
	return diffEditor.getLineChanges()?.length ?? 0;
}
function DiffNavigationProvider({ children }) {
	const editorRef = (0, import_react.useRef)(null);
	const updateSubRef = (0, import_react.useRef)(null);
	const shortcutCleanupRef = (0, import_react.useRef)(null);
	const [changeCount, setChangeCount] = (0, import_react.useState)(0);
	const registerDiffEditor = (0, import_react.useCallback)((diffEditor) => {
		editorRef.current = diffEditor;
		updateSubRef.current?.dispose();
		updateSubRef.current = diffEditor.onDidUpdateDiff(() => {
			if (editorRef.current === diffEditor) setChangeCount(countChanges(diffEditor));
		});
		shortcutCleanupRef.current?.();
		shortcutCleanupRef.current = installMonacoDiffChangeNavigationShortcut(diffEditor);
		setChangeCount(countChanges(diffEditor));
	}, []);
	const unregisterDiffEditor = (0, import_react.useCallback)((diffEditor) => {
		if (editorRef.current !== diffEditor) return;
		updateSubRef.current?.dispose();
		updateSubRef.current = null;
		shortcutCleanupRef.current?.();
		shortcutCleanupRef.current = null;
		editorRef.current = null;
		setChangeCount(0);
	}, []);
	const goToPreviousDiff = (0, import_react.useCallback)(() => {
		editorRef.current?.goToDiff("previous");
	}, []);
	const goToNextDiff = (0, import_react.useCallback)(() => {
		editorRef.current?.goToDiff("next");
	}, []);
	(0, import_react.useEffect)(() => {
		return () => {
			updateSubRef.current?.dispose();
			updateSubRef.current = null;
			shortcutCleanupRef.current?.();
			shortcutCleanupRef.current = null;
		};
	}, []);
	const registrationValue = (0, import_react.useMemo)(() => ({
		registerDiffEditor,
		unregisterDiffEditor
	}), [registerDiffEditor, unregisterDiffEditor]);
	const navigationValue = (0, import_react.useMemo)(() => ({
		goToPreviousDiff,
		goToNextDiff,
		changeCount
	}), [
		goToPreviousDiff,
		goToNextDiff,
		changeCount
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DiffEditorRegistrationContext.Provider, {
		value: registrationValue,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DiffNavigationContext.Provider, {
			value: navigationValue,
			children
		})
	});
}
function useDiffEditorRegistration() {
	return (0, import_react.useContext)(DiffEditorRegistrationContext);
}
function useDiffNavigation() {
	return (0, import_react.useContext)(DiffNavigationContext);
}
export { useDiffEditorRegistration as n, useDiffNavigation as r, DiffNavigationProvider as t };
