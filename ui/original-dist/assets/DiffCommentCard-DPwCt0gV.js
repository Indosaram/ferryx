import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as CornerDownLeft } from "./corner-down-left-DMfCMf9j.js";
import { t as Pencil } from "./pencil-CLc9a5do.js";
import { t as Trash } from "./trash-BEusqY6o.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { t as getDiffCommentLineLabel } from "./diff-comment-compat-CWwyL2nL.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function DiffCommentCard({ lineNumber, startLine, label, quote, body, sentAt, author, createdAtLabel, url, onDelete, onContentResize, observeRenderedSize, onSubmitEdit, headerActions }) {
	const [editing, setEditing] = (0, import_react.useState)(false);
	const [draft, setDraft] = (0, import_react.useState)(body);
	const [submitting, setSubmitting] = (0, import_react.useState)(false);
	const mountedRef = useMountedRef();
	const cardRef = (0, import_react.useRef)(null);
	const textareaRef = (0, import_react.useRef)(null);
	const resizeAfterCloseRef = (0, import_react.useRef)(false);
	const observesRenderedSize = observeRenderedSize === true && onContentResize !== void 0;
	const onContentResizeRef = (0, import_react.useRef)(onContentResize);
	onContentResizeRef.current = onContentResize;
	(0, import_react.useLayoutEffect)(() => {
		const card = cardRef.current;
		if (!card || !observesRenderedSize) return;
		onContentResizeRef.current?.();
		let frameId = null;
		const notifyResize = () => {
			if (frameId !== null) return;
			frameId = requestAnimationFrame(() => {
				frameId = null;
				onContentResizeRef.current?.();
			});
		};
		if (typeof ResizeObserver === "undefined") return () => {
			if (frameId !== null) cancelAnimationFrame(frameId);
		};
		const observer = new ResizeObserver(() => notifyResize());
		observer.observe(card);
		return () => {
			observer.disconnect();
			if (frameId !== null) cancelAnimationFrame(frameId);
		};
	}, [observesRenderedSize]);
	(0, import_react.useLayoutEffect)(() => {
		if (!editing) {
			if (resizeAfterCloseRef.current) {
				resizeAfterCloseRef.current = false;
				onContentResizeRef.current?.();
			}
			return;
		}
		const el = textareaRef.current;
		if (!el) return;
		el.style.height = "auto";
		el.style.height = `${Math.min(el.scrollHeight, 240)}px`;
		el.focus();
		el.setSelectionRange(el.value.length, el.value.length);
		onContentResizeRef.current?.();
	}, [editing]);
	const scheduleContentResizeAfterClose = () => {
		resizeAfterCloseRef.current = true;
	};
	const handleStartEdit = () => {
		setDraft(body);
		setEditing(true);
	};
	const handleCancel = () => {
		scheduleContentResizeAfterClose();
		setEditing(false);
		setDraft(body);
	};
	const trimmedDraft = draft.trim();
	const canSubmit = !submitting && trimmedDraft.length > 0 && trimmedDraft !== body;
	const lineLabel = label === void 0 ? getDiffCommentLineLabel({
		lineNumber,
		startLine
	}).toLowerCase() : label;
	const metaText = [
		author || "Note",
		lineLabel,
		createdAtLabel || (sentAt ? "sent" : null)
	].filter(Boolean).join(" ");
	const handleSubmit = async () => {
		if (!canSubmit || !onSubmitEdit) return;
		setSubmitting(true);
		try {
			if (await onSubmitEdit(trimmedDraft) && mountedRef.current) {
				scheduleContentResizeAfterClose();
				setEditing(false);
			}
		} catch (err) {
			console.error("Failed to submit diff comment edit:", err);
		} finally {
			if (mountedRef.current) setSubmitting(false);
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		ref: cardRef,
		className: "orca-diff-comment-card",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "orca-diff-comment-content-col",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "orca-diff-comment-header",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "orca-diff-comment-meta-group",
						children: metaText
					}), !editing && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "orca-diff-comment-actions-pill",
						onMouseDown: (ev) => ev.stopPropagation(),
						children: [
							headerActions,
							headerActions && (url || onSubmitEdit || onDelete) && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "orca-diff-comment-pill-divider" }),
							url && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								className: "orca-diff-comment-pill-btn",
								title: translate("auto.components.diff.comments.DiffCommentCard.508ee678a5", "Open in browser"),
								"aria-label": translate("auto.components.diff.comments.DiffCommentCard.508ee678a5", "Open in browser"),
								onClick: (ev) => {
									ev.preventDefault();
									ev.stopPropagation();
									window.api.shell.openUrl(url);
								},
								children: translate("auto.components.diff.comments.DiffCommentCard.6978871a3d", "Open")
							}), (onSubmitEdit || onDelete) && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "orca-diff-comment-pill-divider" })] }),
							onSubmitEdit && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								className: "orca-diff-comment-pill-btn",
								title: translate("auto.components.diff.comments.DiffCommentCard.cad3384faa", "Edit note"),
								"aria-label": translate("auto.components.diff.comments.DiffCommentCard.cad3384faa", "Edit note"),
								onClick: (ev) => {
									ev.preventDefault();
									ev.stopPropagation();
									handleStartEdit();
								},
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pencil, { className: "size-3" })
							}), onDelete && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "orca-diff-comment-pill-divider" })] }),
							onDelete && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								className: "orca-diff-comment-pill-btn orca-diff-comment-pill-btn-danger",
								title: translate("auto.components.diff.comments.DiffCommentCard.cce596969e", "Delete note"),
								"aria-label": translate("auto.components.diff.comments.DiffCommentCard.cce596969e", "Delete note"),
								onClick: (ev) => {
									ev.preventDefault();
									ev.stopPropagation();
									onDelete();
								},
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash, { className: "size-3" })
							})
						]
					})]
				}),
				quote ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "orca-diff-comment-quote",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "orca-diff-comment-quote-text",
						children: quote
					})
				}) : null,
				editing ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-col gap-2 mt-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("textarea", {
						ref: textareaRef,
						className: "orca-diff-comment-popover-textarea",
						value: draft,
						onChange: (e) => {
							setDraft(e.target.value);
							const el = e.currentTarget;
							el.style.height = "auto";
							el.style.height = `${Math.min(el.scrollHeight, 240)}px`;
							onContentResizeRef.current?.();
						},
						onKeyDown: (e) => {
							if (e.key === "Escape") {
								e.preventDefault();
								handleCancel();
								return;
							}
							if (e.key === "Enter" && !e.nativeEvent.isComposing && !e.shiftKey) {
								e.preventDefault();
								if (!canSubmit) return;
								handleSubmit();
							}
						},
						rows: 3
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "orca-diff-comment-popover-footer",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "ghost",
							size: "sm",
							onClick: handleCancel,
							disabled: submitting,
							children: translate("auto.components.diff.comments.DiffCommentCard.0203bed775", "Cancel")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							size: "sm",
							onClick: () => void handleSubmit(),
							disabled: !canSubmit,
							title: submitting ? translate("auto.components.diff.comments.DiffCommentCard.bb0a55f856", "Saving…") : void 0,
							children: [translate("auto.components.diff.comments.DiffCommentCard.109a791e7b", "Save"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CornerDownLeft, { className: "ml-1 size-3 opacity-70" })]
						})]
					})]
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "orca-diff-comment-body",
					children: body
				})
			]
		})
	});
}
export { DiffCommentCard as t };
