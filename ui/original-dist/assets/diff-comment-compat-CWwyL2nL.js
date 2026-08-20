function getDiffCommentSource(comment) {
	return comment.source === "markdown" ? "markdown" : "diff";
}
function isDiffComment(comment) {
	return getDiffCommentSource(comment) === "diff";
}
function isMarkdownComment(comment) {
	return getDiffCommentSource(comment) === "markdown";
}
function getDiffCommentLineLabel(comment, compact = false) {
	if (comment.startLine !== void 0 && comment.startLine !== comment.lineNumber) return compact ? `L${comment.startLine}-L${comment.lineNumber}` : `Lines ${comment.startLine}-${comment.lineNumber}`;
	return compact ? `L${comment.lineNumber}` : `Line ${comment.lineNumber}`;
}
export { isMarkdownComment as i, getDiffCommentSource as n, isDiffComment as r, getDiffCommentLineLabel as t };
