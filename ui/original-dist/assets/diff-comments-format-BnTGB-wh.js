function isMarkdownComment(comment) {
	return comment.source === "markdown";
}
function formatDiffComment(c) {
	const escaped = c.body.replace(/\\/g, "\\\\").replace(/"/g, "\\\"").replace(/\r/g, "\\r").replace(/\n/g, "\\n");
	const locationLabel = c.lineNumber === 0 ? "Scope: file" : c.startLine !== void 0 && c.startLine !== c.lineNumber ? `Lines: ${c.startLine}-${c.lineNumber}` : `Line: ${c.lineNumber}`;
	if (!isMarkdownComment(c)) return [
		`File: ${c.filePath}`,
		locationLabel,
		`User comment: "${escaped}"`
	].join("\n");
	return [
		`File: ${c.filePath}`,
		"Source: markdown",
		locationLabel,
		`User comment: "${escaped}"`
	].join("\n");
}
function formatDiffComments(comments) {
	return comments.map(formatDiffComment).join("\n\n");
}
export { formatDiffComments as n, formatDiffComment as t };
