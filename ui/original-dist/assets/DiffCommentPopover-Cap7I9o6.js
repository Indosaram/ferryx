import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as CornerDownLeft } from "./corner-down-left-DMfCMf9j.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { o as installOpenDraftAddReviewNoteGuard } from "./editor-shortcuts-Cg6u73ie.js";
import { n as hasBoundedCommentBodyText, t as getCommentBodySubmitState } from "./comment-body-submit-state-BHQDrSxB.js";
const COMMENT_BODY_LINE_COUNT_SCAN_CODE_UNITS = 64 * 1024;
function getCommentBodyLayoutLineCount(body) {
	if (body.length === 0) return 1;
	let lineCount = 1;
	const scanLength = Math.min(body.length, COMMENT_BODY_LINE_COUNT_SCAN_CODE_UNITS);
	for (let index = 0; index < scanLength; index += 1) {
		if (body.charCodeAt(index) !== 10) continue;
		lineCount += 1;
		if (lineCount >= 80) return 80;
	}
	return lineCount;
}
var FALLBACK_LINE_HEIGHT_PX = 19;
function getDiffCommentPopoverTop(editor, lineNumber, lineHeight) {
	const model = editor.getModel();
	if (!model) return null;
	if (lineNumber < 1 || lineNumber > model.getLineCount()) return null;
	const resolvedLineHeight = typeof lineHeight === "number" && lineHeight > 0 ? lineHeight : FALLBACK_LINE_HEIGHT_PX;
	return editor.getTopForLineNumber(lineNumber) - editor.getScrollTop() + resolvedLineHeight;
}
var POPOVER_VIEWPORT_MARGIN_PX = 8;
function resolveDiffCommentPopoverTop({ belowTop, lineHeight, popoverHeight, viewportHeight, margin = POPOVER_VIEWPORT_MARGIN_PX }) {
	if (popoverHeight <= 0 || viewportHeight <= 0) return belowTop;
	if (belowTop + popoverHeight + margin <= viewportHeight) return belowTop;
	const aboveTop = belowTop - lineHeight - popoverHeight;
	if (aboveTop >= margin) return aboveTop;
	const maxTop = viewportHeight - popoverHeight - margin;
	return Math.max(margin, Math.min(belowTop, maxTop));
}
function getDiffCommentPopoverLeft(editor, offsetParent) {
	const editorDomNode = editor.getDomNode();
	if (!editorDomNode || !offsetParent) return null;
	const editorRect = editorDomNode.getBoundingClientRect();
	const parentRect = offsetParent.getBoundingClientRect();
	return Math.max(0, Math.round(editorRect.left - parentRect.left + editor.getLayoutInfo().contentLeft));
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function hasDraftText(body) {
	return /\S/u.test(body);
}
function DiffCommentPopover({ lineNumber, startLine, top, left, lineHeight = 0, title, placeholder = "Add note for the AI", submitLabel = "Add note", submittingLabel = "Saving…", onCancel, onSubmit }) {
	const [body, setBody] = (0, import_react.useState)("");
	const bodyRef = (0, import_react.useRef)(body);
	bodyRef.current = body;
	const [submitting, setSubmitting] = (0, import_react.useState)(false);
	const mountedRef = useMountedRef();
	const popoverRef = (0, import_react.useRef)(null);
	const onCancelRef = (0, import_react.useRef)(onCancel);
	onCancelRef.current = onCancel;
	const labelId = (0, import_react.useId)();
	const [resolvedTop, setResolvedTop] = (0, import_react.useState)(top);
	const topRef = (0, import_react.useRef)(top);
	topRef.current = top;
	const lineHeightRef = (0, import_react.useRef)(lineHeight);
	lineHeightRef.current = lineHeight;
	const measureResolvedTop = (0, import_react.useCallback)(() => {
		const popover = popoverRef.current;
		const container = popover?.parentElement;
		if (!popover || !container) {
			setResolvedTop(topRef.current);
			return;
		}
		setResolvedTop(resolveDiffCommentPopoverTop({
			belowTop: topRef.current,
			lineHeight: lineHeightRef.current,
			popoverHeight: popover.offsetHeight,
			viewportHeight: container.clientHeight
		}));
	}, []);
	(0, import_react.useLayoutEffect)(() => {
		measureResolvedTop();
	}, [
		top,
		lineHeight,
		measureResolvedTop
	]);
	(0, import_react.useEffect)(() => {
		const popover = popoverRef.current;
		const container = popover?.parentElement;
		if (!popover || !container || typeof ResizeObserver === "undefined") return;
		const observer = new ResizeObserver(() => measureResolvedTop());
		observer.observe(popover);
		observer.observe(container);
		return () => observer.disconnect();
	}, [measureResolvedTop]);
	const focusTextareaRef = (0, import_react.useCallback)((textarea) => {
		textarea?.focus();
	}, []);
	(0, import_react.useEffect)(() => {
		const popover = popoverRef.current;
		if (!popover) return;
		return installOpenDraftAddReviewNoteGuard(popover);
	}, []);
	(0, import_react.useEffect)(() => {
		const onDocumentMouseDown = (ev) => {
			if (!popoverRef.current) return;
			if (popoverRef.current.contains(ev.target)) return;
			if (hasDraftText(bodyRef.current)) return;
			onCancelRef.current();
		};
		document.addEventListener("mousedown", onDocumentMouseDown);
		return () => {
			document.removeEventListener("mousedown", onDocumentMouseDown);
		};
	}, []);
	const autoResize = (el) => {
		el.style.height = "auto";
		el.style.height = `${Math.min(el.scrollHeight, 240)}px`;
	};
	const handleSubmit = async () => {
		if (submitting) return;
		const bodyState = getCommentBodySubmitState(body);
		if (bodyState.status === "empty") return;
		if (bodyState.status === "too-large-leading-whitespace") {
			toast.error(translate("auto.components.diff.comments.DiffCommentPopover.commentTooLarge", "Comment is too large to submit safely."));
			return;
		}
		setSubmitting(true);
		try {
			await onSubmit(bodyState.body);
		} finally {
			if (mountedRef.current) setSubmitting(false);
		}
	};
	const canSubmitComment = hasBoundedCommentBodyText(body);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		ref: popoverRef,
		className: "orca-diff-comment-popover",
		style: {
			top: `${resolvedTop}px`,
			...left == null ? {} : { left: `${left}px` }
		},
		role: "dialog",
		"aria-modal": "true",
		"aria-labelledby": labelId,
		onMouseDown: (ev) => ev.stopPropagation(),
		onClick: (ev) => ev.stopPropagation(),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "orca-diff-comment-content-col",
			style: { gap: "8px" },
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					id: labelId,
					className: "orca-diff-comment-popover-label",
					children: title ?? (startLine && startLine !== lineNumber ? translate("auto.components.diff.comments.DiffCommentPopover.c845170b3b", "Lines {{value0}}-{{value1}}", {
						value0: startLine,
						value1: lineNumber
					}) : translate("auto.components.diff.comments.DiffCommentPopover.e05063cfc1", "Line {{value0}}", { value0: lineNumber }))
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("textarea", {
					ref: focusTextareaRef,
					className: "orca-diff-comment-popover-textarea",
					placeholder,
					value: body,
					onChange: (e) => {
						setBody(e.target.value);
						autoResize(e.currentTarget);
					},
					onKeyDown: (e) => {
						if (e.key === "Escape") {
							e.preventDefault();
							onCancel();
							return;
						}
						if (e.key === "Enter" && !e.nativeEvent.isComposing && !e.shiftKey) {
							e.preventDefault();
							if (submitting) return;
							handleSubmit();
						}
					},
					rows: 3
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "orca-diff-comment-popover-footer",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "ghost",
						size: "sm",
						onClick: onCancel,
						children: translate("auto.components.diff.comments.DiffCommentPopover.2b3ce6d394", "Cancel")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						size: "sm",
						onClick: handleSubmit,
						disabled: submitting || !canSubmitComment,
						children: [submitting ? submittingLabel : submitLabel, !submitting && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CornerDownLeft, { className: "ml-1 size-3 opacity-70" })]
					})]
				})
			]
		})
	});
}
export { getCommentBodyLayoutLineCount as i, getDiffCommentPopoverLeft as n, getDiffCommentPopoverTop as r, DiffCommentPopover as t };
