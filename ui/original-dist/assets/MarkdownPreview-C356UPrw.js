import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as ChevronUp } from "./chevron-up-CuYdIP8o.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { t as CornerDownLeft } from "./corner-down-left-DMfCMf9j.js";
import { t as MessageSquare } from "./message-square-DzGigs-c.js";
import { t as Plus } from "./plus-Db0kWPVa.js";
import { $t as resolveMarkdownLinkTarget, Bt as showLocalPathOpenBlockedToast, Dc as dirname, Jt as openHttpLink, Rt as detectLanguage, Vt as createConnectionIdForFileSelector, Zt as absolutePathToFileUri, qu as findWorktreeById, sp as settingsForRuntimeOwner, t as useAppStore, vt as statRuntimePath, zp as relativePathInsideRoot, zt as isLocalPathOpenBlocked } from "./store-CgXrfmaH.js";
import { t as X } from "./x-BrGKE4uz.js";
import { w as keybindingMatchesAction } from "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./dropdown-menu-Dth6LPK-.js";
import "./tooltip-DPmd1AoJ.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import { n as getConnectionIdForFile } from "./connection-context-BUPsamzR.js";
import "./localized-catalog-DubKHKUR.js";
import { n as computeEditorFontSize } from "./editor-font-zoom-2F4BKkDZ.js";
import "./agent-status-connection-ownership-D5nXPHBo.js";
import "./resolved-worktree-execution-host-BcjAq7e6.js";
import "./useSidebarResize-BhlGhEjK.js";
import { t as getShortcutPlatform } from "./shortcut-platform-BbPBGzth.js";
import "./useShortcutLabel-C-KRYtlB.js";
import "./worktree-agent-rows-C1pW_DbE.js";
import { t as Input } from "./input-DV5rpysh.js";
import "./worktree-title-derived-agent-rows-xbcpjeY8.js";
import "./AgentWorkingSpinner-BpnTWNKF.js";
import { _ as ok, a as unified, c as SKIP, d as remarkParse, f as factorySpace, l as visitParents, m as markdownLineEnding, n as longestStreak, s as visit, t as remarkGfm, u as convert } from "./lib-CtirWBBB.js";
import { a as h, c as defaultUrlTransform, i as webNamespaces, n as defaultSchema, o as s, r as rehypeRaw, s as Markdown, t as rehypeSanitize } from "./lib-D08jHVMa.js";
import { t as remarkBreaks } from "./lib-Cpbsvy64.js";
import "./purify.es-C_rn83UJ.js";
import { t as MermaidBlock } from "./MermaidBlock-gW3wAx0A.js";
import "./AgentStateDot-DFt63YGw.js";
import "./icons-jFAuHbv9.js";
import "./agent-catalog-CBF2CV5Q.js";
import "./useWorktreeAgentRows-DfUM0dP9.js";
import "./useDetectedAgents-KkNokXI_.js";
import { c as katex } from "./katex-BBbt5qcv.js";
import { a as useLocalImageSrc, c as isMarkdownPreviewOpenModifier, d as resolveMarkdownPreviewHref, f as resolveMarkdownPreviewHttpOpenOptions, l as isMarkdownPreviewSystemBrowserModifier, m as grammars, n as loadLocalImageAbsolutePath, o as fileUrlToAbsolutePath, p as createLowlight, s as getMarkdownPreviewLinkTarget, t as getLocalImageCacheKey, u as resolveImageAbsolutePath } from "./useLocalImageSrc-DrVdAMx8.js";
import { c as remarkMarkdownDocLinks, i as getMarkdownDocLinkAnchor, l as resolveMarkdownDocLink, s as parseMarkdownDocLinkHref, t as createMarkdownDocumentIndex } from "./markdown-doc-links-D1db8u5w.js";
import { i as isMarkdownComment } from "./diff-comment-compat-CWwyL2nL.js";
import { t as DiffCommentCard } from "./DiffCommentCard-DPwCt0gV.js";
import "./ReviewNotesSendMenuContent-DnAssgZQ.js";
import "./launch-agent-in-new-tab-44JGNfKl.js";
import "./active-agent-note-send-CsxZ0dL2.js";
import { t as NotesSendMenu } from "./NotesSendMenu-DLIuO9I1.js";
import { o as installOpenDraftAddReviewNoteGuard } from "./editor-shortcuts-Cg6u73ie.js";
import { a as setWithLRU, i as scrollTopCache } from "./scroll-cache-B8ebRfkp.js";
import { t as extractFrontMatter } from "./markdown-frontmatter-COocytJv.js";
import { a as clearMarkdownPreviewSearchHighlights, d as remarkFrontmatter, i as applyMarkdownPreviewSearchHighlights, n as MarkdownTableOfContentsPanel, r as selectMarkdownTableOfContents, s as isMarkdownPreviewFindShortcut, t as copyMarkdownReviewNotesForAgent, u as setActiveMarkdownPreviewSearchMatch } from "./markdown-review-note-copy-CJnDWwUf.js";
import { i as sortMarkdownReviewNotes, n as formatMarkdownReviewNotes, r as getMarkdownReviewCardQuote, t as formatMarkdownReviewCardQuote } from "./markdown-review-notes-CmRnxN_p.js";
function mathFromMarkdown() {
	return {
		enter: {
			mathFlow: enterMathFlow,
			mathFlowFenceMeta: enterMathFlowMeta,
			mathText: enterMathText
		},
		exit: {
			mathFlow: exitMathFlow,
			mathFlowFence: exitMathFlowFence,
			mathFlowFenceMeta: exitMathFlowMeta,
			mathFlowValue: exitMathData,
			mathText: exitMathText,
			mathTextData: exitMathData
		}
	};
	function enterMathFlow(token) {
		this.enter({
			type: "math",
			meta: null,
			value: "",
			data: {
				hName: "pre",
				hChildren: [{
					type: "element",
					tagName: "code",
					properties: { className: ["language-math", "math-display"] },
					children: []
				}]
			}
		}, token);
	}
	function enterMathFlowMeta() {
		this.buffer();
	}
	function exitMathFlowMeta() {
		const data = this.resume();
		const node = this.stack[this.stack.length - 1];
		node.type;
		node.meta = data;
	}
	function exitMathFlowFence() {
		if (this.data.mathFlowInside) return;
		this.buffer();
		this.data.mathFlowInside = true;
	}
	function exitMathFlow(token) {
		const data = this.resume().replace(/^(\r?\n|\r)|(\r?\n|\r)$/g, "");
		const node = this.stack[this.stack.length - 1];
		node.type;
		this.exit(token);
		node.value = data;
		const code = node.data.hChildren[0];
		code.type;
		code.tagName;
		code.children.push({
			type: "text",
			value: data
		});
		this.data.mathFlowInside = void 0;
	}
	function enterMathText(token) {
		this.enter({
			type: "inlineMath",
			value: "",
			data: {
				hName: "code",
				hProperties: { className: ["language-math", "math-inline"] },
				hChildren: []
			}
		}, token);
		this.buffer();
	}
	function exitMathText(token) {
		const data = this.resume();
		const node = this.stack[this.stack.length - 1];
		node.type;
		this.exit(token);
		node.value = data;
		node.data.hChildren.push({
			type: "text",
			value: data
		});
	}
	function exitMathData(token) {
		this.config.enter.data.call(this, token);
		this.config.exit.data.call(this, token);
	}
}
function mathToMarkdown(options) {
	let single = (options || {}).singleDollarTextMath;
	if (single === null || single === void 0) single = true;
	inlineMath.peek = inlineMathPeek;
	return {
		unsafe: [
			{
				character: "\r",
				inConstruct: "mathFlowMeta"
			},
			{
				character: "\n",
				inConstruct: "mathFlowMeta"
			},
			{
				character: "$",
				after: single ? void 0 : "\\$",
				inConstruct: "phrasing"
			},
			{
				character: "$",
				inConstruct: "mathFlowMeta"
			},
			{
				atBreak: true,
				character: "$",
				after: "\\$"
			}
		],
		handlers: {
			math: math$1,
			inlineMath
		}
	};
	function math$1(node, _, state, info) {
		const raw = node.value || "";
		const tracker = state.createTracker(info);
		const sequence = "$".repeat(Math.max(longestStreak(raw, "$") + 1, 2));
		const exit = state.enter("mathFlow");
		let value = tracker.move(sequence);
		if (node.meta) {
			const subexit = state.enter("mathFlowMeta");
			value += tracker.move(state.safe(node.meta, {
				after: "\n",
				before: value,
				encode: ["$"],
				...tracker.current()
			}));
			subexit();
		}
		value += tracker.move("\n");
		if (raw) value += tracker.move(raw + "\n");
		value += tracker.move(sequence);
		exit();
		return value;
	}
	function inlineMath(node, _, state) {
		let value = node.value || "";
		let size = 1;
		if (!single) size++;
		while ((/* @__PURE__ */ new RegExp("(^|[^$])" + "\\$".repeat(size) + "([^$]|$)")).test(value)) size++;
		const sequence = "$".repeat(size);
		if (/[^ \r\n]/.test(value) && (/^[ \r\n]/.test(value) && /[ \r\n]$/.test(value) || /^\$|\$$/.test(value))) value = " " + value + " ";
		let index = -1;
		while (++index < state.unsafe.length) {
			const pattern = state.unsafe[index];
			if (!pattern.atBreak) continue;
			const expression = state.compilePattern(pattern);
			let match;
			while (match = expression.exec(value)) {
				let position = match.index;
				if (value.codePointAt(position) === 10 && value.codePointAt(position - 1) === 13) position--;
				value = value.slice(0, position) + " " + value.slice(match.index + 1);
			}
		}
		return sequence + value + sequence;
	}
	function inlineMathPeek() {
		return "$";
	}
}
const mathFlow = {
	tokenize: tokenizeMathFenced,
	concrete: true,
	name: "mathFlow"
};
var nonLazyContinuation = {
	tokenize: tokenizeNonLazyContinuation,
	partial: true
};
function tokenizeMathFenced(effects, ok$1, nok) {
	const self = this;
	const tail = self.events[self.events.length - 1];
	const initialSize = tail && tail[1].type === "linePrefix" ? tail[2].sliceSerialize(tail[1], true).length : 0;
	let sizeOpen = 0;
	return start;
	function start(code) {
		effects.enter("mathFlow");
		effects.enter("mathFlowFence");
		effects.enter("mathFlowFenceSequence");
		return sequenceOpen(code);
	}
	function sequenceOpen(code) {
		if (code === 36) {
			effects.consume(code);
			sizeOpen++;
			return sequenceOpen;
		}
		if (sizeOpen < 2) return nok(code);
		effects.exit("mathFlowFenceSequence");
		return factorySpace(effects, metaBefore, "whitespace")(code);
	}
	function metaBefore(code) {
		if (code === null || markdownLineEnding(code)) return metaAfter(code);
		effects.enter("mathFlowFenceMeta");
		effects.enter("chunkString", { contentType: "string" });
		return meta(code);
	}
	function meta(code) {
		if (code === null || markdownLineEnding(code)) {
			effects.exit("chunkString");
			effects.exit("mathFlowFenceMeta");
			return metaAfter(code);
		}
		if (code === 36) return nok(code);
		effects.consume(code);
		return meta;
	}
	function metaAfter(code) {
		effects.exit("mathFlowFence");
		if (self.interrupt) return ok$1(code);
		return effects.attempt(nonLazyContinuation, beforeNonLazyContinuation, after)(code);
	}
	function beforeNonLazyContinuation(code) {
		return effects.attempt({
			tokenize: tokenizeClosingFence,
			partial: true
		}, after, contentStart)(code);
	}
	function contentStart(code) {
		return (initialSize ? factorySpace(effects, beforeContentChunk, "linePrefix", initialSize + 1) : beforeContentChunk)(code);
	}
	function beforeContentChunk(code) {
		if (code === null) return after(code);
		if (markdownLineEnding(code)) return effects.attempt(nonLazyContinuation, beforeNonLazyContinuation, after)(code);
		effects.enter("mathFlowValue");
		return contentChunk(code);
	}
	function contentChunk(code) {
		if (code === null || markdownLineEnding(code)) {
			effects.exit("mathFlowValue");
			return beforeContentChunk(code);
		}
		effects.consume(code);
		return contentChunk;
	}
	function after(code) {
		effects.exit("mathFlow");
		return ok$1(code);
	}
	function tokenizeClosingFence(effects$1, ok$2, nok$1) {
		let size = 0;
		return factorySpace(effects$1, beforeSequenceClose, "linePrefix", self.parser.constructs.disable.null.includes("codeIndented") ? void 0 : 4);
		function beforeSequenceClose(code) {
			effects$1.enter("mathFlowFence");
			effects$1.enter("mathFlowFenceSequence");
			return sequenceClose(code);
		}
		function sequenceClose(code) {
			if (code === 36) {
				size++;
				effects$1.consume(code);
				return sequenceClose;
			}
			if (size < sizeOpen) return nok$1(code);
			effects$1.exit("mathFlowFenceSequence");
			return factorySpace(effects$1, afterSequenceClose, "whitespace")(code);
		}
		function afterSequenceClose(code) {
			if (code === null || markdownLineEnding(code)) {
				effects$1.exit("mathFlowFence");
				return ok$2(code);
			}
			return nok$1(code);
		}
	}
}
function tokenizeNonLazyContinuation(effects, ok$1, nok) {
	const self = this;
	return start;
	function start(code) {
		if (code === null) return ok$1(code);
		effects.enter("lineEnding");
		effects.consume(code);
		effects.exit("lineEnding");
		return lineStart;
	}
	function lineStart(code) {
		return self.parser.lazy[self.now().line] ? nok(code) : ok$1(code);
	}
}
function mathText(options) {
	let single = (options || {}).singleDollarTextMath;
	if (single === null || single === void 0) single = true;
	return {
		tokenize: tokenizeMathText,
		resolve: resolveMathText,
		previous,
		name: "mathText"
	};
	function tokenizeMathText(effects, ok$1, nok) {
		let sizeOpen = 0;
		let size;
		let token;
		return start;
		function start(code) {
			effects.enter("mathText");
			effects.enter("mathTextSequence");
			return sequenceOpen(code);
		}
		function sequenceOpen(code) {
			if (code === 36) {
				effects.consume(code);
				sizeOpen++;
				return sequenceOpen;
			}
			if (sizeOpen < 2 && !single) return nok(code);
			effects.exit("mathTextSequence");
			return between(code);
		}
		function between(code) {
			if (code === null) return nok(code);
			if (code === 36) {
				token = effects.enter("mathTextSequence");
				size = 0;
				return sequenceClose(code);
			}
			if (code === 32) {
				effects.enter("space");
				effects.consume(code);
				effects.exit("space");
				return between;
			}
			if (markdownLineEnding(code)) {
				effects.enter("lineEnding");
				effects.consume(code);
				effects.exit("lineEnding");
				return between;
			}
			effects.enter("mathTextData");
			return data(code);
		}
		function data(code) {
			if (code === null || code === 32 || code === 36 || markdownLineEnding(code)) {
				effects.exit("mathTextData");
				return between(code);
			}
			effects.consume(code);
			return data;
		}
		function sequenceClose(code) {
			if (code === 36) {
				effects.consume(code);
				size++;
				return sequenceClose;
			}
			if (size === sizeOpen) {
				effects.exit("mathTextSequence");
				effects.exit("mathText");
				return ok$1(code);
			}
			token.type = "mathTextData";
			return data(code);
		}
	}
}
function resolveMathText(events) {
	let tailExitIndex = events.length - 4;
	let headEnterIndex = 3;
	let index;
	let enter;
	if ((events[headEnterIndex][1].type === "lineEnding" || events[headEnterIndex][1].type === "space") && (events[tailExitIndex][1].type === "lineEnding" || events[tailExitIndex][1].type === "space")) {
		index = headEnterIndex;
		while (++index < tailExitIndex) if (events[index][1].type === "mathTextData") {
			events[tailExitIndex][1].type = "mathTextPadding";
			events[headEnterIndex][1].type = "mathTextPadding";
			headEnterIndex += 2;
			tailExitIndex -= 2;
			break;
		}
	}
	index = headEnterIndex - 1;
	tailExitIndex++;
	while (++index <= tailExitIndex) if (enter === void 0) {
		if (index !== tailExitIndex && events[index][1].type !== "lineEnding") enter = index;
	} else if (index === tailExitIndex || events[index][1].type === "lineEnding") {
		events[enter][1].type = "mathTextData";
		if (index !== enter + 2) {
			events[enter][1].end = events[index - 1][1].end;
			events.splice(enter + 2, index - enter - 2);
			tailExitIndex -= index - enter - 2;
			index = enter + 2;
		}
		enter = void 0;
	}
	return events;
}
function previous(code) {
	return code !== 36 || this.events[this.events.length - 1][1].type === "characterEscape";
}
function math(options) {
	return {
		flow: { [36]: mathFlow },
		text: { [36]: mathText(options) }
	};
}
var emptyOptions$3 = {};
function remarkMath(options) {
	const self = this;
	const settings = options || emptyOptions$3;
	const data = self.data();
	const micromarkExtensions = data.micromarkExtensions || (data.micromarkExtensions = []);
	const fromMarkdownExtensions = data.fromMarkdownExtensions || (data.fromMarkdownExtensions = []);
	const toMarkdownExtensions = data.toMarkdownExtensions || (data.toMarkdownExtensions = []);
	micromarkExtensions.push(math(settings));
	fromMarkdownExtensions.push(mathFromMarkdown());
	toMarkdownExtensions.push(mathToMarkdown(settings));
}
const findAfter = (function(parent, index, test) {
	const is = convert(test);
	if (!parent || !parent.type || !parent.children) throw new Error("Expected parent node");
	if (typeof index === "number") {
		if (index < 0 || index === Number.POSITIVE_INFINITY) throw new Error("Expected positive finite number as index");
	} else {
		index = parent.children.indexOf(index);
		if (index < 0) throw new Error("Expected child node or index");
	}
	while (++index < parent.children.length) if (is(parent.children[index], index, parent)) return parent.children[index];
});
const convertElement = (function(test) {
	if (test === null || test === void 0) return element$1;
	if (typeof test === "string") return tagNameFactory(test);
	if (typeof test === "object") return anyFactory(test);
	if (typeof test === "function") return castFactory(test);
	throw new Error("Expected function, string, or array as `test`");
});
function anyFactory(tests) {
	const checks = [];
	let index = -1;
	while (++index < tests.length) checks[index] = convertElement(tests[index]);
	return castFactory(any);
	function any(...parameters) {
		let index$1 = -1;
		while (++index$1 < checks.length) if (checks[index$1].apply(this, parameters)) return true;
		return false;
	}
}
function tagNameFactory(check) {
	return castFactory(tagName);
	function tagName(element$2) {
		return element$2.tagName === check;
	}
}
function castFactory(testFunction) {
	return check;
	function check(value, index, parent) {
		return Boolean(looksLikeAnElement(value) && testFunction.call(this, value, typeof index === "number" ? index : void 0, parent || void 0));
	}
}
function element$1(element$2) {
	return Boolean(element$2 && typeof element$2 === "object" && "type" in element$2 && element$2.type === "element" && "tagName" in element$2 && typeof element$2.tagName === "string");
}
function looksLikeAnElement(value) {
	return value !== null && typeof value === "object" && "type" in value && "tagName" in value;
}
var searchLineFeeds = /\n/g;
var searchTabOrSpaces = /[\t ]+/g;
var br = convertElement("br");
var cell = convertElement(isCell);
var p = convertElement("p");
var row = convertElement("tr");
var notRendered = convertElement([
	"datalist",
	"head",
	"noembed",
	"noframes",
	"noscript",
	"rp",
	"script",
	"style",
	"template",
	"title",
	hidden,
	closedDialog
]);
var blockOrCaption = convertElement([
	"address",
	"article",
	"aside",
	"blockquote",
	"body",
	"caption",
	"center",
	"dd",
	"dialog",
	"dir",
	"dl",
	"dt",
	"div",
	"figure",
	"figcaption",
	"footer",
	"form,",
	"h1",
	"h2",
	"h3",
	"h4",
	"h5",
	"h6",
	"header",
	"hgroup",
	"hr",
	"html",
	"legend",
	"li",
	"listing",
	"main",
	"menu",
	"nav",
	"ol",
	"p",
	"plaintext",
	"pre",
	"section",
	"ul",
	"xmp"
]);
function toText(tree, options) {
	const options_ = options || {};
	const children = "children" in tree ? tree.children : [];
	const block = blockOrCaption(tree);
	const whitespace = inferWhitespace(tree, {
		whitespace: options_.whitespace || "normal",
		breakBefore: false,
		breakAfter: false
	});
	const results = [];
	if (tree.type === "text" || tree.type === "comment") results.push(...collectText(tree, {
		whitespace,
		breakBefore: true,
		breakAfter: true
	}));
	let index = -1;
	while (++index < children.length) results.push(...renderedTextCollection(children[index], tree, {
		whitespace,
		breakBefore: index ? void 0 : block,
		breakAfter: index < children.length - 1 ? br(children[index + 1]) : block
	}));
	const result = [];
	let count;
	index = -1;
	while (++index < results.length) {
		const value = results[index];
		if (typeof value === "number") {
			if (count !== void 0 && value > count) count = value;
		} else if (value) {
			if (count !== void 0 && count > -1) result.push("\n".repeat(count) || " ");
			count = -1;
			result.push(value);
		}
	}
	return result.join("");
}
function renderedTextCollection(node, parent, info) {
	if (node.type === "element") return collectElement(node, parent, info);
	if (node.type === "text") return info.whitespace === "normal" ? collectText(node, info) : collectPreText(node);
	return [];
}
function collectElement(node, parent, info) {
	const whitespace = inferWhitespace(node, info);
	const children = node.children || [];
	let index = -1;
	let items = [];
	if (notRendered(node)) return items;
	let prefix;
	let suffix;
	if (br(node)) suffix = "\n";
	else if (row(node) && findAfter(parent, node, row)) suffix = "\n";
	else if (p(node)) {
		prefix = 2;
		suffix = 2;
	} else if (blockOrCaption(node)) {
		prefix = 1;
		suffix = 1;
	}
	while (++index < children.length) items = items.concat(renderedTextCollection(children[index], node, {
		whitespace,
		breakBefore: index ? void 0 : prefix,
		breakAfter: index < children.length - 1 ? br(children[index + 1]) : suffix
	}));
	if (cell(node) && findAfter(parent, node, cell)) items.push("	");
	if (prefix) items.unshift(prefix);
	if (suffix) items.push(suffix);
	return items;
}
function collectText(node, info) {
	const value = String(node.value);
	const lines = [];
	const result = [];
	let start = 0;
	while (start <= value.length) {
		searchLineFeeds.lastIndex = start;
		const match = searchLineFeeds.exec(value);
		const end = match && "index" in match ? match.index : value.length;
		lines.push(trimAndCollapseSpacesAndTabs(value.slice(start, end).replace(/[\u061C\u200E\u200F\u202A-\u202E\u2066-\u2069]/g, ""), start === 0 ? info.breakBefore : true, end === value.length ? info.breakAfter : true));
		start = end + 1;
	}
	let index = -1;
	let join;
	while (++index < lines.length) if (lines[index].charCodeAt(lines[index].length - 1) === 8203 || index < lines.length - 1 && lines[index + 1].charCodeAt(0) === 8203) {
		result.push(lines[index]);
		join = void 0;
	} else if (lines[index]) {
		if (typeof join === "number") result.push(join);
		result.push(lines[index]);
		join = 0;
	} else if (index === 0 || index === lines.length - 1) result.push(0);
	return result;
}
function collectPreText(node) {
	return [String(node.value)];
}
function trimAndCollapseSpacesAndTabs(value, breakBefore, breakAfter) {
	const result = [];
	let start = 0;
	let end;
	while (start < value.length) {
		searchTabOrSpaces.lastIndex = start;
		const match = searchTabOrSpaces.exec(value);
		end = match ? match.index : value.length;
		if (!start && !end && match && !breakBefore) result.push("");
		if (start !== end) result.push(value.slice(start, end));
		start = match ? end + match[0].length : end;
	}
	if (start !== end && !breakAfter) result.push("");
	return result.join(" ");
}
function inferWhitespace(node, info) {
	if (node.type === "element") {
		const properties = node.properties || {};
		switch (node.tagName) {
			case "listing":
			case "plaintext":
			case "xmp": return "pre";
			case "nobr": return "nowrap";
			case "pre": return properties.wrap ? "pre-wrap" : "pre";
			case "td":
			case "th": return properties.noWrap ? "nowrap" : info.whitespace;
			case "textarea": return "pre-wrap";
			default:
		}
	}
	return info.whitespace;
}
function hidden(node) {
	return Boolean((node.properties || {}).hidden);
}
function isCell(node) {
	return node.tagName === "td" || node.tagName === "th";
}
function closedDialog(node) {
	return node.tagName === "dialog" && !(node.properties || {}).open;
}
var emptyOptions$2 = {};
function rehypeHighlight(options) {
	const settings = options || emptyOptions$2;
	const aliases = settings.aliases;
	const detect = settings.detect || false;
	const languages = settings.languages || grammars;
	const plainText = settings.plainText;
	const prefix = settings.prefix;
	const subset = settings.subset;
	let name = "hljs";
	const lowlight = createLowlight(languages);
	if (aliases) lowlight.registerAlias(aliases);
	if (prefix) {
		const pos = prefix.indexOf("-");
		name = pos === -1 ? prefix : prefix.slice(0, pos);
	}
	return function(tree, file) {
		visit(tree, "element", function(node, _, parent) {
			if (node.tagName !== "code" || !parent || parent.type !== "element" || parent.tagName !== "pre") return;
			const lang = language(node);
			if (lang === false || !lang && !detect || lang && plainText && plainText.includes(lang)) return;
			if (!Array.isArray(node.properties.className)) node.properties.className = [];
			if (!node.properties.className.includes(name)) node.properties.className.unshift(name);
			const text$1 = toText(node, { whitespace: "pre" });
			let result;
			try {
				result = lang ? lowlight.highlight(lang, text$1, { prefix }) : lowlight.highlightAuto(text$1, {
					prefix,
					subset
				});
			} catch (error) {
				const cause = error;
				if (lang && /Unknown language/.test(cause.message)) {
					file.message("Cannot highlight as `" + lang + "`, it’s not registered", {
						ancestors: [parent, node],
						cause,
						place: node.position,
						ruleId: "missing-language",
						source: "rehype-highlight"
					});
					/* c8 ignore next 5 -- throw arbitrary hljs errors */
					return;
				}
				throw cause;
			}
			if (!lang && result.data && result.data.language) node.properties.className.push("language-" + result.data.language);
			if (result.children.length > 0) node.children = result.children;
		});
	};
}
function language(node) {
	const list = node.properties.className;
	let index = -1;
	if (!Array.isArray(list)) return;
	let name;
	while (++index < list.length) {
		const value = String(list[index]);
		if (value === "no-highlight" || value === "nohighlight") return false;
		if (!name && value.slice(0, 5) === "lang-") name = value.slice(5);
		if (!name && value.slice(0, 9) === "language-") name = value.slice(9);
	}
	return name;
}
function fromDom(tree, options) {
	return transform(tree, options || {}) || {
		type: "root",
		children: []
	};
}
function transform(node, options) {
	const transformed = one$1(node, options);
	if (transformed && options.afterTransform) options.afterTransform(node, transformed);
	return transformed;
}
function one$1(node, options) {
	switch (node.nodeType) {
		case 1: return element(node, options);
		case 3: return text(node);
		case 8: return comment(node);
		case 9: return root(node, options);
		case 10: return doctype();
		case 11: return root(node, options);
		default: return;
	}
}
function root(node, options) {
	return {
		type: "root",
		children: all$1(node, options)
	};
}
function doctype() {
	return { type: "doctype" };
}
function text(node) {
	return {
		type: "text",
		value: node.nodeValue || ""
	};
}
function comment(node) {
	return {
		type: "comment",
		value: node.nodeValue || ""
	};
}
function element(node, options) {
	const space = node.namespaceURI;
	const x = space === webNamespaces.svg ? s : h;
	const tagName = space === webNamespaces.html ? node.tagName.toLowerCase() : node.tagName;
	const content = space === webNamespaces.html && tagName === "template" ? node.content : node;
	const attributes = node.getAttributeNames();
	const properties = {};
	let index = -1;
	while (++index < attributes.length) properties[attributes[index]] = node.getAttribute(attributes[index]) || "";
	return x(tagName, properties, all$1(content, options));
}
function all$1(node, options) {
	const nodes = node.childNodes;
	const children = [];
	let index = -1;
	while (++index < nodes.length) {
		const child = transform(nodes[index], options);
		if (child !== void 0) children.push(child);
	}
	return children;
}
var parser = new DOMParser();
function fromHtmlIsomorphic(value, options) {
	return fromDom(options?.fragment ? parseFragment(value) : parser.parseFromString(value, "text/html"));
}
function parseFragment(value) {
	const template = document.createElement("template");
	template.innerHTML = value;
	return template.content;
}
var emptyOptions$1 = {};
var emptyClasses = [];
function rehypeKatex(options) {
	const settings = options || emptyOptions$1;
	return function(tree, file) {
		visitParents(tree, "element", function(element$2, parents) {
			const classes = Array.isArray(element$2.properties.className) ? element$2.properties.className : emptyClasses;
			const languageMath = classes.includes("language-math");
			const mathDisplay = classes.includes("math-display");
			const mathInline = classes.includes("math-inline");
			let displayMode = mathDisplay;
			if (!languageMath && !mathDisplay && !mathInline) return;
			let parent = parents[parents.length - 1];
			let scope = element$2;
			if (element$2.tagName === "code" && languageMath && parent && parent.type === "element" && parent.tagName === "pre") {
				scope = parent;
				parent = parents[parents.length - 2];
				displayMode = true;
			}
			/* c8 ignore next -- verbose to test. */
			if (!parent) return;
			const value = toText(scope, { whitespace: "pre" });
			let result;
			try {
				result = katex.renderToString(value, {
					...settings,
					displayMode,
					throwOnError: true
				});
			} catch (error) {
				const cause = error;
				const ruleId = cause.name.toLowerCase();
				file.message("Could not render math with KaTeX", {
					ancestors: [...parents, element$2],
					cause,
					place: element$2.position,
					ruleId,
					source: "rehype-katex"
				});
				try {
					result = katex.renderToString(value, {
						...settings,
						displayMode,
						strict: "ignore",
						throwOnError: false
					});
				} catch {
					result = [{
						type: "element",
						tagName: "span",
						properties: {
							className: ["katex-error"],
							style: "color:" + (settings.errorColor || "#cc0000"),
							title: String(error)
						},
						children: [{
							type: "text",
							value
						}]
					}];
				}
			}
			if (typeof result === "string") result = fromHtmlIsomorphic(result, { fragment: true }).children;
			const index = parent.children.indexOf(scope);
			parent.children.splice(index, 1, ...result);
			return SKIP;
		});
	};
}
const regex = /[\0-\x1F!-,\.\/:-@\[-\^`\{-\xA9\xAB-\xB4\xB6-\xB9\xBB-\xBF\xD7\xF7\u02C2-\u02C5\u02D2-\u02DF\u02E5-\u02EB\u02ED\u02EF-\u02FF\u0375\u0378\u0379\u037E\u0380-\u0385\u0387\u038B\u038D\u03A2\u03F6\u0482\u0530\u0557\u0558\u055A-\u055F\u0589-\u0590\u05BE\u05C0\u05C3\u05C6\u05C8-\u05CF\u05EB-\u05EE\u05F3-\u060F\u061B-\u061F\u066A-\u066D\u06D4\u06DD\u06DE\u06E9\u06FD\u06FE\u0700-\u070F\u074B\u074C\u07B2-\u07BF\u07F6-\u07F9\u07FB\u07FC\u07FE\u07FF\u082E-\u083F\u085C-\u085F\u086B-\u089F\u08B5\u08C8-\u08D2\u08E2\u0964\u0965\u0970\u0984\u098D\u098E\u0991\u0992\u09A9\u09B1\u09B3-\u09B5\u09BA\u09BB\u09C5\u09C6\u09C9\u09CA\u09CF-\u09D6\u09D8-\u09DB\u09DE\u09E4\u09E5\u09F2-\u09FB\u09FD\u09FF\u0A00\u0A04\u0A0B-\u0A0E\u0A11\u0A12\u0A29\u0A31\u0A34\u0A37\u0A3A\u0A3B\u0A3D\u0A43-\u0A46\u0A49\u0A4A\u0A4E-\u0A50\u0A52-\u0A58\u0A5D\u0A5F-\u0A65\u0A76-\u0A80\u0A84\u0A8E\u0A92\u0AA9\u0AB1\u0AB4\u0ABA\u0ABB\u0AC6\u0ACA\u0ACE\u0ACF\u0AD1-\u0ADF\u0AE4\u0AE5\u0AF0-\u0AF8\u0B00\u0B04\u0B0D\u0B0E\u0B11\u0B12\u0B29\u0B31\u0B34\u0B3A\u0B3B\u0B45\u0B46\u0B49\u0B4A\u0B4E-\u0B54\u0B58-\u0B5B\u0B5E\u0B64\u0B65\u0B70\u0B72-\u0B81\u0B84\u0B8B-\u0B8D\u0B91\u0B96-\u0B98\u0B9B\u0B9D\u0BA0-\u0BA2\u0BA5-\u0BA7\u0BAB-\u0BAD\u0BBA-\u0BBD\u0BC3-\u0BC5\u0BC9\u0BCE\u0BCF\u0BD1-\u0BD6\u0BD8-\u0BE5\u0BF0-\u0BFF\u0C0D\u0C11\u0C29\u0C3A-\u0C3C\u0C45\u0C49\u0C4E-\u0C54\u0C57\u0C5B-\u0C5F\u0C64\u0C65\u0C70-\u0C7F\u0C84\u0C8D\u0C91\u0CA9\u0CB4\u0CBA\u0CBB\u0CC5\u0CC9\u0CCE-\u0CD4\u0CD7-\u0CDD\u0CDF\u0CE4\u0CE5\u0CF0\u0CF3-\u0CFF\u0D0D\u0D11\u0D45\u0D49\u0D4F-\u0D53\u0D58-\u0D5E\u0D64\u0D65\u0D70-\u0D79\u0D80\u0D84\u0D97-\u0D99\u0DB2\u0DBC\u0DBE\u0DBF\u0DC7-\u0DC9\u0DCB-\u0DCE\u0DD5\u0DD7\u0DE0-\u0DE5\u0DF0\u0DF1\u0DF4-\u0E00\u0E3B-\u0E3F\u0E4F\u0E5A-\u0E80\u0E83\u0E85\u0E8B\u0EA4\u0EA6\u0EBE\u0EBF\u0EC5\u0EC7\u0ECE\u0ECF\u0EDA\u0EDB\u0EE0-\u0EFF\u0F01-\u0F17\u0F1A-\u0F1F\u0F2A-\u0F34\u0F36\u0F38\u0F3A-\u0F3D\u0F48\u0F6D-\u0F70\u0F85\u0F98\u0FBD-\u0FC5\u0FC7-\u0FFF\u104A-\u104F\u109E\u109F\u10C6\u10C8-\u10CC\u10CE\u10CF\u10FB\u1249\u124E\u124F\u1257\u1259\u125E\u125F\u1289\u128E\u128F\u12B1\u12B6\u12B7\u12BF\u12C1\u12C6\u12C7\u12D7\u1311\u1316\u1317\u135B\u135C\u1360-\u137F\u1390-\u139F\u13F6\u13F7\u13FE-\u1400\u166D\u166E\u1680\u169B-\u169F\u16EB-\u16ED\u16F9-\u16FF\u170D\u1715-\u171F\u1735-\u173F\u1754-\u175F\u176D\u1771\u1774-\u177F\u17D4-\u17D6\u17D8-\u17DB\u17DE\u17DF\u17EA-\u180A\u180E\u180F\u181A-\u181F\u1879-\u187F\u18AB-\u18AF\u18F6-\u18FF\u191F\u192C-\u192F\u193C-\u1945\u196E\u196F\u1975-\u197F\u19AC-\u19AF\u19CA-\u19CF\u19DA-\u19FF\u1A1C-\u1A1F\u1A5F\u1A7D\u1A7E\u1A8A-\u1A8F\u1A9A-\u1AA6\u1AA8-\u1AAF\u1AC1-\u1AFF\u1B4C-\u1B4F\u1B5A-\u1B6A\u1B74-\u1B7F\u1BF4-\u1BFF\u1C38-\u1C3F\u1C4A-\u1C4C\u1C7E\u1C7F\u1C89-\u1C8F\u1CBB\u1CBC\u1CC0-\u1CCF\u1CD3\u1CFB-\u1CFF\u1DFA\u1F16\u1F17\u1F1E\u1F1F\u1F46\u1F47\u1F4E\u1F4F\u1F58\u1F5A\u1F5C\u1F5E\u1F7E\u1F7F\u1FB5\u1FBD\u1FBF-\u1FC1\u1FC5\u1FCD-\u1FCF\u1FD4\u1FD5\u1FDC-\u1FDF\u1FED-\u1FF1\u1FF5\u1FFD-\u203E\u2041-\u2053\u2055-\u2070\u2072-\u207E\u2080-\u208F\u209D-\u20CF\u20F1-\u2101\u2103-\u2106\u2108\u2109\u2114\u2116-\u2118\u211E-\u2123\u2125\u2127\u2129\u212E\u213A\u213B\u2140-\u2144\u214A-\u214D\u214F-\u215F\u2189-\u24B5\u24EA-\u2BFF\u2C2F\u2C5F\u2CE5-\u2CEA\u2CF4-\u2CFF\u2D26\u2D28-\u2D2C\u2D2E\u2D2F\u2D68-\u2D6E\u2D70-\u2D7E\u2D97-\u2D9F\u2DA7\u2DAF\u2DB7\u2DBF\u2DC7\u2DCF\u2DD7\u2DDF\u2E00-\u2E2E\u2E30-\u3004\u3008-\u3020\u3030\u3036\u3037\u303D-\u3040\u3097\u3098\u309B\u309C\u30A0\u30FB\u3100-\u3104\u3130\u318F-\u319F\u31C0-\u31EF\u3200-\u33FF\u4DC0-\u4DFF\u9FFD-\u9FFF\uA48D-\uA4CF\uA4FE\uA4FF\uA60D-\uA60F\uA62C-\uA63F\uA673\uA67E\uA6F2-\uA716\uA720\uA721\uA789\uA78A\uA7C0\uA7C1\uA7CB-\uA7F4\uA828-\uA82B\uA82D-\uA83F\uA874-\uA87F\uA8C6-\uA8CF\uA8DA-\uA8DF\uA8F8-\uA8FA\uA8FC\uA92E\uA92F\uA954-\uA95F\uA97D-\uA97F\uA9C1-\uA9CE\uA9DA-\uA9DF\uA9FF\uAA37-\uAA3F\uAA4E\uAA4F\uAA5A-\uAA5F\uAA77-\uAA79\uAAC3-\uAADA\uAADE\uAADF\uAAF0\uAAF1\uAAF7-\uAB00\uAB07\uAB08\uAB0F\uAB10\uAB17-\uAB1F\uAB27\uAB2F\uAB5B\uAB6A-\uAB6F\uABEB\uABEE\uABEF\uABFA-\uABFF\uD7A4-\uD7AF\uD7C7-\uD7CA\uD7FC-\uD7FF\uE000-\uF8FF\uFA6E\uFA6F\uFADA-\uFAFF\uFB07-\uFB12\uFB18-\uFB1C\uFB29\uFB37\uFB3D\uFB3F\uFB42\uFB45\uFBB2-\uFBD2\uFD3E-\uFD4F\uFD90\uFD91\uFDC8-\uFDEF\uFDFC-\uFDFF\uFE10-\uFE1F\uFE30-\uFE32\uFE35-\uFE4C\uFE50-\uFE6F\uFE75\uFEFD-\uFF0F\uFF1A-\uFF20\uFF3B-\uFF3E\uFF40\uFF5B-\uFF65\uFFBF-\uFFC1\uFFC8\uFFC9\uFFD0\uFFD1\uFFD8\uFFD9\uFFDD-\uFFFF]|\uD800[\uDC0C\uDC27\uDC3B\uDC3E\uDC4E\uDC4F\uDC5E-\uDC7F\uDCFB-\uDD3F\uDD75-\uDDFC\uDDFE-\uDE7F\uDE9D-\uDE9F\uDED1-\uDEDF\uDEE1-\uDEFF\uDF20-\uDF2C\uDF4B-\uDF4F\uDF7B-\uDF7F\uDF9E\uDF9F\uDFC4-\uDFC7\uDFD0\uDFD6-\uDFFF]|\uD801[\uDC9E\uDC9F\uDCAA-\uDCAF\uDCD4-\uDCD7\uDCFC-\uDCFF\uDD28-\uDD2F\uDD64-\uDDFF\uDF37-\uDF3F\uDF56-\uDF5F\uDF68-\uDFFF]|\uD802[\uDC06\uDC07\uDC09\uDC36\uDC39-\uDC3B\uDC3D\uDC3E\uDC56-\uDC5F\uDC77-\uDC7F\uDC9F-\uDCDF\uDCF3\uDCF6-\uDCFF\uDD16-\uDD1F\uDD3A-\uDD7F\uDDB8-\uDDBD\uDDC0-\uDDFF\uDE04\uDE07-\uDE0B\uDE14\uDE18\uDE36\uDE37\uDE3B-\uDE3E\uDE40-\uDE5F\uDE7D-\uDE7F\uDE9D-\uDEBF\uDEC8\uDEE7-\uDEFF\uDF36-\uDF3F\uDF56-\uDF5F\uDF73-\uDF7F\uDF92-\uDFFF]|\uD803[\uDC49-\uDC7F\uDCB3-\uDCBF\uDCF3-\uDCFF\uDD28-\uDD2F\uDD3A-\uDE7F\uDEAA\uDEAD-\uDEAF\uDEB2-\uDEFF\uDF1D-\uDF26\uDF28-\uDF2F\uDF51-\uDFAF\uDFC5-\uDFDF\uDFF7-\uDFFF]|\uD804[\uDC47-\uDC65\uDC70-\uDC7E\uDCBB-\uDCCF\uDCE9-\uDCEF\uDCFA-\uDCFF\uDD35\uDD40-\uDD43\uDD48-\uDD4F\uDD74\uDD75\uDD77-\uDD7F\uDDC5-\uDDC8\uDDCD\uDDDB\uDDDD-\uDDFF\uDE12\uDE38-\uDE3D\uDE3F-\uDE7F\uDE87\uDE89\uDE8E\uDE9E\uDEA9-\uDEAF\uDEEB-\uDEEF\uDEFA-\uDEFF\uDF04\uDF0D\uDF0E\uDF11\uDF12\uDF29\uDF31\uDF34\uDF3A\uDF45\uDF46\uDF49\uDF4A\uDF4E\uDF4F\uDF51-\uDF56\uDF58-\uDF5C\uDF64\uDF65\uDF6D-\uDF6F\uDF75-\uDFFF]|\uD805[\uDC4B-\uDC4F\uDC5A-\uDC5D\uDC62-\uDC7F\uDCC6\uDCC8-\uDCCF\uDCDA-\uDD7F\uDDB6\uDDB7\uDDC1-\uDDD7\uDDDE-\uDDFF\uDE41-\uDE43\uDE45-\uDE4F\uDE5A-\uDE7F\uDEB9-\uDEBF\uDECA-\uDEFF\uDF1B\uDF1C\uDF2C-\uDF2F\uDF3A-\uDFFF]|\uD806[\uDC3B-\uDC9F\uDCEA-\uDCFE\uDD07\uDD08\uDD0A\uDD0B\uDD14\uDD17\uDD36\uDD39\uDD3A\uDD44-\uDD4F\uDD5A-\uDD9F\uDDA8\uDDA9\uDDD8\uDDD9\uDDE2\uDDE5-\uDDFF\uDE3F-\uDE46\uDE48-\uDE4F\uDE9A-\uDE9C\uDE9E-\uDEBF\uDEF9-\uDFFF]|\uD807[\uDC09\uDC37\uDC41-\uDC4F\uDC5A-\uDC71\uDC90\uDC91\uDCA8\uDCB7-\uDCFF\uDD07\uDD0A\uDD37-\uDD39\uDD3B\uDD3E\uDD48-\uDD4F\uDD5A-\uDD5F\uDD66\uDD69\uDD8F\uDD92\uDD99-\uDD9F\uDDAA-\uDEDF\uDEF7-\uDFAF\uDFB1-\uDFFF]|\uD808[\uDF9A-\uDFFF]|\uD809[\uDC6F-\uDC7F\uDD44-\uDFFF]|[\uD80A\uD80B\uD80E-\uD810\uD812-\uD819\uD824-\uD82B\uD82D\uD82E\uD830-\uD833\uD837\uD839\uD83D\uD83F\uD87B-\uD87D\uD87F\uD885-\uDB3F\uDB41-\uDBFF][\uDC00-\uDFFF]|\uD80D[\uDC2F-\uDFFF]|\uD811[\uDE47-\uDFFF]|\uD81A[\uDE39-\uDE3F\uDE5F\uDE6A-\uDECF\uDEEE\uDEEF\uDEF5-\uDEFF\uDF37-\uDF3F\uDF44-\uDF4F\uDF5A-\uDF62\uDF78-\uDF7C\uDF90-\uDFFF]|\uD81B[\uDC00-\uDE3F\uDE80-\uDEFF\uDF4B-\uDF4E\uDF88-\uDF8E\uDFA0-\uDFDF\uDFE2\uDFE5-\uDFEF\uDFF2-\uDFFF]|\uD821[\uDFF8-\uDFFF]|\uD823[\uDCD6-\uDCFF\uDD09-\uDFFF]|\uD82C[\uDD1F-\uDD4F\uDD53-\uDD63\uDD68-\uDD6F\uDEFC-\uDFFF]|\uD82F[\uDC6B-\uDC6F\uDC7D-\uDC7F\uDC89-\uDC8F\uDC9A-\uDC9C\uDC9F-\uDFFF]|\uD834[\uDC00-\uDD64\uDD6A-\uDD6C\uDD73-\uDD7A\uDD83\uDD84\uDD8C-\uDDA9\uDDAE-\uDE41\uDE45-\uDFFF]|\uD835[\uDC55\uDC9D\uDCA0\uDCA1\uDCA3\uDCA4\uDCA7\uDCA8\uDCAD\uDCBA\uDCBC\uDCC4\uDD06\uDD0B\uDD0C\uDD15\uDD1D\uDD3A\uDD3F\uDD45\uDD47-\uDD49\uDD51\uDEA6\uDEA7\uDEC1\uDEDB\uDEFB\uDF15\uDF35\uDF4F\uDF6F\uDF89\uDFA9\uDFC3\uDFCC\uDFCD]|\uD836[\uDC00-\uDDFF\uDE37-\uDE3A\uDE6D-\uDE74\uDE76-\uDE83\uDE85-\uDE9A\uDEA0\uDEB0-\uDFFF]|\uD838[\uDC07\uDC19\uDC1A\uDC22\uDC25\uDC2B-\uDCFF\uDD2D-\uDD2F\uDD3E\uDD3F\uDD4A-\uDD4D\uDD4F-\uDEBF\uDEFA-\uDFFF]|\uD83A[\uDCC5-\uDCCF\uDCD7-\uDCFF\uDD4C-\uDD4F\uDD5A-\uDFFF]|\uD83B[\uDC00-\uDDFF\uDE04\uDE20\uDE23\uDE25\uDE26\uDE28\uDE33\uDE38\uDE3A\uDE3C-\uDE41\uDE43-\uDE46\uDE48\uDE4A\uDE4C\uDE50\uDE53\uDE55\uDE56\uDE58\uDE5A\uDE5C\uDE5E\uDE60\uDE63\uDE65\uDE66\uDE6B\uDE73\uDE78\uDE7D\uDE7F\uDE8A\uDE9C-\uDEA0\uDEA4\uDEAA\uDEBC-\uDFFF]|\uD83C[\uDC00-\uDD2F\uDD4A-\uDD4F\uDD6A-\uDD6F\uDD8A-\uDFFF]|\uD83E[\uDC00-\uDFEF\uDFFA-\uDFFF]|\uD869[\uDEDE-\uDEFF]|\uD86D[\uDF35-\uDF3F]|\uD86E[\uDC1E\uDC1F]|\uD873[\uDEA2-\uDEAF]|\uD87A[\uDFE1-\uDFFF]|\uD87E[\uDE1E-\uDFFF]|\uD884[\uDF4B-\uDFFF]|\uDB40[\uDC00-\uDCFF\uDDF0-\uDFFF]/g;
var own = Object.hasOwnProperty;
var BananaSlug = class {
	constructor() {
		this.occurrences;
		this.reset();
	}
	slug(value, maintainCase) {
		const self = this;
		let result = slug(value, maintainCase === true);
		const originalSlug = result;
		while (own.call(self.occurrences, result)) {
			self.occurrences[originalSlug]++;
			result = originalSlug + "-" + self.occurrences[originalSlug];
		}
		self.occurrences[result] = 0;
		return result;
	}
	reset() {
		this.occurrences = Object.create(null);
	}
};
function slug(value, maintainCase) {
	if (typeof value !== "string") return "";
	if (!maintainCase) value = value.toLowerCase();
	return value.replace(regex, "").replace(/ /g, "-");
}
function headingRank(node) {
	const name = node.type === "element" ? node.tagName.toLowerCase() : "";
	const code = name.length === 2 && name.charCodeAt(0) === 104 ? name.charCodeAt(1) : 0;
	return code > 48 && code < 55 ? code - 48 : void 0;
}
function toString(node) {
	if ("children" in node) return all(node);
	return "value" in node ? node.value : "";
}
function one(node) {
	if (node.type === "text") return node.value;
	return "children" in node ? all(node) : "";
}
function all(node) {
	let index = -1;
	const result = [];
	while (++index < node.children.length) result[index] = one(node.children[index]);
	return result.join("");
}
var emptyOptions = {};
var slugs = new BananaSlug();
function rehypeSlug(options) {
	const prefix = (options || emptyOptions).prefix || "";
	return function(tree) {
		slugs.reset();
		visit(tree, "element", function(node) {
			if (headingRank(node) && !node.properties.id) node.properties.id = prefix + slugs.slug(toString(node));
		});
	};
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function CodeBlockCopyButton({ children, ...props }) {
	const [copied, setCopied] = (0, import_react.useState)(false);
	const copiedResetTimerRef = (0, import_react.useRef)(null);
	const isMountedRef = (0, import_react.useRef)(false);
	const clearCopiedResetTimer = (0, import_react.useCallback)(() => {
		if (copiedResetTimerRef.current !== null) {
			window.clearTimeout(copiedResetTimerRef.current);
			copiedResetTimerRef.current = null;
		}
	}, []);
	const setCopyButtonRef = (0, import_react.useCallback)((node) => {
		isMountedRef.current = node !== null;
		if (node === null) clearCopiedResetTimer();
	}, [clearCopiedResetTimer]);
	const handleCopy = (0, import_react.useCallback)(() => {
		let text$1 = "";
		import_react.Children.forEach(children, (child) => {
			if (import_react.isValidElement(child) && child.props) {
				const inner = child.props.children;
				text$1 += typeof inner === "string" ? inner : extractText(inner);
			} else if (typeof child === "string") text$1 += child;
		});
		window.api.ui.writeClipboardText(text$1).then(() => {
			if (!isMountedRef.current) return;
			clearCopiedResetTimer();
			setCopied(true);
			copiedResetTimerRef.current = window.setTimeout(() => {
				copiedResetTimerRef.current = null;
				setCopied(false);
			}, 1500);
		}).catch(() => {});
	}, [children, clearCopiedResetTimer]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "code-block-wrapper",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
			...props,
			children
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
			ref: setCopyButtonRef,
			type: "button",
			className: "code-block-copy-btn",
			onClick: handleCopy,
			"aria-label": translate("auto.components.editor.CodeBlockCopyButton.1f9f4def45", "Copy code"),
			title: translate("auto.components.editor.CodeBlockCopyButton.1f9f4def45", "Copy code"),
			children: copied ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { size: 14 }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "code-block-copy-label",
				children: translate("auto.components.editor.CodeBlockCopyButton.28921f5bf9", "Copied")
			})] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { size: 14 })
		})]
	});
}
function extractText(node) {
	if (typeof node === "string" || typeof node === "number") return String(node);
	if (Array.isArray(node)) return node.map(extractText).join("");
	if (import_react.isValidElement(node) && node.props) return extractText(node.props.children);
	return "";
}
function isMarkdownPreviewAddReviewNoteShortcut(event, platform, keybindings) {
	return keybindingMatchesAction("editor.addReviewNote", event, platform, keybindings);
}
function closestAnnotationBlockKey(node, root$1) {
	const block = (node instanceof Element ? node : node?.parentElement ?? null)?.closest("[data-annotation-block-key]") ?? null;
	if (!block || !root$1.contains(block)) return null;
	return block.getAttribute("data-annotation-block-key");
}
function getMarkdownAnnotationBlockKeyForSelection(root$1, selection) {
	if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null;
	return closestAnnotationBlockKey(selection.anchorNode, root$1) ?? closestAnnotationBlockKey(selection.focusNode, root$1);
}
function previewHasAnnotationBlockKey(root$1, blockKey) {
	for (const block of root$1.querySelectorAll("[data-annotation-block-key]")) if (block.getAttribute("data-annotation-block-key") === blockKey) return true;
	return false;
}
function resolveMarkdownPreviewAddReviewNoteKey(options) {
	const { event, platform, keybindings, targetInsidePreview, markdownAnnotationsEnabled, activeAnnotationBlockKey, root: root$1, selection } = options;
	if (!isMarkdownPreviewAddReviewNoteShortcut(event, platform, keybindings) || !targetInsidePreview || !markdownAnnotationsEnabled) return { action: "ignore" };
	if (activeAnnotationBlockKey) {
		if (previewHasAnnotationBlockKey(root$1, activeAnnotationBlockKey)) return { action: "consume" };
		if (event.repeat) return { action: "clear-stale-and-ignore" };
		const blockKey$1 = getMarkdownAnnotationBlockKeyForSelection(root$1, selection);
		if (blockKey$1) return {
			action: "open",
			blockKey: blockKey$1
		};
		return { action: "clear-stale-and-ignore" };
	}
	if (event.repeat) return { action: "ignore" };
	const blockKey = getMarkdownAnnotationBlockKeyForSelection(root$1, selection);
	if (blockKey) return {
		action: "open",
		blockKey
	};
	return { action: "ignore" };
}
function usePreserveSectionDuringExternalEdit(content, bodyRef) {
	const [renderedContent, setRenderedContent] = (0, import_react.useState)(content);
	const pendingContentRef = (0, import_react.useRef)(content);
	pendingContentRef.current = content;
	(0, import_react.useEffect)(() => {
		if (content === renderedContent) return;
		const body = bodyRef.current;
		const hasSelectionInsideBody = () => {
			if (!body) return false;
			const selection = window.getSelection();
			if (!selection || selection.isCollapsed) return false;
			const anchor = selection.anchorNode;
			const focus = selection.focusNode;
			return anchor instanceof Node && body.contains(anchor) || focus instanceof Node && body.contains(focus);
		};
		if (!hasSelectionInsideBody()) {
			setRenderedContent(content);
			return;
		}
		const deadline = performance.now() + 3e3;
		let frameId = 0;
		const waitForSelectionRelease = () => {
			if (performance.now() >= deadline || !hasSelectionInsideBody()) {
				setRenderedContent(pendingContentRef.current);
				return;
			}
			frameId = window.requestAnimationFrame(waitForSelectionRelease);
		};
		frameId = window.requestAnimationFrame(waitForSelectionRelease);
		return () => window.cancelAnimationFrame(frameId);
	}, [
		bodyRef,
		content,
		renderedContent
	]);
	return renderedContent;
}
function markdownPreviewUrlTransform(value, key) {
	if ((key === "href" || key === "src") && value.toLowerCase().startsWith("file:")) return value;
	return defaultUrlTransform(value);
}
function normalizeReferenceIdentifier(identifier) {
	return identifier.trim().replace(/\s+/g, " ").toLowerCase();
}
function collectImageDefinitions(node, definitions) {
	if (node.type === "definition" && typeof node.identifier === "string" && typeof node.url === "string") definitions.set(normalizeReferenceIdentifier(node.identifier), node.url);
	for (const child of node.children ?? []) collectImageDefinitions(child, definitions);
}
function extractMarkdownPreviewLocalImageCandidates(markdown, filePath, options = {}) {
	const limit = Math.max(0, options.limit ?? 64);
	if (limit === 0) return [];
	const tree = unified().use(remarkParse).use(remarkGfm).use(remarkFrontmatter, ["yaml", "toml"]).parse(markdown);
	const definitions = /* @__PURE__ */ new Map();
	const candidates = [];
	const seenCacheKeys = /* @__PURE__ */ new Set();
	collectImageDefinitions(tree, definitions);
	function appendRawSrc(rawSrc) {
		if (candidates.length >= limit) return;
		const absolutePath = resolveImageAbsolutePath(rawSrc, filePath);
		if (!absolutePath) return;
		const cacheKey = getLocalImageCacheKey(absolutePath, options.connectionId, options.runtimeContext);
		if (seenCacheKeys.has(cacheKey)) return;
		seenCacheKeys.add(cacheKey);
		candidates.push({
			absolutePath,
			cacheKey,
			rawSrc
		});
	}
	function visit$1(node) {
		if (candidates.length >= limit) return;
		if (node.type === "image" && typeof node.url === "string") appendRawSrc(node.url);
		else if (node.type === "imageReference" && typeof node.identifier === "string") {
			const rawSrc = definitions.get(normalizeReferenceIdentifier(node.identifier));
			if (rawSrc) appendRawSrc(rawSrc);
		}
		for (const child of node.children ?? []) visit$1(child);
	}
	visit$1(tree);
	return candidates;
}
function prewarmMarkdownPreviewLocalImages(markdown, filePath, options = {}) {
	const candidates = extractMarkdownPreviewLocalImageCandidates(markdown, filePath, options);
	const concurrency = Math.max(1, options.concurrency ?? 4);
	const loadImage = options.loadImage ?? ((candidate) => loadLocalImageAbsolutePath(candidate.absolutePath, options.connectionId, options.runtimeContext));
	let cancelled = false;
	let nextIndex = 0;
	let activeCount = 0;
	let resolveDone;
	const done = new Promise((resolve) => {
		resolveDone = resolve;
	});
	const settleIfFinished = () => {
		if ((cancelled || nextIndex >= candidates.length) && activeCount === 0) resolveDone();
	};
	const scheduleNext = () => {
		while (!cancelled && activeCount < concurrency && nextIndex < candidates.length) {
			const candidate = candidates[nextIndex];
			nextIndex += 1;
			if (!candidate) continue;
			activeCount += 1;
			let loadPromise;
			try {
				loadPromise = loadImage(candidate);
			} catch {
				loadPromise = Promise.resolve();
			}
			loadPromise.catch(() => void 0).finally(() => {
				activeCount -= 1;
				scheduleNext();
				settleIfFinished();
			});
		}
		settleIfFinished();
	};
	scheduleNext();
	return {
		cancel: () => {
			cancelled = true;
			settleIfFinished();
		},
		done
	};
}
var EMPTY_MARKDOWN_DOCUMENTS = [];
function isMarkdownAnnotationNavigationClick(target) {
	if (!(target instanceof HTMLElement)) return false;
	return !target.closest("a,button,input,textarea,select,summary,[contenteditable=\"true\"],.markdown-annotation-controls");
}
function findMarkdownPreviewSourceOpenFile(openFiles, params) {
	const ownerMatches = (file) => (!params.sourceWorktreeId || file.worktreeId === params.sourceWorktreeId) && (params.sourceRuntimeEnvironmentId === void 0 || (file.runtimeEnvironmentId ?? null) === (params.sourceRuntimeEnvironmentId ?? null));
	if (params.sourceFileId) return openFiles.find((file) => file.id === params.sourceFileId && ownerMatches(file)) ?? openFiles.find((file) => file.mode === "markdown-preview" && file.filePath === params.filePath && file.markdownPreviewSourceFileId === params.sourceFileId && ownerMatches(file)) ?? openFiles.find((file) => file.id === params.sourceFileId);
	return openFiles.find((file) => file.filePath === params.filePath && ownerMatches(file));
}
function findMarkdownPreviewOpenedEditFileId(openFiles, activeFileIdByWorktree, params) {
	const activeFileId = activeFileIdByWorktree[params.worktreeId];
	const activeFile = openFiles.find((file) => file.id === activeFileId && file.filePath === params.filePath && file.worktreeId === params.worktreeId && file.mode === "edit");
	if (activeFile) return activeFile.id;
	return openFiles.find((file) => file.filePath === params.filePath && file.worktreeId === params.worktreeId && file.mode === "edit")?.id ?? params.filePath;
}
function getMarkdownPreviewAnchorScrollTop(container, target) {
	const containerTop = container.getBoundingClientRect().top;
	const targetTop = target.getBoundingClientRect().top;
	return Math.max(0, targetTop - containerTop + container.scrollTop - 12);
}
function cancelMarkdownPreviewEditorRevealFrames(frameIds) {
	for (const frameId of frameIds.current) cancelAnimationFrame(frameId);
	frameIds.current = [];
}
function clearMarkdownPreviewTimeout(timeoutRef) {
	if (timeoutRef.current === null) return;
	window.clearTimeout(timeoutRef.current);
	timeoutRef.current = null;
}
function requestMarkdownPreviewEditorRevealFrame(frameIds, callback) {
	let completed = false;
	let frameId;
	frameId = requestAnimationFrame((timestamp) => {
		completed = true;
		if (frameId !== void 0) frameIds.current = frameIds.current.filter((pendingFrameId) => pendingFrameId !== frameId);
		callback(timestamp);
	});
	if (!completed) frameIds.current.push(frameId);
}
function getMarkdownPreviewBlockRange(node) {
	const startLine = node?.position?.start?.line;
	const endLine = node?.position?.end?.line;
	if (!Number.isInteger(startLine) || !Number.isInteger(endLine)) return null;
	if (typeof startLine !== "number" || typeof endLine !== "number" || startLine < 1) return null;
	return {
		startLine,
		endLine: Math.max(startLine, endLine)
	};
}
function getMarkdownPreviewReactText(node) {
	if (typeof node === "string" || typeof node === "number") return String(node);
	if (!node || typeof node === "boolean") return "";
	if (Array.isArray(node)) return node.map(getMarkdownPreviewReactText).join(" ");
	if (!import_react.isValidElement(node)) return "";
	const props = node.props;
	if (typeof props.alt === "string" && props.alt.trim()) return props.alt;
	return getMarkdownPreviewReactText(props.children);
}
function getMarkdownPreviewAnnotationQuote(node) {
	return formatMarkdownReviewCardQuote(getMarkdownPreviewReactText(node));
}
function hasMarkdownPreviewNestedBlock(node) {
	const blockTags = new Set([
		"p",
		"pre",
		"table",
		"blockquote",
		"ul",
		"ol"
	]);
	return Boolean(node?.children?.some((child) => child.tagName && blockTags.has(child.tagName)));
}
var markdownPreviewSanitizeSchema = {
	...defaultSchema,
	tagNames: [
		...defaultSchema.tagNames ?? [],
		"details",
		"summary",
		"kbd",
		"sub",
		"sup",
		"ins"
	],
	protocols: {
		...defaultSchema.protocols,
		href: [...defaultSchema.protocols?.href ?? [], "file"],
		src: [...defaultSchema.protocols?.src ?? [], "file"]
	},
	attributes: {
		...defaultSchema.attributes,
		"*": [...defaultSchema.attributes?.["*"] ?? [], "id"],
		a: [
			...defaultSchema.attributes?.a ?? [],
			"href",
			"title"
		],
		code: [...defaultSchema.attributes?.code ?? [], [
			"className",
			/^language-[\w-]+$/,
			"math-inline",
			"math-display"
		]],
		div: [
			...defaultSchema.attributes?.div ?? [],
			["className", /^language-[\w-]+$/],
			"align"
		],
		details: [
			...defaultSchema.attributes?.details ?? [],
			"open",
			["className", "orca-details"],
			[
				"dataOrcaToggle",
				"heading-1",
				"heading-2",
				"heading-3",
				"heading-4",
				"heading-5"
			]
		],
		h1: [...defaultSchema.attributes?.h1 ?? [], "id"],
		h2: [...defaultSchema.attributes?.h2 ?? [], "id"],
		h3: [...defaultSchema.attributes?.h3 ?? [], "id"],
		h4: [...defaultSchema.attributes?.h4 ?? [], "id"],
		h5: [...defaultSchema.attributes?.h5 ?? [], "id"],
		h6: [...defaultSchema.attributes?.h6 ?? [], "id"],
		img: [
			...defaultSchema.attributes?.img ?? [],
			"src",
			"alt",
			"title",
			"width",
			"height"
		],
		input: [
			...defaultSchema.attributes?.input ?? [],
			"type",
			"checked",
			"disabled"
		],
		pre: [...defaultSchema.attributes?.pre ?? [], ["className", /^language-[\w-]+$/]],
		span: [...defaultSchema.attributes?.span ?? [], ["className", /^hljs(?:-[\w-]+)?$/]],
		td: [...defaultSchema.attributes?.td ?? [], "align"],
		th: [...defaultSchema.attributes?.th ?? [], "align"]
	}
};
var MARKDOWN_REMARK_PLUGINS = [
	remarkGfm,
	remarkBreaks,
	remarkFrontmatter,
	remarkMath,
	remarkMarkdownDocLinks
];
var MARKDOWN_REHYPE_PLUGINS = [
	rehypeRaw,
	[rehypeSanitize, markdownPreviewSanitizeSchema],
	rehypeSlug,
	rehypeHighlight,
	rehypeKatex
];
var MarkdownBody = (0, import_react.memo)(function MarkdownBody$1({ content, components }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Markdown, {
		components,
		urlTransform: markdownPreviewUrlTransform,
		remarkPlugins: MARKDOWN_REMARK_PLUGINS,
		rehypePlugins: MARKDOWN_REHYPE_PLUGINS,
		children: content
	});
});
function parseLineTarget(hash) {
	if (!hash) return null;
	const trimmed = hash.startsWith("#") ? hash.slice(1) : hash;
	const match = /^L(\d+)(?:C(\d+))?$/i.exec(trimmed);
	if (!match) return null;
	return {
		line: Number(match[1]),
		column: match[2] ? Number(match[2]) : void 0
	};
}
function decodeMarkdownPreviewAnchor(rawAnchor) {
	try {
		return decodeURIComponent(rawAnchor);
	} catch {
		return rawAnchor;
	}
}
function normalizeMarkdownPreviewAbsolutePath(absolutePath) {
	return absolutePath.replaceAll("\\", "/");
}
function normalizeMarkdownPreviewRelativePath(relativePath) {
	return relativePath.replaceAll("\\", "/").replace(/^\/+/, "");
}
function isMarkdownPreviewAbsolutePathLike(path) {
	return path.startsWith("/") || /^[A-Za-z]:[\\/]/.test(path) || path.startsWith("\\\\");
}
function formatMarkdownPreviewRootPath(rootPath) {
	if (rootPath === "") return "/";
	if (/^[A-Za-z]:$/.test(rootPath)) return `${rootPath}/`;
	return rootPath;
}
function deriveMarkdownPreviewSourceRoot(filePath, relativePath) {
	const normalizedFilePath = normalizeMarkdownPreviewAbsolutePath(filePath);
	const normalizedRelativePath = relativePath && !isMarkdownPreviewAbsolutePathLike(relativePath) ? normalizeMarkdownPreviewRelativePath(relativePath) : "";
	if (normalizedRelativePath) {
		const suffix = `/${normalizedRelativePath}`;
		if (normalizedFilePath.endsWith(suffix)) return formatMarkdownPreviewRootPath(normalizedFilePath.slice(0, -suffix.length));
	}
	return formatMarkdownPreviewRootPath(normalizeMarkdownPreviewAbsolutePath(dirname(filePath)));
}
function findWorktreeForMarkdownPreviewPath(worktreesByRepo, absolutePath, acceptsWorktree = () => true) {
	let bestMatch = null;
	let bestMatchLength = -1;
	for (const worktrees of Object.values(worktreesByRepo)) for (const worktree of worktrees) if (acceptsWorktree(worktree) && relativePathInsideRoot(worktree.path, absolutePath) !== null) {
		const normalizedWorktreePathLength = normalizeMarkdownPreviewAbsolutePath(worktree.path).length;
		if (normalizedWorktreePathLength > bestMatchLength) {
			bestMatch = worktree;
			bestMatchLength = normalizedWorktreePathLength;
		}
	}
	return bestMatch;
}
function findMarkdownPreviewTargetWorktree(worktreesByRepo, absolutePath, sourceWorktree, sourceOwner) {
	if (sourceWorktree && relativePathInsideRoot(sourceWorktree.path, absolutePath) !== null) return sourceWorktree;
	return findWorktreeForMarkdownPreviewPath(worktreesByRepo, absolutePath, (worktree) => {
		const connectionId = getConnectionIdForFile(worktree.id, absolutePath);
		if (sourceOwner.kind === "local") return connectionId === null;
		if (sourceOwner.kind === "ssh") return connectionId === sourceOwner.connectionId;
		return false;
	});
}
function resolveMarkdownPreviewSourceWorktree(worktreesByRepo, sourceWorktreeId, filePath) {
	return (sourceWorktreeId ? findWorktreeById(worktreesByRepo, sourceWorktreeId) ?? null : null) ?? findWorktreeForMarkdownPreviewPath(worktreesByRepo, filePath);
}
function getMarkdownPreviewSourceRelativePath(filePath, sourceWorktreePath) {
	return relativePathInsideRoot(sourceWorktreePath, filePath);
}
function MarkdownPreview({ content, filePath, sourceFileId = null, sourceWorktreeId = null, sourceRuntimeEnvironmentId = void 0, scrollCacheKey, initialAnchor = null, showTableOfContents = false, onCloseTableOfContents, markdownDocuments = EMPTY_MARKDOWN_DOCUMENTS, onOpenDocument, markdownAnnotationsEnabled = false }) {
	const rootRef = (0, import_react.useRef)(null);
	const bodyRef = (0, import_react.useRef)(null);
	const inputRef = (0, import_react.useRef)(null);
	const setSearchInputElement = (0, import_react.useCallback)((input) => {
		inputRef.current = input;
		if (!input) return;
		input.focus();
		input.select();
	}, []);
	const matchesRef = (0, import_react.useRef)([]);
	const searchInstanceRef = (0, import_react.useRef)({});
	const lastAppliedInitialAnchorRef = (0, import_react.useRef)(null);
	const pendingEditorRevealFrameIdsRef = (0, import_react.useRef)([]);
	const [isSearchOpen, setIsSearchOpen] = (0, import_react.useState)(false);
	const [query, setQuery] = (0, import_react.useState)("");
	const [matchCount, setMatchCount] = (0, import_react.useState)(0);
	const [searchRevision, setSearchRevision] = (0, import_react.useState)(0);
	const [activeMatchIndex, setActiveMatchIndex] = (0, import_react.useState)(-1);
	const isMac = navigator.userAgent.includes("Mac");
	const openFile = useAppStore((s$1) => s$1.openFile);
	const activateMarkdownLink = useAppStore((s$1) => s$1.activateMarkdownLink);
	const openMarkdownPreview = useAppStore((s$1) => s$1.openMarkdownPreview);
	const setMarkdownViewMode = useAppStore((s$1) => s$1.setMarkdownViewMode);
	const frontmatterVisibleByFile = useAppStore((s$1) => s$1.markdownFrontmatterVisible);
	const setPendingEditorReveal = useAppStore((s$1) => s$1.setPendingEditorReveal);
	const addDiffComment = useAppStore((s$1) => s$1.addDiffComment);
	const deleteDiffComment = useAppStore((s$1) => s$1.deleteDiffComment);
	const updateDiffComment = useAppStore((s$1) => s$1.updateDiffComment);
	const clearDeliveredDiffComments = useAppStore((s$1) => s$1.clearDeliveredDiffComments);
	const keybindings = useAppStore((s$1) => s$1.keybindings);
	const worktreesByRepo = useAppStore((s$1) => s$1.worktreesByRepo);
	const sourceOpenFile = useAppStore((s$1) => findMarkdownPreviewSourceOpenFile(s$1.openFiles, {
		sourceFileId,
		filePath,
		sourceWorktreeId,
		sourceRuntimeEnvironmentId
	}));
	const resolvedSourceWorktreeId = sourceWorktreeId ?? sourceOpenFile?.worktreeId ?? null;
	const resolvedSourceRuntimeEnvironmentId = sourceRuntimeEnvironmentId !== void 0 ? sourceRuntimeEnvironmentId : sourceOpenFile?.runtimeEnvironmentId;
	const sourceWorktree = resolveMarkdownPreviewSourceWorktree(worktreesByRepo, resolvedSourceWorktreeId, filePath);
	const allDiffComments = sourceWorktree?.diffComments;
	const sourceRoutingWorktreeId = sourceWorktree?.id ?? resolvedSourceWorktreeId;
	const runtimeOwnerId = resolvedSourceRuntimeEnvironmentId?.trim();
	const sourceConnectionId = useAppStore((0, import_react.useMemo)(() => createConnectionIdForFileSelector(sourceRoutingWorktreeId, filePath, { skip: Boolean(runtimeOwnerId) }), [
		filePath,
		runtimeOwnerId,
		sourceRoutingWorktreeId
	]));
	const sourceOwner = (0, import_react.useMemo)(() => runtimeOwnerId ? {
		kind: "runtime",
		runtimeEnvironmentId: runtimeOwnerId
	} : sourceConnectionId === void 0 ? { kind: "unknown" } : sourceConnectionId === null ? { kind: "local" } : {
		kind: "ssh",
		connectionId: sourceConnectionId
	}, [runtimeOwnerId, sourceConnectionId]);
	const worktreeRoot = sourceWorktree?.path ?? (sourceRoutingWorktreeId ? deriveMarkdownPreviewSourceRoot(filePath, sourceOpenFile?.relativePath) : null);
	const sourceRelativePath = (0, import_react.useMemo)(() => {
		if (!sourceWorktree) return null;
		return getMarkdownPreviewSourceRelativePath(filePath, sourceWorktree.path);
	}, [filePath, sourceWorktree]);
	const markdownComments = (0, import_react.useMemo)(() => (allDiffComments ?? []).filter((comment$1) => comment$1.filePath === sourceRelativePath && isMarkdownComment(comment$1)), [allDiffComments, sourceRelativePath]);
	const settings = useAppStore((s$1) => s$1.settings);
	const imageRuntimeContext = (0, import_react.useMemo)(() => sourceRoutingWorktreeId && worktreeRoot ? {
		settings: settingsForRuntimeOwner(settings, resolvedSourceRuntimeEnvironmentId),
		worktreeId: sourceRoutingWorktreeId,
		worktreePath: worktreeRoot,
		connectionId: sourceConnectionId,
		expectedExternalSshTargetId: sourceOpenFile?.externalSshTargetId
	} : void 0, [
		settings,
		sourceConnectionId,
		sourceOpenFile?.externalSshTargetId,
		resolvedSourceRuntimeEnvironmentId,
		sourceRoutingWorktreeId,
		worktreeRoot
	]);
	const editorFontSize = computeEditorFontSize(14, useAppStore((s$1) => s$1.editorFontZoomLevel));
	const isDark = settings?.theme === "dark" || settings?.theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches;
	const renderedContent = usePreserveSectionDuringExternalEdit(content, bodyRef);
	(0, import_react.useEffect)(() => {
		return prewarmMarkdownPreviewLocalImages(renderedContent, filePath, { runtimeContext: imageRuntimeContext }).cancel;
	}, [
		renderedContent,
		filePath,
		imageRuntimeContext
	]);
	const frontMatter = (0, import_react.useMemo)(() => extractFrontMatter(renderedContent), [renderedContent]);
	const tableOfContentsItems = (0, import_react.useMemo)(() => selectMarkdownTableOfContents(showTableOfContents, renderedContent), [renderedContent, showTableOfContents]);
	const markdownDocumentIndex = (0, import_react.useMemo)(() => createMarkdownDocumentIndex(markdownDocuments), [markdownDocuments]);
	const frontMatterInner = (0, import_react.useMemo)(() => {
		if (!frontMatter) return "";
		return frontMatter.raw.replace(/^(?:---|\+\+\+)\r?\n/, "").replace(/\r?\n(?:---|\+\+\+)\r?\n?$/, "").trim();
	}, [frontMatter]);
	const toggleableSourceFileId = sourceFileId ?? null;
	const frontmatterVisible = toggleableSourceFileId ? frontmatterVisibleByFile[toggleableSourceFileId] ?? true : true;
	const [activeAnnotationBlockKey, setActiveAnnotationBlockKey] = (0, import_react.useState)(null);
	const activeAnnotationBlockKeyRef = (0, import_react.useRef)(activeAnnotationBlockKey);
	(0, import_react.useEffect)(() => {
		activeAnnotationBlockKeyRef.current = activeAnnotationBlockKey;
	}, [activeAnnotationBlockKey]);
	(0, import_react.useEffect)(() => {
		if (!activeAnnotationBlockKey) return;
		const root$1 = rootRef.current;
		if (!root$1 || previewHasAnnotationBlockKey(root$1, activeAnnotationBlockKey)) return;
		setActiveAnnotationBlockKey(null);
	}, [activeAnnotationBlockKey, renderedContent]);
	const [reviewNotesCopied, setReviewNotesCopied] = (0, import_react.useState)(false);
	const [copiedReviewNoteId, setCopiedReviewNoteId] = (0, import_react.useState)(null);
	const reviewNotesCopiedResetTimerRef = (0, import_react.useRef)(null);
	const copiedReviewNoteResetTimerRef = (0, import_react.useRef)(null);
	const reviewNotesCopyMountedRef = (0, import_react.useRef)(false);
	const [activeReviewCommentId, setActiveReviewCommentId] = (0, import_react.useState)(null);
	const [attentionReviewCommentId, setAttentionReviewCommentId] = (0, import_react.useState)(null);
	const attentionReviewCommentTimeoutRef = (0, import_react.useRef)(null);
	const markdownReviewNotes = (0, import_react.useMemo)(() => sortMarkdownReviewNotes(markdownComments), [markdownComments]);
	const unsentMarkdownReviewNotes = (0, import_react.useMemo)(() => markdownReviewNotes.filter((note) => !note.sentAt), [markdownReviewNotes]);
	const unsentMarkdownReviewPrompt = (0, import_react.useMemo)(() => formatMarkdownReviewNotes(unsentMarkdownReviewNotes, renderedContent), [renderedContent, unsentMarkdownReviewNotes]);
	const unsentMarkdownReviewScope = (0, import_react.useMemo)(() => [{
		id: "all",
		label: translate("auto.components.editor.MarkdownPreview.ddf087d12e", "All unsent notes"),
		notes: unsentMarkdownReviewNotes,
		prompt: unsentMarkdownReviewPrompt
	}], [unsentMarkdownReviewNotes, unsentMarkdownReviewPrompt]);
	const canShowReviewTools = Boolean(markdownAnnotationsEnabled && sourceWorktree && sourceRelativePath !== null);
	(0, import_react.useLayoutEffect)(() => {
		const container = rootRef.current;
		if (!container) return;
		let throttleTimer = null;
		const onScroll = () => {
			if (throttleTimer !== null) clearTimeout(throttleTimer);
			throttleTimer = setTimeout(() => {
				setWithLRU(scrollTopCache, scrollCacheKey, container.scrollTop);
				throttleTimer = null;
			}, 150);
		};
		container.addEventListener("scroll", onScroll, { passive: true });
		return () => {
			if (container.scrollHeight > container.clientHeight || container.scrollTop > 0) setWithLRU(scrollTopCache, scrollCacheKey, container.scrollTop);
			if (throttleTimer !== null) clearTimeout(throttleTimer);
			container.removeEventListener("scroll", onScroll);
		};
	}, [scrollCacheKey]);
	(0, import_react.useLayoutEffect)(() => {
		const container = rootRef.current;
		const targetScrollTop = scrollTopCache.get(scrollCacheKey);
		if (!container || targetScrollTop === void 0) return;
		let frameId = 0;
		let attempts = 0;
		const tryRestore = () => {
			const maxScrollTop = Math.max(0, container.scrollHeight - container.clientHeight);
			container.scrollTop = Math.min(targetScrollTop, maxScrollTop);
			if (Math.abs(container.scrollTop - targetScrollTop) <= 1 || maxScrollTop >= targetScrollTop) return;
			attempts += 1;
			if (attempts < 30) frameId = window.requestAnimationFrame(tryRestore);
		};
		tryRestore();
		return () => window.cancelAnimationFrame(frameId);
	}, [scrollCacheKey, renderedContent]);
	const moveToMatch = (0, import_react.useCallback)((direction) => {
		if (matchesRef.current.length === 0) return;
		setActiveMatchIndex((cur) => {
			return ((cur >= 0 ? cur : direction === 1 ? -1 : 0) + direction + matchesRef.current.length) % matchesRef.current.length;
		});
	}, []);
	const openSearch = (0, import_react.useCallback)(() => {
		if (isSearchOpen) {
			inputRef.current?.focus();
			inputRef.current?.select();
		} else setIsSearchOpen(true);
	}, [isSearchOpen]);
	const closeSearch = (0, import_react.useCallback)(() => {
		setIsSearchOpen(false);
		setQuery("");
		setActiveMatchIndex(-1);
	}, []);
	const clearReviewNotesCopiedResetTimer = (0, import_react.useCallback)(() => {
		if (reviewNotesCopiedResetTimerRef.current !== null) {
			window.clearTimeout(reviewNotesCopiedResetTimerRef.current);
			reviewNotesCopiedResetTimerRef.current = null;
		}
	}, []);
	const clearCopiedReviewNoteResetTimer = (0, import_react.useCallback)(() => {
		if (copiedReviewNoteResetTimerRef.current !== null) {
			window.clearTimeout(copiedReviewNoteResetTimerRef.current);
			copiedReviewNoteResetTimerRef.current = null;
		}
	}, []);
	const cleanupPreviewSurfaceTimers = (0, import_react.useCallback)(() => {
		cancelMarkdownPreviewEditorRevealFrames(pendingEditorRevealFrameIdsRef);
		clearMarkdownPreviewTimeout(attentionReviewCommentTimeoutRef);
		clearReviewNotesCopiedResetTimer();
		clearCopiedReviewNoteResetTimer();
	}, [clearCopiedReviewNoteResetTimer, clearReviewNotesCopiedResetTimer]);
	const setRootRef = (0, import_react.useCallback)((node) => {
		rootRef.current = node;
		reviewNotesCopyMountedRef.current = node !== null;
		if (node === null) cleanupPreviewSurfaceTimers();
	}, [cleanupPreviewSurfaceTimers]);
	const scrollToAnchor = (0, import_react.useCallback)((rawAnchor) => {
		const container = rootRef.current;
		const body = bodyRef.current;
		if (!container || !body) return false;
		const decodedAnchor = decodeMarkdownPreviewAnchor(rawAnchor);
		let target = null;
		for (const candidate of body.querySelectorAll("[id]")) if (candidate.id === decodedAnchor) {
			target = candidate;
			break;
		}
		if (!target) return false;
		container.scrollTo({ top: getMarkdownPreviewAnchorScrollTop(container, target) });
		target.focus({ preventScroll: true });
		return true;
	}, []);
	const navigateToTableOfContentsItem = (0, import_react.useCallback)((id) => {
		scrollToAnchor(id);
	}, [scrollToAnchor]);
	(0, import_react.useEffect)(() => {
		const body = bodyRef.current;
		if (!body) return;
		const instanceId = searchInstanceRef.current;
		if (!isSearchOpen) {
			matchesRef.current = [];
			setMatchCount(0);
			clearMarkdownPreviewSearchHighlights(instanceId);
			return;
		}
		const matches = applyMarkdownPreviewSearchHighlights(instanceId, body, query);
		matchesRef.current = matches;
		setMatchCount(matches.length);
		setSearchRevision((v) => v + 1);
		setActiveMatchIndex((cur) => matches.length === 0 ? -1 : cur >= 0 && cur < matches.length ? cur : 0);
		return () => clearMarkdownPreviewSearchHighlights(instanceId);
	}, [
		renderedContent,
		isSearchOpen,
		query
	]);
	(0, import_react.useEffect)(() => {
		setActiveMarkdownPreviewSearchMatch(searchInstanceRef.current, matchesRef.current, activeMatchIndex);
	}, [
		activeMatchIndex,
		matchCount,
		searchRevision
	]);
	(0, import_react.useLayoutEffect)(() => {
		if (!initialAnchor || initialAnchor === lastAppliedInitialAnchorRef.current) return;
		let frameId = 0;
		let attempts = 0;
		const tryRevealAnchor = () => {
			if (scrollToAnchor(initialAnchor)) {
				lastAppliedInitialAnchorRef.current = initialAnchor;
				return;
			}
			attempts += 1;
			if (attempts < 30) frameId = window.requestAnimationFrame(tryRevealAnchor);
		};
		tryRevealAnchor();
		return () => window.cancelAnimationFrame(frameId);
	}, [
		content,
		initialAnchor,
		scrollToAnchor
	]);
	(0, import_react.useEffect)(() => {
		const handleKeyDown = (event) => {
			const root$1 = rootRef.current;
			if (!root$1) return;
			const target = event.target;
			const targetInsidePreview = target instanceof Node && root$1.contains(target);
			if (isMarkdownPreviewFindShortcut(event, getShortcutPlatform(), keybindings) && targetInsidePreview) {
				event.preventDefault();
				event.stopPropagation();
				openSearch();
				return;
			}
			const reviewNoteKey = resolveMarkdownPreviewAddReviewNoteKey({
				event,
				platform: getShortcutPlatform(),
				keybindings,
				targetInsidePreview,
				markdownAnnotationsEnabled,
				activeAnnotationBlockKey: activeAnnotationBlockKeyRef.current,
				root: root$1,
				selection: window.getSelection()
			});
			if (reviewNoteKey.action === "consume") {
				event.preventDefault();
				event.stopPropagation();
				return;
			}
			if (reviewNoteKey.action === "clear-stale-and-ignore") {
				activeAnnotationBlockKeyRef.current = null;
				setActiveAnnotationBlockKey(null);
				return;
			}
			if (reviewNoteKey.action === "open") {
				event.preventDefault();
				event.stopPropagation();
				activeAnnotationBlockKeyRef.current = reviewNoteKey.blockKey;
				setActiveAnnotationBlockKey(reviewNoteKey.blockKey);
				return;
			}
			if (!isSearchOpen) return;
			if (event.key === "Escape" && (targetInsidePreview || target === inputRef.current)) {
				event.preventDefault();
				event.stopPropagation();
				closeSearch();
				root$1.focus();
			}
		};
		window.addEventListener("keydown", handleKeyDown, { capture: true });
		return () => window.removeEventListener("keydown", handleKeyDown, { capture: true });
	}, [
		closeSearch,
		isSearchOpen,
		keybindings,
		markdownAnnotationsEnabled,
		openSearch
	]);
	const handleCopyMarkdownReviewNotes = (0, import_react.useCallback)(async () => {
		if (markdownReviewNotes.length === 0) return;
		try {
			if (!await copyMarkdownReviewNotesForAgent({
				notes: markdownReviewNotes,
				content: renderedContent,
				writeClipboardText: window.api.ui.writeClipboardText
			}) || !reviewNotesCopyMountedRef.current) return;
			clearReviewNotesCopiedResetTimer();
			setReviewNotesCopied(true);
			reviewNotesCopiedResetTimerRef.current = window.setTimeout(() => {
				reviewNotesCopiedResetTimerRef.current = null;
				setReviewNotesCopied(false);
			}, 1600);
		} catch {}
	}, [
		clearReviewNotesCopiedResetTimer,
		markdownReviewNotes,
		renderedContent
	]);
	const handleCopyMarkdownReviewNote = (0, import_react.useCallback)(async (note) => {
		try {
			if (!await copyMarkdownReviewNotesForAgent({
				notes: [note],
				content: renderedContent,
				writeClipboardText: window.api.ui.writeClipboardText
			}) || !reviewNotesCopyMountedRef.current) return;
			clearCopiedReviewNoteResetTimer();
			setCopiedReviewNoteId(note.id);
			copiedReviewNoteResetTimerRef.current = window.setTimeout(() => {
				copiedReviewNoteResetTimerRef.current = null;
				setCopiedReviewNoteId(null);
			}, 1600);
		} catch {}
	}, [clearCopiedReviewNoteResetTimer, renderedContent]);
	const pulseRenderedMarkdownReviewNote = (0, import_react.useCallback)((commentId) => {
		if (attentionReviewCommentTimeoutRef.current !== null) window.clearTimeout(attentionReviewCommentTimeoutRef.current);
		setAttentionReviewCommentId(null);
		window.requestAnimationFrame(() => {
			setAttentionReviewCommentId(commentId);
			attentionReviewCommentTimeoutRef.current = window.setTimeout(() => {
				setAttentionReviewCommentId(null);
				attentionReviewCommentTimeoutRef.current = null;
			}, 900);
		});
	}, []);
	const findRenderedMarkdownReviewNoteCard = (0, import_react.useCallback)((commentId) => {
		const root$1 = rootRef.current;
		if (!root$1) return null;
		return Array.from(root$1.querySelectorAll("[data-markdown-review-note-id]")).find((candidate) => candidate.dataset.markdownReviewNoteId === commentId) ?? null;
	}, []);
	const scrollRenderedMarkdownReviewNoteIntoView = (0, import_react.useCallback)((comment$1) => {
		setActiveReviewCommentId(comment$1.id);
		pulseRenderedMarkdownReviewNote(comment$1.id);
		window.requestAnimationFrame(() => {
			findRenderedMarkdownReviewNoteCard(comment$1.id)?.scrollIntoView({
				behavior: "smooth",
				block: "center",
				inline: "nearest"
			});
		});
	}, [findRenderedMarkdownReviewNoteCard, pulseRenderedMarkdownReviewNote]);
	const scrollToReviewNote = (0, import_react.useCallback)((comment$1) => {
		setActiveReviewCommentId(comment$1.id);
		const root$1 = rootRef.current;
		if (!root$1) return;
		const blocks = root$1.querySelectorAll("[data-source-line][data-source-end-line]");
		let target = null;
		for (const block of blocks) {
			const startLine = Number(block.dataset.sourceLine);
			const endLine = Number(block.dataset.sourceEndLine);
			if (startLine <= comment$1.lineNumber && comment$1.lineNumber <= endLine) {
				target = block;
				break;
			}
		}
		target?.scrollIntoView({
			behavior: "smooth",
			block: "center"
		});
	}, []);
	const getMarkdownCommentsForRange = (0, import_react.useCallback)((range) => markdownComments.filter((comment$1) => range.startLine <= comment$1.lineNumber && comment$1.lineNumber <= range.endLine), [markdownComments]);
	const handleAnnotatedMarkdownBlockClick = (0, import_react.useCallback)((range, event) => {
		if (!isMarkdownAnnotationNavigationClick(event.target)) return;
		const commentsForBlock = getMarkdownCommentsForRange(range);
		const comment$1 = commentsForBlock.find((candidate) => candidate.id !== activeReviewCommentId) ?? commentsForBlock[0];
		if (!comment$1) return;
		scrollRenderedMarkdownReviewNoteIntoView(comment$1);
	}, [
		activeReviewCommentId,
		getMarkdownCommentsForRange,
		scrollRenderedMarkdownReviewNoteIntoView
	]);
	const renderAnnotationControls = (0, import_react.useCallback)((range, blockKey, annotationQuote) => {
		if (!sourceWorktree || sourceRelativePath === null) return null;
		if (!markdownAnnotationsEnabled) return null;
		const commentsForBlock = getMarkdownCommentsForRange(range);
		const handleSubmit = async (body) => {
			if (await addDiffComment({
				worktreeId: sourceWorktree.id,
				filePath: sourceRelativePath,
				source: "markdown",
				startLine: range.startLine === range.endLine ? void 0 : range.startLine,
				lineNumber: range.endLine,
				...annotationQuote ? { selectedText: annotationQuote } : {},
				body,
				side: "modified"
			})) {
				setActiveAnnotationBlockKey(null);
				return true;
			}
			return false;
		};
		return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "markdown-annotation-controls",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					className: "markdown-annotation-add",
					"aria-label": translate("auto.components.editor.MarkdownPreview.13f94d760c", "Add note"),
					title: translate("auto.components.editor.MarkdownPreview.13f94d760c", "Add note"),
					onClick: (event) => {
						event.preventDefault();
						event.stopPropagation();
						setActiveAnnotationBlockKey((current) => current === blockKey ? null : blockKey);
					},
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "size-3" })
				}),
				activeAnnotationBlockKey === blockKey ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MarkdownAnnotationComposer, {
					lineNumber: range.endLine,
					startLine: range.startLine === range.endLine ? void 0 : range.startLine,
					onCancel: () => setActiveAnnotationBlockKey(null),
					onSubmit: handleSubmit
				}) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "markdown-annotation-note-stack",
					children: commentsForBlock.map((comment$1) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						"data-markdown-review-note-id": comment$1.id,
						className: `markdown-annotation-card ${activeReviewCommentId === comment$1.id ? "is-active" : ""} ${attentionReviewCommentId === comment$1.id ? "is-attention" : ""}`.trim(),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DiffCommentCard, {
							lineNumber: comment$1.lineNumber,
							startLine: comment$1.startLine,
							label: null,
							quote: formatMarkdownReviewCardQuote(comment$1.selectedText) ?? annotationQuote ?? getMarkdownReviewCardQuote(content, comment$1),
							body: comment$1.body,
							sentAt: comment$1.sentAt,
							onDelete: () => void deleteDiffComment(sourceWorktree.id, comment$1.id),
							onSubmitEdit: (body) => updateDiffComment(sourceWorktree.id, comment$1.id, body),
							headerActions: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								className: "orca-diff-comment-pill-btn",
								title: copiedReviewNoteId === comment$1.id ? translate("auto.components.editor.MarkdownPreview.94b520a96a", "Copied note") : translate("auto.components.editor.MarkdownPreview.f961e94057", "Copy note for agent"),
								"aria-label": copiedReviewNoteId === comment$1.id ? translate("auto.components.editor.MarkdownPreview.94b520a96a", "Copied note") : translate("auto.components.editor.MarkdownPreview.f961e94057", "Copy note for agent"),
								onClick: (event) => {
									event.preventDefault();
									event.stopPropagation();
									handleCopyMarkdownReviewNote(comment$1);
								},
								children: copiedReviewNoteId === comment$1.id ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "size-3" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { className: "size-3" })
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MarkdownSingleNoteSendMenu, {
								worktreeId: sourceWorktree.id,
								filePath,
								content: renderedContent,
								note: comment$1,
								modeSlot: "preview-inline",
								onDelivered: (notes) => void clearDeliveredDiffComments(sourceWorktree.id, notes)
							})] })
						})
					}, comment$1.id))
				})
			]
		});
	}, [
		activeAnnotationBlockKey,
		activeReviewCommentId,
		attentionReviewCommentId,
		addDiffComment,
		clearDeliveredDiffComments,
		copiedReviewNoteId,
		deleteDiffComment,
		filePath,
		getMarkdownCommentsForRange,
		handleCopyMarkdownReviewNote,
		markdownAnnotationsEnabled,
		content,
		renderedContent,
		sourceRelativePath,
		sourceWorktree,
		updateDiffComment
	]);
	const wrapAnnotatedBlock = (0, import_react.useCallback)((tagName, node, rendered) => {
		const range = getMarkdownPreviewBlockRange(node);
		if (!range) return rendered;
		const blockKey = `${tagName}:${range.startLine}-${range.endLine}`;
		const controls = renderAnnotationControls(range, blockKey, getMarkdownPreviewAnnotationQuote(rendered));
		if (!controls) return rendered;
		return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: `markdown-annotation-block ${getMarkdownCommentsForRange(range).length > 0 ? "has-review-notes" : ""}`.trim(),
			"data-source-line": range.startLine,
			"data-source-end-line": range.endLine,
			"data-annotation-block-key": blockKey,
			onClick: (event) => handleAnnotatedMarkdownBlockClick(range, event),
			children: [rendered, controls]
		});
	}, [
		getMarkdownCommentsForRange,
		handleAnnotatedMarkdownBlockClick,
		renderAnnotationControls
	]);
	const components = (0, import_react.useMemo)(() => {
		return {
			a: ({ href, children, className, ...props }) => {
				const docLinkTarget = parseMarkdownDocLinkHref(href);
				if (docLinkTarget !== null) {
					const resolution = resolveMarkdownDocLink(docLinkTarget, markdownDocumentIndex);
					const resolvedDocument = resolution.status === "resolved" ? resolution.document : null;
					const title = resolution.status === "ambiguous" ? "Document link is ambiguous" : "Document not found";
					const handleDocLinkClick = (event) => {
						event.preventDefault();
						if (resolvedDocument && onOpenDocument) onOpenDocument(resolvedDocument, { anchor: getMarkdownDocLinkAnchor(docLinkTarget) });
					};
					return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
						...props,
						href,
						className: `${className ?? ""} ${resolvedDocument ? "markdown-doc-link" : "markdown-doc-link-broken"}`.trim(),
						title: resolvedDocument ? void 0 : title,
						onClick: handleDocLinkClick,
						children
					});
				}
				const handleClick = async (event) => {
					if (!href) return;
					event.preventDefault();
					if (href.startsWith("#")) {
						scrollToAnchor(href.slice(1));
						return;
					}
					if (isMarkdownPreviewSystemBrowserModifier(event, isMac)) {
						if (sourceOwner.kind === "unknown") return;
						const osTarget = getMarkdownPreviewLinkTarget(href, filePath);
						if (!osTarget) return;
						let parsed;
						try {
							parsed = new URL(osTarget);
						} catch {
							return;
						}
						if (parsed.protocol === "http:" || parsed.protocol === "https:") {
							openHttpLink(parsed.toString(), resolveMarkdownPreviewHttpOpenOptions(event, isMac, sourceRoutingWorktreeId, sourceOwner));
							return;
						}
						if (parsed.protocol === "file:") {
							if (isLocalPathOpenBlocked(settingsForRuntimeOwner(useAppStore.getState().settings, resolvedSourceRuntimeEnvironmentId), { connectionId: sourceConnectionId })) {
								showLocalPathOpenBlockedToast();
								return;
							}
							const classified$1 = resolveMarkdownLinkTarget(href, filePath, worktreeRoot);
							if (classified$1?.kind === "markdown" || classified$1?.kind === "file" && classified$1.line !== void 0) {
								const cleanUri = absolutePathToFileUri(classified$1.absolutePath);
								window.api.shell.pathExists(classified$1.absolutePath).then((exists) => {
									if (!exists) {
										toast.error(translate("auto.components.editor.MarkdownPreview.6c043947ae", "File not found: {{value0}}", { value0: classified$1.relativePath ?? classified$1.absolutePath }));
										return;
									}
									window.api.shell.openFileUri(cleanUri);
								});
								return;
							}
							window.api.shell.openFileUri(parsed.toString());
						}
						return;
					}
					const target = resolveMarkdownPreviewHref(href, filePath);
					if (!target) return;
					if (target.protocol === "http:" || target.protocol === "https:") {
						openHttpLink(target.toString(), resolveMarkdownPreviewHttpOpenOptions(event, isMac, sourceRoutingWorktreeId, sourceOwner));
						return;
					}
					if (target.protocol !== "file:") return;
					const classified = resolveMarkdownLinkTarget(href, filePath, worktreeRoot);
					const classifiedFileTarget = classified?.kind === "markdown" || classified?.kind === "file" ? classified : null;
					const absolutePath = classifiedFileTarget?.absolutePath ?? fileUrlToAbsolutePath(target);
					if (!absolutePath) return;
					const lineTarget = classifiedFileTarget?.line !== void 0 ? {
						line: classifiedFileTarget.line,
						column: classifiedFileTarget.column
					} : parseLineTarget(target.hash);
					if (absolutePath === filePath && target.hash && !lineTarget) {
						scrollToAnchor(target.hash.slice(1));
						return;
					}
					if (sourceOwner.kind === "unknown") return;
					const targetWorktree = findMarkdownPreviewTargetWorktree(worktreesByRepo, absolutePath, sourceWorktree, sourceOwner);
					if (!targetWorktree) {
						if (sourceRoutingWorktreeId && worktreeRoot) {
							activateMarkdownLink(href, {
								sourceFilePath: filePath,
								worktreeId: sourceRoutingWorktreeId,
								worktreeRoot,
								runtimeEnvironmentId: resolvedSourceRuntimeEnvironmentId,
								sourceOwner
							});
							return;
						}
						if (isLocalPathOpenBlocked(settingsForRuntimeOwner(useAppStore.getState().settings, resolvedSourceRuntimeEnvironmentId), { connectionId: sourceConnectionId })) {
							showLocalPathOpenBlockedToast();
							return;
						}
						window.api.shell.openFileUri(target.toString());
						return;
					}
					const relativePath = relativePathInsideRoot(targetWorktree.path, absolutePath);
					if (relativePath === null) return;
					const language$1 = detectLanguage(absolutePath);
					const targetConnectionId = getConnectionIdForFile(targetWorktree.id, absolutePath);
					if (targetConnectionId === void 0) return;
					try {
						if ((await statRuntimePath({
							settings: settingsForRuntimeOwner(useAppStore.getState().settings, resolvedSourceRuntimeEnvironmentId),
							worktreeId: targetWorktree.id,
							worktreePath: targetWorktree.path,
							connectionId: targetConnectionId ?? void 0
						}, absolutePath)).isDirectory) {
							toast.error(translate("auto.components.editor.MarkdownPreview.759463a221", "Cannot open directory: {{value0}}", { value0: relativePath }));
							return;
						}
					} catch {
						toast.error(translate("auto.components.editor.MarkdownPreview.6c043947ae", "File not found: {{value0}}", { value0: relativePath }));
						return;
					}
					if (lineTarget) {
						openFile({
							filePath: absolutePath,
							relativePath,
							worktreeId: targetWorktree.id,
							runtimeEnvironmentId: resolvedSourceRuntimeEnvironmentId,
							language: language$1,
							mode: "edit"
						});
						const openedState = useAppStore.getState();
						const targetFileId = findMarkdownPreviewOpenedEditFileId(openedState.openFiles, openedState.activeFileIdByWorktree, {
							filePath: absolutePath,
							worktreeId: targetWorktree.id
						});
						if (language$1 === "markdown") setMarkdownViewMode(targetFileId, "source");
						cancelMarkdownPreviewEditorRevealFrames(pendingEditorRevealFrameIdsRef);
						setPendingEditorReveal(null);
						requestMarkdownPreviewEditorRevealFrame(pendingEditorRevealFrameIdsRef, () => {
							requestMarkdownPreviewEditorRevealFrame(pendingEditorRevealFrameIdsRef, () => {
								setPendingEditorReveal({
									filePath: absolutePath,
									fileId: targetFileId,
									line: lineTarget.line,
									column: lineTarget.column ?? 1,
									matchLength: 0
								});
							});
						});
						return;
					}
					if (language$1 === "markdown") {
						openMarkdownPreview({
							filePath: absolutePath,
							relativePath,
							worktreeId: targetWorktree.id,
							runtimeEnvironmentId: resolvedSourceRuntimeEnvironmentId,
							language: language$1
						}, { anchor: target.hash ? target.hash.slice(1) : null });
						return;
					}
					openFile({
						filePath: absolutePath,
						relativePath,
						worktreeId: targetWorktree.id,
						runtimeEnvironmentId: resolvedSourceRuntimeEnvironmentId,
						language: language$1,
						mode: "edit"
					});
				};
				return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
					...props,
					href,
					className,
					onClick: handleClick,
					style: { cursor: "pointer" },
					children
				});
			},
			img: function MarkdownImg({ src, alt, ...props }) {
				const resolvedSrc = useLocalImageSrc(src, filePath, void 0, imageRuntimeContext);
				const handleImageClick = (event) => {
					if (!isMarkdownPreviewOpenModifier(event, isMac)) return;
					if (!src || !sourceRoutingWorktreeId || !worktreeRoot) return;
					event.preventDefault();
					event.stopPropagation();
					activateMarkdownLink(src, {
						sourceFilePath: filePath,
						worktreeId: sourceRoutingWorktreeId,
						worktreeRoot,
						runtimeEnvironmentId: resolvedSourceRuntimeEnvironmentId,
						sourceOwner
					});
				};
				return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
					...props,
					src: resolvedSrc,
					alt: alt ?? "",
					onClick: handleImageClick
				});
			},
			code: ({ className, children, ...props }) => {
				if (/language-mermaid/.test(className || "")) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MermaidBlock, {
					content: String(children).trimEnd(),
					isDark,
					htmlLabels: false
				});
				return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", {
					className,
					...props,
					children
				});
			},
			pre: ({ node, children, ...props }) => {
				const child = import_react.Children.toArray(children)[0];
				if (import_react.isValidElement(child) && child.type === MermaidBlock) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_jsx_runtime.Fragment, { children });
				return wrapAnnotatedBlock("pre", node, /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CodeBlockCopyButton, {
					...props,
					children
				}));
			},
			p: ({ node, children, ...props }) => wrapAnnotatedBlock("p", node, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				...props,
				children
			})),
			blockquote: ({ node, children, ...props }) => wrapAnnotatedBlock("blockquote", node, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("blockquote", {
				...props,
				children
			})),
			table: ({ node, children, ...props }) => wrapAnnotatedBlock("table", node, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("table", {
				...props,
				children
			})),
			li: ({ node, children, ...props }) => {
				const positionNode = node;
				const range = hasMarkdownPreviewNestedBlock(positionNode) ? null : getMarkdownPreviewBlockRange(positionNode);
				if (!range) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", {
					...props,
					children
				});
				const blockKey = `li:${range.startLine}-${range.endLine}`;
				const hasReviewNotes = getMarkdownCommentsForRange(range).length > 0;
				const controls = renderAnnotationControls(range, blockKey, getMarkdownPreviewAnnotationQuote(children));
				return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", {
					...props,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: `markdown-annotation-list-block ${hasReviewNotes ? "has-review-notes" : ""}`.trim(),
						"data-source-line": range.startLine,
						"data-source-end-line": range.endLine,
						"data-annotation-block-key": controls ? blockKey : void 0,
						onClick: (event) => handleAnnotatedMarkdownBlockClick(range, event),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "markdown-annotation-list-content",
							children
						}), controls]
					})
				});
			},
			h1: ({ node, children, ...props }) => {
				return wrapAnnotatedBlock("h1", node, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", {
					...props,
					tabIndex: -1,
					children
				}));
			},
			h2: ({ node, children, ...props }) => {
				return wrapAnnotatedBlock("h2", node, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
					...props,
					tabIndex: -1,
					children
				}));
			},
			h3: ({ node, children, ...props }) => {
				return wrapAnnotatedBlock("h3", node, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
					...props,
					tabIndex: -1,
					children
				}));
			},
			h4: ({ node, children, ...props }) => {
				return wrapAnnotatedBlock("h4", node, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h4", {
					...props,
					tabIndex: -1,
					children
				}));
			},
			h5: ({ node, children, ...props }) => {
				return wrapAnnotatedBlock("h5", node, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h5", {
					...props,
					tabIndex: -1,
					children
				}));
			},
			h6: ({ node, children, ...props }) => {
				return wrapAnnotatedBlock("h6", node, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h6", {
					...props,
					tabIndex: -1,
					children
				}));
			}
		};
	}, [
		filePath,
		activateMarkdownLink,
		isDark,
		isMac,
		imageRuntimeContext,
		getMarkdownCommentsForRange,
		handleAnnotatedMarkdownBlockClick,
		markdownDocumentIndex,
		onOpenDocument,
		openFile,
		openMarkdownPreview,
		renderAnnotationControls,
		scrollToAnchor,
		setMarkdownViewMode,
		setPendingEditorReveal,
		sourceConnectionId,
		sourceOwner,
		sourceWorktree,
		resolvedSourceRuntimeEnvironmentId,
		sourceRoutingWorktreeId,
		worktreeRoot,
		worktreesByRepo,
		wrapAnnotatedBlock
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "markdown-preview-shell",
		children: [showTableOfContents ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MarkdownTableOfContentsPanel, {
			items: tableOfContentsItems,
			onClose: onCloseTableOfContents ?? (() => {}),
			onNavigate: navigateToTableOfContentsItem
		}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			ref: setRootRef,
			tabIndex: 0,
			style: { fontSize: `${editorFontSize}px` },
			className: `markdown-preview h-full min-h-0 overflow-auto scrollbar-editor ${isDark ? "markdown-dark" : "markdown-light"}`,
			children: [
				isSearchOpen ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "markdown-preview-search",
					onKeyDown: (event) => event.stopPropagation(),
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "markdown-preview-search-field",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								ref: setSearchInputElement,
								value: query,
								onChange: (event) => setQuery(event.target.value),
								onKeyDown: (event) => {
									if (event.key === "Enter" && event.shiftKey) {
										event.preventDefault();
										moveToMatch(-1);
										return;
									}
									if (event.key === "Enter") {
										event.preventDefault();
										moveToMatch(1);
										return;
									}
									if (event.key === "Escape") {
										event.preventDefault();
										closeSearch();
										rootRef.current?.focus();
									}
								},
								placeholder: translate("auto.components.editor.MarkdownPreview.517aea303b", "Find in preview"),
								className: "markdown-preview-search-input h-7 !border-0 bg-transparent px-2 shadow-none focus-visible:!border-0 focus-visible:ring-0",
								"aria-label": translate("auto.components.editor.MarkdownPreview.ec77985138", "Find in markdown preview")
							})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "markdown-preview-search-status",
							children: query && matchCount === 0 ? translate("auto.components.editor.MarkdownPreview.c5dc92cfe3", "No results") : `${matchCount === 0 ? 0 : activeMatchIndex + 1}/${matchCount}`
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "ghost",
							size: "icon-xs",
							onClick: () => moveToMatch(-1),
							disabled: matchCount === 0,
							title: translate("auto.components.editor.MarkdownPreview.1febd97f5c", "Previous match"),
							"aria-label": translate("auto.components.editor.MarkdownPreview.1febd97f5c", "Previous match"),
							className: "markdown-preview-search-button",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronUp, { size: 14 })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "ghost",
							size: "icon-xs",
							onClick: () => moveToMatch(1),
							disabled: matchCount === 0,
							title: translate("auto.components.editor.MarkdownPreview.b42c41bd0d", "Next match"),
							"aria-label": translate("auto.components.editor.MarkdownPreview.b42c41bd0d", "Next match"),
							className: "markdown-preview-search-button",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { size: 14 })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "markdown-preview-search-divider" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "ghost",
							size: "icon-xs",
							onClick: closeSearch,
							title: translate("auto.components.editor.MarkdownPreview.12052c639c", "Close search"),
							"aria-label": translate("auto.components.editor.MarkdownPreview.12052c639c", "Close search"),
							className: "markdown-preview-search-button",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { size: 14 })
						})
					]
				}) : null,
				canShowReviewTools ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "markdown-review-toolbar",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							className: "markdown-review-toolbar-button",
							onClick: () => {
								const firstNote = markdownReviewNotes[0];
								if (firstNote) scrollToReviewNote(firstNote);
							},
							disabled: markdownReviewNotes.length === 0,
							title: translate("auto.components.editor.MarkdownPreview.0f9969a159", "Jump to first review note"),
							"aria-label": translate("auto.components.editor.MarkdownPreview.0f9969a159", "Jump to first review note"),
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MessageSquare, { className: "size-3.5" }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.editor.MarkdownPreview.322afab6ff", "Review notes") }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "markdown-review-count",
									children: markdownReviewNotes.length
								})
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
							type: "button",
							className: "markdown-review-icon-button",
							onClick: () => void handleCopyMarkdownReviewNotes(),
							disabled: markdownReviewNotes.length === 0,
							title: translate("auto.components.editor.MarkdownPreview.bb629de58a", "Copy notes for agent"),
							"aria-label": translate("auto.components.editor.MarkdownPreview.bb629de58a", "Copy notes for agent"),
							children: reviewNotesCopied ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "size-3.5" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { className: "size-3.5" })
						}),
						sourceWorktree ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotesSendMenu, {
							worktreeId: sourceWorktree.id,
							groupId: sourceWorktree.id,
							modeIdParts: [
								"markdown-notes",
								sourceWorktree.id,
								filePath,
								"preview-toolbar"
							],
							scopes: unsentMarkdownReviewScope,
							triggerClassName: "markdown-review-icon-button",
							onDelivered: (notes) => void clearDeliveredDiffComments(sourceWorktree.id, notes)
						}) : null
					]
				}) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					ref: bodyRef,
					className: "markdown-body",
					translate: "no",
					children: [frontMatter && frontmatterVisible ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mb-4 rounded border border-border/60 bg-muted/40 px-3 py-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground",
							children: translate("auto.components.editor.MarkdownPreview.2b2b31382c", "Front Matter")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
							className: "max-h-48 overflow-auto whitespace-pre-wrap text-xs text-muted-foreground font-mono scrollbar-editor",
							children: frontMatterInner
						})]
					}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MarkdownBody, {
						content: renderedContent,
						components
					})]
				})
			]
		})]
	});
}
function MarkdownSingleNoteSendMenu({ worktreeId, filePath, content, note, modeSlot, onDelivered }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotesSendMenu, {
		worktreeId,
		groupId: worktreeId,
		modeIdParts: [
			"markdown-notes",
			worktreeId,
			filePath,
			modeSlot,
			note.id
		],
		scopes: [{
			id: "note",
			label: translate("auto.components.editor.MarkdownPreview.f37b98999e", "This note"),
			notes: note.sentAt ? [] : [note],
			prompt: formatMarkdownReviewNotes([note], content)
		}],
		targetModeLabel: "This note",
		triggerClassName: "orca-diff-comment-pill-btn",
		disabledTooltip: "Note already sent",
		onDelivered
	});
}
function MarkdownAnnotationComposer({ onCancel, onSubmit }) {
	const [body, setBody] = (0, import_react.useState)("");
	const [submitting, setSubmitting] = (0, import_react.useState)(false);
	const mountedRef = useMountedRef();
	const composerRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		const composer = composerRef.current;
		if (!composer) return;
		return installOpenDraftAddReviewNoteGuard(composer);
	}, []);
	const focusTextareaRef = (0, import_react.useCallback)((textarea) => {
		textarea?.focus();
	}, []);
	const trimmed = body.trim();
	const submit = async () => {
		if (submitting || !trimmed) return;
		setSubmitting(true);
		try {
			const ok$1 = await onSubmit(trimmed);
			if (!mountedRef.current) return;
			if (ok$1) setBody("");
		} finally {
			if (mountedRef.current) setSubmitting(false);
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: composerRef,
		className: "markdown-annotation-composer",
		onClick: (event) => event.stopPropagation(),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "orca-diff-comment-popover-label",
				children: translate("auto.components.editor.MarkdownPreview.b1bfc04034", "Selected text")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("textarea", {
				ref: focusTextareaRef,
				className: "orca-diff-comment-popover-textarea",
				placeholder: translate("auto.components.editor.MarkdownPreview.d737791433", "Add note for the AI"),
				value: body,
				onChange: (event) => {
					setBody(event.target.value);
					const el = event.currentTarget;
					el.style.height = "auto";
					el.style.height = `${Math.min(el.scrollHeight, 240)}px`;
				},
				onKeyDown: (event) => {
					if (event.key === "Escape") {
						event.preventDefault();
						onCancel();
						return;
					}
					if (event.key === "Enter" && !event.nativeEvent.isComposing && !event.shiftKey) {
						event.preventDefault();
						submit();
					}
				},
				rows: 3
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "orca-diff-comment-popover-footer",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "sm",
					onClick: onCancel,
					disabled: submitting,
					children: translate("auto.components.editor.MarkdownPreview.e4683f70c4", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					size: "sm",
					onClick: () => void submit(),
					disabled: submitting || !trimmed,
					children: [submitting ? translate("auto.components.editor.MarkdownPreview.d652c87c91", "Saving…") : translate("auto.components.editor.MarkdownPreview.13f94d760c", "Add note"), !submitting && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CornerDownLeft, { className: "ml-1 size-3 opacity-70" })]
				})]
			})
		]
	});
}
export { decodeMarkdownPreviewAnchor, MarkdownPreview as default, deriveMarkdownPreviewSourceRoot, findMarkdownPreviewOpenedEditFileId, findMarkdownPreviewSourceOpenFile, getMarkdownPreviewAnchorScrollTop, getMarkdownPreviewSourceRelativePath, resolveMarkdownPreviewSourceWorktree };
