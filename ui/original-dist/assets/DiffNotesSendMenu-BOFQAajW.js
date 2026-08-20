import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { n as formatDiffComments } from "./diff-comments-format-BnTGB-wh.js";
import { t as NotesSendMenu } from "./NotesSendMenu-DLIuO9I1.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var OPEN_REQUEST_TTL_MS = 5e3;
function DiffNotesSendMenu({ worktreeId, groupId, comments, filePath, showFileScope = false, triggerClassName, triggerLabel, triggerCount, actionLabel, iconClassName = "size-3.5", align = "end", respondToOpenRequest = false }) {
	const clearDeliveredDiffComments = useAppStore((s) => s.clearDeliveredDiffComments);
	const openRequest = useAppStore((s) => s.diffNotesSendMenuOpenRequest);
	const consumeOpenRequest = useAppStore((s) => s.consumeDiffNotesSendMenuOpenRequest);
	const openRequestNonce = respondToOpenRequest && openRequest?.worktreeId === worktreeId && Date.now() - openRequest.issuedAt < OPEN_REQUEST_TTL_MS ? openRequest.nonce : null;
	const handleOpenRequestHandled = (0, import_react.useCallback)(() => consumeOpenRequest(worktreeId), [consumeOpenRequest, worktreeId]);
	const unsentNotes = (0, import_react.useMemo)(() => comments.filter((comment) => !comment.sentAt), [comments]);
	const unsentPrompt = (0, import_react.useMemo)(() => formatDiffComments(unsentNotes), [unsentNotes]);
	const fileNotes = (0, import_react.useMemo)(() => filePath ? comments.filter((comment) => comment.filePath === filePath) : [], [comments, filePath]);
	const unsentFileNotes = (0, import_react.useMemo)(() => fileNotes.filter((comment) => !comment.sentAt), [fileNotes]);
	const unsentFilePrompt = (0, import_react.useMemo)(() => formatDiffComments(unsentFileNotes), [unsentFileNotes]);
	const canSendFileScope = showFileScope && Boolean(filePath);
	const scopes = (0, import_react.useMemo)(() => {
		const allNotesScope = {
			id: "all",
			label: translate("auto.components.editor.DiffNotesSendMenu.8b87612461", "All unsent notes"),
			notes: unsentNotes,
			prompt: unsentPrompt
		};
		if (!canSendFileScope) return [allNotesScope];
		return [{
			id: "file",
			label: translate("auto.components.editor.DiffNotesSendMenu.f1aa04b5cf", "This file"),
			notes: unsentFileNotes,
			prompt: unsentFilePrompt
		}, allNotesScope];
	}, [
		canSendFileScope,
		unsentFileNotes,
		unsentFilePrompt,
		unsentNotes,
		unsentPrompt
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotesSendMenu, {
		worktreeId,
		groupId,
		modeIdParts: [
			"diff-notes",
			worktreeId,
			groupId,
			filePath ?? "all"
		],
		scopes,
		defaultScopeId: canSendFileScope ? "file" : "all",
		triggerClassName,
		triggerLabel,
		triggerCount,
		actionLabel,
		iconClassName,
		align,
		openRequestNonce,
		onOpenRequestHandled: handleOpenRequestHandled,
		onDelivered: (notes) => void clearDeliveredDiffComments(worktreeId, notes)
	});
}
export { DiffNotesSendMenu as t };
