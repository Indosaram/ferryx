var FRONTMATTER_RE = /^(---|\+\+\+)\r?\n(?:[\s\S]*?\r?\n)?\1(?:\r?\n|$)/;
function extractFrontMatter(content) {
	const match = content.match(FRONTMATTER_RE);
	if (!match) return null;
	const raw = match[0];
	return {
		raw,
		body: content.slice(raw.length)
	};
}
function prependFrontMatter(raw, body) {
	return `${raw.endsWith("\n") ? raw : `${raw}\n`}${body}`;
}
export { prependFrontMatter as n, extractFrontMatter as t };
