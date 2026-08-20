import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./dropdown-menu-Dth6LPK-.js";
import "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import "./connection-context-BUPsamzR.js";
import "./localized-catalog-DubKHKUR.js";
import { i as resolveEditorFontFamily, t as computeDiffEditorFontSize } from "./editor-font-zoom-2F4BKkDZ.js";
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
import "./primary-selection-BsidtYsF.js";
import "./client-XKKXQWGM.js";
import { f as Uri, p as editor } from "./editor.api2-DX_-Ye6K.js";
import "./workers-D5nLH-xK.js";
import "./monaco.contribution-BINL69Me.js";
import { r as we } from "./monaco-setup-CKSA6ArO.js";
import { t as editor_main_exports } from "./editor.main-BGL6BKIn.js";
import { t as selectWorktreeDiffComments } from "./worktree-diff-comments-selector-B8AXyh9e.js";
import { n as getDiffCommentPopoverLeft, r as getDiffCommentPopoverTop, t as DiffCommentPopover } from "./DiffCommentPopover-Cap7I9o6.js";
import { r as isDiffComment } from "./diff-comment-compat-CWwyL2nL.js";
import "./DiffCommentCard-DPwCt0gV.js";
import { n as useDiffCommentDecorator, t as monacoFindOptions } from "./monaco-find-options-BHU4fGzO.js";
import "./ReviewNotesSendMenuContent-DnAssgZQ.js";
import "./launch-agent-in-new-tab-44JGNfKl.js";
import "./active-agent-note-send-CsxZ0dL2.js";
import "./NotesSendMenu-DLIuO9I1.js";
import { i as getLargeDiffRenderLimit, l as diffEditorScrollbarOptions, o as buildDiffEditorWordWrapOptions, s as LargeDiffFallback, u as applyDiffEditorLineNumberOptions } from "./large-diff-render-limit-CxR8f1bs.js";
import { a as installMonacoEditorFindShortcut, r as installEditorSaveShortcut } from "./editor-shortcuts-Cg6u73ie.js";
import "./comment-body-submit-state-BHQDrSxB.js";
import { a as getDiffViewerMonacoModelPaths, n as disposeUnattachedMonacoModelPaths, t as disposeUnattachedDiffViewerMonacoModels } from "./diff-monaco-model-disposal-C43B-hgU.js";
import { a as setWithLRU, n as diffViewStateCache } from "./scroll-cache-B8ebRfkp.js";
import { t as useContextualCopySetup } from "./useContextualCopySetup-CJ5QxJCT.js";
import { n as useDiffEditorRegistration } from "./diff-navigation-context-osOZIP0x.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function useDiffViewerLargeDiffLifecycle({ limited, modelKey, originalModelKey, modifiedModelKey, diffEditorRef, onEnterFallback }) {
	const [largeDiffModelGeneration, setLargeDiffModelGeneration] = (0, import_react.useState)(0);
	const largeDiffModelGenerationSuffix = largeDiffModelGeneration === 0 ? "" : `:large-diff-generation:${largeDiffModelGeneration}`;
	const currentDiffModelPaths = (0, import_react.useMemo)(() => getDiffViewerMonacoModelPaths({
		modelKey,
		originalModelKey,
		modifiedModelKey,
		generationSuffix: largeDiffModelGenerationSuffix
	}), [
		modelKey,
		originalModelKey,
		modifiedModelKey,
		largeDiffModelGenerationSuffix
	]);
	const currentDiffModelPathsRef = (0, import_react.useRef)(currentDiffModelPaths);
	currentDiffModelPathsRef.current = currentDiffModelPaths;
	const previousDiffModelPathsRef = (0, import_react.useRef)(currentDiffModelPaths);
	(0, import_react.useEffect)(() => {
		const previousModelPaths = previousDiffModelPathsRef.current;
		previousDiffModelPathsRef.current = currentDiffModelPaths;
		const supersededModelPaths = [previousModelPaths.originalModelPath !== currentDiffModelPaths.originalModelPath ? previousModelPaths.originalModelPath : null, previousModelPaths.modifiedModelPath !== currentDiffModelPaths.modifiedModelPath ? previousModelPaths.modifiedModelPath : null].filter((modelPath) => modelPath !== null);
		if (supersededModelPaths.length === 0) return;
		const diffEditor = diffEditorRef.current;
		if (diffEditor) {
			const originalModel = editor.getModel(Uri.parse(currentDiffModelPaths.originalModelPath));
			const modifiedModel = editor.getModel(Uri.parse(currentDiffModelPaths.modifiedModelPath));
			if (!originalModel || !modifiedModel) return;
			const activeModels = diffEditor.getModel();
			if (activeModels?.original !== originalModel || activeModels.modified !== modifiedModel) diffEditor.setModel({
				original: originalModel,
				modified: modifiedModel
			});
		}
		disposeUnattachedMonacoModelPaths(editor_main_exports, supersededModelPaths);
	}, [currentDiffModelPaths, diffEditorRef]);
	(0, import_react.useEffect)(() => {
		if (!limited) return;
		const modelPathsToDispose = currentDiffModelPathsRef.current;
		setLargeDiffModelGeneration((generation) => generation + 1);
		onEnterFallback();
		const disposeTimer = window.setTimeout(() => {
			disposeUnattachedDiffViewerMonacoModels(editor_main_exports, modelPathsToDispose);
		}, 0);
		return () => window.clearTimeout(disposeTimer);
	}, [limited, onEnterFallback]);
	return currentDiffModelPaths;
}
function getDiffViewerLargeDiffSaveAction({ editable, modifiedContent, onSave, saveContentAvailable = true }) {
	if (!editable || !onSave || !saveContentAvailable) return;
	return {
		label: translate("auto.components.editor.DiffViewer.b5675b0694", "Save"),
		description: translate("auto.components.editor.DiffViewer.593f2193f6", "This draft crossed the safe display limit, but it can still be saved."),
		onClick: () => onSave(modifiedContent)
	};
}
function preserveDiffViewStateAcrossModelSwaps(diffEditor) {
	let pendingViewState = null;
	let restoreFrame = null;
	const captureViewState = () => {
		pendingViewState ?? (pendingViewState = diffEditor.saveViewState());
	};
	const scheduleRestore = () => {
		if (!pendingViewState) return;
		if (restoreFrame !== null) cancelAnimationFrame(restoreFrame);
		restoreFrame = requestAnimationFrame(() => {
			restoreFrame = null;
			const viewState = pendingViewState;
			pendingViewState = null;
			if (viewState && diffEditor.getModel()) diffEditor.restoreViewState(viewState);
		});
	};
	const originalEditor = diffEditor.getOriginalEditor();
	const modifiedEditor = diffEditor.getModifiedEditor();
	const subscriptions = [
		originalEditor.onWillChangeModel(captureViewState),
		originalEditor.onDidChangeModel(scheduleRestore),
		modifiedEditor.onWillChangeModel(captureViewState),
		modifiedEditor.onDidChangeModel(scheduleRestore)
	];
	return { dispose: () => {
		for (const subscription of subscriptions) subscription.dispose();
		if (restoreFrame !== null) cancelAnimationFrame(restoreFrame);
		restoreFrame = null;
		pendingViewState = null;
	} };
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function DiffViewer({ modelKey, originalModelKey, modifiedModelKey, originalContent, modifiedContent, language, filePath, relativePath, sideBySide, editable, worktreeId, onAddLineComment, commentableLineNumbers, addLineCommentLabel, addLineCommentPlaceholder, onContentChange, onSave, largeDiffRenderLimit, largeDiffSaveContentAvailable }) {
	const settings = useAppStore((s) => s.settings);
	const editorFontZoomLevel = useAppStore((s) => s.editorFontZoomLevel);
	const addDiffComment = useAppStore((s) => s.addDiffComment);
	const deleteDiffComment = useAppStore((s) => s.deleteDiffComment);
	const updateDiffComment = useAppStore((s) => s.updateDiffComment);
	const scrollToDiffCommentId = useAppStore((s) => s.scrollToDiffCommentId);
	const setScrollToDiffCommentId = useAppStore((s) => s.setScrollToDiffCommentId);
	const allDiffComments = useAppStore((s) => selectWorktreeDiffComments(s, worktreeId));
	const diffComments = (0, import_react.useMemo)(() => (allDiffComments ?? []).filter((c) => c.filePath === relativePath && isDiffComment(c)), [allDiffComments, relativePath]);
	const diffEditorFontSize = computeDiffEditorFontSize(settings?.terminalFontSize ?? 13, editorFontZoomLevel);
	const isDark = settings?.theme === "dark" || settings?.theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches;
	const diffEditorRef = (0, import_react.useRef)(null);
	const { registerDiffEditor, unregisterDiffEditor } = useDiffEditorRegistration();
	const diffBodyRef = (0, import_react.useRef)(null);
	const lineNumberOptionsSubRef = (0, import_react.useRef)(null);
	const [modifiedEditor, setModifiedEditor] = (0, import_react.useState)(null);
	const [popover, setPopover] = (0, import_react.useState)(null);
	const renderLimit = (0, import_react.useMemo)(() => largeDiffRenderLimit ?? getLargeDiffRenderLimit({
		originalContent,
		modifiedContent
	}), [
		largeDiffRenderLimit,
		originalContent,
		modifiedContent
	]);
	const hasLineCommentAction = Boolean(worktreeId || onAddLineComment);
	const pendingScrollForThisViewer = (0, import_react.useMemo)(() => {
		if (!worktreeId || !scrollToDiffCommentId) return null;
		return diffComments.some((c) => c.id === scrollToDiffCommentId) ? scrollToDiffCommentId : null;
	}, [
		scrollToDiffCommentId,
		diffComments,
		worktreeId
	]);
	useDiffCommentDecorator({
		editor: hasLineCommentAction ? modifiedEditor : null,
		monacoModelIdentity: modifiedModelKey ?? modelKey,
		filePath: relativePath,
		worktreeId: worktreeId ?? "",
		comments: worktreeId ? diffComments : [],
		commentableLineNumbers,
		addButtonLabel: addLineCommentLabel,
		onAddCommentClick: ({ lineNumber, startLine, top }) => setPopover({
			lineNumber,
			startLine,
			top,
			left: modifiedEditor ? getDiffCommentPopoverLeft(modifiedEditor, diffBodyRef.current) ?? void 0 : void 0,
			lineHeight: modifiedEditor?.getOption(editor.EditorOption.lineHeight) ?? 0
		}),
		onDeleteComment: (id) => {
			if (worktreeId) deleteDiffComment(worktreeId, id);
		},
		onUpdateComment: worktreeId ? (id, body) => updateDiffComment(worktreeId, id, body) : void 0,
		pendingScrollCommentId: pendingScrollForThisViewer,
		onPendingScrollConsumed: () => setScrollToDiffCommentId(null)
	});
	(0, import_react.useEffect)(() => {
		if (!modifiedEditor || !popover) return;
		const update = () => {
			const lineHeight = modifiedEditor.getOption(editor.EditorOption.lineHeight);
			const top = getDiffCommentPopoverTop(modifiedEditor, popover.lineNumber, lineHeight);
			if (top == null) {
				setPopover(null);
				return;
			}
			const left = getDiffCommentPopoverLeft(modifiedEditor, diffBodyRef.current);
			setPopover((prev) => prev ? {
				...prev,
				top,
				left: left == null ? prev.left : left,
				lineHeight
			} : prev);
		};
		const scrollSub = modifiedEditor.onDidScrollChange(update);
		const contentSub = modifiedEditor.onDidContentSizeChange(update);
		const layoutSub = modifiedEditor.onDidLayoutChange(update);
		return () => {
			scrollSub.dispose();
			contentSub.dispose();
			layoutSub.dispose();
		};
	}, [modifiedEditor, popover?.lineNumber]);
	const didAutoScrollFirstDiffRef = (0, import_react.useRef)(false);
	const didAutoScrollModelKeyRef = (0, import_react.useRef)(modelKey);
	(0, import_react.useEffect)(() => {
		if (didAutoScrollModelKeyRef.current !== modelKey) {
			didAutoScrollModelKeyRef.current = modelKey;
			didAutoScrollFirstDiffRef.current = false;
		}
		const diffEditor = diffEditorRef.current;
		if (!diffEditor || !modifiedEditor) return;
		if (didAutoScrollFirstDiffRef.current) return;
		if (diffViewStateCache.get(modelKey)) return;
		if (pendingScrollForThisViewer) {
			didAutoScrollFirstDiffRef.current = true;
			return;
		}
		let rafId = null;
		const run = () => {
			if (didAutoScrollFirstDiffRef.current) return;
			const changes = diffEditor.getLineChanges();
			if (!changes || changes.length === 0) return;
			const line = Math.max(1, changes[0].modifiedStartLineNumber);
			if (rafId !== null) cancelAnimationFrame(rafId);
			rafId = requestAnimationFrame(() => {
				rafId = null;
				if (didAutoScrollFirstDiffRef.current || !modifiedEditor.getModel()) return;
				const top = modifiedEditor.getTopForLineNumber(line, true);
				const editorHeight = modifiedEditor.getLayoutInfo().height;
				modifiedEditor.setPosition({
					lineNumber: line,
					column: 1
				});
				modifiedEditor.setScrollTop(Math.max(0, top - editorHeight / 2));
				didAutoScrollFirstDiffRef.current = true;
			});
		};
		if (diffEditor.getLineChanges()) run();
		const sub = diffEditor.onDidUpdateDiff(() => run());
		return () => {
			sub.dispose();
			if (rafId !== null) cancelAnimationFrame(rafId);
		};
	}, [
		modifiedEditor,
		modelKey,
		pendingScrollForThisViewer
	]);
	const handleEnterLargeDiffFallback = (0, import_react.useCallback)(() => {
		lineNumberOptionsSubRef.current?.dispose();
		lineNumberOptionsSubRef.current = null;
		const fallenBackEditor = diffEditorRef.current;
		diffEditorRef.current = null;
		if (fallenBackEditor) unregisterDiffEditor(fallenBackEditor);
		setModifiedEditor(null);
		setPopover(null);
	}, [unregisterDiffEditor]);
	const handleSubmitComment = async (body) => {
		if (!popover) return;
		if (onAddLineComment) {
			if (await onAddLineComment({
				lineNumber: popover.lineNumber,
				startLine: popover.startLine,
				body
			})) setPopover(null);
			return;
		}
		if (!worktreeId) return;
		if (await addDiffComment({
			worktreeId,
			filePath: relativePath,
			source: "diff",
			startLine: popover.startLine,
			lineNumber: popover.lineNumber,
			body,
			side: "modified"
		})) setPopover(null);
		else console.error("Failed to add diff comment — draft preserved");
	};
	const onSaveRef = (0, import_react.useRef)(onSave);
	onSaveRef.current = onSave;
	const onContentChangeRef = (0, import_react.useRef)(onContentChange);
	onContentChangeRef.current = onContentChange;
	const { setupCopy, toastNode } = useContextualCopySetup();
	const propsRef = (0, import_react.useRef)({
		relativePath,
		language,
		onSave
	});
	propsRef.current = {
		relativePath,
		language,
		onSave
	};
	const currentDiffModelPaths = useDiffViewerLargeDiffLifecycle({
		limited: renderLimit.limited,
		modelKey,
		originalModelKey,
		modifiedModelKey,
		diffEditorRef,
		onEnterFallback: handleEnterLargeDiffFallback
	});
	const handleMount = (0, import_react.useCallback)((diffEditor, monaco) => {
		diffEditorRef.current = diffEditor;
		registerDiffEditor(diffEditor);
		lineNumberOptionsSubRef.current?.dispose();
		lineNumberOptionsSubRef.current = applyDiffEditorLineNumberOptions(diffEditor, sideBySide);
		const originalEditor = diffEditor.getOriginalEditor();
		const modifiedEditor$1 = diffEditor.getModifiedEditor();
		diffEditor.onDidDispose(preserveDiffViewStateAcrossModelSwaps(diffEditor).dispose);
		setupCopy(originalEditor, monaco, filePath, propsRef);
		setupCopy(modifiedEditor$1, monaco, filePath, propsRef);
		setModifiedEditor(modifiedEditor$1);
		const savedViewState = diffViewStateCache.get(modelKey);
		if (savedViewState) requestAnimationFrame(() => diffEditor.restoreViewState(savedViewState));
		if (editable) {
			const cleanupSaveShortcut = installEditorSaveShortcut(modifiedEditor$1.getContainerDomNode(), () => {
				onSaveRef.current?.(modifiedEditor$1.getValue());
			});
			const cleanupOriginalFindShortcut = installMonacoEditorFindShortcut(originalEditor);
			const cleanupModifiedFindShortcut = installMonacoEditorFindShortcut(modifiedEditor$1);
			const modelContentSub = modifiedEditor$1.onDidChangeModelContent(() => {
				onContentChangeRef.current?.(modifiedEditor$1.getValue());
			});
			modifiedEditor$1.onDidDispose(() => {
				cleanupSaveShortcut();
				cleanupOriginalFindShortcut();
				cleanupModifiedFindShortcut();
				modelContentSub.dispose();
			});
			modifiedEditor$1.focus();
		} else diffEditor.focus();
		diffEditor.onDidDispose(() => {
			lineNumberOptionsSubRef.current?.dispose();
			lineNumberOptionsSubRef.current = null;
			diffEditorRef.current = null;
			unregisterDiffEditor(diffEditor);
			setModifiedEditor(null);
			setPopover(null);
		});
	}, [
		editable,
		setupCopy,
		modelKey,
		filePath,
		sideBySide,
		registerDiffEditor,
		unregisterDiffEditor
	]);
	(0, import_react.useLayoutEffect)(() => {
		return () => {
			const de = diffEditorRef.current;
			if (de) {
				const currentViewState = de.saveViewState();
				if (currentViewState) setWithLRU(diffViewStateCache, modelKey, currentViewState);
			}
		};
	}, [modelKey]);
	(0, import_react.useEffect)(() => {
		const diffEditor = diffEditorRef.current;
		if (!diffEditor) return;
		lineNumberOptionsSubRef.current?.dispose();
		lineNumberOptionsSubRef.current = applyDiffEditorLineNumberOptions(diffEditor, sideBySide);
		return () => {
			lineNumberOptionsSubRef.current?.dispose();
			lineNumberOptionsSubRef.current = null;
		};
	}, [sideBySide]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col flex-1 min-h-0",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			ref: diffBodyRef,
			className: "flex-1 min-h-0 relative",
			children: [popover && hasLineCommentAction && !renderLimit.limited && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DiffCommentPopover, {
				lineNumber: popover.lineNumber,
				startLine: popover.startLine,
				top: popover.top,
				left: popover.left,
				lineHeight: popover.lineHeight,
				placeholder: addLineCommentPlaceholder,
				submitLabel: addLineCommentLabel,
				submittingLabel: "Posting…",
				onCancel: () => setPopover(null),
				onSubmit: handleSubmitComment
			}, popover.lineNumber), renderLimit.limited ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LargeDiffFallback, {
				filePath: relativePath,
				renderLimit,
				action: getDiffViewerLargeDiffSaveAction({
					editable,
					modifiedContent,
					onSave,
					saveContentAvailable: largeDiffSaveContentAvailable
				})
			}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(we, {
				height: "100%",
				language,
				original: originalContent,
				modified: modifiedContent,
				theme: isDark ? "vs-dark" : "vs",
				onMount: handleMount,
				originalModelPath: currentDiffModelPaths.originalModelPath,
				modifiedModelPath: currentDiffModelPaths.modifiedModelPath,
				keepCurrentOriginalModel: true,
				keepCurrentModifiedModel: true,
				options: {
					readOnly: !editable,
					originalEditable: false,
					renderSideBySide: sideBySide,
					minimap: { enabled: false },
					scrollBeyondLastLine: false,
					fontSize: diffEditorFontSize,
					fontFamily: resolveEditorFontFamily(settings),
					lineNumbers: "on",
					...buildDiffEditorWordWrapOptions(settings?.diffWordWrap),
					automaticLayout: true,
					renderOverviewRuler: true,
					scrollbar: diffEditorScrollbarOptions,
					padding: { top: 0 },
					find: monacoFindOptions
				}
			})]
		}), toastNode]
	});
}
export { DiffViewer as default };
