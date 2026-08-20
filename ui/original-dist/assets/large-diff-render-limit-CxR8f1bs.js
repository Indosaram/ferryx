import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
function buildDiffEditorLineNumberOptions(sideBySide) {
	return {
		original: sideBySide ? "on" : "off",
		modified: "on"
	};
}
function applyDiffEditorLineNumberOptions(diffEditor, sideBySide) {
	const lineNumberOptions = buildDiffEditorLineNumberOptions(sideBySide);
	const originalEditor = diffEditor.getOriginalEditor();
	const modifiedEditor = diffEditor.getModifiedEditor();
	const reapplyIfNeeded = () => {
		if (originalEditor.getRawOptions().lineNumbers !== lineNumberOptions.original) originalEditor.updateOptions({ lineNumbers: lineNumberOptions.original });
		if (modifiedEditor.getRawOptions().lineNumbers !== lineNumberOptions.modified) modifiedEditor.updateOptions({ lineNumbers: lineNumberOptions.modified });
	};
	reapplyIfNeeded();
	const originalOptionsSub = originalEditor.onDidChangeConfiguration(reapplyIfNeeded);
	const modifiedOptionsSub = modifiedEditor.onDidChangeConfiguration(reapplyIfNeeded);
	return { dispose: () => {
		originalOptionsSub.dispose();
		modifiedOptionsSub.dispose();
	} };
}
var DIFF_EDITOR_SCROLLBAR_SIZE = 20;
const diffEditorScrollbarOptions = {
	verticalScrollbarSize: DIFF_EDITOR_SCROLLBAR_SIZE,
	horizontalScrollbarSize: DIFF_EDITOR_SCROLLBAR_SIZE,
	verticalSliderSize: DIFF_EDITOR_SCROLLBAR_SIZE,
	horizontalSliderSize: DIFF_EDITOR_SCROLLBAR_SIZE
};
const combinedDiffSectionScrollbarOptions = {
	...diffEditorScrollbarOptions,
	vertical: "hidden",
	handleMouseWheel: false
};
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var numberFormatter = new Intl.NumberFormat();
function formatCount(value) {
	return numberFormatter.format(value);
}
function formatLineCount(renderLimit, side) {
	if (!renderLimit.lineCounts) return translate("auto.components.editor.LargeDiffFallback.7944ed9fb8", "Not counted");
	const suffix = renderLimit.lineCountsAreMinimum?.[side] ? "+" : "";
	return `${formatCount(renderLimit.lineCounts[side])}${suffix}`;
}
function LargeDiffFallback({ filePath, renderLimit, action }) {
	const reason = renderLimit.reason === "line-count" ? translate("auto.components.editor.LargeDiffFallback.a3c74f8a21", "line count exceeds the safe display limit") : translate("auto.components.editor.LargeDiffFallback.fd92fbde46", "character count exceeds the safe display limit");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		"data-testid": "large-diff-fallback",
		className: "flex h-full min-h-[120px] items-center justify-center border border-border bg-muted/10 px-4 py-6 text-muted-foreground",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "max-w-xl space-y-3 text-center",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "text-sm font-medium text-foreground",
					children: translate("auto.components.editor.LargeDiffFallback.7d424bb761", "This diff is too large to display safely.")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "break-all text-xs",
					children: filePath
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "grid gap-1 text-xs sm:grid-cols-2 sm:text-left",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
							translate("auto.components.editor.LargeDiffFallback.28aa2cc90b", "Original lines"),
							":",
							" ",
							formatLineCount(renderLimit, "original")
						] }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
							translate("auto.components.editor.LargeDiffFallback.20857938dd", "Modified lines"),
							":",
							" ",
							formatLineCount(renderLimit, "modified")
						] }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
							translate("auto.components.editor.LargeDiffFallback.e5f0d2182e", "Characters"),
							":",
							" ",
							formatCount(renderLimit.characterCount)
						] }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
							translate("auto.components.editor.LargeDiffFallback.877c25a02f", "Reason"),
							": ",
							reason
						] })
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "text-[11px]",
					children: [
						translate("auto.components.editor.LargeDiffFallback.5fca073b72", "Limits"),
						":",
						" ",
						formatCount(renderLimit.limits.maxLinesPerSide),
						" ",
						translate("auto.components.editor.LargeDiffFallback.f1d136a163", "lines per side"),
						" ·",
						" ",
						formatCount(renderLimit.limits.maxCombinedCharacters),
						" ",
						translate("auto.components.editor.LargeDiffFallback.23433fcdea", "combined characters")
					]
				}),
				action ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-2",
					children: [action.description ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-[11px]",
						children: action.description
					}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "secondary",
						size: "xs",
						onClick: (event) => {
							event.stopPropagation();
							action.onClick();
						},
						children: action.label
					})]
				}) : null
			]
		})
	});
}
function buildDiffEditorWordWrapOptions(diffWordWrap) {
	return { wordWrap: diffWordWrap === true ? "on" : "off" };
}
const MAX_RENDERED_DIFF_LINES_PER_SIDE = 12e4;
const MAX_RENDERED_DIFF_COMBINED_CHARACTERS = 6e6;
function countLinesEmptyAsZeroUpToLimit(content, maxLines) {
	if (content.length === 0) return {
		count: 0,
		exceeded: false
	};
	let lineCount = 1;
	for (let index = 0; index < content.length; index += 1) {
		if (content.charCodeAt(index) !== 10) continue;
		lineCount += 1;
		if (lineCount > maxLines) return {
			count: lineCount,
			exceeded: true
		};
	}
	return {
		count: lineCount,
		exceeded: false
	};
}
function countLinesLikeSplit(content) {
	let lineCount = 1;
	for (let index = 0; index < content.length; index += 1) if (content.charCodeAt(index) === 10) lineCount += 1;
	return lineCount;
}
function getLargeDiffRenderLimitFromCounts({ originalLineCount, modifiedLineCount, originalCharacterCount, modifiedCharacterCount }) {
	const lineCounts = {
		original: originalLineCount,
		modified: modifiedLineCount
	};
	const characterCount = originalCharacterCount + modifiedCharacterCount;
	const limits = {
		maxLinesPerSide: MAX_RENDERED_DIFF_LINES_PER_SIDE,
		maxCombinedCharacters: MAX_RENDERED_DIFF_COMBINED_CHARACTERS
	};
	if (lineCounts.original > 12e4 || lineCounts.modified > 12e4) return {
		limited: true,
		reason: "line-count",
		lineCounts,
		characterCount,
		limits
	};
	if (characterCount > 6e6) return {
		limited: true,
		reason: "character-count",
		lineCounts,
		characterCount,
		limits
	};
	return {
		limited: false,
		lineCounts,
		characterCount
	};
}
function getLargeDiffRenderLimit({ originalContent, modifiedContent }) {
	const characterCount = originalContent.length + modifiedContent.length;
	const limits = {
		maxLinesPerSide: MAX_RENDERED_DIFF_LINES_PER_SIDE,
		maxCombinedCharacters: MAX_RENDERED_DIFF_COMBINED_CHARACTERS
	};
	if (characterCount > 6e6) return {
		limited: true,
		reason: "character-count",
		lineCounts: null,
		characterCount,
		limits
	};
	const originalLineCount = countLinesEmptyAsZeroUpToLimit(originalContent, MAX_RENDERED_DIFF_LINES_PER_SIDE);
	const modifiedLineCount = countLinesEmptyAsZeroUpToLimit(modifiedContent, MAX_RENDERED_DIFF_LINES_PER_SIDE);
	if (originalLineCount.exceeded || modifiedLineCount.exceeded) return {
		limited: true,
		reason: "line-count",
		lineCounts: {
			original: originalLineCount.count,
			modified: modifiedLineCount.count
		},
		lineCountsAreMinimum: {
			original: originalLineCount.exceeded,
			modified: modifiedLineCount.exceeded
		},
		characterCount,
		limits
	};
	return {
		limited: false,
		lineCounts: {
			original: originalLineCount.count,
			modified: modifiedLineCount.count
		},
		characterCount
	};
}
export { getLargeDiffRenderLimitFromCounts as a, combinedDiffSectionScrollbarOptions as c, getLargeDiffRenderLimit as i, diffEditorScrollbarOptions as l, MAX_RENDERED_DIFF_LINES_PER_SIDE as n, buildDiffEditorWordWrapOptions as o, countLinesLikeSplit as r, LargeDiffFallback as s, MAX_RENDERED_DIFF_COMBINED_CHARACTERS as t, applyDiffEditorLineNumberOptions as u };
