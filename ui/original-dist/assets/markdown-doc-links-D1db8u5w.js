import { n as init_defineProperty, t as _defineProperty } from "./defineProperty-BAtR-r70.js";
init_defineProperty();
var markdownSlugPunctuationPattern = /[^\p{L}\p{M}\p{N} _-]/gu;
var MarkdownHeadingSlugger = class {
	constructor() {
		_defineProperty(this, "occurrences", /* @__PURE__ */ new Map());
	}
	reset() {
		this.occurrences.clear();
	}
	slug(value) {
		const baseSlug = slugMarkdownHeading(value);
		let nextSlug = baseSlug;
		while (this.occurrences.has(nextSlug)) {
			const nextCount = (this.occurrences.get(baseSlug) ?? 0) + 1;
			this.occurrences.set(baseSlug, nextCount);
			nextSlug = `${baseSlug}-${nextCount}`;
		}
		this.occurrences.set(nextSlug, 0);
		return nextSlug;
	}
};
function slugMarkdownHeading(value) {
	return value.toLowerCase().replace(markdownSlugPunctuationPattern, "").replace(/ /g, "-");
}
const MARKDOWN_DOC_LINK_PREFIX = "#orca-doc-link=";
function stripMarkdownExtension(value) {
	const lower = value.toLowerCase();
	for (const extension of [
		".markdown",
		".mdx",
		".md"
	]) if (lower.endsWith(extension)) return value.slice(0, -extension.length);
	return value;
}
function getMarkdownDocLinkDocumentTarget(target) {
	const hashIndex = target.indexOf("#");
	if (hashIndex <= 0) return target;
	return target.slice(0, hashIndex);
}
function getMarkdownDocLinkAnchor(target) {
	const hashIndex = target.indexOf("#");
	if (hashIndex === -1 || hashIndex === target.length - 1) return null;
	const anchor = target.slice(hashIndex + 1).trim();
	return anchor ? slugMarkdownHeading(anchor) : null;
}
function normalizeDocLinkKey(value) {
	let normalized = value.trim().replaceAll("\\", "/");
	while (normalized.startsWith("./")) normalized = normalized.slice(2);
	return normalized.toLowerCase();
}
function addIndexedDocument(map, key, document) {
	const existing = map.get(key);
	if (existing) existing.push(document);
	else map.set(key, [document]);
}
function resolveMatches(matches) {
	if (!matches) return null;
	return matches.length === 1 ? {
		status: "resolved",
		document: matches[0]
	} : {
		status: "ambiguous",
		matches
	};
}
function createMarkdownDocumentIndex(documents) {
	const byName = /* @__PURE__ */ new Map();
	const byRelativePath = /* @__PURE__ */ new Map();
	const byRelativePathWithoutExtension = /* @__PURE__ */ new Map();
	for (const document of documents) {
		addIndexedDocument(byName, normalizeDocLinkKey(document.name), document);
		addIndexedDocument(byRelativePath, normalizeDocLinkKey(document.relativePath), document);
		addIndexedDocument(byRelativePathWithoutExtension, normalizeDocLinkKey(stripMarkdownExtension(document.relativePath)), document);
	}
	return {
		byName,
		byRelativePath,
		byRelativePathWithoutExtension
	};
}
function resolveMarkdownDocLink(target, index) {
	const normalizedTarget = normalizeDocLinkKey(getMarkdownDocLinkDocumentTarget(target));
	const extensionlessTarget = stripMarkdownExtension(normalizedTarget);
	const relativeWithExtension = resolveMatches(index.byRelativePath.get(normalizedTarget));
	if (relativeWithExtension) return relativeWithExtension;
	const relativeWithoutExtension = resolveMatches(index.byRelativePathWithoutExtension.get(extensionlessTarget));
	if (relativeWithoutExtension) return relativeWithoutExtension;
	if (!normalizedTarget.includes("/")) {
		const byName = resolveMatches(index.byName.get(extensionlessTarget));
		if (byName) return byName;
	}
	return { status: "missing" };
}
function parseMarkdownDocLink(rawTarget) {
	const separatorIndex = rawTarget.indexOf("|");
	const target = separatorIndex === -1 ? rawTarget.trim() : rawTarget.slice(0, separatorIndex).trim();
	const alias = separatorIndex === -1 ? null : rawTarget.slice(separatorIndex + 1).trim() || null;
	if (!target || /[\r\n[\]]/.test(target) || alias !== null && /[\r\n[\]]/.test(alias)) return null;
	if (separatorIndex !== -1 && alias === null) return null;
	return {
		target,
		alias,
		label: alias ?? target
	};
}
function getMarkdownDocLinkTarget(rawTarget) {
	return parseMarkdownDocLink(rawTarget)?.target ?? null;
}
function formatMarkdownDocLinkBody(target, alias) {
	return alias ? `${target}|${alias}` : target;
}
function formatMarkdownDocLink(target, alias) {
	return `[[${formatMarkdownDocLinkBody(target, alias)}]]`;
}
function splitMarkdownDocLinkText(value) {
	const parts = [];
	let position = 0;
	while (position < value.length) {
		const start = value.indexOf("[[", position);
		if (start === -1) {
			parts.push({
				type: "text",
				value: value.slice(position)
			});
			break;
		}
		const end = value.indexOf("]]", start + 2);
		if (end === -1) {
			parts.push({
				type: "text",
				value: value.slice(position)
			});
			break;
		}
		const link = parseMarkdownDocLink(value.slice(start + 2, end));
		if (!link) {
			parts.push({
				type: "text",
				value: value.slice(position, end + 2)
			});
			position = end + 2;
			continue;
		}
		if (start > position) parts.push({
			type: "text",
			value: value.slice(position, start)
		});
		parts.push({
			type: "docLink",
			target: link.target,
			label: link.label
		});
		position = end + 2;
	}
	return parts.length === 0 ? [{
		type: "text",
		value
	}] : parts;
}
function createMarkdownDocLinkHref(target) {
	return `${MARKDOWN_DOC_LINK_PREFIX}${encodeURIComponent(target)}`;
}
function parseMarkdownDocLinkHref(href) {
	if (!href?.startsWith("#orca-doc-link=")) return null;
	try {
		return decodeURIComponent(href.slice(15));
	} catch {
		return null;
	}
}
function createDocLinkNode(target, label) {
	return {
		type: "link",
		url: createMarkdownDocLinkHref(target),
		title: null,
		children: [{
			type: "text",
			value: label
		}]
	};
}
function transformChildren(node) {
	if (!node.children || node.type === "link" || node.type === "image") return;
	const nextChildren = [];
	for (const child of node.children) if (child.type === "text" && child.value !== void 0) for (const part of splitMarkdownDocLinkText(child.value)) nextChildren.push(part.type === "text" ? {
		type: "text",
		value: part.value
	} : createDocLinkNode(part.target, part.label));
	else {
		transformChildren(child);
		nextChildren.push(child);
	}
	node.children = nextChildren;
}
function remarkMarkdownDocLinks() {
	return (tree) => transformChildren(tree);
}
export { getMarkdownDocLinkTarget as a, remarkMarkdownDocLinks as c, MarkdownHeadingSlugger as d, getMarkdownDocLinkAnchor as i, resolveMarkdownDocLink as l, formatMarkdownDocLink as n, parseMarkdownDocLink as o, formatMarkdownDocLinkBody as r, parseMarkdownDocLinkHref as s, createMarkdownDocumentIndex as t, stripMarkdownExtension as u };
