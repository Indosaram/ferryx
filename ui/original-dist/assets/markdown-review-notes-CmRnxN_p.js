import { t as getDiffCommentLineLabel } from "./diff-comment-compat-CWwyL2nL.js";
var MAX_EXCERPT_LINES = 8;
var MAX_CARD_QUOTE_LENGTH = 60;
function sortMarkdownReviewNotes(notes) {
	return [...notes].sort((a, b) => {
		const pathCompare = a.filePath.localeCompare(b.filePath);
		if (pathCompare !== 0) return pathCompare;
		const startA = a.startLine ?? a.lineNumber;
		const startB = b.startLine ?? b.lineNumber;
		if (startA !== startB) return startA - startB;
		if (a.lineNumber !== b.lineNumber) return a.lineNumber - b.lineNumber;
		return a.createdAt - b.createdAt;
	});
}
function getMarkdownReviewExcerpt(content, note) {
	const startLine = Math.max(1, note.startLine ?? note.lineNumber);
	const selected = getMarkdownReviewSelectedLines(content, startLine, Math.max(startLine, note.lineNumber));
	if (selected.count === 0) return "";
	return (selected.count <= MAX_EXCERPT_LINES ? selected.lines : [
		...selected.headLines,
		"...",
		...selected.tailLines
	]).map((line) => `> ${line}`).join("\n");
}
function getMarkdownReviewSelectedLines(content, startLine, endLine) {
	const headLimit = Math.ceil(MAX_EXCERPT_LINES / 2);
	const tailLimit = Math.floor(MAX_EXCERPT_LINES / 2);
	const lines = [];
	const tailLines = [];
	let count = 0;
	forEachMarkdownReviewLine(content, (line, lineNumber) => {
		if (lineNumber < startLine) return;
		if (lineNumber > endLine) return false;
		count += 1;
		if (count <= MAX_EXCERPT_LINES) {
			lines.push(line);
			return lineNumber >= endLine ? false : void 0;
		}
		if (count === MAX_EXCERPT_LINES + 1) {
			tailLines.push(...lines.slice(-(tailLimit - 1)), line);
			return lineNumber >= endLine ? false : void 0;
		}
		tailLines.push(line);
		if (tailLines.length > tailLimit) tailLines.shift();
		return lineNumber >= endLine ? false : void 0;
	});
	return {
		count,
		lines,
		headLines: lines.slice(0, headLimit),
		tailLines
	};
}
function forEachMarkdownReviewLine(content, visit) {
	let lineStart = 0;
	let lineNumber = 1;
	for (let index = 0; index <= content.length; index += 1) {
		if (index < content.length && content.charCodeAt(index) !== 10) continue;
		const lineEnd = index > lineStart && content.charCodeAt(index - 1) === 13 ? index - 1 : index;
		if (visit(content.slice(lineStart, lineEnd), lineNumber) === false) return;
		lineStart = index + 1;
		lineNumber += 1;
	}
}
function getMarkdownReviewHighlightedText(content, note) {
	const selectedText = note.selectedText?.trim();
	if (selectedText) return selectedText;
	return getMarkdownReviewExcerpt(content, note).replace(/^> ?/gm, "").trim();
}
function formatMarkdownReviewCardQuote(text) {
	if (text === null || text === void 0) return;
	return formatBoundedMarkdownReviewCardQuote(text);
}
function formatBoundedMarkdownReviewCardQuote(text) {
	let normalized = "";
	let pendingWhitespace = false;
	for (let index = 0; index < text.length; index += 1) {
		if (isMarkdownReviewCardQuoteWhitespace(text.charCodeAt(index))) {
			pendingWhitespace = normalized.length > 0;
			continue;
		}
		if (pendingWhitespace) {
			normalized += " ";
			pendingWhitespace = false;
		}
		normalized += text.charAt(index);
		if (normalized.length > MAX_CARD_QUOTE_LENGTH) return `${normalized.slice(0, MAX_CARD_QUOTE_LENGTH - 3).trimEnd()}...`;
	}
	return normalized.length > 0 ? normalized : void 0;
}
function isMarkdownReviewCardQuoteWhitespace(code) {
	return code === 32 || code >= 9 && code <= 13 || code === 160 || code === 5760 || code >= 8192 && code <= 8202 || code === 8232 || code === 8233 || code === 8239 || code === 8287 || code === 12288 || code === 65279;
}
function getMarkdownReviewCardQuote(content, note) {
	return formatMarkdownReviewCardQuote(getMarkdownReviewHighlightedText(content, note));
}
function escapeMarkdownReviewNoteBody(body) {
	return body.replace(/\\/g, "\\\\").replace(/"/g, "\\\"").replace(/\r/g, "\\r").replace(/\n/g, "\\n");
}
function formatMarkdownReviewNoteDetails(note, content) {
	const excerpt = note.selectedText ? quoteMarkdownReviewText(getMarkdownReviewHighlightedText(content, note)) : getMarkdownReviewExcerpt(content, note);
	return [
		getDiffCommentLineLabel(note),
		excerpt ? `Excerpt:\n${excerpt}` : null,
		`User comment: "${escapeMarkdownReviewNoteBody(note.body)}"`
	].filter((part) => part !== null).join("\n");
}
function quoteMarkdownReviewText(text) {
	return `> ${text.replace(/\r\n|\r|\n/g, "\n> ")}`;
}
function formatMarkdownReviewNotes(notes, content) {
	const groups = /* @__PURE__ */ new Map();
	for (const note of sortMarkdownReviewNotes(notes)) {
		const group = groups.get(note.filePath);
		if (group) group.push(note);
		else groups.set(note.filePath, [note]);
	}
	return [...groups.entries()].map(([filePath, fileNotes]) => {
		return [
			`File: ${filePath}`,
			"Source: markdown",
			"",
			fileNotes.map((note) => formatMarkdownReviewNoteDetails(note, content)).join("\n\n")
		].join("\n");
	}).join("\n\n");
}
export { sortMarkdownReviewNotes as i, formatMarkdownReviewNotes as n, getMarkdownReviewCardQuote as r, formatMarkdownReviewCardQuote as t };
