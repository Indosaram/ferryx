import { o as __toESM, t as __commonJSMin } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ChevronRight } from "./chevron-right-CZtMe6Ev.js";
import { t as ListTree } from "./list-tree-DeLnlXav.js";
import { $a as isClipboardTextByteLengthOverLimit, gi as computeMaxMarkdownTocPanelWidth, hi as clampMarkdownTocPanelWidth, mi as MARKDOWN_TOC_PANEL_MIN_WIDTH, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as X } from "./x-BrGKE4uz.js";
import { w as keybindingMatchesAction } from "./plugin-manifest-Bs-50M_g.js";
import { t as useSidebarResize } from "./useSidebarResize-BhlGhEjK.js";
import { _ as ok, a as unified, d as remarkParse, h as markdownSpace, i as escapeStringRegexp, m as markdownLineEnding, t as remarkGfm } from "./lib-CtirWBBB.js";
import { d as MarkdownHeadingSlugger } from "./markdown-doc-links-D1db8u5w.js";
import { n as formatMarkdownReviewNotes } from "./markdown-review-notes-CmRnxN_p.js";
var import_format = /* @__PURE__ */ __toESM((/* @__PURE__ */ __commonJSMin(((exports, module) => {
	(function() {
		var namespace;
		if (typeof module !== "undefined") namespace = module.exports = format;
		else namespace = function() {
			return this || (0, eval)("this");
		}();
		namespace.format = format;
		namespace.vsprintf = vsprintf;
		if (typeof console !== "undefined" && typeof console.log === "function") namespace.printf = printf;
		function printf() {
			console.log(format.apply(null, arguments));
		}
		function vsprintf(fmt, replacements) {
			return format.apply(null, [fmt].concat(replacements));
		}
		function format(fmt) {
			var argIndex = 1, args = [].slice.call(arguments), i = 0, n = fmt.length, result = "", c, escaped = false, arg, tmp, leadingZero = false, precision, nextArg = function() {
				return args[argIndex++];
			}, slurpNumber = function() {
				var digits = "";
				while (/\d/.test(fmt[i])) {
					digits += fmt[i++];
					c = fmt[i];
				}
				return digits.length > 0 ? parseInt(digits) : null;
			};
			for (; i < n; ++i) {
				c = fmt[i];
				if (escaped) {
					escaped = false;
					if (c == ".") {
						leadingZero = false;
						c = fmt[++i];
					} else if (c == "0" && fmt[i + 1] == ".") {
						leadingZero = true;
						i += 2;
						c = fmt[i];
					} else leadingZero = true;
					precision = slurpNumber();
					switch (c) {
						case "b":
							result += parseInt(nextArg(), 10).toString(2);
							break;
						case "c":
							arg = nextArg();
							if (typeof arg === "string" || arg instanceof String) result += arg;
							else result += String.fromCharCode(parseInt(arg, 10));
							break;
						case "d":
							result += parseInt(nextArg(), 10);
							break;
						case "f":
							tmp = String(parseFloat(nextArg()).toFixed(precision || 6));
							result += leadingZero ? tmp : tmp.replace(/^0/, "");
							break;
						case "j":
							result += JSON.stringify(nextArg());
							break;
						case "o":
							result += "0" + parseInt(nextArg(), 10).toString(8);
							break;
						case "s":
							result += nextArg();
							break;
						case "x":
							result += "0x" + parseInt(nextArg(), 10).toString(16);
							break;
						case "X":
							result += "0x" + parseInt(nextArg(), 10).toString(16).toUpperCase();
							break;
						default:
							result += c;
							break;
					}
				} else if (c === "%") escaped = true;
				else result += c;
			}
			return result;
		}
	})();
})))(), 1);
const fault = Object.assign(create(Error), {
	eval: create(EvalError),
	range: create(RangeError),
	reference: create(ReferenceError),
	syntax: create(SyntaxError),
	type: create(TypeError),
	uri: create(URIError)
});
function create(Constructor) {
	FormattedError.displayName = Constructor.displayName || Constructor.name;
	return FormattedError;
	function FormattedError(format, ...values) {
		return new Constructor(format ? (0, import_format.default)(format, ...values) : format);
	}
}
var own = {}.hasOwnProperty;
var markers = {
	yaml: "-",
	toml: "+"
};
function toMatters(options) {
	const result = [];
	let index = -1;
	const presetsOrMatters = Array.isArray(options) ? options : options ? [options] : ["yaml"];
	while (++index < presetsOrMatters.length) result[index] = matter(presetsOrMatters[index]);
	return result;
}
function matter(option) {
	let result = option;
	if (typeof result === "string") {
		if (!own.call(markers, result)) throw fault("Missing matter definition for `%s`", result);
		result = {
			type: result,
			marker: markers[result]
		};
	} else if (typeof result !== "object") throw fault("Expected matter to be an object, not `%j`", result);
	if (!own.call(result, "type")) throw fault("Missing `type` in matter `%j`", result);
	if (!own.call(result, "fence") && !own.call(result, "marker")) throw fault("Missing `marker` or `fence` in matter `%j`", result);
	return result;
}
function frontmatter(options) {
	const matters = toMatters(options);
	const flow = {};
	let index = -1;
	while (++index < matters.length) {
		const matter$1 = matters[index];
		const code = fence$1(matter$1, "open").charCodeAt(0);
		const construct = createConstruct(matter$1);
		const existing = flow[code];
		if (Array.isArray(existing)) existing.push(construct);
		else flow[code] = [construct];
	}
	return { flow };
}
function createConstruct(matter$1) {
	const anywhere = matter$1.anywhere;
	const frontmatterType = matter$1.type;
	const fenceType = frontmatterType + "Fence";
	const sequenceType = fenceType + "Sequence";
	const valueType = frontmatterType + "Value";
	const closingFenceConstruct = {
		tokenize: tokenizeClosingFence,
		partial: true
	};
	let buffer;
	let bufferIndex = 0;
	return {
		tokenize: tokenizeFrontmatter,
		concrete: true
	};
	function tokenizeFrontmatter(effects, ok$1, nok) {
		const self = this;
		return start;
		function start(code) {
			const position = self.now();
			if (position.column === 1 && (position.line === 1 || anywhere)) {
				buffer = fence$1(matter$1, "open");
				bufferIndex = 0;
				if (code === buffer.charCodeAt(bufferIndex)) {
					effects.enter(frontmatterType);
					effects.enter(fenceType);
					effects.enter(sequenceType);
					return openSequence(code);
				}
			}
			return nok(code);
		}
		function openSequence(code) {
			if (bufferIndex === buffer.length) {
				effects.exit(sequenceType);
				if (markdownSpace(code)) {
					effects.enter("whitespace");
					return openSequenceWhitespace(code);
				}
				return openAfter(code);
			}
			if (code === buffer.charCodeAt(bufferIndex++)) {
				effects.consume(code);
				return openSequence;
			}
			return nok(code);
		}
		function openSequenceWhitespace(code) {
			if (markdownSpace(code)) {
				effects.consume(code);
				return openSequenceWhitespace;
			}
			effects.exit("whitespace");
			return openAfter(code);
		}
		function openAfter(code) {
			if (markdownLineEnding(code)) {
				effects.exit(fenceType);
				effects.enter("lineEnding");
				effects.consume(code);
				effects.exit("lineEnding");
				buffer = fence$1(matter$1, "close");
				bufferIndex = 0;
				return effects.attempt(closingFenceConstruct, after, contentStart);
			}
			return nok(code);
		}
		function contentStart(code) {
			if (code === null || markdownLineEnding(code)) return contentEnd(code);
			effects.enter(valueType);
			return contentInside(code);
		}
		function contentInside(code) {
			if (code === null || markdownLineEnding(code)) {
				effects.exit(valueType);
				return contentEnd(code);
			}
			effects.consume(code);
			return contentInside;
		}
		function contentEnd(code) {
			if (code === null) return nok(code);
			effects.enter("lineEnding");
			effects.consume(code);
			effects.exit("lineEnding");
			return effects.attempt(closingFenceConstruct, after, contentStart);
		}
		function after(code) {
			effects.exit(frontmatterType);
			return ok$1(code);
		}
	}
	function tokenizeClosingFence(effects, ok$1, nok) {
		let bufferIndex$1 = 0;
		return closeStart;
		function closeStart(code) {
			if (code === buffer.charCodeAt(bufferIndex$1)) {
				effects.enter(fenceType);
				effects.enter(sequenceType);
				return closeSequence(code);
			}
			return nok(code);
		}
		function closeSequence(code) {
			if (bufferIndex$1 === buffer.length) {
				effects.exit(sequenceType);
				if (markdownSpace(code)) {
					effects.enter("whitespace");
					return closeSequenceWhitespace(code);
				}
				return closeAfter(code);
			}
			if (code === buffer.charCodeAt(bufferIndex$1++)) {
				effects.consume(code);
				return closeSequence;
			}
			return nok(code);
		}
		function closeSequenceWhitespace(code) {
			if (markdownSpace(code)) {
				effects.consume(code);
				return closeSequenceWhitespace;
			}
			effects.exit("whitespace");
			return closeAfter(code);
		}
		function closeAfter(code) {
			if (code === null || markdownLineEnding(code)) {
				effects.exit(fenceType);
				return ok$1(code);
			}
			return nok(code);
		}
	}
}
function fence$1(matter$1, prop) {
	return matter$1.marker ? pick$1(matter$1.marker, prop).repeat(3) : pick$1(matter$1.fence, prop);
}
function pick$1(schema, prop) {
	return typeof schema === "string" ? schema : schema[prop];
}
function frontmatterFromMarkdown(options) {
	const matters = toMatters(options);
	const enter = {};
	const exit = {};
	let index = -1;
	while (++index < matters.length) {
		const matter$1 = matters[index];
		enter[matter$1.type] = opener(matter$1);
		exit[matter$1.type] = close;
		exit[matter$1.type + "Value"] = value;
	}
	return {
		enter,
		exit
	};
}
function opener(matter$1) {
	return open;
	function open(token) {
		this.enter({
			type: matter$1.type,
			value: ""
		}, token);
		this.buffer();
	}
}
function close(token) {
	const data = this.resume();
	const node = this.stack[this.stack.length - 1];
	"value" in node;
	this.exit(token);
	node.value = data.replace(/^(\r?\n|\r)|(\r?\n|\r)$/g, "");
}
function value(token) {
	this.config.enter.data.call(this, token);
	this.config.exit.data.call(this, token);
}
function frontmatterToMarkdown(options) {
	const unsafe = [];
	const handlers = {};
	const matters = toMatters(options);
	let index = -1;
	while (++index < matters.length) {
		const matter$1 = matters[index];
		handlers[matter$1.type] = handler(matter$1);
		const open = fence(matter$1, "open");
		unsafe.push({
			atBreak: true,
			character: open.charAt(0),
			after: escapeStringRegexp(open.charAt(1))
		});
	}
	return {
		unsafe,
		handlers
	};
}
function handler(matter$1) {
	const open = fence(matter$1, "open");
	const close$1 = fence(matter$1, "close");
	return handle;
	function handle(node) {
		return open + (node.value ? "\n" + node.value : "") + "\n" + close$1;
	}
}
function fence(matter$1, prop) {
	return matter$1.marker ? pick(matter$1.marker, prop).repeat(3) : pick(matter$1.fence, prop);
}
function pick(schema, prop) {
	return typeof schema === "string" ? schema : schema[prop];
}
var emptyOptions = "yaml";
function remarkFrontmatter(options) {
	const self = this;
	const settings = options || emptyOptions;
	const data = self.data();
	const micromarkExtensions = data.micromarkExtensions || (data.micromarkExtensions = []);
	const fromMarkdownExtensions = data.fromMarkdownExtensions || (data.fromMarkdownExtensions = []);
	const toMarkdownExtensions = data.toMarkdownExtensions || (data.toMarkdownExtensions = []);
	micromarkExtensions.push(frontmatter(settings));
	fromMarkdownExtensions.push(frontmatterFromMarkdown(settings));
	toMarkdownExtensions.push(frontmatterToMarkdown(settings));
}
const MARKDOWN_PREVIEW_SEARCH_QUERY_MAX_BYTES = 2 * 1024;
function isMarkdownPreviewSearchQueryTooLarge(query, maxBytes = MARKDOWN_PREVIEW_SEARCH_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function isMarkdownPreviewFindShortcut(event, platform, keybindings) {
	return keybindingMatchesAction("editor.find", event, platform, keybindings);
}
function isMarkdownPreviewReplaceShortcut(event, platform, keybindings) {
	return keybindingMatchesAction("editor.replace", event, platform, keybindings);
}
function findTextMatchRanges(text, query, options = {}) {
	if (!query) return [];
	if (isMarkdownPreviewSearchQueryTooLarge(query)) return [];
	const ranges = options.matchCase ? findCaseSensitiveMatchRanges(text, query) : findCaseInsensitiveMatchRanges(text, query);
	if (!options.wholeWord) return ranges;
	return ranges.filter((range) => isWholeWordMatch(text, range.start, range.end));
}
function findCaseSensitiveMatchRanges(text, query) {
	const matches = [];
	let searchStart = 0;
	while (searchStart <= text.length - query.length) {
		const matchStart = text.indexOf(query, searchStart);
		if (matchStart === -1) break;
		matches.push({
			start: matchStart,
			end: matchStart + query.length
		});
		searchStart = matchStart + query.length;
	}
	return matches;
}
function findCaseInsensitiveMatchRanges(text, query) {
	const normalizedText = buildLocaleLowercaseIndex(text);
	const normalizedQuery = query.toLocaleLowerCase();
	const matches = [];
	let searchStart = 0;
	while (searchStart <= normalizedText.text.length - normalizedQuery.length) {
		const matchStart = normalizedText.text.indexOf(normalizedQuery, searchStart);
		if (matchStart === -1) break;
		const matchEnd = matchStart + normalizedQuery.length;
		matches.push({
			start: normalizedText.originalStartByNormalizedOffset[matchStart] ?? text.length,
			end: normalizedText.originalEndByNormalizedOffset[matchEnd - 1] ?? text.length
		});
		searchStart = matchEnd + (normalizedQuery.length === 0 ? 1 : 0);
	}
	return matches;
}
var WORD_CHARACTER = /[\p{L}\p{N}_]/u;
function isWordCharacter(char) {
	return char !== void 0 && WORD_CHARACTER.test(char);
}
function codePointBefore(text, index) {
	if (index <= 0) return;
	const previousCodeUnit = text.charCodeAt(index - 1);
	if (previousCodeUnit >= 56320 && previousCodeUnit <= 57343 && index > 1 && text.charCodeAt(index - 2) >= 55296 && text.charCodeAt(index - 2) <= 56319) return text.slice(index - 2, index);
	return text[index - 1];
}
function codePointAt(text, index) {
	const codePoint = text.codePointAt(index);
	return codePoint === void 0 ? void 0 : String.fromCodePoint(codePoint);
}
function isWholeWordMatch(text, start, end) {
	const before = codePointBefore(text, start);
	const after = codePointAt(text, end);
	return !isWordCharacter(before) && !isWordCharacter(after);
}
function buildLocaleLowercaseIndex(text) {
	let normalized = "";
	const originalStartByNormalizedOffset = [];
	const originalEndByNormalizedOffset = [];
	let originalOffset = 0;
	for (const char of text) {
		const normalizedChar = char.toLocaleLowerCase();
		const originalEnd = originalOffset + char.length;
		for (let i = 0; i < normalizedChar.length; i += 1) {
			originalStartByNormalizedOffset.push(originalOffset);
			originalEndByNormalizedOffset.push(originalEnd);
		}
		normalized += normalizedChar;
		originalOffset = originalEnd;
	}
	return {
		text: normalized,
		originalStartByNormalizedOffset,
		originalEndByNormalizedOffset
	};
}
var SEARCH_HIGHLIGHT_NAME = "markdown-preview-search-match";
var ACTIVE_SEARCH_HIGHLIGHT_NAME = "markdown-preview-search-active-match";
function getHighlightApi() {
	const scope = globalThis;
	const registry = scope.CSS?.highlights;
	const HighlightCtor = scope.Highlight;
	if (!registry || typeof HighlightCtor !== "function") return null;
	return {
		registry,
		create: (ranges) => {
			const highlight = new HighlightCtor();
			for (const range of ranges) highlight.add(range);
			return highlight;
		}
	};
}
var searchRangesByInstance = /* @__PURE__ */ new Map();
var activeRangeByInstance = /* @__PURE__ */ new Map();
function paintMatchHighlight(api) {
	const matchRanges = [];
	for (const ranges of searchRangesByInstance.values()) for (const range of ranges) matchRanges.push(range);
	if (matchRanges.length > 0) api.registry.set(SEARCH_HIGHLIGHT_NAME, api.create(matchRanges));
	else api.registry.delete(SEARCH_HIGHLIGHT_NAME);
}
function paintActiveHighlight(api) {
	const activeRanges = [];
	for (const range of activeRangeByInstance.values()) activeRanges.push(range);
	if (activeRanges.length > 0) api.registry.set(ACTIVE_SEARCH_HIGHLIGHT_NAME, api.create(activeRanges));
	else api.registry.delete(ACTIVE_SEARCH_HIGHLIGHT_NAME);
}
function clearMarkdownPreviewSearchHighlights(instanceId) {
	searchRangesByInstance.delete(instanceId);
	activeRangeByInstance.delete(instanceId);
	const api = getHighlightApi();
	if (api) {
		paintMatchHighlight(api);
		paintActiveHighlight(api);
	}
}
function applyMarkdownPreviewSearchHighlights(instanceId, root, query) {
	const ranges = [];
	if (query && !isMarkdownPreviewSearchQueryTooLarge(query)) {
		const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, { acceptNode(node) {
			if (!(node.parentElement instanceof HTMLElement)) return NodeFilter.FILTER_REJECT;
			if (!node.textContent?.trim()) return NodeFilter.FILTER_REJECT;
			return NodeFilter.FILTER_ACCEPT;
		} });
		let currentNode = walker.nextNode();
		while (currentNode) {
			if (currentNode instanceof Text) {
				const text = currentNode.textContent ?? "";
				for (const { start, end } of findTextMatchRanges(text, query)) {
					const range = document.createRange();
					range.setStart(currentNode, start);
					range.setEnd(currentNode, end);
					ranges.push(range);
				}
			}
			currentNode = walker.nextNode();
		}
	}
	searchRangesByInstance.set(instanceId, ranges);
	activeRangeByInstance.delete(instanceId);
	const api = getHighlightApi();
	if (api) {
		paintMatchHighlight(api);
		paintActiveHighlight(api);
	}
	return ranges;
}
function setActiveMarkdownPreviewSearchMatch(instanceId, matches, activeIndex) {
	const active = activeIndex >= 0 ? matches[activeIndex] : void 0;
	if (active) activeRangeByInstance.set(instanceId, active);
	else activeRangeByInstance.delete(instanceId);
	const api = getHighlightApi();
	if (api) paintActiveHighlight(api);
	if (active) active.startContainer.parentElement?.scrollIntoView({
		block: "center",
		inline: "nearest"
	});
}
function isMarkdownTocLevel(value$1) {
	return value$1 >= 1 && value$1 <= 5;
}
function foldMarkdownTocWhitespace(value$1) {
	let normalized = "";
	let pendingWhitespace = false;
	for (let index = 0; index < value$1.length; index += 1) {
		if (isMarkdownTocWhitespace(value$1.charCodeAt(index))) {
			pendingWhitespace = normalized.length > 0;
			continue;
		}
		if (pendingWhitespace) {
			normalized += " ";
			pendingWhitespace = false;
		}
		normalized += value$1.charAt(index);
	}
	return normalized;
}
function isMarkdownTocWhitespace(code) {
	return code === 32 || code >= 9 && code <= 13 || code === 160 || code === 5760 || code >= 8192 && code <= 8202 || code === 8232 || code === 8233 || code === 8239 || code === 8287 || code === 12288 || code === 65279;
}
function nearestParent(stack, level) {
	for (let index = stack.length - 1; index >= 0; index -= 1) {
		const item = stack.at(index);
		if (item && item.level < level) return item;
	}
	return stack[0];
}
function appendTocItem(stack, item) {
	nearestParent(stack, item.level).children.push(item);
	Reflect.set(stack, item.level, item);
	stack.length = item.level + 1;
}
function markdownAstNodeToText(node) {
	if (typeof node.value === "string") return node.value;
	if (typeof node.alt === "string") return node.alt;
	return (node.children ?? []).map(markdownAstNodeToText).join("");
}
function buildMarkdownTableOfContents(markdown) {
	const slugger = new MarkdownHeadingSlugger();
	const root = {
		id: "toc-root",
		level: 1,
		title: "",
		children: []
	};
	const stack = [root];
	const tree = unified().use(remarkParse).use(remarkGfm).use(remarkFrontmatter, ["yaml", "toml"]).parse(markdown);
	function visit(node) {
		if (node.type === "heading" && typeof node.depth === "number" && isMarkdownTocLevel(node.depth)) {
			const title = foldMarkdownTocWhitespace(markdownAstNodeToText(node));
			if (title) appendTocItem(stack, {
				children: [],
				id: slugger.slug(title),
				level: node.depth,
				title
			});
		}
		for (const child of node.children ?? []) visit(child);
	}
	visit(tree);
	return root.children;
}
var EMPTY_MARKDOWN_TOC = [];
function selectMarkdownTableOfContents(showTableOfContents, content, build = buildMarkdownTableOfContents) {
	return showTableOfContents ? build(content) : EMPTY_MARKDOWN_TOC;
}
function collectMarkdownTocParentIds(items) {
	const parentIds = /* @__PURE__ */ new Set();
	function visit(nodes) {
		for (const item of nodes) if (item.children.length > 0) {
			parentIds.add(item.id);
			visit(item.children);
		}
	}
	visit(items);
	return parentIds;
}
function collapseMarkdownTocToLevel(items, maxExpandedLevel) {
	const collapsed = /* @__PURE__ */ new Set();
	function visit(nodes) {
		for (const item of nodes) {
			if (item.children.length > 0 && item.level >= maxExpandedLevel) collapsed.add(item.id);
			visit(item.children);
		}
	}
	visit(items);
	return collapsed;
}
function pruneMarkdownTocCollapsedIds(collapsedIds, items) {
	const parentIds = collectMarkdownTocParentIds(items);
	const next = /* @__PURE__ */ new Set();
	for (const id of collapsedIds) if (parentIds.has(id)) next.add(id);
	return next;
}
function toggleMarkdownTocCollapsedId(collapsedIds, id) {
	const next = new Set(collapsedIds);
	if (next.has(id)) next.delete(id);
	else next.add(id);
	return next;
}
function isMarkdownTocItemExpanded(collapsedIds, item) {
	return item.children.length === 0 || !collapsedIds.has(item.id);
}
const MARKDOWN_TOC_RESIZE_HANDLE_CLASS_NAME = "absolute top-0 right-0 z-10 h-full w-1 cursor-col-resize transition-colors hover:bg-ring/20 active:bg-ring/30";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var TOC_LEVELS = [
	1,
	2,
	3,
	4,
	5
];
var TOC_EXPAND_ALL_LEVEL = 5;
var TOC_INDENT_BASE_PX = 12;
var TOC_INDENT_STEP_PX = 12;
function MarkdownTocRow({ collapsedIds, depth, item, onNavigate, onToggleCollapsed }) {
	const hasChildren = item.children.length > 0;
	const expanded = isMarkdownTocItemExpanded(collapsedIds, item);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "markdown-toc-row",
		style: { paddingLeft: hasChildren ? depth === 0 ? TOC_INDENT_BASE_PX : depth * TOC_INDENT_STEP_PX : TOC_INDENT_BASE_PX + depth * TOC_INDENT_STEP_PX },
		children: [hasChildren ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
			type: "button",
			className: "markdown-toc-disclosure",
			"aria-label": expanded ? translate("auto.components.editor.MarkdownTableOfContentsPanel.97ad46f11f", "Collapse {{value0}}", { value0: item.title }) : translate("auto.components.editor.MarkdownTableOfContentsPanel.65b036a6c8", "Expand {{value0}}", { value0: item.title }),
			"aria-expanded": expanded,
			onClick: () => onToggleCollapsed(item.id),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: cn("size-3 shrink-0 text-muted-foreground transition-transform", expanded && "rotate-90") })
		}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
			type: "button",
			className: "markdown-toc-title-button",
			onClick: () => onNavigate(item.id),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "markdown-toc-title",
				children: item.title
			})
		})]
	}), hasChildren && expanded ? item.children.map((child) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MarkdownTocRow, {
		collapsedIds,
		depth: depth + 1,
		item: child,
		onNavigate,
		onToggleCollapsed
	}, child.id)) : null] });
}
function MarkdownTableOfContentsPanel({ items, onClose, onNavigate }) {
	const [collapsedIds, setCollapsedIds] = (0, import_react.useState)(() => /* @__PURE__ */ new Set());
	const markdownTocPanelWidth = useAppStore((s) => s.markdownTocPanelWidth);
	const setMarkdownTocPanelWidth = useAppStore((s) => s.setMarkdownTocPanelWidth);
	const [layoutWidth, setLayoutWidth] = (0, import_react.useState)(null);
	const maxPanelWidth = computeMaxMarkdownTocPanelWidth(layoutWidth ?? 0);
	const { containerRef, onResizeStart } = useSidebarResize({
		isOpen: true,
		width: clampMarkdownTocPanelWidth(markdownTocPanelWidth, layoutWidth ?? void 0),
		minWidth: 200,
		maxWidth: maxPanelWidth,
		deltaSign: 1,
		setWidth: setMarkdownTocPanelWidth
	});
	(0, import_react.useEffect)(() => {
		setCollapsedIds((current) => pruneMarkdownTocCollapsedIds(current, items));
	}, [items]);
	(0, import_react.useEffect)(() => {
		const layout = containerRef.current?.parentElement;
		if (!layout) return;
		const updateMaxWidth = () => {
			setLayoutWidth(layout.clientWidth);
		};
		updateMaxWidth();
		const observer = new ResizeObserver(updateMaxWidth);
		observer.observe(layout);
		return () => observer.disconnect();
	}, [containerRef]);
	const collapseToLevel = (level) => {
		setCollapsedIds(collapseMarkdownTocToLevel(items, level));
	};
	const toggleCollapsed = (id) => {
		setCollapsedIds((current) => toggleMarkdownTocCollapsedId(current, id));
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("aside", {
		ref: containerRef,
		className: "markdown-toc-panel",
		"aria-label": translate("auto.components.editor.MarkdownTableOfContentsPanel.27d0a9c49a", "Table of contents"),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "markdown-toc-header",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ListTree, { className: "size-3.5 text-muted-foreground" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.editor.MarkdownTableOfContentsPanel.06357eea60", "Table of Contents") }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "markdown-toc-header-actions",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "markdown-toc-level-controls",
							role: "group",
							"aria-label": translate("auto.components.editor.MarkdownTableOfContentsPanel.0dc7b2f05a", "Collapse by level"),
							children: TOC_LEVELS.map((level) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								type: "button",
								variant: "ghost",
								size: "icon-xs",
								className: "markdown-toc-level-button",
								"aria-label": level === TOC_EXPAND_ALL_LEVEL ? translate("auto.components.editor.MarkdownTableOfContentsPanel.f3de856175", "Expand all heading levels") : translate("auto.components.editor.MarkdownTableOfContentsPanel.111e66b85d", "Collapse to heading level {{value0}}", { value0: level }),
								title: level === TOC_EXPAND_ALL_LEVEL ? translate("auto.components.editor.MarkdownTableOfContentsPanel.a5daadd68b", "Expand all") : translate("auto.components.editor.MarkdownTableOfContentsPanel.4680a4b808", "Collapse to H{{value0}}", { value0: level }),
								onClick: () => collapseToLevel(level),
								children: ["H", level]
							}, level))
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "ghost",
							size: "icon-xs",
							"aria-label": translate("auto.components.editor.MarkdownTableOfContentsPanel.bbe8369097", "Close table of contents"),
							title: translate("auto.components.editor.MarkdownTableOfContentsPanel.bbe8369097", "Close table of contents"),
							onClick: onClose,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3.5" })
						})]
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "markdown-toc-list",
				children: items.length > 0 ? items.map((item) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MarkdownTocRow, {
					collapsedIds,
					depth: 0,
					item,
					onNavigate,
					onToggleCollapsed: toggleCollapsed
				}, item.id)) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "markdown-toc-empty",
					children: translate("auto.components.editor.MarkdownTableOfContentsPanel.de3928b6e4", "No headings")
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				"data-markdown-toc-resize-handle": "",
				className: MARKDOWN_TOC_RESIZE_HANDLE_CLASS_NAME,
				role: "separator",
				"aria-orientation": "vertical",
				"aria-label": translate("auto.components.editor.MarkdownTableOfContentsPanel.8f4d2c1a9b", "Resize table of contents"),
				onMouseDown: onResizeStart
			})
		]
	});
}
async function copyMarkdownReviewNotesForAgent({ notes, content, writeClipboardText }) {
	if (notes.length === 0) return false;
	await writeClipboardText(formatMarkdownReviewNotes(notes, content));
	return true;
}
export { clearMarkdownPreviewSearchHighlights as a, isMarkdownPreviewReplaceShortcut as c, remarkFrontmatter as d, applyMarkdownPreviewSearchHighlights as i, isMarkdownPreviewSearchQueryTooLarge as l, MarkdownTableOfContentsPanel as n, findTextMatchRanges as o, selectMarkdownTableOfContents as r, isMarkdownPreviewFindShortcut as s, copyMarkdownReviewNotesForAgent as t, setActiveMarkdownPreviewSearchMatch as u };
