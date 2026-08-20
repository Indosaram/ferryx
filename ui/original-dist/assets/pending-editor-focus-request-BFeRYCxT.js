import { $a as isClipboardTextByteLengthOverLimit } from "./store-CgXrfmaH.js";
const MARKDOWN_DOC_COMPLETION_QUERY_MAX_BYTES = 2 * 1024;
function isMarkdownDocCompletionQueryTooLarge(query, maxBytes = MARKDOWN_DOC_COMPLETION_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function normalizeCompletionText(value) {
	return value.trim().replaceAll("\\", "/").toLowerCase();
}
function getMarkdownDocCompletionContext(linePrefix) {
	const start = linePrefix.lastIndexOf("[[");
	if (start === -1) return null;
	if (linePrefix.length - start - 2 > 2048) return null;
	const partial = linePrefix.slice(start + 2);
	if (isMarkdownDocCompletionQueryTooLarge(partial)) return null;
	if (partial.includes("[") || partial.includes("]") || partial.includes("|")) return null;
	return { partial };
}
function getMarkdownDocCompletionDocuments(documents, partial) {
	if (isMarkdownDocCompletionQueryTooLarge(partial)) return [];
	const normalizedPartial = normalizeCompletionText(partial);
	return documents.filter((document) => {
		if (!normalizedPartial) return true;
		return normalizeCompletionText(document.name).startsWith(normalizedPartial) || normalizeCompletionText(document.relativePath).startsWith(normalizedPartial);
	}).sort((a, b) => a.relativePath.localeCompare(b.relativePath));
}
function matchesPendingEditorFocusRequest(request, pane) {
	if (!request || pane.worktreeId === void 0 || pane.viewStateId === void 0) return false;
	return request.fileId === pane.fileId && request.worktreeId === pane.worktreeId && request.viewStateId === pane.viewStateId;
}
export { getMarkdownDocCompletionContext as n, getMarkdownDocCompletionDocuments as r, matchesPendingEditorFocusRequest as t };
