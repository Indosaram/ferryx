import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { r as formatShortcutLabel } from "./useShortcutLabel-C-KRYtlB.js";
import { i as isPrimarySelectionEnabled, s as setPrimarySelectionText, t as PRIMARY_SELECTION_MAX_LENGTH } from "./primary-selection-BsidtYsF.js";
import { p as editor } from "./editor.api2-DX_-Ye6K.js";
import { t as editorShortcutMatches } from "./editor-shortcuts-Cg6u73ie.js";
function formatCopiedSelectionWithContext({ relativePath, language, selection, selectedText }) {
	const { startLine, endLine } = getContextualCopyLineRange(selection);
	if (selection.startLineNumber === selection.endLineNumber) return null;
	if (endLine < startLine) return null;
	const codeFenceLanguage = getCodeFenceLanguage(language);
	const codeBlock = selectedText.endsWith("\n") ? selectedText : `${selectedText}\n`;
	return `File: ${relativePath}\n${startLine === endLine ? `Line: ${startLine}` : `Lines: ${startLine}-${endLine}`}\n\n\`\`\`${codeFenceLanguage}\n${codeBlock}\`\`\``;
}
function getContextualCopyLineRange(selection) {
	return {
		startLine: selection.startLineNumber,
		endLine: getInclusiveEndLine(selection)
	};
}
function getInclusiveEndLine(selection) {
	if (selection.startLineNumber === selection.endLineNumber) return selection.endLineNumber;
	if (selection.endColumn === 1) return selection.endLineNumber - 1;
	return selection.endLineNumber;
}
function getCodeFenceLanguage(language) {
	switch (language) {
		case "plaintext": return "";
		case "typescript": return "ts";
		case "javascript": return "js";
		default: return language;
	}
}
function setupContextualCopy({ editorInstance, filePath, setCopyToast, propsRef, copyToastTimeoutRef }) {
	let copyHintInterval = null;
	let primarySelectionTimer = null;
	let copyHintWidgetPosition = null;
	let lastCopiedSelectionKey = null;
	const copyHintNode = document.createElement("div");
	copyHintNode.className = "pointer-events-none rounded-md border border-border/90 bg-background px-2.5 py-1 text-xs font-medium text-foreground shadow-[0_6px_18px_rgba(15,23,42,0.18)] backdrop-blur whitespace-nowrap";
	const updateCopyHintLabel = () => {
		copyHintNode.textContent = `Copy context ${formatShortcutLabel("editor.copyContext", useAppStore.getState().keybindings)}`;
	};
	updateCopyHintLabel();
	copyHintNode.style.display = "none";
	const copyHintWidget = {
		allowEditorOverflow: true,
		suppressMouseDown: true,
		getId: () => `orca.copy-context-hint.${filePath}`,
		getDomNode: () => copyHintNode,
		getPosition: () => copyHintWidgetPosition
	};
	editorInstance.addContentWidget(copyHintWidget);
	const showCopyToast = () => {
		const selection = editorInstance.getSelection();
		if (!selection) return;
		const visiblePosition = editorInstance.getScrolledVisiblePosition(selection.getEndPosition());
		const bounds = editorInstance.getContainerDomNode().getBoundingClientRect();
		setCopyToast({
			left: bounds.left + (visiblePosition?.left ?? bounds.width - 120),
			top: bounds.top + (visiblePosition?.top ?? 16) + (visiblePosition?.height ?? 20) + 8
		});
		if (copyToastTimeoutRef.current !== null) window.clearTimeout(copyToastTimeoutRef.current);
		copyToastTimeoutRef.current = window.setTimeout(() => {
			setCopyToast(null);
			copyToastTimeoutRef.current = null;
		}, 1200);
	};
	const getSelectionKey = () => {
		const selection = editorInstance.getSelection();
		if (!selection) return null;
		return [
			selection.startLineNumber,
			selection.startColumn,
			selection.endLineNumber,
			selection.endColumn
		].join(":");
	};
	const updateCopyHint = () => {
		updateCopyHintLabel();
		if (!getContextualCopyText()) {
			copyHintNode.style.display = "none";
			copyHintWidgetPosition = null;
			editorInstance.layoutContentWidget(copyHintWidget);
			return;
		}
		if (lastCopiedSelectionKey !== null && lastCopiedSelectionKey === getSelectionKey()) {
			copyHintNode.style.display = "none";
			copyHintWidgetPosition = null;
			editorInstance.layoutContentWidget(copyHintWidget);
			return;
		}
		const model = editorInstance.getModel();
		const selection = editorInstance.getSelection();
		if (!model || !selection) {
			copyHintNode.style.display = "none";
			copyHintWidgetPosition = null;
			editorInstance.layoutContentWidget(copyHintWidget);
			return;
		}
		const { startLine, endLine } = getContextualCopyLineRange(selection);
		const startVisiblePosition = editorInstance.getScrolledVisiblePosition(selection.getStartPosition());
		const endColumn = selection.endLineNumber === endLine ? selection.endColumn : model.getLineMaxColumn(endLine);
		const endVisiblePosition = editorInstance.getScrolledVisiblePosition({
			lineNumber: endLine,
			column: endColumn
		});
		if (!startVisiblePosition || !endVisiblePosition) {
			copyHintNode.style.display = "none";
			copyHintWidgetPosition = null;
			editorInstance.layoutContentWidget(copyHintWidget);
			return;
		}
		const hintHeight = copyHintNode.offsetHeight || 28;
		const verticalGap = 8;
		const viewportHeight = editorInstance.getLayoutInfo().height;
		const selectionTop = startVisiblePosition.top;
		const selectionBottom = endVisiblePosition.top + endVisiblePosition.height;
		const spaceAbove = selectionTop;
		const spaceBelow = viewportHeight - selectionBottom;
		const placeAbove = spaceAbove >= hintHeight + verticalGap || spaceAbove >= spaceBelow;
		const anchorLineNumber = placeAbove ? startLine : endLine;
		const anchorColumn = placeAbove ? model.getLineMaxColumn(startLine) : selection.endLineNumber !== endLine ? model.getLineMaxColumn(endLine) : selection.endColumn;
		copyHintWidgetPosition = {
			position: {
				lineNumber: anchorLineNumber,
				column: anchorColumn
			},
			secondaryPosition: {
				lineNumber: anchorLineNumber,
				column: Math.max(1, anchorColumn - 1)
			},
			preference: [placeAbove ? editor.ContentWidgetPositionPreference.ABOVE : editor.ContentWidgetPositionPreference.BELOW]
		};
		copyHintNode.style.display = "block";
		editorInstance.layoutContentWidget(copyHintWidget);
	};
	const isCopyHintVisible = () => copyHintNode.style.display === "block";
	const startCopyHintPolling = () => {
		if (copyHintInterval !== null) return;
		copyHintInterval = window.setInterval(() => {
			updateCopyHint();
			if (!isCopyHintVisible()) stopCopyHintPolling();
		}, 150);
	};
	const stopCopyHintPolling = () => {
		if (copyHintInterval !== null) {
			window.clearInterval(copyHintInterval);
			copyHintInterval = null;
		}
	};
	const refreshCopyHintAndPolling = () => {
		updateCopyHint();
		if (editorInstance.hasTextFocus() && isCopyHintVisible()) startCopyHintPolling();
		else stopCopyHintPolling();
	};
	const getContextualCopyText = () => {
		const model = editorInstance.getModel();
		const selection = editorInstance.getSelection();
		if (!model || !selection || selection.isEmpty()) return null;
		return formatCopiedSelectionWithContext({
			relativePath: propsRef.current.relativePath,
			language: propsRef.current.language,
			selection,
			selectedText: model.getValueInRange(selection)
		});
	};
	const updatePrimarySelectionBuffer = () => {
		const model = editorInstance.getModel();
		const selections = editorInstance.getSelections();
		if (!isPrimarySelectionEnabled() || !model || !selections?.length) return;
		const sortedSelections = selections.slice().sort((a, b) => {
			if (a.startLineNumber !== b.startLineNumber) return a.startLineNumber - b.startLineNumber;
			return a.startColumn - b.startColumn;
		});
		let totalLength = 0;
		for (const selection of sortedSelections) {
			if (selection.isEmpty()) return;
			totalLength += model.getValueLengthInRange(selection);
			if (totalLength > 65536) return;
		}
		setPrimarySelectionText(sortedSelections.map((selection) => model.getValueInRange(selection)).join(model.getEOL()));
	};
	const schedulePrimarySelectionBufferUpdate = () => {
		if (primarySelectionTimer !== null) window.clearTimeout(primarySelectionTimer);
		primarySelectionTimer = window.setTimeout(() => {
			primarySelectionTimer = null;
			updatePrimarySelectionBuffer();
		}, 100);
	};
	const copySelectionWithContext = async () => {
		const copiedText = getContextualCopyText();
		if (!copiedText) return false;
		await window.api.ui.writeClipboardText(copiedText);
		lastCopiedSelectionKey = getSelectionKey();
		copyHintNode.style.display = "none";
		copyHintWidgetPosition = null;
		editorInstance.layoutContentWidget(copyHintWidget);
		showCopyToast();
		return true;
	};
	const selectionListener = editorInstance.onDidChangeCursorSelection((event) => {
		if (event.source !== "restoreState") schedulePrimarySelectionBufferUpdate();
		if (getSelectionKey() !== lastCopiedSelectionKey) lastCopiedSelectionKey = null;
		refreshCopyHintAndPolling();
	});
	const scrollListener = editorInstance.onDidScrollChange(() => {
		refreshCopyHintAndPolling();
	});
	const focusListener = editorInstance.onDidFocusEditorText(() => {
		refreshCopyHintAndPolling();
	});
	const blurListener = editorInstance.onDidBlurEditorText(() => {
		stopCopyHintPolling();
		copyHintNode.style.display = "none";
		copyHintWidgetPosition = null;
		editorInstance.layoutContentWidget(copyHintWidget);
	});
	const editorDomNode = editorInstance.getContainerDomNode();
	const handleKeyDown = (event) => {
		if (!editorShortcutMatches("editor.copyContext", event)) return;
		event.preventDefault();
		event.stopPropagation();
		copySelectionWithContext();
	};
	editorDomNode.addEventListener("keydown", handleKeyDown, true);
	editorDomNode.addEventListener("mouseup", refreshCopyHintAndPolling, true);
	editorDomNode.addEventListener("keyup", refreshCopyHintAndPolling, true);
	editorInstance.onDidDispose(() => {
		selectionListener.dispose();
		scrollListener.dispose();
		focusListener.dispose();
		blurListener.dispose();
		if (copyToastTimeoutRef.current !== null) {
			window.clearTimeout(copyToastTimeoutRef.current);
			copyToastTimeoutRef.current = null;
			setCopyToast(null);
		}
		if (primarySelectionTimer !== null) {
			window.clearTimeout(primarySelectionTimer);
			primarySelectionTimer = null;
		}
		editorDomNode.removeEventListener("keydown", handleKeyDown, true);
		editorDomNode.removeEventListener("mouseup", refreshCopyHintAndPolling, true);
		editorDomNode.removeEventListener("keyup", refreshCopyHintAndPolling, true);
		stopCopyHintPolling();
		editorInstance.removeContentWidget(copyHintWidget);
	});
	if (editorInstance.hasTextFocus()) refreshCopyHintAndPolling();
	else updateCopyHint();
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function useContextualCopySetup() {
	const [copyToast, setCopyToast] = (0, import_react.useState)(null);
	const copyToastTimeoutRef = (0, import_react.useRef)(null);
	return {
		setupCopy: (0, import_react.useCallback)((editorInstance, _monaco, filePath, propsRef) => {
			setupContextualCopy({
				editorInstance,
				filePath,
				setCopyToast,
				propsRef,
				copyToastTimeoutRef
			});
		}, []),
		toastNode: copyToast ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "pointer-events-none fixed z-50 rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-sm",
			style: {
				left: copyToast.left,
				top: copyToast.top
			},
			children: translate("auto.components.editor.useContextualCopySetup.059bfb0d94", "Context copied")
		}) : null
	};
}
export { useContextualCopySetup as t };
