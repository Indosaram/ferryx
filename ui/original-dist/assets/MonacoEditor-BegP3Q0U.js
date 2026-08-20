import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as Plus } from "./plus-Db0kWPVa.js";
import { Uf as getRuntimeGitRemoteFileUrl, qu as findWorktreeById, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { i as DropdownMenuItem, m as DropdownMenuTrigger, r as DropdownMenuContent, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
import "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import { t as getConnectionId } from "./connection-context-BUPsamzR.js";
import "./localized-catalog-DubKHKUR.js";
import { i as resolveEditorFontFamily, n as computeEditorFontSize } from "./editor-font-zoom-2F4BKkDZ.js";
import "./agent-status-connection-ownership-D5nXPHBo.js";
import "./resolved-worktree-execution-host-BcjAq7e6.js";
import "./useShortcutLabel-C-KRYtlB.js";
import "./worktree-agent-rows-C1pW_DbE.js";
import "./worktree-title-derived-agent-rows-xbcpjeY8.js";
import "./AgentWorkingSpinner-BpnTWNKF.js";
import "./AgentStateDot-DFt63YGw.js";
import "./icons-jFAuHbv9.js";
import "./agent-catalog-CBF2CV5Q.js";
import "./useWorktreeAgentRows-DfUM0dP9.js";
import "./text-control-paste-PhBVbE2p.js";
import "./paste-payload-metadata-pr3nuODB.js";
import "./useDetectedAgents-KkNokXI_.js";
import { i as isLinuxUserAgent } from "./pane-helpers-9eOmrw__.js";
import "./primary-selection-BsidtYsF.js";
import { n as normalizeSelectedTextForFileSearch, r as registerFileSearchSelectedTextProvider } from "./file-search-selection-D5svLyqM.js";
import "./client-XKKXQWGM.js";
import "./editor.api2-DX_-Ye6K.js";
import "./workers-D5nLH-xK.js";
import "./monaco.contribution-BINL69Me.js";
import { a as getMarkdownDocLinkTarget } from "./markdown-doc-links-D1db8u5w.js";
import { n as Ft, t as handleMonacoLargeTextPaste } from "./monaco-setup-CKSA6ArO.js";
import "./editor.main-BGL6BKIn.js";
import { t as selectWorktreeDiffComments } from "./worktree-diff-comments-selector-B8AXyh9e.js";
import { n as getDiffCommentPopoverLeft, r as getDiffCommentPopoverTop, t as DiffCommentPopover } from "./DiffCommentPopover-Cap7I9o6.js";
import { i as isMarkdownComment } from "./diff-comment-compat-CWwyL2nL.js";
import "./DiffCommentCard-DPwCt0gV.js";
import { n as useDiffCommentDecorator, t as monacoFindOptions } from "./monaco-find-options-BHU4fGzO.js";
import "./ReviewNotesSendMenuContent-DnAssgZQ.js";
import "./launch-agent-in-new-tab-44JGNfKl.js";
import "./active-agent-note-send-CsxZ0dL2.js";
import "./NotesSendMenu-DLIuO9I1.js";
import { a as installMonacoEditorFindShortcut, n as installEditorAddReviewNoteShortcut, r as installEditorSaveShortcut } from "./editor-shortcuts-Cg6u73ie.js";
import "./comment-body-submit-state-BHQDrSxB.js";
import { a as setWithLRU, i as scrollTopCache, t as cursorPositionCache } from "./scroll-cache-B8ebRfkp.js";
import { t as useContextualCopySetup } from "./useContextualCopySetup-CJ5QxJCT.js";
import { i as hasGitConflictMarkers, t as buildGitConflictDecorations } from "./monaco-conflict-decorations-BW90iwoa.js";
import { n as formatMarkdownReviewNotes } from "./markdown-review-notes-CmRnxN_p.js";
import { n as getMarkdownDocCompletionContext, r as getMarkdownDocCompletionDocuments, t as matchesPendingEditorFocusRequest } from "./pending-editor-focus-request-BFeRYCxT.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function computeMonacoRevealRange(params) {
	const { line, column, matchLength, maxLine, lineMaxColumn } = params;
	const safeLine = Math.min(Math.max(1, line), Math.max(1, maxLine));
	const safeStartColumn = Math.min(Math.max(1, column), Math.max(1, lineMaxColumn));
	const safeLength = Math.max(1, matchLength);
	const safeEndColumn = Math.min(safeStartColumn + safeLength, Math.max(2, lineMaxColumn));
	return {
		startLineNumber: safeLine,
		startColumn: safeStartColumn,
		endLineNumber: safeLine,
		endColumn: Math.max(safeStartColumn + 1, safeEndColumn)
	};
}
function performReveal(ed, line, column, matchLength, clearTransientRevealHighlight, revealDecorationRef, revealHighlightTimerRef) {
	const model = ed.getModel();
	if (!model) {
		ed.focus();
		return;
	}
	const range = computeMonacoRevealRange({
		line,
		column,
		matchLength,
		maxLine: model.getLineCount(),
		lineMaxColumn: model.getLineMaxColumn(Math.min(Math.max(1, line), model.getLineCount()))
	});
	const shouldHighlight = matchLength > 0;
	ed.setPosition({
		lineNumber: range.startLineNumber,
		column: range.startColumn
	});
	if (shouldHighlight) {
		ed.setSelection(range);
		ed.revealRangeInCenter(range);
	} else {
		ed.setSelection({
			startLineNumber: range.startLineNumber,
			startColumn: range.startColumn,
			endLineNumber: range.startLineNumber,
			endColumn: range.startColumn
		});
		ed.revealPositionInCenter({
			lineNumber: range.startLineNumber,
			column: range.startColumn
		});
	}
	clearTransientRevealHighlight();
	if (shouldHighlight) {
		revealDecorationRef.current = ed.createDecorationsCollection([{
			range,
			options: {
				inlineClassName: "monaco-search-result-highlight",
				stickiness: 1
			}
		}]);
		revealHighlightTimerRef.current = setTimeout(() => {
			revealDecorationRef.current?.clear();
			revealDecorationRef.current = null;
			revealHighlightTimerRef.current = null;
		}, 1200);
	}
	ed.focus();
}
function normalizeToModelEol(content, model) {
	const eol = model.getEOL();
	if (eol === "\n" && !content.includes("\r")) return content;
	return content.replace(/\r\n|\r|\n/g, eol);
}
function applyModelEdit(editorInstance, model, edit, mode, withUndoStops) {
	if (mode === "read-only-live-tail") {
		model.applyEdits([edit]);
		return;
	}
	if (withUndoStops) editorInstance.pushUndoStop();
	model.pushEditOperations([], [edit], () => null);
	if (withUndoStops) editorInstance.pushUndoStop();
}
function replaceModelContent(editorInstance, model, currentContent, content, mode, withUndoStops) {
	if (currentContent === content) return;
	applyModelEdit(editorInstance, model, {
		range: model.getFullModelRange(),
		text: content
	}, mode, withUndoStops);
}
function syncContentOnMount(editorInstance, content, mode = "undoable") {
	const model = editorInstance.getModel();
	if (!model) return false;
	const currentContent = model.getValue();
	const normalizedContent = normalizeToModelEol(content, model);
	if (currentContent === normalizedContent) return false;
	replaceModelContent(editorInstance, model, currentContent, normalizedContent, mode, false);
	return true;
}
function syncContentUpdate(editorInstance, content, mode = "undoable") {
	const model = editorInstance.getModel();
	if (!model) return;
	const currentContent = model.getValue();
	const normalizedContent = normalizeToModelEol(content, model);
	if (currentContent.length === normalizedContent.length) {
		replaceModelContent(editorInstance, model, currentContent, normalizedContent, mode, true);
		return;
	}
	if (normalizedContent.length > currentContent.length && normalizedContent.startsWith(currentContent)) {
		const fullRange = model.getFullModelRange();
		applyModelEdit(editorInstance, model, {
			range: {
				startLineNumber: fullRange.endLineNumber,
				startColumn: fullRange.endColumn,
				endLineNumber: fullRange.endLineNumber,
				endColumn: fullRange.endColumn
			},
			text: normalizedContent.slice(currentContent.length)
		}, mode, true);
		return;
	}
	replaceModelContent(editorInstance, model, currentContent, normalizedContent, mode, true);
}
function getMonacoCodebaseSearchQuery(model, selection, position) {
	if (!model) return null;
	if (selection && !selection.isEmpty()) {
		const selectedQuery = normalizeSelectedTextForFileSearch(model.getValueInRange(selection));
		if (selectedQuery) return selectedQuery;
	}
	if (!position) return null;
	return normalizeSelectedTextForFileSearch(model.getWordAtPosition(position)?.word);
}
var programmaticContentSyncDepthByFilePath = /* @__PURE__ */ new Map();
function beginProgrammaticContentSync(filePath) {
	programmaticContentSyncDepthByFilePath.set(filePath, (programmaticContentSyncDepthByFilePath.get(filePath) ?? 0) + 1);
}
function endProgrammaticContentSync(filePath) {
	const depth = programmaticContentSyncDepthByFilePath.get(filePath) ?? 0;
	if (depth <= 1) {
		programmaticContentSyncDepthByFilePath.delete(filePath);
		return;
	}
	programmaticContentSyncDepthByFilePath.set(filePath, depth - 1);
}
function isProgrammaticContentSyncInFlight(filePath) {
	return (programmaticContentSyncDepthByFilePath.get(filePath) ?? 0) > 0;
}
function shouldIgnoreMonacoContentChange(args) {
	const { filePath, isApplyingProgrammaticContent } = args;
	return isApplyingProgrammaticContent || isProgrammaticContentSyncInFlight(filePath);
}
var provider = null;
var providerMonaco = null;
var documentsByModel = /* @__PURE__ */ new Map();
function ensureMarkdownDocCompletionProvider(monaco) {
	if (provider && providerMonaco === monaco) return;
	if (provider) {
		provider.dispose();
		documentsByModel.clear();
	}
	providerMonaco = monaco;
	provider = monaco.languages.registerCompletionItemProvider("markdown", {
		triggerCharacters: ["["],
		provideCompletionItems(model, position) {
			const line = model.getLineContent(position.lineNumber);
			const context = getMarkdownDocCompletionContext(line.slice(0, position.column - 1));
			if (!context) return { suggestions: [] };
			const documents = documentsByModel.get(model.uri.toString()) ?? [];
			const suffix = line.slice(position.column - 1);
			const range = {
				startLineNumber: position.lineNumber,
				startColumn: position.column - context.partial.length,
				endLineNumber: position.lineNumber,
				endColumn: position.column
			};
			return { suggestions: getMarkdownDocCompletionDocuments(documents, context.partial).map((document) => ({
				label: document.name,
				kind: monaco.languages.CompletionItemKind.File,
				detail: document.relativePath,
				insertText: suffix.startsWith("]]") ? document.name : `${document.name}]]`,
				range
			})) };
		}
	});
}
function setMarkdownDocCompletionDocuments(modelKey, documents) {
	documentsByModel.set(modelKey, documents);
}
function clearMarkdownDocCompletionDocuments(modelKey) {
	documentsByModel.delete(modelKey);
}
function formatPathLineReference(filePath, line) {
	return `${filePath}:${line}`;
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function MonacoGutterContextMenu({ open, onOpenChange, point, line, filePath, relativePath }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, {
		open,
		onOpenChange,
		modal: false,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				"aria-hidden": true,
				tabIndex: -1,
				className: "pointer-events-none fixed size-px opacity-0",
				style: {
					left: point.x,
					top: point.y
				}
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuContent, {
			sideOffset: 0,
			align: "start",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					onSelect: () => window.api.ui.writeClipboardText(formatPathLineReference(filePath, line)),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { className: "w-3.5 h-3.5 mr-1.5" }), translate("auto.components.editor.MonacoGutterContextMenu.4eaa991bde", "Copy Path to Line")]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					onSelect: () => window.api.ui.writeClipboardText(formatPathLineReference(relativePath, line)),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { className: "w-3.5 h-3.5 mr-1.5" }), translate("auto.components.editor.MonacoGutterContextMenu.2e0b1cdc05", "Copy Rel. Path to Line")]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					onSelect: async () => {
						const state = useAppStore.getState();
						const activeFile = state.openFiles.find((f) => f.filePath === filePath);
						if (!activeFile) return;
						const worktree = findWorktreeById(state.worktreesByRepo, activeFile.worktreeId);
						if (!worktree) return;
						const connectionId = getConnectionId(activeFile?.worktreeId ?? null) ?? void 0;
						const url = await getRuntimeGitRemoteFileUrl({
							settings: state.settings,
							worktreeId: activeFile.worktreeId,
							worktreePath: worktree.path,
							connectionId
						}, {
							relativePath,
							line
						});
						if (url) window.api.ui.writeClipboardText(url);
					},
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "w-3.5 h-3.5 mr-1.5" }), translate("auto.components.editor.MonacoGutterContextMenu.7b57b1b468", "Copy Remote URL")]
				})
			]
		})]
	});
}
function getInlineCodeSpans(line) {
	const spans = [];
	let start = -1;
	for (let index = 0; index < line.length; index += 1) {
		if (line[index] !== "`" || index > 0 && line[index - 1] === "\\") continue;
		if (start === -1) start = index;
		else {
			spans.push({
				start,
				end: index + 1
			});
			start = -1;
		}
	}
	return spans;
}
function isInsideSpan(index, spans) {
	return spans.some((span) => index >= span.start && index < span.end);
}
function getMarkdownDocLinkDecorationRanges(content) {
	const ranges = [];
	let insideFence = false;
	forEachMarkdownLine(content, (line, lineNumber) => {
		if (/^\s*(```|~~~)/.test(line)) {
			insideFence = !insideFence;
			return;
		}
		if (insideFence) return;
		const inlineCodeSpans = getInlineCodeSpans(line);
		let searchFrom = 0;
		while (searchFrom < line.length) {
			const start = line.indexOf("[[", searchFrom);
			if (start === -1) break;
			const end = line.indexOf("]]", start + 2);
			if (end === -1) break;
			if (!isInsideSpan(start, inlineCodeSpans)) {
				if (getMarkdownDocLinkTarget(line.slice(start + 2, end))) ranges.push({
					startLineNumber: lineNumber,
					startColumn: start + 1,
					endLineNumber: lineNumber,
					endColumn: end + 3
				});
			}
			searchFrom = end + 2;
		}
	});
	return ranges;
}
function forEachMarkdownLine(content, visit) {
	let lineStart = 0;
	let lineNumber = 1;
	for (let index = 0; index <= content.length; index += 1) {
		if (index < content.length && content.charCodeAt(index) !== 10) continue;
		const lineEnd = index > lineStart && content.charCodeAt(index - 1) === 13 ? index - 1 : index;
		visit(content.slice(lineStart, lineEnd), lineNumber);
		lineStart = index + 1;
		lineNumber += 1;
	}
}
function createMarkdownDocLinkDecorationController(editorInstance, getLanguage) {
	const collection = editorInstance.createDecorationsCollection();
	let refreshTimer = null;
	const cancelPendingRefresh = () => {
		if (refreshTimer === null) return;
		clearTimeout(refreshTimer);
		refreshTimer = null;
	};
	const refreshNow = () => {
		cancelPendingRefresh();
		const model = editorInstance.getModel();
		if (!model || getLanguage() !== "markdown") {
			collection.clear();
			return;
		}
		collection.set(getMarkdownDocLinkDecorationRanges(model.getValue()).map((range) => ({
			range,
			options: {
				inlineClassName: "monaco-markdown-doc-link",
				stickiness: 1
			}
		})));
	};
	const refresh = () => {
		if (getLanguage() !== "markdown") {
			refreshNow();
			return;
		}
		cancelPendingRefresh();
		refreshTimer = setTimeout(refreshNow, 120);
	};
	const listener = editorInstance.onDidChangeModelContent(refresh);
	refreshNow();
	return {
		refresh,
		dispose: () => {
			cancelPendingRefresh();
			listener.dispose();
			collection.clear();
		}
	};
}
var FALLBACK_LINE_HEIGHT_PX = 19;
function isEmptySelection(selection) {
	return selection.startLineNumber === selection.endLineNumber && selection.startColumn === selection.endColumn;
}
function getSelectionTextEndLine(selection) {
	if (selection.endColumn === 1 && selection.endLineNumber > selection.startLineNumber) return selection.endLineNumber - 1;
	return selection.endLineNumber;
}
function getMonacoMarkdownSelectionAnnotationTarget(editorInstance, selection, left) {
	if (!selection || isEmptySelection(selection)) return null;
	const model = editorInstance.getModel();
	if (!model) return null;
	const selectedText = model.getValueInRange(selection).trim();
	if (!selectedText) return null;
	const textEndLine = getSelectionTextEndLine(selection);
	const startLine = Math.min(selection.startLineNumber, textEndLine);
	const lineNumber = Math.max(selection.startLineNumber, textEndLine);
	if (startLine < 1 || lineNumber > model.getLineCount()) return null;
	const top = editorInstance.getTopForLineNumber(lineNumber) - editorInstance.getScrollTop() + FALLBACK_LINE_HEIGHT_PX;
	return {
		lineNumber,
		startLine: startLine === lineNumber ? void 0 : startLine,
		selectedText,
		top,
		left
	};
}
function buildFileEditorWordWrapOptions(editorWordWrap) {
	return { wordWrap: editorWordWrap === false ? "off" : "on" };
}
const MONACO_AUTO_HEIGHT_LINE_SCAN_CODE_UNITS = 64 * 1024;
const MONACO_AUTO_HEIGHT_MAX_LINES = 2e3;
var MONACO_AUTO_HEIGHT_EXTRA_PX = 18;
var MONACO_AUTO_HEIGHT_MIN_PX = 80;
function getMonacoAutoHeightForContent(content, lineHeight) {
	return clampMonacoAutoHeight(countMonacoAutoHeightLines(content) * lineHeight + MONACO_AUTO_HEIGHT_EXTRA_PX, lineHeight);
}
function clampMonacoAutoHeight(height, lineHeight) {
	return Math.max(MONACO_AUTO_HEIGHT_MIN_PX, Math.min(Math.ceil(height), getMonacoAutoHeightMaxPx(lineHeight)));
}
function isMonacoAutoHeightCapped(height, lineHeight) {
	return height !== null && height >= getMonacoAutoHeightMaxPx(lineHeight);
}
function countMonacoAutoHeightLines(content) {
	if (content.length === 0) return 1;
	const scanLength = Math.min(content.length, MONACO_AUTO_HEIGHT_LINE_SCAN_CODE_UNITS);
	let lineCount = 1;
	for (let index = 0; index < scanLength; index += 1) {
		if (content.charCodeAt(index) !== 10) continue;
		lineCount += 1;
		if (lineCount >= 2e3) return MONACO_AUTO_HEIGHT_MAX_LINES;
	}
	return lineCount;
}
function getMonacoAutoHeightMaxPx(lineHeight) {
	return MONACO_AUTO_HEIGHT_MAX_LINES * lineHeight + MONACO_AUTO_HEIGHT_EXTRA_PX;
}
function installMonacoE2EProbe(editorInstance, filePath) {
	return () => {};
}
function MonacoEditor({ fileId, filePath, viewStateKey, viewStateId, relativePath, content, language, onContentChange, onSave, revealLine, revealColumn, revealMatchLength, markdownDocuments, worktreeId, markdownAnnotationsEnabled = false, conflictDecorationsEnabled = false, readOnly = false, liveTail = false, autoHeight = false }) {
	const editorRef = (0, import_react.useRef)(null);
	const editorContainerRef = (0, import_react.useRef)(null);
	const [mountedEditor, setMountedEditor] = (0, import_react.useState)(null);
	const [autoHeightContentHeight, setAutoHeightContentHeight] = (0, import_react.useState)(null);
	const modelKeyRef = (0, import_react.useRef)(null);
	const languageRef = (0, import_react.useRef)(language);
	languageRef.current = language;
	const markdownDocLinkDecorationsRef = (0, import_react.useRef)(null);
	const conflictDecorationsRef = (0, import_react.useRef)(null);
	const revealDecorationRef = (0, import_react.useRef)(null);
	const revealHighlightTimerRef = (0, import_react.useRef)(null);
	const revealRafRef = (0, import_react.useRef)(null);
	const revealInnerRafRef = (0, import_react.useRef)(null);
	const unregisterFileSearchSelectionRef = (0, import_react.useRef)(null);
	const { setupCopy, toastNode } = useContextualCopySetup();
	const scrollThrottleTimerRef = (0, import_react.useRef)(null);
	const propsRef = (0, import_react.useRef)({
		relativePath,
		language,
		onSave,
		onContentChange
	});
	propsRef.current = {
		relativePath,
		language,
		onSave,
		onContentChange
	};
	const readOnlyRef = (0, import_react.useRef)(readOnly);
	readOnlyRef.current = readOnly;
	const contentSyncModeRef = (0, import_react.useRef)("undoable");
	contentSyncModeRef.current = readOnly && liveTail ? "read-only-live-tail" : "undoable";
	const settings = useAppStore((s) => s.settings);
	const editorFontZoomLevel = useAppStore((s) => s.editorFontZoomLevel);
	const setPendingEditorReveal = useAppStore((s) => s.setPendingEditorReveal);
	const setEditorCursorLine = useAppStore((s) => s.setEditorCursorLine);
	const addDiffComment = useAppStore((s) => s.addDiffComment);
	const deleteDiffComment = useAppStore((s) => s.deleteDiffComment);
	const updateDiffComment = useAppStore((s) => s.updateDiffComment);
	const scrollToDiffCommentId = useAppStore((s) => s.scrollToDiffCommentId);
	const setScrollToDiffCommentId = useAppStore((s) => s.setScrollToDiffCommentId);
	const allDiffComments = useAppStore((s) => selectWorktreeDiffComments(s, worktreeId));
	const editorFontSize = computeEditorFontSize(settings?.terminalFontSize ?? 13, editorFontZoomLevel);
	const editorFontFamily = resolveEditorFontFamily(settings);
	const editorWordWrap = settings?.editorWordWrap;
	const estimatedAutoHeight = (0, import_react.useMemo)(() => {
		if (!autoHeight) return null;
		return getMonacoAutoHeightForContent(content, Math.ceil(editorFontSize * 1.45));
	}, [
		autoHeight,
		content,
		editorFontSize
	]);
	const renderedEditorHeight = autoHeight ? autoHeightContentHeight ?? estimatedAutoHeight ?? 80 : null;
	const autoHeightLineHeight = Math.ceil(editorFontSize * 1.45);
	const autoHeightUsesInternalScroll = autoHeight && isMonacoAutoHeightCapped(renderedEditorHeight, autoHeightLineHeight);
	const contentRef = (0, import_react.useRef)(content);
	contentRef.current = content;
	const lastSyncedContentRef = (0, import_react.useRef)(content);
	const markdownComments = (0, import_react.useMemo)(() => (allDiffComments ?? []).filter((c) => c.filePath === relativePath && isMarkdownComment(c)), [allDiffComments, relativePath]);
	const [gutterMenuOpen, setGutterMenuOpen] = (0, import_react.useState)(false);
	const [gutterMenuPoint, setGutterMenuPoint] = (0, import_react.useState)({
		x: 0,
		y: 0
	});
	const [gutterMenuLine, setGutterMenuLine] = (0, import_react.useState)(1);
	const [commentPopover, setCommentPopover] = (0, import_react.useState)(null);
	const [selectionAnnotationTarget, setSelectionAnnotationTarget] = (0, import_react.useState)(null);
	const commentPopoverRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		commentPopoverRef.current = commentPopover;
	}, [commentPopover]);
	const isDark = settings?.theme === "dark" || settings?.theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches;
	const updateMarkdownCompletionDocuments = (0, import_react.useCallback)(() => {
		const modelKey = editorRef.current?.getModel()?.uri.toString() ?? null;
		if (modelKeyRef.current && modelKeyRef.current !== modelKey) clearMarkdownDocCompletionDocuments(modelKeyRef.current);
		modelKeyRef.current = modelKey;
		if (!modelKey) return;
		if (language === "markdown" && markdownDocuments) setMarkdownDocCompletionDocuments(modelKey, markdownDocuments);
		else clearMarkdownDocCompletionDocuments(modelKey);
	}, [language, markdownDocuments]);
	const shouldShowMarkdownAnnotations = markdownAnnotationsEnabled && language === "markdown" && Boolean(worktreeId);
	const shouldShowMarkdownAnnotationsRef = (0, import_react.useRef)(shouldShowMarkdownAnnotations);
	(0, import_react.useEffect)(() => {
		shouldShowMarkdownAnnotationsRef.current = shouldShowMarkdownAnnotations;
	}, [shouldShowMarkdownAnnotations]);
	const pendingScrollForThisEditor = (0, import_react.useMemo)(() => {
		if (!shouldShowMarkdownAnnotations || !scrollToDiffCommentId) return null;
		return markdownComments.some((c) => c.id === scrollToDiffCommentId) ? scrollToDiffCommentId : null;
	}, [
		markdownComments,
		scrollToDiffCommentId,
		shouldShowMarkdownAnnotations
	]);
	const formatMarkdownCommentPrompt = (0, import_react.useCallback)((comment) => formatMarkdownReviewNotes([comment], content), [content]);
	useDiffCommentDecorator({
		editor: shouldShowMarkdownAnnotations ? mountedEditor : null,
		filePath: relativePath,
		worktreeId: worktreeId ?? "",
		comments: shouldShowMarkdownAnnotations ? markdownComments : [],
		onAddCommentClick: ({ lineNumber, startLine, top }) => {
			setSelectionAnnotationTarget(null);
			setCommentPopover({
				lineNumber,
				startLine,
				top,
				left: mountedEditor ? getDiffCommentPopoverLeft(mountedEditor, editorContainerRef.current) ?? void 0 : void 0
			});
		},
		onDeleteComment: (id) => {
			if (worktreeId) deleteDiffComment(worktreeId, id);
		},
		onUpdateComment: worktreeId ? (id, body) => updateDiffComment(worktreeId, id, body) : void 0,
		formatCommentPrompt: formatMarkdownCommentPrompt,
		pendingScrollCommentId: pendingScrollForThisEditor,
		onPendingScrollConsumed: () => setScrollToDiffCommentId(null)
	});
	const clearTransientRevealHighlight = (0, import_react.useCallback)(() => {
		if (revealHighlightTimerRef.current !== null) {
			clearTimeout(revealHighlightTimerRef.current);
			revealHighlightTimerRef.current = null;
		}
		revealDecorationRef.current?.clear();
		revealDecorationRef.current = null;
	}, []);
	const cancelScheduledReveal = (0, import_react.useCallback)(() => {
		if (revealRafRef.current !== null) {
			cancelAnimationFrame(revealRafRef.current);
			revealRafRef.current = null;
		}
		if (revealInnerRafRef.current !== null) {
			cancelAnimationFrame(revealInnerRafRef.current);
			revealInnerRafRef.current = null;
		}
	}, []);
	const queueReveal = (0, import_react.useCallback)((editorInstance, line, column, matchLength, onApplied) => {
		cancelScheduledReveal();
		let waitFrames = 0;
		const schedule = () => {
			revealRafRef.current = requestAnimationFrame(() => {
				revealInnerRafRef.current = requestAnimationFrame(() => {
					revealRafRef.current = null;
					revealInnerRafRef.current = null;
					const modelLineCount = editorInstance.getModel()?.getLineCount() ?? 0;
					if (line > 1 && modelLineCount < line && waitFrames < 120) {
						waitFrames += 2;
						schedule();
						return;
					}
					performReveal(editorInstance, line, column, matchLength, clearTransientRevealHighlight, revealDecorationRef, revealHighlightTimerRef);
					onApplied?.();
				});
			});
		};
		schedule();
	}, [cancelScheduledReveal, clearTransientRevealHighlight]);
	const isApplyingProgrammaticContentRef = (0, import_react.useRef)(false);
	const isApplyingLargePasteRef = (0, import_react.useRef)(false);
	const handleMount = (0, import_react.useCallback)((editorInstance, monaco) => {
		editorRef.current = editorInstance;
		setMountedEditor(editorInstance);
		const uninstallE2EProbe = installMonacoE2EProbe(editorInstance, filePath);
		let autoHeightSub = null;
		let autoHeightFrame = null;
		const updateAutoHeight = () => {
			if (!autoHeight) return;
			if (autoHeightFrame !== null) return;
			autoHeightFrame = window.requestAnimationFrame(() => {
				autoHeightFrame = null;
				setAutoHeightContentHeight(clampMonacoAutoHeight(Math.ceil(editorInstance.getContentHeight()) + 1, autoHeightLineHeight));
			});
		};
		if (autoHeight) {
			updateAutoHeight();
			autoHeightSub = editorInstance.onDidContentSizeChange(updateAutoHeight);
		}
		markdownDocLinkDecorationsRef.current = createMarkdownDocLinkDecorationController(editorInstance, () => languageRef.current);
		ensureMarkdownDocCompletionProvider(monaco);
		updateMarkdownCompletionDocuments();
		beginProgrammaticContentSync(filePath);
		isApplyingProgrammaticContentRef.current = true;
		try {
			if (syncContentOnMount(editorInstance, contentRef.current, contentSyncModeRef.current)) lastSyncedContentRef.current = contentRef.current;
		} finally {
			isApplyingProgrammaticContentRef.current = false;
			endProgrammaticContentSync(filePath);
		}
		setupCopy(editorInstance, monaco, filePath, propsRef);
		unregisterFileSearchSelectionRef.current?.();
		unregisterFileSearchSelectionRef.current = registerFileSearchSelectedTextProvider(() => {
			if (!editorInstance.hasTextFocus()) return null;
			const model = editorInstance.getModel();
			const selection = editorInstance.getSelection();
			if (!model || !selection || selection.isEmpty()) return null;
			return model.getValueInRange(selection);
		});
		const editorDomNode = editorInstance.getContainerDomNode();
		const cleanupSaveShortcut = installEditorSaveShortcut(editorDomNode, () => {
			const value = editorInstance.getValue();
			propsRef.current.onSave(value);
		});
		const cleanupFindShortcut = installMonacoEditorFindShortcut(editorInstance);
		const cleanupAddReviewNoteShortcut = installEditorAddReviewNoteShortcut(editorDomNode, () => {
			if (commentPopoverRef.current) return true;
			if (!shouldShowMarkdownAnnotationsRef.current) return false;
			const target = getMonacoMarkdownSelectionAnnotationTarget(editorInstance, editorInstance.getSelection(), getDiffCommentPopoverLeft(editorInstance, editorContainerRef.current) ?? void 0);
			if (!target) return false;
			commentPopoverRef.current = target;
			setCommentPopover(target);
			setSelectionAnnotationTarget(null);
			return true;
		});
		const searchInFilesAction = editorInstance.addAction({
			id: "orca.searchInFiles",
			label: translate("auto.components.editor.MonacoEditor.fd68ae03b3", "Search in Files"),
			contextMenuGroupId: "navigation",
			contextMenuOrder: 2,
			run: () => {
				if (!worktreeId) return;
				const query = getMonacoCodebaseSearchQuery(editorInstance.getModel(), editorInstance.getSelection(), editorInstance.getPosition());
				if (!query) return;
				useAppStore.getState().showRightSidebarSearch({ query });
			}
		});
		const onLargeTextPaste = (event) => {
			handleMonacoLargeTextPaste(editorInstance, event, {
				readOnly: readOnlyRef.current,
				onPasteStart: () => {
					isApplyingLargePasteRef.current = true;
				},
				onPasteResult: (result) => {
					isApplyingLargePasteRef.current = false;
					if (result.status === "pasted" || result.status === "cancelled") {
						const value = editorInstance.getValue();
						lastSyncedContentRef.current = value;
						propsRef.current.onContentChange(value);
					}
					if (result.status === "rejected" && result.reason === "too-large") toast.error(translate("auto.components.editor.MonacoEditor.largePasteTooLarge", "Paste is too large."));
				}
			});
		};
		editorDomNode.addEventListener("paste", onLargeTextPaste, { capture: true });
		const pos = editorInstance.getPosition();
		if (pos) setEditorCursorLine(filePath, pos.lineNumber);
		const cursorPositionSub = editorInstance.onDidChangeCursorPosition((e) => {
			setEditorCursorLine(filePath, e.position.lineNumber);
			setWithLRU(cursorPositionCache, viewStateKey, {
				lineNumber: e.position.lineNumber,
				column: e.position.column
			});
		});
		const scrollStateSub = editorInstance.onDidScrollChange((e) => {
			if (scrollThrottleTimerRef.current !== null) clearTimeout(scrollThrottleTimerRef.current);
			scrollThrottleTimerRef.current = setTimeout(() => {
				setWithLRU(scrollTopCache, viewStateKey, e.scrollTop);
				scrollThrottleTimerRef.current = null;
			}, 150);
		});
		const gutterMouseDownSub = editorInstance.onMouseDown((e) => {
			if (e.event.rightButton && e.target.type === monaco.editor.MouseTargetType.GUTTER_LINE_NUMBERS) {
				e.event.preventDefault();
				e.event.stopPropagation();
				const line = e.target.position?.lineNumber ?? 1;
				editorInstance.setPosition({
					lineNumber: line,
					column: 1
				});
				setGutterMenuLine(line);
				setGutterMenuPoint({
					x: e.event.posx,
					y: e.event.posy
				});
				setGutterMenuOpen(true);
			}
		});
		editorInstance.onDidDispose(() => {
			cursorPositionSub.dispose();
			scrollStateSub.dispose();
			gutterMouseDownSub.dispose();
			cleanupSaveShortcut();
			cleanupFindShortcut();
			cleanupAddReviewNoteShortcut();
			editorDomNode.removeEventListener("paste", onLargeTextPaste, { capture: true });
			searchInFilesAction.dispose();
			autoHeightSub?.dispose();
			if (autoHeightFrame !== null) {
				window.cancelAnimationFrame(autoHeightFrame);
				autoHeightFrame = null;
			}
			conflictDecorationsRef.current?.clear();
			conflictDecorationsRef.current = null;
			uninstallE2EProbe();
			editorRef.current = null;
			setMountedEditor(null);
			setCommentPopover(null);
		});
		const reveal = useAppStore.getState().pendingEditorReveal;
		const revealMatchesEditor = reveal?.fileId ? reveal.fileId === fileId : reveal?.filePath === filePath;
		if (reveal && revealMatchesEditor) queueReveal(editorInstance, reveal.line, reveal.column, reveal.matchLength, () => {
			useAppStore.getState().setPendingEditorReveal(null);
		});
		else {
			const savedCursor = cursorPositionCache.get(viewStateKey);
			const savedScrollTop = scrollTopCache.get(viewStateKey);
			if (savedScrollTop !== void 0 || savedCursor) requestAnimationFrame(() => {
				if (savedCursor) editorInstance.setPosition(savedCursor);
				if (savedScrollTop !== void 0) editorInstance.setScrollTop(savedScrollTop);
				editorInstance.focus();
			});
			else editorInstance.focus();
		}
		const focusRequest = useAppStore.getState().pendingEditorFocusRequest;
		if (focusRequest && matchesPendingEditorFocusRequest(focusRequest, {
			fileId,
			worktreeId,
			viewStateId
		})) useAppStore.getState().consumeEditorFocusRequest(focusRequest.token);
	}, [
		queueReveal,
		setupCopy,
		fileId,
		filePath,
		setEditorCursorLine,
		updateMarkdownCompletionDocuments,
		viewStateKey,
		viewStateId,
		autoHeight,
		autoHeightLineHeight,
		worktreeId
	]);
	(0, import_react.useEffect)(() => {
		if (!mountedEditor || !commentPopover) return;
		const update = () => {
			const top = getDiffCommentPopoverTop(mountedEditor, commentPopover.lineNumber, void 0);
			const left = getDiffCommentPopoverLeft(mountedEditor, editorContainerRef.current);
			setCommentPopover((prev) => prev ? {
				...prev,
				top: top ?? prev.top,
				left: left == null ? prev.left : left
			} : prev);
		};
		const scrollSub = mountedEditor.onDidScrollChange(update);
		const contentSub = mountedEditor.onDidContentSizeChange(update);
		const layoutSub = mountedEditor.onDidLayoutChange(update);
		return () => {
			scrollSub.dispose();
			contentSub.dispose();
			layoutSub.dispose();
		};
	}, [mountedEditor, commentPopover?.lineNumber]);
	(0, import_react.useEffect)(() => {
		if (!mountedEditor || !shouldShowMarkdownAnnotations || commentPopover) {
			setSelectionAnnotationTarget(null);
			return;
		}
		const update = () => {
			const left = getDiffCommentPopoverLeft(mountedEditor, editorContainerRef.current);
			setSelectionAnnotationTarget(getMonacoMarkdownSelectionAnnotationTarget(mountedEditor, mountedEditor.getSelection(), left ?? void 0));
		};
		update();
		const selectionSub = mountedEditor.onDidChangeCursorSelection(update);
		const scrollSub = mountedEditor.onDidScrollChange(update);
		const layoutSub = mountedEditor.onDidLayoutChange(update);
		return () => {
			selectionSub.dispose();
			scrollSub.dispose();
			layoutSub.dispose();
		};
	}, [
		commentPopover,
		mountedEditor,
		shouldShowMarkdownAnnotations
	]);
	const handleSubmitMarkdownComment = async (body) => {
		if (!commentPopover || !worktreeId) return;
		if (await addDiffComment({
			worktreeId,
			filePath: relativePath,
			source: "markdown",
			startLine: commentPopover.startLine,
			lineNumber: commentPopover.lineNumber,
			selectedText: commentPopover.selectedText,
			body,
			side: "modified"
		})) setCommentPopover(null);
		else console.error("Failed to add markdown comment — draft preserved");
	};
	const handleChange = (0, import_react.useCallback)((value) => {
		if (value !== void 0) {
			if (isApplyingLargePasteRef.current) {
				lastSyncedContentRef.current = value;
				return;
			}
			if (shouldIgnoreMonacoContentChange({
				filePath,
				isApplyingProgrammaticContent: isApplyingProgrammaticContentRef.current
			})) return;
			lastSyncedContentRef.current = value;
			onContentChange(value);
		}
	}, [filePath, onContentChange]);
	(0, import_react.useLayoutEffect)(() => {
		const ed = editorRef.current;
		if (!ed || lastSyncedContentRef.current === content) return;
		beginProgrammaticContentSync(filePath);
		isApplyingProgrammaticContentRef.current = true;
		try {
			syncContentUpdate(ed, content, contentSyncModeRef.current);
			lastSyncedContentRef.current = content;
		} finally {
			isApplyingProgrammaticContentRef.current = false;
			endProgrammaticContentSync(filePath);
		}
	}, [content, filePath]);
	(0, import_react.useLayoutEffect)(() => {
		return () => {
			if (scrollThrottleTimerRef.current !== null) {
				clearTimeout(scrollThrottleTimerRef.current);
				scrollThrottleTimerRef.current = null;
			}
			const ed = editorRef.current;
			if (ed) {
				setWithLRU(scrollTopCache, viewStateKey, ed.getScrollTop());
				const pos = ed.getPosition();
				if (pos) setWithLRU(cursorPositionCache, viewStateKey, {
					lineNumber: pos.lineNumber,
					column: pos.column
				});
			}
			cancelScheduledReveal();
			clearTransientRevealHighlight();
			unregisterFileSearchSelectionRef.current?.();
			unregisterFileSearchSelectionRef.current = null;
		};
	}, [
		cancelScheduledReveal,
		clearTransientRevealHighlight,
		viewStateKey
	]);
	(0, import_react.useEffect)(() => {
		if (!editorRef.current) return;
		editorRef.current.updateOptions({
			fontSize: editorFontSize,
			fontFamily: editorFontFamily,
			...buildFileEditorWordWrapOptions(editorWordWrap)
		});
	}, [
		editorFontFamily,
		editorFontSize,
		editorWordWrap
	]);
	(0, import_react.useEffect)(() => {
		markdownDocLinkDecorationsRef.current?.refresh();
	}, [content, language]);
	(0, import_react.useEffect)(() => {
		const ed = mountedEditor;
		if (!ed) return;
		if (!conflictDecorationsEnabled || !hasGitConflictMarkers(content)) {
			conflictDecorationsRef.current?.clear();
			return;
		}
		const decorations = buildGitConflictDecorations(content);
		if (!conflictDecorationsRef.current) {
			conflictDecorationsRef.current = ed.createDecorationsCollection(decorations);
			return;
		}
		conflictDecorationsRef.current.set(decorations);
	}, [
		conflictDecorationsEnabled,
		content,
		mountedEditor
	]);
	(0, import_react.useEffect)(() => {
		updateMarkdownCompletionDocuments();
	}, [updateMarkdownCompletionDocuments]);
	(0, import_react.useEffect)(() => {
		return () => {
			if (modelKeyRef.current) clearMarkdownDocCompletionDocuments(modelKeyRef.current);
			markdownDocLinkDecorationsRef.current?.dispose();
			markdownDocLinkDecorationsRef.current = null;
			conflictDecorationsRef.current?.clear();
			conflictDecorationsRef.current = null;
		};
	}, []);
	(0, import_react.useEffect)(() => {
		if (!revealLine || !editorRef.current) return;
		queueReveal(editorRef.current, revealLine, revealColumn ?? 1, revealMatchLength ?? 0, () => {
			setPendingEditorReveal(null);
		});
	}, [
		queueReveal,
		revealLine,
		revealColumn,
		revealMatchLength,
		setPendingEditorReveal
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: editorContainerRef,
		className: autoHeight ? "relative" : "relative h-full",
		style: renderedEditorHeight === null ? void 0 : { height: renderedEditorHeight },
		children: [
			commentPopover && shouldShowMarkdownAnnotations && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DiffCommentPopover, {
				lineNumber: commentPopover.lineNumber,
				startLine: commentPopover.startLine,
				top: commentPopover.top,
				left: commentPopover.left,
				onCancel: () => setCommentPopover(null),
				onSubmit: handleSubmitMarkdownComment
			}, commentPopover.lineNumber),
			selectionAnnotationTarget && shouldShowMarkdownAnnotations && !commentPopover ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				className: "orca-diff-comment-add-btn",
				style: {
					display: "flex",
					top: Math.max(4, selectionAnnotationTarget.top - 22),
					left: selectionAnnotationTarget.left ?? 4
				},
				title: translate("auto.components.editor.MonacoEditor.68cb83f4a7", "Add note on selected text"),
				"aria-label": translate("auto.components.editor.MonacoEditor.68cb83f4a7", "Add note on selected text"),
				onMouseDown: (event) => {
					event.preventDefault();
					event.stopPropagation();
				},
				onClick: (event) => {
					event.preventDefault();
					event.stopPropagation();
					setCommentPopover(selectionAnnotationTarget);
					setSelectionAnnotationTarget(null);
				},
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "size-3" })
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Ft, {
				height: renderedEditorHeight === null ? "100%" : `${renderedEditorHeight}px`,
				language,
				defaultValue: content,
				theme: isDark ? "vs-dark" : "vs",
				onChange: handleChange,
				onMount: handleMount,
				options: {
					minimap: { enabled: settings?.editorMinimapEnabled ?? false },
					scrollBeyondLastLine: false,
					...buildFileEditorWordWrapOptions(editorWordWrap),
					fontSize: editorFontSize,
					fontFamily: editorFontFamily,
					lineNumbers: "on",
					renderLineHighlight: "line",
					automaticLayout: true,
					tabSize: 2,
					readOnly,
					scrollbar: autoHeight ? {
						vertical: autoHeightUsesInternalScroll ? "auto" : "hidden",
						handleMouseWheel: autoHeightUsesInternalScroll
					} : void 0,
					smoothScrolling: true,
					cursorSmoothCaretAnimation: "off",
					padding: { top: 0 },
					find: monacoFindOptions,
					selectionClipboard: settings?.primarySelectionMiddleClickPaste ?? isLinuxUserAgent()
				},
				path: filePath,
				saveViewState: false,
				keepCurrentModel: true
			}),
			toastNode,
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MonacoGutterContextMenu, {
				open: gutterMenuOpen,
				onOpenChange: setGutterMenuOpen,
				point: gutterMenuPoint,
				line: gutterMenuLine,
				filePath,
				relativePath
			})
		]
	});
}
export { MonacoEditor as default };
