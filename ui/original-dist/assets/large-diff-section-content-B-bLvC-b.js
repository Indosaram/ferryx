const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./ImageDiffViewer-CYxF1fyi.js","./ImageViewer-EZPfzHX3.js","./classPrivateFieldGet2-CvaeS1Sp.js","./chunk-Dhmk_5SA.js","./classPrivateMethodInitSpec-AvUU-6NC.js","./defineProperty-BAtR-r70.js","./preload-helper-Cgw39-ka.js","./button-DszXJEV6.js","./jsx-runtime-Cv_nyRjc.js","./react-Da2TLWQy.js","./chevron-down-BRkP96Md.js","./chevron-up-CuYdIP8o.js","./image-wyTi8HuE.js","./rotate-ccw-hK0RKgaG.js","./search-DK1nVA6d.js","./store-CgXrfmaH.js","./dist-DgqligFk.js","./react-dom-Da8MQai-.js","./plugin-manifest-Bs-50M_g.js","./useMountedRef-1omUd-IV.js","./agent-status-3vUKbY6l.js","./agent-kind-Dfx6MnkP.js","./telemetry-ZyUPyKMD.js","./x-BrGKE4uz.js","./zoom-out-Z8dr_jqi.js","./dialog-BbelfMSB.js","./dist-CN60QqbN.js","./dist-CUdeCwrc.js","./dist-BvH-oDES.js","./dist-DGfr86jh.js","./dist-DW1EJH6e.js","./es2015-B5WZ-7WO.js","./useShortcutLabel-C-KRYtlB.js","./shortcut-platform-BbPBGzth.js","./find-query-bounds-BKNiI6IV.js","./scroll-cache-B8ebRfkp.js","./ImageViewer-Chm2gOCI.css"])))=>i.map(i=>d[i]);
import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as lazyWithRetry } from "./lazy-with-retry-pSZJrSfN.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as ChevronRight } from "./chevron-right-CZtMe6Ev.js";
import { t as CircleAlert } from "./circle-alert-keRTpMg-.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as Eye } from "./eye-BvQpbQVj.js";
import { t as getFileTypeIcon } from "./file-type-icons-CeipsYgO.js";
import { t as FolderOpen } from "./folder-open-B2ZB-rfY.js";
import { t as Folder } from "./folder-CYUB3i-Q.js";
import { t as Funnel } from "./funnel-sY2hg24b.js";
import { t as PanelLeftClose } from "./panel-left-close-B9t5w2Nh.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { t as Search } from "./search-DK1nVA6d.js";
import { $a as isClipboardTextByteLengthOverLimit, Dc as dirname, Ec as basename, Rt as detectLanguage, di as COMBINED_DIFF_FILE_TREE_RESIZE_STEP, fi as clampCombinedDiffFileTreeWidth, kc as joinPath, pi as computeCombinedDiffFileTreeWidthBounds, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
import { i as PopoverTrigger, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import { i as resolveEditorFontFamily, t as computeDiffEditorFontSize } from "./editor-font-zoom-2F4BKkDZ.js";
import { t as useSidebarResize } from "./useSidebarResize-BhlGhEjK.js";
import { t as Input } from "./input-DV5rpysh.js";
import { n as WORKSPACE_FILE_PATH_MIME } from "./workspace-file-drag-BnROcEA_.js";
import { p as editor } from "./editor.api2-DX_-Ye6K.js";
import { r as we } from "./monaco-setup-CKSA6ArO.js";
import { t as editor_main_exports } from "./editor.main-BGL6BKIn.js";
import { t as selectWorktreeDiffComments } from "./worktree-diff-comments-selector-B8AXyh9e.js";
import { n as getDiffCommentPopoverLeft, r as getDiffCommentPopoverTop, t as DiffCommentPopover } from "./DiffCommentPopover-Cap7I9o6.js";
import { r as isDiffComment } from "./diff-comment-compat-CWwyL2nL.js";
import { n as useDiffCommentDecorator, t as monacoFindOptions } from "./monaco-find-options-BHU4fGzO.js";
import { a as getLargeDiffRenderLimitFromCounts, c as combinedDiffSectionScrollbarOptions, o as buildDiffEditorWordWrapOptions, r as countLinesLikeSplit, s as LargeDiffFallback, u as applyDiffEditorLineNumberOptions } from "./large-diff-render-limit-CxR8f1bs.js";
import { a as installMonacoEditorFindShortcut, r as installEditorSaveShortcut } from "./editor-shortcuts-Cg6u73ie.js";
import { n as disposeUnattachedMonacoModelPaths } from "./diff-monaco-model-disposal-C43B-hgU.js";
import { a as compactSourceControlTree, n as buildGitStatusSourceControlTree, o as flattenSourceControlTree, r as buildSourceControlTree } from "./source-control-tree-B0FKTU3G.js";
import { n as STATUS_LABELS, t as STATUS_COLORS } from "./status-display-BIGXsq1P.js";
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function DiffSectionHeader({ path, dirty, collapsed, added, removed, onToggle, onOpenSection, openSectionTitle, onOpenPreview, trailingContent }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "sticky top-0 z-10 bg-background flex items-center w-full px-3 py-1.5 text-left text-xs hover:bg-accent transition-colors group cursor-pointer",
		onClick: onToggle,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: "min-w-0 flex-1 truncate text-muted-foreground",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					role: "button",
					tabIndex: 0,
					className: "cursor-copy hover:underline",
					onMouseDown: (event) => {
						event.preventDefault();
						event.stopPropagation();
					},
					onClick: (event) => {
						event.preventDefault();
						event.stopPropagation();
						window.api.ui.writeClipboardText(path).catch((error) => {
							console.error("Failed to copy diff path:", error);
						});
					},
					onKeyDown: (event) => {
						if (event.key !== "Enter" && event.key !== " ") return;
						event.preventDefault();
						event.stopPropagation();
						window.api.ui.writeClipboardText(path).catch((error) => {
							console.error("Failed to copy diff path:", error);
						});
					},
					title: translate("auto.components.editor.DiffSectionHeader.8915726e93", "Copy path"),
					children: path
				}),
				dirty && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "font-medium ml-1",
					children: "M"
				}),
				(added > 0 || removed > 0) && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "tabular-nums ml-2",
					children: [
						added > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
							className: "text-green-600 dark:text-green-500",
							children: ["+", added]
						}),
						added > 0 && removed > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: " " }),
						removed > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
							className: "text-red-500",
							children: ["-", removed]
						})
					]
				})
			]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center gap-1 shrink-0 ml-2",
			children: [
				trailingContent,
				onOpenPreview != null && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					className: "p-0.5 rounded text-muted-foreground hover:text-foreground transition-colors",
					onClick: (event) => {
						event.stopPropagation();
						onOpenPreview(event);
					},
					title: translate("auto.components.editor.EditorPanelHeader.fb8331694e", "Open Preview to the Side"),
					"aria-label": translate("auto.components.editor.EditorPanelHeader.fb8331694e", "Open Preview to the Side"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Eye, { className: "size-3.5" })
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					className: "p-0.5 rounded text-muted-foreground hover:text-foreground transition-colors",
					onClick: (event) => {
						event.stopPropagation();
						onOpenSection(event);
					},
					title: openSectionTitle,
					"aria-label": openSectionTitle,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3.5" })
				}),
				collapsed ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "size-3.5 shrink-0 text-muted-foreground" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: "size-3.5 shrink-0 text-muted-foreground" })
			]
		})]
	});
}
var ImageDiffViewer = lazyWithRetry(() => __vitePreload(() => import("./ImageDiffViewer-CYxF1fyi.js"), __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36]), import.meta.url));
function DiffSectionBody({ section, index, sectionBodyRef, sectionBodyHeight, useIntrinsicImageHeight, popover, addLineCommentPlaceholder, addLineCommentLabel, isBranchMode, sideBySide, isDark, language, modelPathBase, isEditable, diffEditorFontSize, diffWordWrap, editorFontFamily, onCancelComment, onSubmitComment, onRetrySection, onSaveLimitedDiff, onMount }) {
	const renderLimit = section.largeDiffRenderLimit?.limited ? section.largeDiffRenderLimit : null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: sectionBodyRef,
		className: cn("relative", useIntrinsicImageHeight && "overflow-visible"),
		style: sectionBodyHeight === void 0 ? void 0 : { height: sectionBodyHeight },
		children: [popover && !renderLimit?.limited ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DiffCommentPopover, {
			lineNumber: popover.lineNumber,
			startLine: popover.startLine,
			top: popover.top,
			left: popover.left,
			lineHeight: popover.lineHeight,
			placeholder: addLineCommentPlaceholder,
			submitLabel: addLineCommentLabel,
			submittingLabel: "Posting…",
			onCancel: onCancelComment,
			onSubmit: onSubmitComment
		}, popover.lineNumber) : null, section.loading ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex h-full items-center gap-2 bg-muted/10 px-3 text-[11px] text-muted-foreground",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "h-1.5 w-1.5 rounded-full bg-muted-foreground/50" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.editor.DiffSectionBody.f5cf81cec2", "Loading diff...") })]
		}) : section.error ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex h-full items-center justify-between gap-3 bg-muted/10 px-3 text-[11px] text-muted-foreground",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 items-center gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleAlert, { className: "size-3.5 shrink-0 text-destructive" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "truncate",
					children: section.error
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				type: "button",
				variant: "ghost",
				size: "xs",
				className: "h-6 shrink-0 px-2 text-[11px]",
				onClick: (event) => {
					event.stopPropagation();
					onRetrySection(index);
				},
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: "size-3" }), translate("auto.components.editor.DiffSectionBody.cef4cf0ff5", "Retry")]
			})]
		}) : section.diffResult?.kind === "binary" ? section.diffResult.isImage ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ImageDiffViewer, {
			originalContent: section.diffResult.originalContent,
			modifiedContent: section.diffResult.modifiedContent,
			filePath: section.path,
			mimeType: section.diffResult.mimeType,
			sideBySide,
			layout: useIntrinsicImageHeight ? "intrinsic" : "fill"
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "flex h-full items-center justify-center px-6 text-center",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "text-sm font-medium text-foreground",
					children: translate("auto.components.editor.DiffSectionBody.35d6afb5be", "Binary file changed")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "text-xs text-muted-foreground",
					children: isBranchMode ? translate("auto.components.editor.DiffSectionBody.7ce8436458", "Text diff is unavailable for this file in branch compare.") : translate("auto.components.editor.DiffSectionBody.72f71f52eb", "Text diff is unavailable for this file.")
				})]
			})
		}) : renderLimit?.limited ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LargeDiffFallback, {
			filePath: section.path,
			renderLimit,
			action: isEditable && section.dirty ? {
				label: translate("auto.components.editor.DiffSectionBody.b5675b0694", "Save"),
				description: translate("auto.components.editor.DiffSectionBody.593f2193f6", "This draft crossed the safe display limit, but it can still be saved."),
				onClick: onSaveLimitedDiff
			} : void 0
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(we, {
			height: "100%",
			language,
			original: section.originalContent,
			modified: section.modifiedContent,
			theme: isDark ? "vs-dark" : "vs",
			onMount,
			originalModelPath: `${modelPathBase}:original`,
			modifiedModelPath: `${modelPathBase}:modified`,
			keepCurrentOriginalModel: true,
			keepCurrentModifiedModel: true,
			options: {
				readOnly: !isEditable,
				originalEditable: false,
				renderSideBySide: sideBySide,
				minimap: { enabled: false },
				scrollBeyondLastLine: false,
				fontSize: diffEditorFontSize,
				fontFamily: editorFontFamily || "monospace",
				lineNumbers: "on",
				...buildDiffEditorWordWrapOptions(diffWordWrap),
				automaticLayout: true,
				renderOverviewRuler: false,
				scrollbar: combinedDiffSectionScrollbarOptions,
				hideUnchangedRegions: { enabled: true },
				find: monacoFindOptions
			}
		})]
	});
}
function computeLineStats(original, modified, status) {
	if (original.length + modified.length > 5e5) return null;
	if (status === "added") return {
		added: modified ? countLinesWithoutAllocation(modified) : 0,
		removed: 0
	};
	if (status === "deleted") return {
		added: 0,
		removed: original ? countLinesWithoutAllocation(original) : 0
	};
	const origMap = /* @__PURE__ */ new Map();
	const originalLineCount = countDiffLinesIntoMultiset(original, origMap);
	let modifiedLineCount = 0;
	let matched = 0;
	forEachDiffLine(modified, (line) => {
		modifiedLineCount += 1;
		const count = origMap.get(line) ?? 0;
		if (count > 0) {
			origMap.set(line, count - 1);
			matched += 1;
		}
	});
	return {
		added: modifiedLineCount - matched,
		removed: originalLineCount - matched
	};
}
function countDiffLinesIntoMultiset(content, lineCounts) {
	let lineCount = 0;
	forEachDiffLine(content, (line) => {
		lineCount += 1;
		lineCounts.set(line, (lineCounts.get(line) ?? 0) + 1);
	});
	return lineCount;
}
function countLinesWithoutAllocation(content) {
	let lineCount = 1;
	for (let index = 0; index < content.length; index += 1) if (content.charCodeAt(index) === 10) lineCount += 1;
	return lineCount;
}
function forEachDiffLine(content, visit) {
	let lineStart = 0;
	for (let index = 0; index <= content.length; index += 1) {
		if (index < content.length && content.charCodeAt(index) !== 10) continue;
		visit(content.slice(lineStart, index));
		lineStart = index + 1;
	}
}
var DIFF_LINE_HEIGHT = 19;
var DIFF_SECTION_PADDING_HEIGHT = 19;
var MIN_DIFF_SECTION_BODY_HEIGHT = 60;
var DIFF_SECTION_HEADER_HEIGHT = 28;
var DIFF_UNCHANGED_CONTEXT_LINE_ESTIMATE = 12;
var MAX_UNMEASURED_TEXT_BODY_LINES = 80;
var LARGE_DIFF_FALLBACK_BODY_HEIGHT = 160;
function isIntrinsicHeightImageDiff(diffResult) {
	return diffResult?.kind === "binary" && diffResult.mimeType?.startsWith("image/") === true;
}
function getLargeDiffFallbackBodyHeight() {
	return LARGE_DIFF_FALLBACK_BODY_HEIGHT;
}
function getDiffSectionBodyHeight({ measuredContentHeight, originalContent, modifiedContent, changedLineCount, useIntrinsicImageHeight, lineCounts }) {
	if (useIntrinsicImageHeight) return;
	if (measuredContentHeight !== void 0 && measuredContentHeight > 0) return measuredContentHeight + DIFF_SECTION_PADDING_HEIGHT;
	const fullLineCount = lineCounts ? Math.max(lineCounts.original, lineCounts.modified) : Math.max(countLinesLikeSplit(originalContent), countLinesLikeSplit(modifiedContent));
	const estimatedLineCount = changedLineCount !== void 0 ? Math.min(fullLineCount, Math.max(2, changedLineCount + DIFF_UNCHANGED_CONTEXT_LINE_ESTIMATE)) : Math.min(fullLineCount, MAX_UNMEASURED_TEXT_BODY_LINES);
	return Math.max(MIN_DIFF_SECTION_BODY_HEIGHT, estimatedLineCount * DIFF_LINE_HEIGHT + DIFF_SECTION_PADDING_HEIGHT);
}
function getDiffSectionEstimatedHeight({ collapsed, measuredContentHeight, originalContent, modifiedContent, changedLineCount, useIntrinsicImageHeight, lineCounts, isLargeDiffLimited = false }) {
	if (collapsed) return DIFF_SECTION_HEADER_HEIGHT;
	if (isLargeDiffLimited) return DIFF_SECTION_HEADER_HEIGHT + getLargeDiffFallbackBodyHeight();
	return DIFF_SECTION_HEADER_HEIGHT + (getDiffSectionBodyHeight({
		measuredContentHeight,
		originalContent,
		modifiedContent,
		changedLineCount,
		useIntrinsicImageHeight,
		lineCounts
	}) ?? MIN_DIFF_SECTION_BODY_HEIGHT);
}
var import_react = /* @__PURE__ */ __toESM(require_react());
function useDiffSectionLayoutMetrics({ section, sectionHeight }) {
	const renderLimit = section.largeDiffRenderLimit;
	const isLargeDiffLimited = renderLimit?.limited === true;
	const lineStats = (0, import_react.useMemo)(() => section.loading || section.error || isLargeDiffLimited ? null : computeLineStats(section.originalContent, section.modifiedContent, section.status), [
		section.error,
		section.loading,
		section.originalContent,
		section.modifiedContent,
		section.status,
		isLargeDiffLimited
	]);
	const changedLineCount = (0, import_react.useMemo)(() => {
		if (isLargeDiffLimited) return;
		if (lineStats) return lineStats.added + lineStats.removed;
		if (section.added === void 0 && section.removed === void 0) return;
		return (section.added ?? 0) + (section.removed ?? 0);
	}, [
		lineStats,
		section.added,
		section.removed,
		isLargeDiffLimited
	]);
	const useIntrinsicImageHeight = isIntrinsicHeightImageDiff(section.diffResult);
	return {
		lineStats,
		sectionBodyHeight: isLargeDiffLimited ? getLargeDiffFallbackBodyHeight() : getDiffSectionBodyHeight({
			measuredContentHeight: sectionHeight,
			originalContent: section.originalContent,
			modifiedContent: section.modifiedContent,
			changedLineCount,
			useIntrinsicImageHeight,
			lineCounts: renderLimit?.lineCounts ?? void 0
		}),
		useIntrinsicImageHeight,
		isLargeDiffLimited
	};
}
function getLiveDiffSectionRenderLimit({ section, modifiedEditor, modifiedContent }) {
	const modifiedLineCount = modifiedContent.length === 0 ? 0 : modifiedEditor.getModel()?.getLineCount() ?? section.largeDiffRenderLimit?.lineCounts?.modified ?? 0;
	return getLargeDiffRenderLimitFromCounts({
		originalLineCount: section.largeDiffRenderLimit?.lineCounts?.original ?? 0,
		modifiedLineCount,
		originalCharacterCount: section.originalContent.length,
		modifiedCharacterCount: modifiedContent.length
	});
}
function removeDiffSectionMeasuredHeight(heights, index) {
	if (!(index in heights)) return heights;
	const { [index]: _removed, ...rest } = heights;
	return rest;
}
function useDiffSectionFallbackCleanup({ disposeDiffModels, index, isLargeDiffLimited, setSectionHeights }) {
	(0, import_react.useEffect)(() => {
		if (isLargeDiffLimited) {
			setSectionHeights((prev) => removeDiffSectionMeasuredHeight(prev, index));
			disposeDiffModels();
		}
	}, [
		disposeDiffModels,
		index,
		isLargeDiffLimited,
		setSectionHeights
	]);
}
async function submitDiffSectionComment({ addDiffComment, body, onAddLineComment, popover, section, worktreeId }) {
	if (onAddLineComment) return onAddLineComment(section, {
		lineNumber: popover.lineNumber,
		startLine: popover.startLine,
		body
	});
	if (!worktreeId) return false;
	const result = await addDiffComment({
		worktreeId,
		filePath: section.path,
		source: "diff",
		startLine: popover.startLine,
		lineNumber: popover.lineNumber,
		body,
		side: "modified"
	});
	if (!result) console.error("Failed to add diff comment — draft preserved");
	return Boolean(result);
}
function useDiffSectionModelLifecycle(params) {
	const disposeDiffModels = (0, import_react.useCallback)(() => {
		window.setTimeout(() => {
			disposeUnattachedMonacoModelPaths(editor_main_exports, [`${params.modelPathBase}:original`, `${params.modelPathBase}:modified`]);
		}, 0);
	}, [params.modelPathBase]);
	const disposeDiffModelsRef = (0, import_react.useRef)(disposeDiffModels);
	(0, import_react.useEffect)(() => {
		disposeDiffModelsRef.current = disposeDiffModels;
	}, [disposeDiffModels]);
	const setSectionRootNode = (0, import_react.useCallback)((node) => {
		if (node) return;
		disposeDiffModelsRef.current();
	}, []);
	(0, import_react.useEffect)(() => {
		if (params.collapsed) disposeDiffModels();
	}, [disposeDiffModels, params.collapsed]);
	return {
		disposeDiffModels,
		setSectionRootNode
	};
}
function DiffSectionItem({ section, index, isBranchMode, sideBySide, isDark, settings, sectionHeight, worktreeId, loadSection, retrySection, toggleSection, openSection, openSectionTitle, onOpenPreview, renderHeaderTrailingContent, onAddLineComment, addLineCommentLabel, addLineCommentPlaceholder, inlineComments, getCommentableLineNumbers, setSectionHeights, setSections, modifiedEditorsRef, handleSectionSaveRef }) {
	const editorFontZoomLevel = useAppStore((s) => s.editorFontZoomLevel);
	const addDiffComment = useAppStore((s) => s.addDiffComment);
	const deleteDiffComment = useAppStore((s) => s.deleteDiffComment);
	const updateDiffComment = useAppStore((s) => s.updateDiffComment);
	const scrollToDiffCommentId = useAppStore((s) => s.scrollToDiffCommentId);
	const setScrollToDiffCommentId = useAppStore((s) => s.setScrollToDiffCommentId);
	const allDiffComments = useAppStore((s) => selectWorktreeDiffComments(s, worktreeId));
	const diffComments = (0, import_react.useMemo)(() => (allDiffComments ?? []).filter((c) => c.filePath === section.path && isDiffComment(c)), [allDiffComments, section.path]);
	const language = detectLanguage(section.path);
	const isEditable = section.area === "unstaged";
	const modelPathBase = (0, import_react.useMemo)(() => `diff-section:${encodeURIComponent(worktreeId ?? "review")}:${encodeURIComponent(section.key)}:${section.contentGeneration ?? 0}`, [
		section.contentGeneration,
		section.key,
		worktreeId
	]);
	const diffEditorFontSize = computeDiffEditorFontSize(settings?.terminalFontSize ?? 13, editorFontZoomLevel);
	const [modifiedEditor, setModifiedEditor] = (0, import_react.useState)(null);
	const diffEditorRef = (0, import_react.useRef)(null);
	const sectionBodyRef = (0, import_react.useRef)(null);
	const lineNumberOptionsSubRef = (0, import_react.useRef)(null);
	const [popover, setPopover] = (0, import_react.useState)(null);
	const hasLineCommentAction = Boolean(worktreeId || onAddLineComment);
	const { disposeDiffModels, setSectionRootNode } = useDiffSectionModelLifecycle({
		modelPathBase,
		collapsed: section.collapsed
	});
	const pendingScrollForThisSection = (0, import_react.useMemo)(() => {
		if (!scrollToDiffCommentId) return null;
		return diffComments.some((c) => c.id === scrollToDiffCommentId) ? scrollToDiffCommentId : null;
	}, [scrollToDiffCommentId, diffComments]);
	useDiffCommentDecorator({
		editor: hasLineCommentAction ? modifiedEditor : null,
		filePath: section.path,
		worktreeId: worktreeId ?? "",
		comments: inlineComments ?? (worktreeId ? diffComments : []),
		commentableLineNumbers: getCommentableLineNumbers?.(section),
		addButtonLabel: addLineCommentLabel,
		onAddCommentClick: ({ lineNumber, startLine, top }) => setPopover({
			lineNumber,
			startLine,
			top,
			left: modifiedEditor ? getDiffCommentPopoverLeft(modifiedEditor, sectionBodyRef.current) ?? void 0 : void 0,
			lineHeight: modifiedEditor?.getOption(editor.EditorOption.lineHeight) ?? 0
		}),
		onDeleteComment: (id) => {
			if (worktreeId) deleteDiffComment(worktreeId, id);
		},
		onUpdateComment: worktreeId ? (id, body) => updateDiffComment(worktreeId, id, body) : void 0,
		pendingScrollCommentId: pendingScrollForThisSection,
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
			const left = getDiffCommentPopoverLeft(modifiedEditor, sectionBodyRef.current);
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
	const handleSubmitComment = async (body) => {
		if (!popover) return;
		if (await submitDiffSectionComment({
			addDiffComment,
			body,
			onAddLineComment,
			popover,
			section,
			worktreeId
		})) setPopover(null);
	};
	const { lineStats, sectionBodyHeight, useIntrinsicImageHeight, isLargeDiffLimited } = useDiffSectionLayoutMetrics({
		section,
		sectionHeight
	});
	useDiffSectionFallbackCleanup({
		disposeDiffModels,
		index,
		isLargeDiffLimited,
		setSectionHeights
	});
	const handleMount = (editor$1, _monaco) => {
		diffEditorRef.current = editor$1;
		lineNumberOptionsSubRef.current?.dispose();
		lineNumberOptionsSubRef.current = applyDiffEditorLineNumberOptions(editor$1, sideBySide);
		const modified = editor$1.getModifiedEditor();
		let diffLayoutReady = false;
		let pendingHeightFrame = null;
		const updateHeight = () => {
			const contentHeight = editor$1.getModifiedEditor().getContentHeight();
			setSectionHeights((prev) => {
				if (prev[index] === contentHeight) return prev;
				return {
					...prev,
					[index]: contentHeight
				};
			});
		};
		const requestHeightUpdate = () => {
			if (pendingHeightFrame !== null) return;
			pendingHeightFrame = window.requestAnimationFrame(() => {
				pendingHeightFrame = null;
				updateHeight();
			});
		};
		const markDiffLayoutReady = () => {
			diffLayoutReady = true;
			requestHeightUpdate();
		};
		const contentSizeSub = modified.onDidContentSizeChange(() => {
			if (diffLayoutReady) requestHeightUpdate();
		});
		const diffUpdateSub = editor$1.onDidUpdateDiff(markDiffLayoutReady);
		if (editor$1.getLineChanges() !== null) markDiffLayoutReady();
		setModifiedEditor(modified);
		modified.onDidDispose(() => {
			contentSizeSub.dispose();
			diffUpdateSub.dispose();
			if (pendingHeightFrame !== null) {
				window.cancelAnimationFrame(pendingHeightFrame);
				pendingHeightFrame = null;
			}
			lineNumberOptionsSubRef.current?.dispose();
			lineNumberOptionsSubRef.current = null;
			diffEditorRef.current = null;
			if (modifiedEditorsRef.current.get(index) === modified) modifiedEditorsRef.current.delete(index);
			setModifiedEditor(null);
			setPopover(null);
		});
		if (!isEditable) return;
		modifiedEditorsRef.current.set(index, modified);
		const original = editor$1.getOriginalEditor();
		const cleanupSaveShortcut = installEditorSaveShortcut(modified.getContainerDomNode(), () => handleSectionSaveRef.current(index));
		const cleanupOriginalFindShortcut = installMonacoEditorFindShortcut(original);
		const cleanupModifiedFindShortcut = installMonacoEditorFindShortcut(modified);
		const modelContentSub = modified.onDidChangeModelContent(() => {
			const current = modified.getValue();
			setSections((prev) => {
				let changed = false;
				const next = prev.map((s, i) => {
					if (i !== index) return s;
					const dirty = current !== (s.diffResult?.kind === "text" ? s.diffResult.modifiedContent : s.modifiedContent);
					if (s.modifiedContent === current && s.dirty === dirty) return s;
					changed = true;
					return {
						...s,
						modifiedContent: current,
						dirty,
						largeDiffRenderLimit: getLiveDiffSectionRenderLimit({
							section: s,
							modifiedEditor: modified,
							modifiedContent: current
						})
					};
				});
				return changed ? next : prev;
			});
		});
		modified.onDidDispose(() => {
			cleanupSaveShortcut();
			cleanupOriginalFindShortcut();
			cleanupModifiedFindShortcut();
			modelContentSub.dispose();
		});
	};
	(0, import_react.useEffect)(() => {
		loadSection(index);
	}, [index, loadSection]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: setSectionRootNode,
		className: "border-b border-border",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DiffSectionHeader, {
			path: section.path,
			dirty: section.dirty,
			collapsed: section.collapsed,
			added: lineStats?.added ?? section.added ?? 0,
			removed: lineStats?.removed ?? section.removed ?? 0,
			onToggle: () => toggleSection(index),
			onOpenSection: (event) => {
				event.stopPropagation();
				openSection(index);
			},
			openSectionTitle,
			onOpenPreview: onOpenPreview ? () => {
				onOpenPreview(section, index);
			} : void 0,
			trailingContent: renderHeaderTrailingContent?.(section, index)
		}), !section.collapsed && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DiffSectionBody, {
			section,
			index,
			sectionBodyRef,
			sectionBodyHeight,
			useIntrinsicImageHeight,
			popover,
			addLineCommentPlaceholder,
			addLineCommentLabel,
			isBranchMode,
			sideBySide,
			isDark,
			language,
			modelPathBase,
			isEditable,
			diffEditorFontSize,
			diffWordWrap: settings?.diffWordWrap,
			editorFontFamily: resolveEditorFontFamily(settings),
			onCancelComment: () => setPopover(null),
			onSubmitComment: handleSubmitComment,
			onRetrySection: retrySection,
			onSaveLimitedDiff: () => void handleSectionSaveRef.current(index),
			onMount: handleMount
		})]
	});
}
const NO_EXTENSION_KEY = "(no extension)";
const COMBINED_DIFF_FILE_TREE_QUERY_MAX_BYTES = 2 * 1024;
function isCombinedDiffFileTreeQueryTooLarge(query, maxBytes = COMBINED_DIFF_FILE_TREE_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function getCombinedDiffFileTreeSectionKey(mode, entry) {
	if ((mode === "all" || mode === "uncommitted") && "area" in entry) return `${entry.area}:${entry.path}`;
	return `${mode === "commit" ? "combined-commit" : "combined-branch"}:${entry.path}`;
}
function createCombinedDiffSectionIndexMap(sections) {
	return new Map(sections.map((section, index) => [section.key, index]));
}
function getCombinedDiffFileTreeNavigationIndex({ mode, entry, sectionIndexByKey }) {
	return sectionIndexByKey.get(getCombinedDiffFileTreeSectionKey(mode, entry)) ?? null;
}
function handleCombinedDiffFileTreeNavigation({ mode, entry, sections, sectionIndexByKey, toggleSection, loadSection, scrollToIndex }) {
	const index = getCombinedDiffFileTreeNavigationIndex({
		mode,
		entry,
		sectionIndexByKey
	});
	if (index === null || !sections[index]) return null;
	if (sections[index].collapsed) toggleSection(index);
	loadSection?.(index);
	scrollToIndex(index);
	return index;
}
function isGitStatusEntry(entry) {
	return "area" in entry;
}
function getEntryExtension(entry) {
	const name = basename(entry.path);
	const index = name.lastIndexOf(".");
	if (index <= 0 || index === name.length - 1) return NO_EXTENSION_KEY;
	return name.slice(index).toLowerCase();
}
function getEntrySearchText(entry) {
	return [
		entry.path,
		entry.oldPath ?? "",
		entry.status,
		isGitStatusEntry(entry) ? entry.area : ""
	].join(" ").toLowerCase();
}
function getFilteredCombinedDiffFileTreeEntries({ entries, mode, query, excludedExtensions, includeViewed, viewedSectionKeys }) {
	if (isCombinedDiffFileTreeQueryTooLarge(query)) return [];
	const normalizedQuery = query.trim().toLowerCase();
	return entries.filter((entry) => {
		if (excludedExtensions.has(getEntryExtension(entry))) return false;
		if (!includeViewed && viewedSectionKeys.has(getCombinedDiffFileTreeSectionKey(mode, entry))) return false;
		return normalizedQuery.length === 0 || getEntrySearchText(entry).includes(normalizedQuery);
	});
}
function getCombinedDiffBranchEntriesInTreeOrder(mode, entries) {
	return flattenSourceControlTree(compactSourceControlTree(buildSourceControlTree(mode === "commit" ? "combined-commit" : "combined-branch", [...entries])), /* @__PURE__ */ new Set()).filter((node) => node.type === "file").map((node) => node.entry);
}
var COMBINED_DIFF_TREE_INDENT_PX = 12;
var COMBINED_DIFF_TREE_DIRECTORY_PADDING_PX = 8;
var COMBINED_DIFF_TREE_FILE_PADDING_PX = 20;
function CombinedDiffFileTreeRow({ node, mode, worktreePath, activeSectionKey, sectionIndexByKey, isCollapsed, onToggleDirectory, onNavigate }) {
	if (node.type === "directory") return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "group relative flex w-full items-center gap-1 py-1 pr-3 text-xs text-muted-foreground transition-colors hover:bg-accent/40 hover:text-foreground",
		style: { paddingLeft: `${node.depth * COMBINED_DIFF_TREE_INDENT_PX + COMBINED_DIFF_TREE_DIRECTORY_PADDING_PX}px` },
		draggable: true,
		onDragStart: (event) => {
			event.dataTransfer.setData(WORKSPACE_FILE_PATH_MIME, joinPath(worktreePath, node.path));
			event.dataTransfer.effectAllowed = "copy";
		},
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
			type: "button",
			className: "flex min-w-0 flex-1 items-center gap-1 text-left",
			onClick: () => onToggleDirectory(node.key),
			"aria-expanded": !isCollapsed,
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: cn("size-3 shrink-0 transition-transform", isCollapsed && "-rotate-90") }),
				isCollapsed ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Folder, { className: "size-3 shrink-0" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderOpen, { className: "size-3 shrink-0" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "min-w-0 flex-1 truncate",
					children: node.name
				})
			]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "w-4 shrink-0 text-center text-[10px] font-bold tabular-nums text-muted-foreground/80",
			children: node.fileCount
		})]
	});
	const sectionKey = getCombinedDiffFileTreeSectionKey(mode, node.entry);
	const FileIcon = getFileTypeIcon(node.entry.path);
	const fileName = basename(node.entry.path);
	const parentDir = dirname(node.entry.path);
	const dirPath = parentDir === "." ? "" : parentDir;
	const status = node.entry.status;
	const disabled = !sectionIndexByKey.has(sectionKey);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
		type: "button",
		className: cn("group flex w-full min-w-0 cursor-pointer items-center gap-1 py-1 pr-3 text-left text-xs transition-colors hover:bg-accent/40 disabled:cursor-default disabled:opacity-50 disabled:hover:bg-transparent", activeSectionKey === sectionKey && "bg-accent/60"),
		style: { paddingLeft: `${node.depth * COMBINED_DIFF_TREE_INDENT_PX + COMBINED_DIFF_TREE_FILE_PADDING_PX}px` },
		disabled,
		draggable: !disabled,
		onDragStart: (event) => {
			if (disabled) {
				event.preventDefault();
				return;
			}
			event.dataTransfer.setData(WORKSPACE_FILE_PATH_MIME, joinPath(worktreePath, node.entry.path));
			event.dataTransfer.effectAllowed = "copy";
		},
		onClick: () => onNavigate(node.entry),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FileIcon, {
				className: "size-3.5 shrink-0",
				style: { color: STATUS_COLORS[status] }
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
				className: "min-w-0 flex-1 truncate",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-foreground",
					children: fileName
				}), dirPath && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "ml-1.5 text-[11px] text-muted-foreground",
					children: dirPath
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "w-4 shrink-0 text-center text-[10px] font-bold",
				style: { color: STATUS_COLORS[status] },
				children: STATUS_LABELS[status]
			})
		]
	});
}
function useCombinedDiffFileTreeResize(collapsed) {
	const storedWidth = useAppStore((s) => s.combinedDiffFileTreeWidth);
	const setStoredWidth = useAppStore((s) => s.setCombinedDiffFileTreeWidth);
	const [containerWidth, setContainerWidth] = (0, import_react.useState)(null);
	const { maxWidth, minWidth } = computeCombinedDiffFileTreeWidthBounds(containerWidth ?? 0);
	const width = clampCombinedDiffFileTreeWidth(storedWidth, containerWidth ?? void 0);
	const { containerRef, onResizeStart } = useSidebarResize({
		isOpen: !collapsed,
		width,
		minWidth,
		maxWidth,
		deltaSign: 1,
		setWidth: setStoredWidth
	});
	(0, import_react.useEffect)(() => {
		const container = containerRef.current?.parentElement;
		if (collapsed || !container) return;
		const measure = () => {
			setContainerWidth(container.clientWidth);
		};
		measure();
		const observer = new ResizeObserver(measure);
		observer.observe(container);
		return () => observer.disconnect();
	}, [collapsed, containerRef]);
	return {
		handleResizeKeyDown: (0, import_react.useCallback)((event) => {
			if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
			event.preventDefault();
			event.stopPropagation();
			setStoredWidth(clampCombinedDiffFileTreeWidth(width + (event.key === "ArrowLeft" ? -1 : 1) * (16 * (event.shiftKey ? 2 : 1)), containerWidth ?? void 0));
		}, [
			containerWidth,
			setStoredWidth,
			width
		]),
		handleResizeStart: onResizeStart,
		maxWidth,
		minWidth,
		treeRef: containerRef,
		width
	};
}
var UNCOMMITTED_AREA_ORDER = [
	"unstaged",
	"staged",
	"untracked"
];
var UNCOMMITTED_AREA_LABELS = {
	unstaged: "Changes",
	staged: "Staged Changes",
	untracked: "Untracked Files"
};
function buildUncommittedRows(entries, collapsedDirectoryKeys) {
	return UNCOMMITTED_AREA_ORDER.map((area) => {
		const areaEntries = entries.filter((entry) => isGitStatusEntry(entry) && entry.area === area);
		if (areaEntries.length === 0) return null;
		const roots = compactSourceControlTree(buildGitStatusSourceControlTree(area, areaEntries));
		return {
			area,
			label: UNCOMMITTED_AREA_LABELS[area],
			rows: flattenSourceControlTree(roots, collapsedDirectoryKeys)
		};
	}).filter((group) => Boolean(group));
}
function buildBranchRows(mode, entries, collapsedDirectoryKeys) {
	const branchEntries = entries.filter((entry) => !isGitStatusEntry(entry));
	return flattenSourceControlTree(compactSourceControlTree(buildSourceControlTree(mode === "commit" ? "combined-commit" : "combined-branch", branchEntries)), collapsedDirectoryKeys);
}
function CombinedDiffFileTree({ mode, worktreePath, entries, sectionIndexByKey, activeSectionKey, viewedSectionKeys, collapsed, onCollapsedChange, onNavigate }) {
	const [collapsedDirectoryKeys, setCollapsedDirectoryKeys] = import_react.useState(() => /* @__PURE__ */ new Set());
	const [query, setQuery] = import_react.useState("");
	const [excludedExtensions, setExcludedExtensions] = import_react.useState(() => /* @__PURE__ */ new Set());
	const [includeViewed, setIncludeViewed] = import_react.useState(true);
	const { handleResizeKeyDown, handleResizeStart, maxWidth, minWidth, treeRef, width } = useCombinedDiffFileTreeResize(collapsed);
	const toggleDirectory = import_react.useCallback((key) => {
		setCollapsedDirectoryKeys((prev) => {
			const next = new Set(prev);
			if (next.has(key)) next.delete(key);
			else next.add(key);
			return next;
		});
	}, []);
	const availableExtensions = import_react.useMemo(() => Array.from(new Set(entries.map(getEntryExtension))).sort(), [entries]);
	const filteredEntries = import_react.useMemo(() => getFilteredCombinedDiffFileTreeEntries({
		entries,
		mode,
		query,
		excludedExtensions,
		includeViewed,
		viewedSectionKeys
	}), [
		entries,
		excludedExtensions,
		includeViewed,
		mode,
		query,
		viewedSectionKeys
	]);
	const toggleExtension = import_react.useCallback((extension) => {
		setExcludedExtensions((prev) => {
			const next = new Set(prev);
			if (next.has(extension)) next.delete(extension);
			else next.add(extension);
			return next;
		});
	}, []);
	const resetFilters = import_react.useCallback(() => {
		setQuery("");
		setExcludedExtensions(/* @__PURE__ */ new Set());
		setIncludeViewed(true);
	}, []);
	const activeFilterCount = excludedExtensions.size + (includeViewed ? 0 : 1) + (query.trim().length > 0 ? 1 : 0);
	const uncommittedGroups = import_react.useMemo(() => mode === "all" || mode === "uncommitted" ? buildUncommittedRows(filteredEntries, collapsedDirectoryKeys) : [], [
		collapsedDirectoryKeys,
		filteredEntries,
		mode
	]);
	const branchRows = import_react.useMemo(() => mode === "all" || mode === "branch" || mode === "commit" ? buildBranchRows(mode, filteredEntries, collapsedDirectoryKeys) : [], [
		collapsedDirectoryKeys,
		filteredEntries,
		mode
	]);
	if (collapsed) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("aside", {
		ref: treeRef,
		className: "relative flex min-h-0 shrink-0 flex-col overflow-hidden border-r border-border bg-background",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "sticky top-0 z-20 shrink-0 bg-background",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center justify-between gap-2 border-b border-border px-3 py-1.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground",
						children: translate("auto.components.editor.CombinedDiffFileTree.481e63ca52", "Files")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "ghost",
						size: "icon-xs",
						"aria-label": translate("auto.components.editor.CombinedDiffFileTree.21783df79f", "Collapse file tree"),
						onClick: () => onCollapsedChange(true),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PanelLeftClose, { className: "size-3.5" })
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-2 border-b border-border px-2 py-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "relative min-w-0 flex-1",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Search, { className: "pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: query,
							onChange: (event) => setQuery(event.target.value),
							placeholder: translate("auto.components.editor.CombinedDiffFileTree.4cc7b83ffe", "Filter files..."),
							className: "h-8 pl-7 text-xs"
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "outline",
							size: "icon-sm",
							"aria-label": translate("auto.components.editor.CombinedDiffFileTree.cd0e0ed79e", "Filter diff files"),
							className: cn(activeFilterCount > 0 && "border-foreground/30 text-foreground"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Funnel, { className: "size-3.5" })
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(PopoverContent, {
						align: "end",
						side: "bottom",
						sideOffset: 6,
						className: "w-56 p-0",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "border-b border-border px-3 py-2 text-xs font-semibold text-foreground",
								children: translate("auto.components.editor.CombinedDiffFileTree.c00020f081", "File extensions")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "max-h-60 overflow-auto py-1 scrollbar-sleek",
								children: availableExtensions.map((extension) => {
									return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
										type: "button",
										className: "flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground",
										onClick: () => toggleExtension(extension),
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: cn("size-3.5 shrink-0", !excludedExtensions.has(extension) ? "opacity-100" : "opacity-0") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
											className: "min-w-0 flex-1 truncate",
											children: extension
										})]
									}, extension);
								})
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "border-t border-border py-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
									type: "button",
									className: "flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground",
									onClick: () => setIncludeViewed((prev) => !prev),
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: cn("size-3.5 shrink-0", includeViewed ? "opacity-100" : "opacity-0") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "min-w-0 flex-1 truncate",
										children: translate("auto.components.editor.CombinedDiffFileTree.be119cb9d1", "Viewed files")
									})]
								}), activeFilterCount > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
									type: "button",
									className: "w-full px-3 py-1.5 text-left text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
									onClick: resetFilters,
									children: translate("auto.components.editor.CombinedDiffFileTree.eafe1aeb53", "Reset filters")
								})]
							})
						]
					})] })]
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "min-h-0 flex-1 overflow-auto py-1 scrollbar-sleek",
				children: filteredEntries.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "px-3 py-6 text-center text-xs text-muted-foreground",
					children: translate("auto.components.editor.CombinedDiffFileTree.f984289373", "No files match the current filters.")
				}) : mode === "all" || mode === "uncommitted" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [uncommittedGroups.map((group) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "py-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "px-3 pb-1 text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground",
						children: group.label
					}), group.rows.map((node) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CombinedDiffFileTreeRow, {
						node,
						mode,
						worktreePath,
						activeSectionKey,
						sectionIndexByKey,
						isCollapsed: collapsedDirectoryKeys.has(node.key),
						onToggleDirectory: toggleDirectory,
						onNavigate
					}, node.key))]
				}, group.area)), mode === "all" && branchRows.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "py-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "px-3 pb-1 text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground",
						children: translate("auto.components.editor.CombinedDiffFileTree.39b6b9e4e4", "Committed on Branch")
					}), branchRows.map((node) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CombinedDiffFileTreeRow, {
						node,
						mode,
						worktreePath,
						activeSectionKey,
						sectionIndexByKey,
						isCollapsed: collapsedDirectoryKeys.has(node.key),
						onToggleDirectory: toggleDirectory,
						onNavigate
					}, node.key))]
				}) : null] }) : branchRows.map((node) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CombinedDiffFileTreeRow, {
					node,
					mode,
					worktreePath,
					activeSectionKey,
					sectionIndexByKey,
					isCollapsed: collapsedDirectoryKeys.has(node.key),
					onToggleDirectory: toggleDirectory,
					onNavigate
				}, node.key))
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				role: "separator",
				"aria-label": translate("auto.components.editor.CombinedDiffFileTree.resizeFileTree", "Resize file tree"),
				"aria-orientation": "vertical",
				"aria-valuemax": Math.round(maxWidth),
				"aria-valuemin": Math.round(minWidth),
				"aria-valuenow": Math.round(width),
				tabIndex: 0,
				className: "group absolute inset-y-0 right-0 z-30 w-1 cursor-col-resize outline-none focus-visible:ring-1 focus-visible:ring-ring",
				onMouseDown: handleResizeStart,
				onKeyDown: handleResizeKeyDown,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "ml-auto h-full w-px bg-transparent transition-colors group-hover:bg-ring/50 group-active:bg-ring group-focus-visible:bg-ring" })
			})
		]
	});
}
function shouldPruneLargeDiffContent(renderLimit) {
	return renderLimit?.limited === true;
}
function getStoredTextDiffResult(result, renderLimit) {
	if (result.kind !== "text" || !shouldPruneLargeDiffContent(renderLimit)) return result;
	return {
		...result,
		originalContent: "",
		modifiedContent: ""
	};
}
function getStoredTextDiffContent(result, renderLimit) {
	if (result.kind !== "text" || shouldPruneLargeDiffContent(renderLimit)) return {
		originalContent: "",
		modifiedContent: ""
	};
	return {
		originalContent: result.originalContent,
		modifiedContent: result.modifiedContent
	};
}
export { getCombinedDiffBranchEntriesInTreeOrder as a, DiffSectionItem as c, isIntrinsicHeightImageDiff as d, createCombinedDiffSectionIndexMap as i, removeDiffSectionMeasuredHeight as l, getStoredTextDiffResult as n, getCombinedDiffFileTreeSectionKey as o, CombinedDiffFileTree as r, handleCombinedDiffFileTreeNavigation as s, getStoredTextDiffContent as t, getDiffSectionEstimatedHeight as u };
