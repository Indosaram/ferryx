import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { r as TooltipProvider } from "./tooltip-DPmd1AoJ.js";
import { t as require_client } from "./client-XKKXQWGM.js";
import { c as Range, p as editor } from "./editor.api2-DX_-Ye6K.js";
import { i as getCommentBodyLayoutLineCount, r as getDiffCommentPopoverTop } from "./DiffCommentPopover-Cap7I9o6.js";
import { t as getDiffCommentLineLabel } from "./diff-comment-compat-CWwyL2nL.js";
import { n as formatDiffComments } from "./diff-comments-format-BnTGB-wh.js";
import { t as DiffCommentCard } from "./DiffCommentCard-DPwCt0gV.js";
import { t as NotesSendMenu } from "./NotesSendMenu-DLIuO9I1.js";
function installDiffCommentZoneMouseDownStopper(target) {
	const stopMouseDownPropagation = (ev) => ev.stopPropagation();
	target.addEventListener("mousedown", stopMouseDownPropagation);
	return () => target.removeEventListener("mousedown", stopMouseDownPropagation);
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_client = require_client();
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var ZONE_CHROME_PX = 68;
var ZONE_LINE_PX = 20;
var ZONE_MIN_PX = 88;
function getRenderSignature(comment, formatCommentPrompt) {
	return JSON.stringify({
		body: comment.body,
		sentAt: comment.sentAt ?? null,
		author: comment.author ?? null,
		authorAvatarUrl: comment.authorAvatarUrl ?? null,
		createdAtLabel: comment.createdAtLabel ?? null,
		url: comment.url ?? null,
		canDelete: comment.canDelete ?? null,
		canEdit: comment.canEdit ?? null,
		sendPrompt: formatCommentPrompt ? formatCommentPrompt(comment) : null
	});
}
function getSingleCommentSendScopes(comment, formatCommentPrompt) {
	return [{
		id: "note",
		label: translate("auto.components.diff.comments.useDiffCommentDecorator.995fa28b50", "This note"),
		notes: comment.sentAt ? [] : [comment],
		prompt: formatCommentPrompt ? formatCommentPrompt(comment) : formatDiffComments([comment])
	}];
}
function useDiffCommentDecorator({ editor: editor$1, monacoModelIdentity, filePath, worktreeId, comments, commentableLineNumbers, addButtonLabel = "Add note for the AI", onAddCommentClick, onDeleteComment, onUpdateComment, formatCommentPrompt, pendingScrollCommentId, onPendingScrollConsumed }) {
	const clearDeliveredDiffComments = useAppStore((s) => s.clearDeliveredDiffComments);
	const activeGroupId = useAppStore((s) => worktreeId ? s.activeGroupIdByWorktree[worktreeId] ?? worktreeId : worktreeId);
	const hoverLineRef = (0, import_react.useRef)(null);
	const zonesRef = (0, import_react.useRef)(/* @__PURE__ */ new Map());
	const disposablesRef = (0, import_react.useRef)([]);
	const pendingScrollRef = (0, import_react.useRef)(null);
	const scrollToZoneRef = (0, import_react.useRef)(null);
	const scrollToZoneFrameRef = (0, import_react.useRef)(null);
	const onAddCommentClickRef = (0, import_react.useRef)(onAddCommentClick);
	const onDeleteCommentRef = (0, import_react.useRef)(onDeleteComment);
	const onUpdateCommentRef = (0, import_react.useRef)(onUpdateComment);
	const onPendingScrollConsumedRef = (0, import_react.useRef)(onPendingScrollConsumed);
	onAddCommentClickRef.current = onAddCommentClick;
	onDeleteCommentRef.current = onDeleteComment;
	onUpdateCommentRef.current = onUpdateComment;
	onPendingScrollConsumedRef.current = onPendingScrollConsumed;
	const cancelScrollToZoneFrame = (0, import_react.useCallback)(() => {
		if (scrollToZoneFrameRef.current === null) return;
		cancelAnimationFrame(scrollToZoneFrameRef.current);
		scrollToZoneFrameRef.current = null;
	}, []);
	const commentableLineSet = (0, import_react.useMemo)(() => commentableLineNumbers ? new Set(commentableLineNumbers) : null, [commentableLineNumbers]);
	(0, import_react.useEffect)(() => {
		if (!editor$1) return;
		const editorDomNode = editor$1.getDomNode();
		if (!editorDomNode) return;
		const zones = zonesRef.current;
		const plus = document.createElement("button");
		plus.type = "button";
		plus.className = "orca-diff-comment-add-btn";
		plus.title = addButtonLabel;
		plus.setAttribute("aria-label", addButtonLabel);
		plus.innerHTML = "<svg viewBox=\"0 0 16 16\" width=\"12\" height=\"12\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\"><path d=\"M8 3v10M3 8h10\"/></svg>";
		plus.style.display = "none";
		editorDomNode.appendChild(plus);
		const getLineHeight = () => {
			const h = editor$1.getOption(editor.EditorOption.lineHeight);
			return typeof h === "number" && h > 0 ? h : 19;
		};
		let lastTop = null;
		let lastDisplay = null;
		const setDisplay = (value) => {
			if (lastDisplay === value) return;
			plus.style.display = value;
			lastDisplay = value;
		};
		const BUTTON_SIZE = 18;
		let rangeDecorationIds = [];
		let dragState = null;
		const clearRangeDecoration = () => {
			if (rangeDecorationIds.length > 0) rangeDecorationIds = editor$1.deltaDecorations(rangeDecorationIds, []);
		};
		const updateRangeDecoration = (startLine, endLine) => {
			const from = Math.min(startLine, endLine);
			const to = Math.max(startLine, endLine);
			rangeDecorationIds = editor$1.deltaDecorations(rangeDecorationIds, [{
				range: new Range(from, 1, to, 1),
				options: {
					isWholeLine: true,
					className: "orca-diff-comment-range-highlight"
				}
			}]);
		};
		const getLineAtClientPoint = (clientX, clientY) => {
			return editor$1.getTargetAtClientPoint(clientX, clientY)?.position?.lineNumber ?? null;
		};
		const canCommentOnLine = (lineNumber) => {
			return commentableLineSet === null || commentableLineSet.has(lineNumber);
		};
		const canCommentOnRange = (startLine, endLine) => {
			if (commentableLineSet === null) return true;
			const from = Math.min(startLine, endLine);
			const to = Math.max(startLine, endLine);
			for (let line = from; line <= to; line++) if (!commentableLineSet.has(line)) return false;
			return true;
		};
		const positionAtLine = (lineNumber) => {
			const lineTop = editor$1.getTopForLineNumber(lineNumber) - editor$1.getScrollTop();
			const top = Math.round(lineTop + (getLineHeight() - BUTTON_SIZE) / 2);
			if (top !== lastTop) {
				plus.style.top = `${top}px`;
				lastTop = top;
			}
			setDisplay("flex");
		};
		const finishRangeDrag = (ev) => {
			ev.preventDefault();
			ev.stopPropagation();
			document.removeEventListener("mousemove", handleRangeDragMove);
			document.removeEventListener("mouseup", finishRangeDrag);
			const currentDrag = dragState;
			dragState = null;
			clearRangeDecoration();
			if (!currentDrag) return;
			if (!canCommentOnRange(currentDrag.startLine, currentDrag.endLine)) return;
			const startLine = Math.min(currentDrag.startLine, currentDrag.endLine);
			const lineNumber = Math.max(currentDrag.startLine, currentDrag.endLine);
			const top = getDiffCommentPopoverTop(editor$1, lineNumber, getLineHeight());
			if (top == null) return;
			onAddCommentClickRef.current({
				lineNumber,
				startLine: startLine === lineNumber ? void 0 : startLine,
				top
			});
		};
		const handleRangeDragMove = (ev) => {
			if (!dragState) return;
			const line = getLineAtClientPoint(ev.clientX, ev.clientY);
			if (line == null || line === dragState.endLine || !canCommentOnLine(line) || !canCommentOnRange(dragState.startLine, line)) return;
			dragState = {
				...dragState,
				endLine: line
			};
			updateRangeDecoration(dragState.startLine, line);
		};
		const handleMouseDown = (ev) => {
			ev.preventDefault();
			ev.stopPropagation();
			const line = hoverLineRef.current;
			if (line == null || !canCommentOnLine(line)) return;
			dragState = {
				startLine: line,
				endLine: line
			};
			updateRangeDecoration(line, line);
			document.addEventListener("mousemove", handleRangeDragMove);
			document.addEventListener("mouseup", finishRangeDrag);
		};
		plus.addEventListener("mousedown", handleMouseDown);
		disposablesRef.current = [
			editor$1.onMouseMove((e) => {
				const srcEvent = e.event?.browserEvent;
				if (srcEvent && plus.contains(srcEvent.target)) return;
				const ln = e.target.position?.lineNumber ?? null;
				if (ln == null || !canCommentOnLine(ln)) {
					hoverLineRef.current = null;
					setDisplay("none");
					return;
				}
				hoverLineRef.current = ln;
				positionAtLine(ln);
			}),
			editor$1.onMouseLeave(() => {
				setDisplay("none");
			}),
			editor$1.onDidScrollChange(() => {
				if (hoverLineRef.current != null) positionAtLine(hoverLineRef.current);
			})
		];
		return () => {
			for (const d of disposablesRef.current) d.dispose();
			disposablesRef.current = [];
			document.removeEventListener("mousemove", handleRangeDragMove);
			document.removeEventListener("mouseup", finishRangeDrag);
			clearRangeDecoration();
			plus.removeEventListener("mousedown", handleMouseDown);
			plus.remove();
			const rootsToUnmount = Array.from(zones.values(), (z) => {
				z.disposeMouseDownStopper();
				return z.root;
			});
			zones.clear();
			if (rootsToUnmount.length > 0) queueMicrotask(() => {
				for (const root of rootsToUnmount) root.unmount();
			});
			cancelScrollToZoneFrame();
			pendingScrollRef.current = null;
			scrollToZoneRef.current = null;
		};
	}, [
		addButtonLabel,
		cancelScrollToZoneFrame,
		commentableLineSet,
		editor$1,
		monacoModelIdentity
	]);
	(0, import_react.useEffect)(() => {
		if (!editor$1) return;
		const relevant = comments.filter((c) => c.filePath === filePath && c.worktreeId === worktreeId);
		const relevantMap = new Map(relevant.map((c) => [c.id, c]));
		const zones = zonesRef.current;
		const rootsToUnmount = [];
		const resizeZone = (commentId) => {
			const entry = zones.get(commentId);
			if (!entry) return;
			const child = entry.domNode.firstElementChild;
			const wrapperStyle = window.getComputedStyle(entry.domNode);
			const verticalPadding = Number.parseFloat(wrapperStyle.paddingTop) + Number.parseFloat(wrapperStyle.paddingBottom);
			const childHeight = child?.getBoundingClientRect().height ?? 0;
			if (childHeight <= 0) return;
			const measured = Math.ceil(childHeight + verticalPadding);
			if (entry.delegate.heightInPx === measured) return;
			entry.delegate.heightInPx = measured;
			editor$1.changeViewZones((acc) => {
				acc.layoutZone(entry.zoneId);
			});
		};
		const scrollToZone = (commentId) => {
			cancelScrollToZoneFrame();
			scrollToZoneFrameRef.current = requestAnimationFrame(() => {
				scrollToZoneFrameRef.current = null;
				const entry = zones.get(commentId);
				if (!entry || !editor$1.getModel()) return;
				if (pendingScrollRef.current !== commentId) return;
				const top = editor$1.getTopForLineNumber(entry.delegate.afterLineNumber, true);
				const editorHeight = editor$1.getLayoutInfo().height;
				editor$1.setScrollTop(Math.max(0, top - editorHeight / 2));
				pendingScrollRef.current = null;
				onPendingScrollConsumedRef.current?.();
			});
		};
		scrollToZoneRef.current = scrollToZone;
		const renderCard = (root, comment) => {
			root.render(/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipProvider, {
				delayDuration: 400,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DiffCommentCard, {
					lineNumber: comment.lineNumber,
					startLine: comment.startLine,
					label: comment.author ? getDiffCommentLineLabel(comment).toLowerCase() : void 0,
					body: comment.body,
					sentAt: comment.sentAt,
					author: comment.author,
					createdAtLabel: comment.createdAtLabel,
					url: comment.url,
					onDelete: comment.canDelete === false ? void 0 : () => onDeleteCommentRef.current(comment.id),
					onSubmitEdit: onUpdateCommentRef.current && comment.canEdit !== false ? async (body) => {
						const fn = onUpdateCommentRef.current;
						if (!fn) return false;
						return fn(comment.id, body);
					} : void 0,
					onContentResize: () => resizeZone(comment.id),
					observeRenderedSize: true,
					headerActions: worktreeId && comment.author === void 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotesSendMenu, {
						worktreeId,
						groupId: activeGroupId,
						modeIdParts: [
							"diff-comment-note",
							worktreeId,
							filePath,
							comment.id
						],
						scopes: getSingleCommentSendScopes(comment, formatCommentPrompt),
						targetModeLabel: "This note",
						triggerClassName: "orca-diff-comment-edit",
						disabledTooltip: "Note already sent",
						onDelivered: (notes) => void clearDeliveredDiffComments(worktreeId, notes)
					}) : null
				})
			}));
		};
		editor$1.changeViewZones((accessor) => {
			for (const [commentId, entry] of zones) if (!relevantMap.has(commentId)) {
				accessor.removeZone(entry.zoneId);
				entry.disposeMouseDownStopper();
				rootsToUnmount.push(entry.root);
				zones.delete(commentId);
				if (pendingScrollRef.current === commentId) pendingScrollRef.current = null;
			}
			for (const c of relevant) {
				if (zones.has(c.id)) continue;
				const dom = document.createElement("div");
				dom.className = "orca-diff-comment-inline";
				const disposeMouseDownStopper = installDiffCommentZoneMouseDownStopper(dom);
				const root = (0, import_client.createRoot)(dom);
				const lineCount = getCommentBodyLayoutLineCount(c.body);
				const heightInPx = Math.max(ZONE_MIN_PX, ZONE_CHROME_PX + lineCount * ZONE_LINE_PX);
				const commentId = c.id;
				const delegate = {
					afterLineNumber: c.lineNumber,
					heightInPx,
					domNode: dom,
					suppressMouseDown: false,
					onDomNodeTop: () => {
						const entry = zones.get(commentId);
						if (!entry) return;
						const wasLaidOut = entry.laidOut;
						entry.laidOut = true;
						if (!wasLaidOut && pendingScrollRef.current === commentId) scrollToZone(commentId);
					}
				};
				const zoneId = accessor.addZone(delegate);
				zones.set(c.id, {
					zoneId,
					domNode: dom,
					delegate,
					root,
					disposeMouseDownStopper,
					lastRenderSignature: getRenderSignature(c, formatCommentPrompt),
					laidOut: false
				});
				renderCard(root, c);
			}
			for (const c of relevant) {
				const entry = zones.get(c.id);
				if (!entry) continue;
				const renderSignature = getRenderSignature(c, formatCommentPrompt);
				if (entry.lastRenderSignature === renderSignature) continue;
				entry.lastRenderSignature = renderSignature;
				renderCard(entry.root, c);
			}
		});
		if (rootsToUnmount.length > 0) queueMicrotask(() => {
			for (const root of rootsToUnmount) root.unmount();
		});
	}, [
		activeGroupId,
		cancelScrollToZoneFrame,
		clearDeliveredDiffComments,
		editor$1,
		filePath,
		formatCommentPrompt,
		monacoModelIdentity,
		worktreeId,
		comments
	]);
	(0, import_react.useEffect)(() => {
		if (!editor$1) return;
		if (!pendingScrollCommentId) {
			cancelScrollToZoneFrame();
			pendingScrollRef.current = null;
			return;
		}
		if (!comments.find((c) => c.id === pendingScrollCommentId && c.filePath === filePath && c.worktreeId === worktreeId)) {
			cancelScrollToZoneFrame();
			pendingScrollRef.current = null;
			return;
		}
		pendingScrollRef.current = pendingScrollCommentId;
		if (zonesRef.current.get(pendingScrollCommentId)?.laidOut) scrollToZoneRef.current?.(pendingScrollCommentId);
	}, [
		cancelScrollToZoneFrame,
		editor$1,
		comments,
		pendingScrollCommentId,
		filePath,
		monacoModelIdentity,
		worktreeId
	]);
}
const monacoFindOptions = {
	addExtraSpaceOnTop: false,
	autoFindInSelection: "never",
	seedSearchStringFromSelection: "selection"
};
export { useDiffCommentDecorator as n, monacoFindOptions as t };
